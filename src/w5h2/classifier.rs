//! SetFit ONNX classifier for W5H2 intent classification.
//!
//! Two-step inference:
//!   1. Tokenize query -> run ONNX model -> get 384-dim normalized embedding
//!   2. Apply classification head: logits = embedding @ weights^T + bias -> argmax

use std::path::Path;

use crate::core::error::{NyayaError, Result};
use crate::w5h2::types::{parse_label, W5H2Intent};

/// Classes that the BERT model was trained on (subset of W5H2_CLASSES).
/// New classes beyond these are routed to Tier 2 (cheap LLM) for classification.
pub const BERT_TRAINED_CLASSES: &[&str] = &[
    "add_shopping",
    "check_calendar",
    "check_email",
    "check_price",
    "check_weather",
    "control_lights",
    "send_email",
    "set_reminder",
];

/// Classification head weights exported from sklearn LogisticRegression
#[derive(Debug, serde::Deserialize)]
struct HeadWeights {
    weights: Vec<Vec<f32>>,
    bias: Vec<f32>,
    classes: Vec<String>,
    embedding_dim: usize,
    n_classes: usize,
}

/// The SetFit ONNX classifier
pub struct W5H2Classifier {
    session: ort::session::Session,
    tokenizer: tokenizers::Tokenizer,
    head: HeadWeights,
    max_length: usize,
}

impl W5H2Classifier {
    /// Load the classifier from a model directory containing:
    /// - model.onnx (sentence transformer backbone)
    /// - head_weights.json (classification head)
    /// - tokenizer.json (HuggingFace tokenizer)
    pub fn load(model_dir: &Path) -> Result<Self> {
        if !crate::security::bert_classifier::ort_available() {
            return Err(NyayaError::ModelLoad("ONNX runtime not available".to_string()).into());
        }
        let onnx_path = model_dir.join("model.onnx");
        let head_path = model_dir.join("head_weights.json");
        let tokenizer_path = model_dir.join("tokenizer.json");

        // Load ONNX session
        let session = ort::session::Session::builder()
            .map_err(|e| {
                NyayaError::ModelLoad(format!("Failed to create ONNX session builder: {}", e))
            })?
            .with_intra_threads(1)
            .map_err(|e| NyayaError::ModelLoad(format!("Failed to set thread count: {}", e)))?
            .commit_from_file(&onnx_path)
            .map_err(|e| {
                NyayaError::ModelLoad(format!(
                    "Failed to load ONNX model from {}: {}",
                    onnx_path.display(),
                    e
                ))
            })?;

        // Load tokenizer
        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path).map_err(|e| {
            NyayaError::ModelLoad(format!(
                "Failed to load tokenizer from {}: {}",
                tokenizer_path.display(),
                e
            ))
        })?;

        // Load classification head
        let head_json = std::fs::read_to_string(&head_path)?;
        let head: HeadWeights = serde_json::from_str(&head_json)?;

        tracing::info!(
            n_classes = head.n_classes,
            embedding_dim = head.embedding_dim,
            "W5H2 classifier loaded"
        );

