//! WebBERT Action Classifier — Layer 2 in the browser cascade.
//!
//! DistilBERT-based action classifier for web navigation. Given a task goal,
//! page URL, and detected elements, predicts the next browser action (click,
//! type, scroll, etc.) with a confidence score.
//!
//! Input format: `[TASK] goal [ELEMENTS] label:type @(cx,cy) ... [PAGE] domain`
//!
//! This is Layer 2 in the detection cascade:
//!   Layer 1: DOM heuristics (rule-based, <1ms)
//!   Layer 2: WebBERT classifier (ONNX, ~5-10ms) ← THIS MODULE
//!   Layer 3: YOLO element detector (vision, ~50ms)
//!   Cascade combiner merges all layers.

use std::path::Path;

use crate::core::error::{NyayaError, Result};

/// The 15 action classes WebBERT can predict.
pub const WEBBERT_CLASSES: &[&str] = &[
    "click",
    "type",
    "scroll_down",
    "scroll_up",
    "wait",
    "go_back",
    "skip",
    "extract_content",
    "dismiss_popup",
    "accept_cookies",
    "fill_form",
    "submit_form",
    "click_next",
    "download",
    "select_dropdown",
];

/// Self-contained element representation for WebBERT input.
///
/// Decoupled from the element_detector module's `DetectedElement` so that
/// this module compiles independently. The cascade combiner (Task 6) handles
/// conversion between types.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebBertElement {
    /// Element type string, e.g. "button", "input", "link", "text"
    pub element_type: String,
    /// Bounding box center X (normalized 0.0-1.0)
    pub bbox_cx: f32,
    /// Bounding box center Y (normalized 0.0-1.0)
    pub bbox_cy: f32,
    /// Detection confidence (0.0-1.0)
    pub confidence: f32,
    /// Optional visible label / text content
    pub label: Option<String>,
}

/// The result of a WebBERT classification.
#[derive(Debug, Clone)]
pub struct WebBertPrediction {
    /// Predicted action class (one of WEBBERT_CLASSES)
    pub action_class: String,
    /// Index into the input elements slice for the target element, if applicable
    pub target_element_idx: Option<usize>,
    /// Softmax confidence score (0.0-1.0)
    pub confidence: f32,
    /// Whether confidence >= cascade_threshold
    pub confident: bool,
}

/// DistilBERT-based action classifier for web navigation.
pub struct WebBertClassifier {
    session: ort::session::Session,
    tokenizer: tokenizers::Tokenizer,
    classes: Vec<String>,
    max_length: usize,
    cascade_threshold: f32,
}

impl WebBertClassifier {
    /// Load the WebBERT classifier from a model directory containing:
    /// - `webbert.onnx` (fine-tuned DistilBERT for action classification)
    /// - `webbert-tokenizer.json` (tokenizer)
    /// - `webbert-classes.json` (class label list, JSON array of strings)
    pub fn load(model_dir: &Path) -> Result<Self> {
        if !crate::security::bert_classifier::ort_available() {
            return Err(NyayaError::ModelLoad("ONNX runtime not available".to_string()).into());
        }
        let onnx_path = model_dir.join("webbert.onnx");
        let tokenizer_path = model_dir.join("webbert-tokenizer.json");
        let classes_path = model_dir.join("webbert-classes.json");

        let session = ort::session::Session::builder()
            .map_err(|e| NyayaError::ModelLoad(format!("WebBERT session builder failed: {}", e)))?
            .with_intra_threads(1)
            .map_err(|e| NyayaError::ModelLoad(format!("WebBERT thread config failed: {}", e)))?
            .commit_from_file(&onnx_path)
            .map_err(|e| {
                NyayaError::ModelLoad(format!(
                    "WebBERT ONNX load from {} failed: {}",
                    onnx_path.display(),
                    e
                ))
            })?;

        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| NyayaError::ModelLoad(format!("WebBERT tokenizer load failed: {}", e)))?;

        let classes_json = std::fs::read_to_string(&classes_path)?;
        let classes: Vec<String> = serde_json::from_str(&classes_json)?;

