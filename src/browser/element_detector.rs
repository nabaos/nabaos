//! YOLO-based UI element detection (Layer 1 of cascade detection).
//!
//! Loads an OmniParser YOLO model via ONNX Runtime and runs inference
//! on page screenshots to detect interactive UI elements (buttons, inputs,
//! links, dropdowns, etc.).  Output is a list of `DetectedElement` with
//! bounding boxes, class labels, and confidence scores.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::error::{NyayaError, Result};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A UI element detected by the YOLO model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedElement {
    /// Semantic element type (button, input, link, ...).
    pub element_type: ElementType,
    /// Bounding box in normalised image coordinates (0.0 .. 1.0).
    pub bbox: BoundingBox,
    /// Model confidence score (0.0 .. 1.0).
    pub confidence: f32,
    /// Optional human-readable label extracted from the model output.
    pub label: Option<String>,
}

/// Semantic type of a detected UI element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElementType {
    Button,
    Input,
    Link,
    Dropdown,
    Toggle,
    Checkbox,
    Image,
    Icon,
    Text,
    Nav,
    Tab,
    Unknown,
}

/// Axis-aligned bounding box in normalised coordinates (0.0 .. 1.0).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BoundingBox {
    /// Left edge (normalised).
    pub x: f32,
    /// Top edge (normalised).
    pub y: f32,
    /// Width (normalised).
    pub width: f32,
    /// Height (normalised).
    pub height: f32,
}

impl BoundingBox {
    /// Returns the centre point `(cx, cy)` of this bounding box.
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

// ---------------------------------------------------------------------------
// ElementDetector
// ---------------------------------------------------------------------------

/// YOLO-based element detector backed by ONNX Runtime.
pub struct ElementDetector {
    session: ort::session::Session,
    input_width: u32,
    input_height: u32,
    confidence_threshold: f32,
    nms_threshold: f32,
    class_names: Vec<String>,
}

impl ElementDetector {
    /// Load the YOLO element detector from `model_dir`.
    ///
    /// Expects `model_dir/omniparser-yolo.onnx` to exist.
    pub fn load(model_dir: &Path) -> Result<Self> {
        if !crate::security::bert_classifier::ort_available() {
            return Err(NyayaError::ModelLoad("ONNX runtime not available".to_string()).into());
        }
        let onnx_path = model_dir.join("omniparser-yolo.onnx");

        let session = ort::session::Session::builder()
            .map_err(|e| NyayaError::ModelLoad(format!("YOLO session builder failed: {e}")))?
            .with_intra_threads(1)
            .map_err(|e| NyayaError::ModelLoad(format!("YOLO thread config failed: {e}")))?
            .commit_from_file(&onnx_path)
            .map_err(|e| {
                NyayaError::ModelLoad(format!(
                    "YOLO ONNX load from {} failed: {e}",
                    onnx_path.display()
                ))
            })?;

        tracing::info!("YOLO element detector loaded from {}", onnx_path.display());

        Ok(Self {
            session,
            input_width: 640,
            input_height: 640,
            confidence_threshold: 0.25,
            nms_threshold: 0.45,
            class_names: default_class_names(),
        })
    }

    /// Try to load the detector; returns `None` if the model file is missing.
    pub fn try_load(model_dir: &Path) -> Option<Self> {
        let onnx_path = model_dir.join("omniparser-yolo.onnx");
        if !onnx_path.exists() {
            tracing::info!(
                "YOLO model not found at {} -- skipping element detector",
                onnx_path.display()
            );
            return None;
        }
        match Self::load(model_dir) {
            Ok(det) => Some(det),
            Err(e) => {
                tracing::warn!("YOLO element detector load failed (degrading): {e}");
                None
            }
        }
    }