        Ok(Self {
            session,
            tokenizer,
            head,
            max_length: 128,
        })
    }

    /// Generate a 384-dim normalized embedding without applying the classification head.
    /// Reuses the same ONNX model (all-MiniLM-L6-v2) used for classification.
    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| NyayaError::Inference(format!("Tokenization failed: {}", e)))?;

        let mut input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let mut attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect();

        input_ids.truncate(self.max_length);
        attention_mask.truncate(self.max_length);
        while input_ids.len() < self.max_length {
            input_ids.push(0);
            attention_mask.push(0);
        }

        let shape = vec![1i64, self.max_length as i64];

        let input_ids_value = ort::value::Tensor::from_array((shape.clone(), input_ids))
            .map_err(|e| NyayaError::Inference(format!("Tensor creation failed: {}", e)))?;

        let attention_mask_value = ort::value::Tensor::from_array((shape, attention_mask))
            .map_err(|e| NyayaError::Inference(format!("Tensor creation failed: {}", e)))?;

        let outputs = self
            .session
            .run(ort::inputs![
                "input_ids" => input_ids_value,
                "attention_mask" => attention_mask_value,
            ])
            .map_err(|e| NyayaError::Inference(format!("ONNX inference failed: {}", e)))?;

        let (_shape, embedding_slice) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| NyayaError::Inference(format!("Output extraction failed: {}", e)))?;

        Ok(embedding_slice.to_vec())
    }

    /// Classify a query into a W5H2 intent
    pub fn classify(&mut self, query: &str) -> Result<W5H2Intent> {
        // Step 1: Tokenize
        let encoding = self
            .tokenizer
            .encode(query, true)
            .map_err(|e| NyayaError::Inference(format!("Tokenization failed: {}", e)))?;

        let mut input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let mut attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect();

        // Pad or truncate to max_length
        input_ids.truncate(self.max_length);
        attention_mask.truncate(self.max_length);
        while input_ids.len() < self.max_length {
            input_ids.push(0);
            attention_mask.push(0);
        }

        // Step 2: Run ONNX inference to get embedding
        let shape = vec![1i64, self.max_length as i64];

        let input_ids_value = ort::value::Tensor::from_array((shape.clone(), input_ids))
            .map_err(|e| NyayaError::Inference(format!("Tensor creation failed: {}", e)))?;

        let attention_mask_value = ort::value::Tensor::from_array((shape, attention_mask))
            .map_err(|e| NyayaError::Inference(format!("Tensor creation failed: {}", e)))?;

        let outputs = self
            .session
            .run(ort::inputs![
                "input_ids" => input_ids_value,
                "attention_mask" => attention_mask_value,
            ])
            .map_err(|e| NyayaError::Inference(format!("ONNX inference failed: {}", e)))?;

        let (_shape, embedding_slice) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| NyayaError::Inference(format!("Output extraction failed: {}", e)))?;

        // Step 3: Apply classification head (logits = embedding @ weights^T + bias)
        let mut logits = vec![0.0f32; self.head.n_classes];
        for (class_idx, (weight_row, &bias)) in self
            .head
            .weights
            .iter()
            .zip(self.head.bias.iter())
            .enumerate()
        {
            let mut dot = bias;
            for (w, &e) in weight_row.iter().zip(embedding_slice.iter()) {
                dot += w * e;
            }
            logits[class_idx] = dot;
        }

        // Softmax for confidence
        let max_logit = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exp_sum: f32 = logits.iter().map(|&l| (l - max_logit).exp()).sum();
        let probs: Vec<f32> = logits
            .iter()
            .map(|&l| (l - max_logit).exp() / exp_sum)
            .collect();

        let (pred_idx, &confidence) = probs
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .ok_or_else(|| NyayaError::Inference("Empty probability vector".to_string()))?;

        let pred_label = self.head.classes.get(pred_idx).ok_or_else(|| {
            NyayaError::Inference(format!("Prediction index {} out of bounds", pred_idx))
        })?;
        let (action, target) = parse_label(pred_label)
            .ok_or_else(|| NyayaError::Inference(format!("Unknown class label: {}", pred_label)))?;

        Ok(W5H2Intent {
            action,
            target,
            confidence,
            params: std::collections::HashMap::new(),
        })
    }

    /// Get the list of class labels
    pub fn class_labels(&self) -> &[String] {
        &self.head.classes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_head_weights_deser() {
        let json = r#"{
            "weights": [[0.1, 0.2], [0.3, 0.4]],
            "bias": [0.01, 0.02],
            "classes": ["check_email", "check_weather"],
            "embedding_dim": 2,
            "n_classes": 2
        }"#;
        let head: HeadWeights = serde_json::from_str(json).unwrap();
        assert_eq!(head.n_classes, 2);
        assert_eq!(head.embedding_dim, 2);
    }
}