        tracing::info!(
            n_classes = classes.len(),
            "WebBERT Layer 2 classifier loaded"
        );

        Ok(Self {
            session,
            tokenizer,
            classes,
            max_length: 256,
            cascade_threshold: 0.85,
        })
    }

    /// Try to load the WebBERT classifier. Returns `None` if model files are missing.
    /// Allows graceful degradation — if WebBERT is unavailable, skip Layer 2.
    pub fn try_load(model_dir: &Path) -> Option<Self> {
        let onnx_path = model_dir.join("webbert.onnx");
        if !onnx_path.exists() {
            tracing::info!(
                "WebBERT model not found at {} — skipping Layer 2",
                onnx_path.display()
            );
            return None;
        }
        match Self::load(model_dir) {
            Ok(classifier) => {
                tracing::info!("WebBERT Layer 2 classifier loaded successfully");
                Some(classifier)
            }
            Err(e) => {
                tracing::warn!("WebBERT Layer 2 load failed (degrading): {}", e);
                None
            }
        }
    }

    /// Classify the next browser action given the task goal, page URL, and
    /// detected elements on the page.
    pub fn classify(
        &mut self,
        task_goal: &str,
        page_url: &str,
        elements: &[WebBertElement],
    ) -> Result<WebBertPrediction> {
        let input_text = serialize_input(task_goal, page_url, elements);

        // Tokenize
        let encoding = self
            .tokenizer
            .encode(input_text.as_str(), true)
            .map_err(|e| NyayaError::Inference(format!("WebBERT tokenization failed: {}", e)))?;

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

        let shape = vec![1i64, self.max_length as i64];

        let input_ids_tensor = ort::value::Tensor::from_array((shape.clone(), input_ids))
            .map_err(|e| NyayaError::Inference(format!("WebBERT tensor failed: {}", e)))?;

        let attention_mask_tensor = ort::value::Tensor::from_array((shape, attention_mask))
            .map_err(|e| NyayaError::Inference(format!("WebBERT tensor failed: {}", e)))?;

        // Run inference
        let outputs = self
            .session
            .run(ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor,
            ])
            .map_err(|e| NyayaError::Inference(format!("WebBERT inference failed: {}", e)))?;

        // Extract logits
        let (_output_shape, logits_slice) =
            outputs[0].try_extract_tensor::<f32>().map_err(|e| {
                NyayaError::Inference(format!("WebBERT output extraction failed: {}", e))
            })?;

        // Softmax
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
            .ok_or_else(|| NyayaError::Inference("Empty WebBERT output".into()))?;

        let action_class = self.classes[pred_idx].clone();
        let target_element_idx = pick_target_element(&action_class, elements);

        Ok(WebBertPrediction {
            action_class,
            target_element_idx,
            confidence,
            confident: confidence >= self.cascade_threshold,
        })
    }

    /// Get the current cascade threshold.
    pub fn cascade_threshold(&self) -> f32 {
        self.cascade_threshold
    }

    /// Set the cascade threshold. Predictions with confidence below this
    /// are marked as not confident, signaling the cascade to consult Layer 3.
    pub fn set_cascade_threshold(&mut self, threshold: f32) {
        self.cascade_threshold = threshold;
    }
}

/// Serialize the classification input into the format expected by WebBERT:
///
/// `[TASK] <goal> [ELEMENTS] <label>:<type> @(<cx>,<cy>) ... [PAGE] <domain>`
pub(crate) fn serialize_input(
    task_goal: &str,
    page_url: &str,
    elements: &[WebBertElement],
) -> String {
    let mut out = String::with_capacity(512);
    out.push_str("[TASK] ");
    out.push_str(task_goal);
    out.push_str(" [ELEMENTS]");

    for elem in elements {
        out.push(' ');
        if let Some(ref label) = elem.label {
            out.push_str(label);
        } else {
            out.push('_');
        }
        out.push(':');
        out.push_str(&elem.element_type);
        out.push_str(&format!(" @({:.2},{:.2})", elem.bbox_cx, elem.bbox_cy));
    }

    // Extract domain from URL
    let domain = extract_domain(page_url);
    out.push_str(" [PAGE] ");
    out.push_str(&domain);

    out
}