    /// Detect UI elements in a screenshot provided as encoded image bytes
    /// (PNG, JPEG, etc.).
    pub fn detect(&mut self, image_bytes: &[u8]) -> Result<Vec<DetectedElement>> {
        // 1. Decode image
        let img = image::load_from_memory(image_bytes)
            .map_err(|e| NyayaError::Inference(format!("Image decode failed: {e}")))?;

        let orig_width = img.width() as f32;
        let orig_height = img.height() as f32;

        // 2. Resize to model input dimensions
        let resized = img.resize_exact(
            self.input_width,
            self.input_height,
            image::imageops::FilterType::Triangle,
        );
        let rgb = resized.to_rgb8();

        // 3. Normalise to [0, 1] and arrange as CHW float tensor
        let mut input_tensor: Vec<f32> =
            vec![0.0; (3 * self.input_width * self.input_height) as usize];
        let hw = (self.input_width * self.input_height) as usize;

        for (i, pixel) in rgb.pixels().enumerate() {
            input_tensor[i] = pixel[0] as f32 / 255.0; // R
            input_tensor[hw + i] = pixel[1] as f32 / 255.0; // G
            input_tensor[2 * hw + i] = pixel[2] as f32 / 255.0; // B
        }

        let shape: Vec<i64> = vec![1, 3, self.input_height as i64, self.input_width as i64];

        let input = ort::value::Tensor::from_array((shape, input_tensor))
            .map_err(|e| NyayaError::Inference(format!("YOLO tensor creation failed: {e}")))?;

        // 4. Run inference and extract output into an owned Vec
        let raw_vec: Vec<f32> = {
            let outputs = self
                .session
                .run(ort::inputs!["images" => input])
                .map_err(|e| NyayaError::Inference(format!("YOLO inference failed: {e}")))?;

            let (_out_shape, raw) = outputs[0].try_extract_tensor::<f32>().map_err(|e| {
                NyayaError::Inference(format!("YOLO output extraction failed: {e}"))
            })?;

            raw.to_vec()
        };

        // 5. Parse YOLOv8 output tensor and apply NMS
        let detections = self.parse_yolo_output(&raw_vec, orig_width, orig_height);

        Ok(non_max_suppression(detections, self.nms_threshold))
    }

