"""
SetFit ONNX classifier service for W5H2 intent classification.

Two-step inference pipeline (ported from classifier.rs):
  1. Tokenize query -> run ONNX model -> get 384-dim normalized embedding
  2. Apply classification head: logits = embedding @ weights^T + bias -> argmax
  3. Temperature scaling (T=10.0) for calibration before softmax
  4. OOS detection: max calibrated confidence < 0.3 -> out-of-scope
"""

import json
import os
import time
from collections import defaultdict
from pathlib import Path
from threading import Lock
from typing import Any

import numpy as np
import onnxruntime as ort
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
from tokenizers import Tokenizer

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

MODEL_DIR = Path(__file__).resolve().parent.parent / "models" / "setfit-w5h2"
MAX_LENGTH = 128
TEMPERATURE = 10.0  # Temperature scaling: reduces ECE from 0.51 to 0.04
OOS_THRESHOLD = 0.3  # If max calibrated confidence < this, mark as out-of-scope

# ---------------------------------------------------------------------------
# Request / Response schemas
# ---------------------------------------------------------------------------


class ClassifyRequest(BaseModel):
    query: str


class ClassifyResponse(BaseModel):
    intent: str
    confidence: float
    cache_key: str
    calibrated_confidence: float
    oos: bool


class RetrainRequest(BaseModel):
    examples: list[dict[str, str]]


class RetrainResponse(BaseModel):
    queued: int


class HealthResponse(BaseModel):
    status: str
    model_loaded: bool


class MetricsResponse(BaseModel):
    total_requests: int
    avg_latency_ms: float
    cache_key_distribution: dict[str, int]


# ---------------------------------------------------------------------------
# Model holder (loaded once at startup)
# ---------------------------------------------------------------------------


class W5H2Classifier:
    """Port of src/w5h2/classifier.rs inference pipeline to Python."""

    def __init__(self, model_dir: Path) -> None:
        onnx_path = model_dir / "model.onnx"
        head_path = model_dir / "head_weights.json"
        tokenizer_path = model_dir / "tokenizer.json"

        if not onnx_path.exists():
            raise FileNotFoundError(f"ONNX model not found: {onnx_path}")

        # Load ONNX session (single-threaded, matching Rust config)
        sess_opts = ort.SessionOptions()
        sess_opts.intra_op_num_threads = 1
        self.session = ort.InferenceSession(str(onnx_path), sess_options=sess_opts)

        # Load tokenizer
        self.tokenizer = Tokenizer.from_file(str(tokenizer_path))

        # Load classification head weights
        with open(head_path) as f:
            head = json.load(f)
        self.weights = np.array(head["weights"], dtype=np.float32)  # (n_classes, embedding_dim)
        self.bias = np.array(head["bias"], dtype=np.float32)        # (n_classes,)
        self.classes = head["classes"]                                # list[str]
        self.embedding_dim = head["embedding_dim"]
        self.n_classes = head["n_classes"]

        # Load label_map (index -> label, for reference)
        label_map_path = model_dir / "label_map.json"
        if label_map_path.exists():
            with open(label_map_path) as f:
                self.label_map = json.load(f)

    def classify(self, query: str) -> dict[str, Any]:
        """
        Full inference pipeline:
          tokenize -> ONNX embedding -> head weights matmul -> logits
          -> temperature-scaled softmax -> argmax -> result
        """
        # Step 1: Tokenize (matches Rust: encode with special tokens=True)
        encoding = self.tokenizer.encode(query, add_special_tokens=True)
        input_ids = list(encoding.ids)
        attention_mask = list(encoding.attention_mask)

        # Pad or truncate to MAX_LENGTH (matches Rust logic)
        input_ids = input_ids[:MAX_LENGTH]
        attention_mask = attention_mask[:MAX_LENGTH]
        pad_len = MAX_LENGTH - len(input_ids)
        if pad_len > 0:
            input_ids.extend([0] * pad_len)
            attention_mask.extend([0] * pad_len)

        # Convert to numpy arrays with batch dimension
        input_ids_np = np.array([input_ids], dtype=np.int64)        # (1, MAX_LENGTH)
        attention_mask_np = np.array([attention_mask], dtype=np.int64)  # (1, MAX_LENGTH)

        # Step 2: Run ONNX inference to get 384-dim embedding
        outputs = self.session.run(
            None,
            {
                "input_ids": input_ids_np,
                "attention_mask": attention_mask_np,
            },
        )
        embedding = outputs[0][0]  # shape: (384,)

        # Step 3: Classification head: logits = embedding @ weights^T + bias
        # Matches Rust: dot product per class row + bias
        logits = self.weights @ embedding + self.bias  # (n_classes,)

        # Step 4a: Raw softmax for uncalibrated confidence (matches Rust argmax logic)
        raw_probs = _softmax(logits)
        pred_idx = int(np.argmax(raw_probs))
        raw_confidence = float(raw_probs[pred_idx])

        # Step 4b: Temperature-scaled softmax for calibrated confidence
        scaled_logits = logits / TEMPERATURE
        calibrated_probs = _softmax(scaled_logits)
        calibrated_confidence = float(calibrated_probs[pred_idx])

        # Step 5: OOS detection
        oos = calibrated_confidence < OOS_THRESHOLD

        pred_label = self.classes[pred_idx]
        # cache_key is the label itself (action_target format)
        cache_key = pred_label

        return {
            "intent": pred_label,
            "confidence": round(raw_confidence, 4),
            "cache_key": cache_key,
            "calibrated_confidence": round(calibrated_confidence, 4),
            "oos": oos,
        }