/// Pick the best target element index for a given action class.
///
/// Heuristic: certain actions prefer certain element types.
/// Returns `None` for actions that don't target a specific element (e.g. scroll, wait, go_back).
pub(crate) fn pick_target_element(
    action_class: &str,
    elements: &[WebBertElement],
) -> Option<usize> {
    if elements.is_empty() {
        return None;
    }

    let preferred_type = match action_class {
        "click" | "click_next" | "dismiss_popup" | "accept_cookies" | "download" => Some("button"),
        "type" | "fill_form" => Some("input"),
        "submit_form" => Some("button"),
        "select_dropdown" => Some("select"),
        "extract_content" => Some("text"),
        // Actions without a specific element target
        "scroll_down" | "scroll_up" | "wait" | "go_back" | "skip" => return None,
        _ => None,
    };

    if let Some(ptype) = preferred_type {
        // Find the element with the matching type and highest confidence
        let best = elements
            .iter()
            .enumerate()
            .filter(|(_, e)| e.element_type == ptype)
            .max_by(|(_, a), (_, b)| {
                a.confidence
                    .partial_cmp(&b.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

        if let Some((idx, _)) = best {
            return Some(idx);
        }
    }

    // Fallback: pick the highest-confidence element
    elements
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            a.confidence
                .partial_cmp(&b.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(idx, _)| idx)
}

/// Extract domain from a URL string. Falls back to the raw URL on parse failure.
fn extract_domain(url: &str) -> String {
    // Simple extraction: find the host part between :// and the next /
    if let Some(after_scheme) = url.split("://").nth(1) {
        let host = after_scheme.split('/').next().unwrap_or(after_scheme);
        // Strip port if present
        host.split(':').next().unwrap_or(host).to_string()
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_element(element_type: &str, label: Option<&str>, confidence: f32) -> WebBertElement {
        WebBertElement {
            element_type: element_type.to_string(),
            bbox_cx: 0.5,
            bbox_cy: 0.5,
            confidence,
            label: label.map(|s| s.to_string()),
        }
    }

    #[test]
    fn test_serialize_input_format() {
        let elements = vec![
            WebBertElement {
                element_type: "button".to_string(),
                bbox_cx: 0.25,
                bbox_cy: 0.75,
                confidence: 0.95,
                label: Some("Submit".to_string()),
            },
            WebBertElement {
                element_type: "input".to_string(),
                bbox_cx: 0.50,
                bbox_cy: 0.30,
                confidence: 0.88,
                label: None,
            },
        ];

        let result = serialize_input(
            "fill out the login form",
            "https://example.com/login",
            &elements,
        );

        assert!(result.starts_with("[TASK] fill out the login form [ELEMENTS]"));
        assert!(result.contains("Submit:button @(0.25,0.75)"));
        assert!(result.contains("_:input @(0.50,0.30)"));
        assert!(result.ends_with("[PAGE] example.com"));
    }

    #[test]
    fn test_pick_target_element_button() {
        let elements = vec![
            make_element("input", Some("email"), 0.90),
            make_element("button", Some("Login"), 0.85),
            make_element("link", Some("Forgot password"), 0.70),
        ];

        let idx = pick_target_element("click", &elements);
        assert_eq!(idx, Some(1), "click action should prefer button elements");
    }

    #[test]
    fn test_pick_target_element_input() {
        let elements = vec![
            make_element("button", Some("Submit"), 0.95),
            make_element("input", Some("username"), 0.80),
            make_element("input", Some("password"), 0.85),
        ];

        let idx = pick_target_element("type", &elements);
        // Should prefer input elements; pick the one with highest confidence (password at idx 2)
        assert_eq!(idx, Some(2), "type action should prefer input elements");
    }

    #[test]
    fn test_webbert_classes_count() {
        assert_eq!(WEBBERT_CLASSES.len(), 15);
    }

    #[test]
    fn test_try_load_missing() {
        let result = WebBertClassifier::try_load(std::path::Path::new("/nonexistent"));
        assert!(result.is_none());
    }
}