    /// Parse the raw YOLOv8 output tensor into a list of detections.
    ///
    /// YOLOv8 output shape: `[1, num_classes + 4, num_predictions]`.
    /// Each prediction column has `[cx, cy, w, h, class_scores...]`.
    fn parse_yolo_output(
        &self,
        raw: &[f32],
        orig_width: f32,
        orig_height: f32,
    ) -> Vec<DetectedElement> {
        let num_classes = self.class_names.len();
        let row_len = 4 + num_classes; // cx, cy, w, h, class_scores...

        // Number of prediction columns
        if raw.is_empty() || row_len == 0 {
            return Vec::new();
        }
        let num_preds = raw.len() / row_len;
        if num_preds == 0 {
            return Vec::new();
        }

        let mut detections = Vec::new();

        for col in 0..num_preds {
            // YOLOv8 stores data transposed: row-major [rows, cols]
            // Row 0 = cx, row 1 = cy, row 2 = w, row 3 = h, rows 4.. = class scores
            let cx = raw[0 * num_preds + col];
            let cy = raw[num_preds + col];
            let w = raw[2 * num_preds + col];
            let h = raw[3 * num_preds + col];

            // Find best class
            let mut best_class = 0usize;
            let mut best_score = f32::NEG_INFINITY;
            for cls in 0..num_classes {
                let score = raw[(4 + cls) * num_preds + col];
                if score > best_score {
                    best_score = score;
                    best_class = cls;
                }
            }

            if best_score < self.confidence_threshold {
                continue;
            }

            // Convert from pixel coords (relative to 640x640) to normalised
            let nx = (cx - w / 2.0) / self.input_width as f32;
            let ny = (cy - h / 2.0) / self.input_height as f32;
            let nw = w / self.input_width as f32;
            let nh = h / self.input_height as f32;

            // Clamp to [0, 1]
            let nx = nx.clamp(0.0, 1.0);
            let ny = ny.clamp(0.0, 1.0);
            let nw = nw.clamp(0.0, 1.0 - nx);
            let nh = nh.clamp(0.0, 1.0 - ny);

            let label = self.class_names.get(best_class).cloned();

            detections.push(DetectedElement {
                element_type: class_to_element_type(best_class),
                bbox: BoundingBox {
                    x: nx,
                    y: ny,
                    width: nw,
                    height: nh,
                },
                confidence: best_score,
                label,
            });
        }

        // Sort by confidence descending (useful for NMS)
        detections.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

        let _ = orig_width;
        let _ = orig_height;

        detections
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Intersection-over-union of two bounding boxes.
pub(crate) fn iou(a: &BoundingBox, b: &BoundingBox) -> f32 {
    let x1 = a.x.max(b.x);
    let y1 = a.y.max(b.y);
    let x2 = (a.x + a.width).min(b.x + b.width);
    let y2 = (a.y + a.height).min(b.y + b.height);

    let inter_w = (x2 - x1).max(0.0);
    let inter_h = (y2 - y1).max(0.0);
    let inter_area = inter_w * inter_h;

    let area_a = a.width * a.height;
    let area_b = b.width * b.height;
    let union_area = area_a + area_b - inter_area;

    if union_area <= 0.0 {
        return 0.0;
    }

    inter_area / union_area
}

/// Non-maximum suppression: keep only the highest-confidence detection among
/// overlapping boxes that exceed `threshold` IoU.
pub(crate) fn non_max_suppression(
    mut detections: Vec<DetectedElement>,
    threshold: f32,
) -> Vec<DetectedElement> {
    // Already sorted by confidence descending from parse_yolo_output,
    // but sort again to be safe.
    detections.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

    let mut keep: Vec<DetectedElement> = Vec::new();

    for det in detections {
        let dominated = keep
            .iter()
            .any(|kept| iou(&kept.bbox, &det.bbox) > threshold);
        if !dominated {
            keep.push(det);
        }
    }

    keep
}

/// Map a class index to an `ElementType`.
pub(crate) fn class_to_element_type(class_idx: usize) -> ElementType {
    match class_idx {
        0 => ElementType::Button,
        1 => ElementType::Input,
        2 => ElementType::Link,
        3 => ElementType::Dropdown,
        4 => ElementType::Toggle,
        5 => ElementType::Checkbox,
        6 => ElementType::Image,
        7 => ElementType::Icon,
        8 => ElementType::Text,
        9 => ElementType::Nav,
        10 => ElementType::Tab,
        _ => ElementType::Unknown,
    }
}

/// Default class names for the OmniParser YOLO model.
pub(crate) fn default_class_names() -> Vec<String> {
    vec![
        "button".into(),
        "input".into(),
        "link".into(),
        "dropdown".into(),
        "toggle".into(),
        "checkbox".into(),
        "image".into(),
        "icon".into(),
        "text".into(),
        "nav".into(),
        "tab".into(),
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounding_box_center() {
        let bbox = BoundingBox {
            x: 0.1,
            y: 0.2,
            width: 0.4,
            height: 0.6,
        };
        let (cx, cy) = bbox.center();
        assert!((cx - 0.3).abs() < 0.001);
        assert!((cy - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_iou_identical() {
        let b = BoundingBox {
            x: 0.1,
            y: 0.1,
            width: 0.5,
            height: 0.5,
        };
        assert!((iou(&b, &b) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_iou_no_overlap() {
        let a = BoundingBox {
            x: 0.0,
            y: 0.0,
            width: 0.1,
            height: 0.1,
        };
        let b = BoundingBox {
            x: 0.5,
            y: 0.5,
            width: 0.1,
            height: 0.1,
        };
        assert!((iou(&a, &b)).abs() < 0.001);
    }

    #[test]
    fn test_nms_removes_overlapping() {
        // Two overlapping detections of same area, different confidence
        // NMS should keep only the higher confidence one
        let detections = vec![
            DetectedElement {
                element_type: ElementType::Button,
                bbox: BoundingBox {
                    x: 0.1,
                    y: 0.1,
                    width: 0.5,
                    height: 0.5,
                },
                confidence: 0.9,
                label: Some("button".into()),
            },
            DetectedElement {
                element_type: ElementType::Button,
                bbox: BoundingBox {
                    x: 0.1,
                    y: 0.1,
                    width: 0.5,
                    height: 0.5,
                },
                confidence: 0.7,
                label: Some("button".into()),
            },
        ];

        let result = non_max_suppression(detections, 0.45);
        assert_eq!(result.len(), 1);
        assert!((result[0].confidence - 0.9).abs() < 0.001);
    }

    #[test]
    fn test_class_to_element_type() {
        assert_eq!(class_to_element_type(0), ElementType::Button);
        assert_eq!(class_to_element_type(1), ElementType::Input);
        assert_eq!(class_to_element_type(99), ElementType::Unknown);
    }

    #[test]
    fn test_try_load_missing_model() {
        let result = ElementDetector::try_load(std::path::Path::new("/nonexistent"));
        assert!(result.is_none());
    }
}
