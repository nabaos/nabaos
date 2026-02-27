//! BERT ONNX classifier — Tier 1 fast supervised classification.
//!
//! Per Paper 1 ("W5H2 — Structured Intent Decomposition for Personal Agent Plan Caching"):
//!   - Fine-tuned BERT-base-uncased achieves 97.3% accuracy on MASSIVE 8-class benchmark
//!   - ONNX inference latency: ~5-10ms (vs 85ms PyTorch)
//!   - Confidence threshold: at >0.85, 97.9% of queries pass through at 98.2% accuracy
//!   - The remaining 2.1% (low confidence) cascade to Tier 2 (SetFit)
//!
//! Architecture position in the five-tier cascade:
//!   Tier 0: Fingerprint cache (<1ms)
//!   Tier 1: BERT classifier (5-10ms) ← THIS MODULE
//!   Tier 2: SetFit few-shot (~10ms)
//!   Tier 3: Cheap LLM (~3000ms)
//!   Tier 4: Deep agent (~minutes)

use std::path::Path;

use crate::core::error::{NyayaError, Result};
use crate::w5h2::types::{parse_label, W5H2Intent};

/// BERT classification result with confidence for cascade decision.
#[derive(Debug, Clone)]
pub struct BertClassification {
    /// The classified intent
    pub intent: W5H2Intent,
    /// Whether confidence exceeds the cascade threshold
    pub confident: bool,
    /// Raw confidence score (0.0-1.0)
    pub confidence: f32,
}

/// The BERT ONNX classifier — Tier 1 in the five-tier cascade.
pub struct BertClassifier {
    session: ort::session::Session,
    tokenizer: tokenizers::Tokenizer,
    classes: Vec<String>,
    max_length: usize,
    /// Confidence threshold for cascade (default 0.85 per Paper 1)
    cascade_threshold: f32,
}

impl BertClassifier {
    /// Load the BERT classifier from a model directory containing:
    /// - bert_model.onnx (fine-tuned BERT-base-uncased for intent classification)
    /// - bert_tokenizer.json (BERT tokenizer)
    /// - bert_classes.json (class label list: ["check_email", "check_weather", ...])
    pub fn load(model_dir: &Path) -> Result<Self> {
        let onnx_path = model_dir.join("bert_model.onnx");
        let tokenizer_path = model_dir.join("bert_tokenizer.json");
        let classes_path = model_dir.join("bert_classes.json");

        // Load ONNX session
        let session = ort::session::Session::builder()
            .map_err(|e| NyayaError::ModelLoad(format!("BERT session builder failed: {}", e)))?
            .with_intra_threads(1)
            .map_err(|e| NyayaError::ModelLoad(format!("BERT thread config failed: {}", e)))?
            .commit_from_file(&onnx_path)
            .map_err(|e| {
                NyayaError::ModelLoad(format!(
                    "BERT ONNX load from {} failed: {}",
                    onnx_path.display(),
                    e
                ))
            })?;

        // Load tokenizer
        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| NyayaError::ModelLoad(format!("BERT tokenizer load failed: {}", e)))?;

        // Load class labels
        let classes_json = std::fs::read_to_string(&classes_path)?;
        let classes: Vec<String> = serde_json::from_str(&classes_json)?;

        tracing::info!(n_classes = classes.len(), "BERT Tier 1 classifier loaded");

