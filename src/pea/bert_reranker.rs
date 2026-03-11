//! Tier 2 BERT re-ranker for search candidates.
//! Uses sentence-transformer (all-MiniLM-L6-v2) to embed objective and
//! candidate titles, then ranks by cosine similarity.

use std::path::Path;

use crate::core::error::{NyayaError, Result};

/// BERT-based re-ranker using sentence-transformer embeddings.
pub struct BertReranker {
    session: ort::session::Session,
    tokenizer: tokenizers::Tokenizer,
    max_length: usize,
}

impl BertReranker {
    /// Load sentence-transformer model from directory.
    /// Expects: `model.onnx` and `tokenizer.json`.
    pub fn load(model_dir: &Path) -> Result<Self> {
        let onnx_path = model_dir.join("model.onnx");
        let tokenizer_path = model_dir.join("tokenizer.json");

        if !onnx_path.exists() {
            return Err(NyayaError::ModelLoad(format!(
                "Sentence-transformer model not found at {}. \
                 Run: hf download sentence-transformers/all-MiniLM-L6-v2 --local-dir {}",
                onnx_path.display(),
                model_dir.display()
            )));
        }

        let session = ort::session::Session::builder()
            .map_err(|e| NyayaError::ModelLoad(format!("ONNX session builder: {}", e)))?
            .with_intra_threads(1)
            .map_err(|e| NyayaError::ModelLoad(format!("ONNX thread config: {}", e)))?
            .commit_from_file(&onnx_path)
            .map_err(|e| NyayaError::ModelLoad(format!(
                "ONNX load from {}: {}", onnx_path.display(), e
            )))?;

        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| NyayaError::ModelLoad(format!("Tokenizer load: {}", e)))?;

        Ok(Self { session, tokenizer, max_length: 128 })
    }

    /// Embed a text string into a normalized vector.
    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>> {
        let encoding = self.tokenizer
            .encode(text, true)
            .map_err(|e| NyayaError::Inference(format!("Tokenization: {}", e)))?;

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

        let input_ids_tensor = ort::value::Tensor::from_array((shape.clone(), input_ids))
            .map_err(|e| NyayaError::Inference(format!("Tensor: {}", e)))?;
        let attention_mask_tensor = ort::value::Tensor::from_array((shape, attention_mask))
            .map_err(|e| NyayaError::Inference(format!("Tensor: {}", e)))?;

        let outputs = self.session
            .run(ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor,
            ])
            .map_err(|e| NyayaError::Inference(format!("ONNX inference: {}", e)))?;

        let (_shape, embedding) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| NyayaError::Inference(format!("Output extraction: {}", e)))?;

        // Mean pooling + L2 normalize
        let emb: Vec<f32> = embedding.to_vec();
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            Ok(emb.iter().map(|x| x / norm).collect())
        } else {
            Ok(emb)
        }
    }

    /// Score candidates by cosine similarity to objective embedding.
    /// Returns (index, score) pairs sorted by score descending.
    pub fn rank(
        &mut self,
        objective_embedding: &[f32],
        candidates: &[(String, String)],  // (title, snippet)
    ) -> Vec<(usize, f32)> {
        let mut scores: Vec<(usize, f32)> = candidates
            .iter()
            .enumerate()
            .map(|(i, (title, snippet))| {
                let text = format!("{} {}", title, &snippet[..snippet.len().min(100)]);
                match self.embed(&text) {
                    Ok(emb) => {
                        let sim = cosine_similarity(objective_embedding, &emb);
                        (i, sim)
                    }
                    Err(_) => (i, 0.0),
                }
            })
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    dot.clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![0.5, 0.5, 0.5, 0.5];
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        let normalized: Vec<f32> = v.iter().map(|x| x / norm).collect();
        let sim = cosine_similarity(&normalized, &normalized);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &b)).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_mismatched_length() {
        assert_eq!(cosine_similarity(&[1.0, 2.0], &[1.0]), 0.0);
    }
}