def _softmax(x: np.ndarray) -> np.ndarray:
    """Numerically stable softmax (matches Rust implementation)."""
    e = np.exp(x - np.max(x))
    return e / e.sum()


# ---------------------------------------------------------------------------
# Metrics tracker
# ---------------------------------------------------------------------------


class MetricsTracker:
    def __init__(self) -> None:
        self._lock = Lock()
        self.total_requests: int = 0
        self.total_latency_ms: float = 0.0
        self.cache_key_distribution: dict[str, int] = defaultdict(int)

    def record(self, latency_ms: float, cache_key: str) -> None:
        with self._lock:
            self.total_requests += 1
            self.total_latency_ms += latency_ms
            self.cache_key_distribution[cache_key] += 1

    def snapshot(self) -> dict[str, Any]:
        with self._lock:
            avg = (self.total_latency_ms / self.total_requests) if self.total_requests > 0 else 0.0
            return {
                "total_requests": self.total_requests,
                "avg_latency_ms": round(avg, 2),
                "cache_key_distribution": dict(self.cache_key_distribution),
            }


# ---------------------------------------------------------------------------
# Retrain queue (simple in-memory queue; production would use a task queue)
# ---------------------------------------------------------------------------


class RetrainQueue:
    def __init__(self) -> None:
        self._lock = Lock()
        self._examples: list[dict[str, str]] = []

    def enqueue(self, examples: list[dict[str, str]]) -> int:
        with self._lock:
            self._examples.extend(examples)
            return len(examples)

    @property
    def size(self) -> int:
        with self._lock:
            return len(self._examples)


# ---------------------------------------------------------------------------
# FastAPI application
# ---------------------------------------------------------------------------

app = FastAPI(title="W5H2 Classifier Service", version="1.0.0")

# Globals initialised at startup
classifier: W5H2Classifier | None = None
metrics = MetricsTracker()
retrain_queue = RetrainQueue()


@app.on_event("startup")
def load_model() -> None:
    global classifier
    try:
        classifier = W5H2Classifier(MODEL_DIR)
        print(
            f"Model loaded: {classifier.n_classes} classes, "
            f"{classifier.embedding_dim}-dim embeddings"
        )
    except Exception as exc:
        print(f"WARNING: Failed to load model: {exc}")
        classifier = None


# ------ Endpoints ----------------------------------------------------------


@app.post("/classify", response_model=ClassifyResponse)
def classify(req: ClassifyRequest) -> ClassifyResponse:
    if classifier is None:
        raise HTTPException(status_code=503, detail="Model not loaded")

    t0 = time.perf_counter()
    result = classifier.classify(req.query)
    latency_ms = (time.perf_counter() - t0) * 1000.0

    metrics.record(latency_ms, result["cache_key"])

    return ClassifyResponse(**result)


@app.get("/health", response_model=HealthResponse)
def health() -> HealthResponse:
    return HealthResponse(status="ok", model_loaded=classifier is not None)


@app.post("/retrain", response_model=RetrainResponse)
def retrain(req: RetrainRequest) -> RetrainResponse:
    n = retrain_queue.enqueue(req.examples)
    return RetrainResponse(queued=n)


@app.get("/metrics", response_model=MetricsResponse)
def get_metrics() -> MetricsResponse:
    return MetricsResponse(**metrics.snapshot())