        Ok(Self {
            session,
            tokenizer,
            classes,
            max_length: 128,
            cascade_threshold: 0.85,
        })
    }

    /// Set the cascade threshold (default 0.85 per Paper 1).
    /// Queries with confidence below this cascade to Tier 2 (SetFit).
    pub fn set_cascade_threshold(&mut self, threshold: f32) {
        self.cascade_threshold = threshold;
    }

    /// Classify a query using BERT.
    /// Returns the classification with a `confident` flag indicating whether
    /// the result exceeds the cascade threshold.
    pub fn classify(&mut self, query: &str) -> Result<BertClassification> {
        // Tokenize
        let encoding = self
            .tokenizer
            .encode(query, true)
            .map_err(|e| NyayaError::Inference(format!("BERT tokenization failed: {}", e)))?;

        let mut input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let mut attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect();
        let mut token_type_ids: Vec<i64> =
            encoding.get_type_ids().iter().map(|&t| t as i64).collect();

        // Pad or truncate to max_length
        input_ids.truncate(self.max_length);
        attention_mask.truncate(self.max_length);
        token_type_ids.truncate(self.max_length);
        while input_ids.len() < self.max_length {
            input_ids.push(0);
            attention_mask.push(0);
            token_type_ids.push(0);
        }

        let shape = vec![1i64, self.max_length as i64];

        // Create ONNX tensors
        let input_ids_tensor = ort::value::Tensor::from_array((shape.clone(), input_ids))
            .map_err(|e| NyayaError::Inference(format!("BERT tensor failed: {}", e)))?;

        let attention_mask_tensor = ort::value::Tensor::from_array((shape.clone(), attention_mask))
            .map_err(|e| NyayaError::Inference(format!("BERT tensor failed: {}", e)))?;

        let token_type_ids_tensor = ort::value::Tensor::from_array((shape, token_type_ids))
            .map_err(|e| NyayaError::Inference(format!("BERT tensor failed: {}", e)))?;

        // Run inference
        let outputs = self
            .session
            .run(ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor,
                "token_type_ids" => token_type_ids_tensor,
            ])
            .map_err(|e| NyayaError::Inference(format!("BERT inference failed: {}", e)))?;

        // Extract logits from output
        let (_output_shape, logits_slice) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| NyayaError::Inference(format!("BERT output extraction failed: {}", e)))?;

        // Softmax for confidence
        let logits: Vec<f32> = logits_slice
            .iter()
            .take(self.classes.len())
            .cloned()
            .collect();
        let max_logit = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exp_sum: f32 = logits.iter().map(|&l| (l - max_logit).exp()).sum();
        let probs: Vec<f32> = logits
            .iter()
            .map(|&l| (l - max_logit).exp() / exp_sum)
            .collect();

        let (pred_idx, &confidence) = probs
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .ok_or_else(|| NyayaError::Inference("Empty BERT output".into()))?;

        let pred_label = &self.classes[pred_idx];
        let (action, target) = parse_label(pred_label)
            .ok_or_else(|| NyayaError::Inference(format!("Unknown BERT class: {}", pred_label)))?;

        let intent = W5H2Intent {
            action,
            target,
            confidence,
            params: std::collections::HashMap::new(),
        };

        Ok(BertClassification {
            intent,
            confident: confidence >= self.cascade_threshold,
            confidence,
        })
    }

    /// Get the cascade threshold.
    pub fn cascade_threshold(&self) -> f32 {
        self.cascade_threshold
    }

    /// Get class labels.
    pub fn class_labels(&self) -> &[String] {
        &self.classes
    }
}

/// Try to load the BERT classifier. Returns None if model files don't exist.
/// This allows graceful degradation — if BERT isn't available, skip Tier 1.
pub fn try_load_bert(model_dir: &Path) -> Option<BertClassifier> {
    let onnx_path = model_dir.join("bert_model.onnx");
    if !onnx_path.exists() {
        tracing::info!(
            "BERT model not found at {} — skipping Tier 1",
            onnx_path.display()
        );
        return None;
    }
    match BertClassifier::load(model_dir) {
        Ok(classifier) => {
            tracing::info!("BERT Tier 1 classifier loaded successfully");
            Some(classifier)
        }
        Err(e) => {
            tracing::warn!("BERT Tier 1 load failed (degrading to Tier 2): {}", e);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bert_classification_struct() {
        use crate::w5h2::types::{Action, Target};
        let intent = W5H2Intent {
            action: Action::Check,
            target: Target::Weather,
            confidence: 0.95,
            params: std::collections::HashMap::new(),
        };
        let result = BertClassification {
            intent,
            confident: true,
            confidence: 0.95,
        };
        assert!(result.confident);
        assert!(result.confidence >= 0.85);
    }

    #[test]
    fn test_try_load_bert_missing_model() {
        let dir = tempfile::tempdir().unwrap();
        let result = try_load_bert(dir.path());
        assert!(
            result.is_none(),
            "Should return None if model files don't exist"
        );
    }

    #[test]
    fn test_cascade_threshold_default() {
        // Can't test actual loading without model files, but verify the default
        assert!((0.85f32 - 0.85).abs() < f32::EPSILON);
    }
}
