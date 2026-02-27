//! Navigation Cascade — Layers 0-3 unified.
//!
//! Orchestrates all 4 navigation layers and records training data
//! for self-improvement.
//!
//! - **Layer 0**: DOM heuristics (rule-based, <1ms)
//! - **Layer 1**: YOLO element detection (vision, ~50ms)
//! - **Layer 2**: WebBERT action classification (~5-10ms)
//! - **Layer 3**: LLM fallback (uses LlmProvider if configured, else Skip)

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::core::error::Result;
use crate::llm_router::provider::LlmProvider;

use super::captcha_solver::CaptchaSolver;
use super::dom_heuristics::{
    dom_heuristic_action, ActionDecision, DomElement, NavAction, ScrollDirection, TaskContext,
};
use super::element_detector::{DetectedElement, ElementDetector};
use super::web_bert::{WebBertClassifier, WebBertElement, WebBertPrediction};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Statistics tracking for the cascade decision pipeline.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CascadeStats {
    pub l0_decisions: u32,
    pub l1_detections: u32,
    pub l2_classifications: u32,
    pub l3_fallbacks: u32,
    pub total_nav_time_ms: u64,
}

/// A training data entry recorded when the cascade reaches Layer 3 (LLM fallback).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CascadeTrainingEntry {
    pub task_context: String,
    pub page_url: String,
    pub elements_json: String,
    pub action_type: String,
    pub target_element_idx: Option<usize>,
    pub source: String,
    pub created_at: i64,
}

// ---------------------------------------------------------------------------
// NavCascade
// ---------------------------------------------------------------------------

/// The 4-layer navigation cascade.
///
/// Tries each layer in order (L0 -> L1 -> L2 -> L3) and returns the first
/// confident decision. Records training data when L3 (LLM) is reached so
/// that earlier layers can be improved over time.
pub struct NavCascade {
    element_detector: Option<ElementDetector>,
    web_bert: Option<WebBertClassifier>,
    llm_provider: Option<Arc<LlmProvider>>,
    captcha_solver: CaptchaSolver,
    stats: CascadeStats,
    training_log: Vec<CascadeTrainingEntry>,
    l0_threshold: f32,
    l2_threshold: f32,
}

impl NavCascade {
    /// Create a new cascade with optional detector and classifier.
    pub fn new(
        element_detector: Option<ElementDetector>,
        web_bert: Option<WebBertClassifier>,
    ) -> Self {
        Self {
            element_detector,
            web_bert,
            llm_provider: None,
            captcha_solver: CaptchaSolver::default(),
            stats: CascadeStats::default(),
            training_log: Vec::new(),
            l0_threshold: 0.90,
            l2_threshold: 0.85,
        }
    }

    /// Attach an LLM provider for Layer 3 fallback.
    pub fn with_llm(mut self, provider: Arc<LlmProvider>) -> Self {
        self.llm_provider = Some(provider);
        self
    }

    /// Attach a CAPTCHA solver.
    pub fn with_captcha_solver(mut self, solver: CaptchaSolver) -> Self {
        self.captcha_solver = solver;
        self
    }

    /// Get a reference to the CAPTCHA solver.
    pub fn captcha_solver(&self) -> &CaptchaSolver {
        &self.captcha_solver
    }

    /// Run the cascade to decide the next navigation action.
    ///
    /// Layers are tried in order:
    /// - L0: DOM heuristics (if confidence >= l0_threshold)
    /// - L1: YOLO element detection (if detector available + screenshot provided)
    /// - L2: WebBERT classification (if classifier available + elements found)
    /// - L3: LLM fallback (calls LLM with DOM elements + goal, parses JSON nav action; Skip if no provider)
    pub fn decide_action(
        &mut self,
        task: &TaskContext,
        dom_elements: &[DomElement],
        screenshot_bytes: Option<&[u8]>,
    ) -> Result<ActionDecision> {
        let start = std::time::Instant::now();

        // ----- Layer 0: DOM heuristics -----
        if let Some(decision) = dom_heuristic_action(task, dom_elements) {
            if decision.confidence >= self.l0_threshold {
                self.stats.l0_decisions += 1;
                self.stats.total_nav_time_ms += start.elapsed().as_millis() as u64;
                return Ok(decision);
            }
        }

        // ----- Layer 1: YOLO element detection -----
        let mut detected_elements: Vec<DetectedElement> = Vec::new();

        if let (Some(detector), Some(screenshot)) =
            (self.element_detector.as_mut(), screenshot_bytes)
        {
            detected_elements = detector.detect(screenshot)?;
            self.stats.l1_detections += 1;
        }

        // ----- Layer 2: WebBERT classification -----
        if let Some(classifier) = self.web_bert.as_mut() {
            if !detected_elements.is_empty() {
                let wb_elements = detected_to_webbert(&detected_elements);
                let prediction = classifier.classify(&task.goal, &task.page_url, &wb_elements)?;

                if prediction.confident && prediction.confidence >= self.l2_threshold {
                    self.stats.l2_classifications += 1;
                    let action = webbert_prediction_to_action(&prediction, &detected_elements);
                    self.stats.total_nav_time_ms += start.elapsed().as_millis() as u64;
                    return Ok(ActionDecision {
                        action,
                        confidence: prediction.confidence,
                        source: "cascade:webbert_l2",
                    });
                }
            }
        }

        // ----- Layer 3: LLM fallback -----
        self.stats.l3_fallbacks += 1;

        // Record training data for self-improvement
        let elements_json = serde_json::to_string(dom_elements).unwrap_or_default();
        self.training_log.push(CascadeTrainingEntry {
            task_context: task.goal.clone(),
            page_url: task.page_url.clone(),
            elements_json: elements_json.clone(),
            action_type: "l3_attempt".to_string(),
            target_element_idx: None,
            source: "cascade:l3_fallback".to_string(),
            created_at: chrono::Utc::now().timestamp(),
        });

        // Try LLM if provider is configured
        if let Some(provider) = &self.llm_provider {
            // Safe truncation: find a char boundary at or before 2000 bytes
            let max_bytes = 2000.min(elements_json.len());
            let end = elements_json.floor_char_boundary(max_bytes);
            let truncated = &elements_json[..end];
            let prompt = format!(
                "Given the page at {} with goal '{}', and these DOM elements:\n{}\n\n\
                 What navigation action should be taken? Respond ONLY with JSON: \
                 {{\"action\": \"click|type|scroll_down|scroll_up|wait|go_back|extract_content|skip\", \
                 \"selector\": \"css selector if applicable\", \"value\": \"text if type action\"}}",
                task.page_url, task.goal, truncated
            );
            match provider.complete(
                "You are a web navigation assistant. Respond only with JSON.",
                &prompt,
            ) {
                Ok(resp) => {
                    if let Some(action) = parse_llm_nav_response(&resp.text) {
                        self.stats.total_nav_time_ms += start.elapsed().as_millis() as u64;
                        return Ok(ActionDecision {
                            action,
                            confidence: 0.7,
                            source: "cascade:l3_llm",
                        });
                    }
                }
                Err(_) => { /* LLM failed, fall through to skip */ }
            }
        }

        self.stats.total_nav_time_ms += start.elapsed().as_millis() as u64;
        Ok(ActionDecision {
            action: NavAction::Skip {
                reason: "LLM fallback not configured or could not determine action".to_string(),
            },
            confidence: 0.0,
            source: "cascade:l3_fallback",
        })
    }

    /// Get a reference to the cascade statistics.
    pub fn stats(&self) -> &CascadeStats {
        &self.stats
    }

    /// Drain all accumulated training log entries.
    pub fn drain_training_log(&mut self) -> Vec<CascadeTrainingEntry> {
        std::mem::take(&mut self.training_log)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert `DetectedElement`s (from YOLO) to `WebBertElement`s (for WebBERT).
pub fn detected_to_webbert(elements: &[DetectedElement]) -> Vec<WebBertElement> {
    elements
        .iter()
        .map(|det| {
            let (cx, cy) = det.bbox.center();
            WebBertElement {
                element_type: format!("{:?}", det.element_type).to_lowercase(),
                bbox_cx: cx,
                bbox_cy: cy,
                confidence: det.confidence,
                label: det.label.clone(),
            }
        })
        .collect()
}

/// Map a `WebBertPrediction` back to a `NavAction`, using detected elements
/// for target selectors when applicable.
pub fn webbert_prediction_to_action(
    prediction: &WebBertPrediction,
    elements: &[DetectedElement],
) -> NavAction {
    let target_label = prediction
        .target_element_idx
        .and_then(|idx| elements.get(idx))
        .and_then(|el| el.label.clone())
        .unwrap_or_default();

    match prediction.action_class.as_str() {
        "click" | "click_next" | "accept_cookies" | "dismiss_popup" | "submit_form" => {
            NavAction::Click {
                selector: target_label,
            }
        }
        "type" | "fill_form" => NavAction::Type {
            selector: target_label,
            value: String::new(),
        },
        "scroll_down" => NavAction::Scroll {
            direction: ScrollDirection::Down,
        },
        "scroll_up" => NavAction::Scroll {
            direction: ScrollDirection::Up,
        },
        "wait" => NavAction::Wait { ms: 1000 },
        "go_back" => NavAction::GoBack,
        "extract_content" => NavAction::ExtractContent {
            selector: target_label,
        },
        "download" => NavAction::Download { url: target_label },
        "select_dropdown" => NavAction::Click {
            selector: target_label,
        },
        _ => NavAction::Skip {
            reason: format!("Unknown WebBERT class: {}", prediction.action_class),
        },
    }
}

/// Parse an LLM navigation response (JSON) into a NavAction.
///
/// Expected format: `{"action": "click", "selector": "#btn", "value": ""}`
pub fn parse_llm_nav_response(text: &str) -> Option<NavAction> {
    // Try to find JSON in the response (LLM may wrap it in text)
    let json_start = text.find('{')?;
    let json_end = text.rfind('}')? + 1;
    let json_str = &text[json_start..json_end];

    let parsed: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let action_type = parsed["action"].as_str()?;
    let selector = parsed["selector"].as_str().unwrap_or("").to_string();
    let value = parsed["value"].as_str().unwrap_or("").to_string();

    match action_type {
        "click" => Some(NavAction::Click { selector }),
        "type" => Some(NavAction::Type { selector, value }),
        "scroll_down" => Some(NavAction::Scroll {
            direction: ScrollDirection::Down,
        }),
        "scroll_up" => Some(NavAction::Scroll {
            direction: ScrollDirection::Up,
        }),
        "wait" => Some(NavAction::Wait { ms: 1000 }),
        "go_back" => Some(NavAction::GoBack),
        "extract_content" => Some(NavAction::ExtractContent { selector }),
        "skip" => Some(NavAction::Skip { reason: value }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::element_detector::{BoundingBox, ElementType};

    fn make_dom_element(tag: &str) -> DomElement {
        DomElement {
            tag: tag.into(),
            id: None,
            classes: vec![],
            role: None,
            aria_label: None,
            text: String::new(),
            href: None,
            input_type: None,
            name: None,
            placeholder: None,
        }
    }

    fn make_task(goal: &str) -> TaskContext {
        TaskContext {
            goal: goal.into(),
            target_data: None,
            page_url: "https://example.com".into(),
            page_title: "Example".into(),
        }
    }

    #[test]
    fn test_cascade_dom_only() {
        // Cookie banner button should be caught at L0
        let mut cascade = NavCascade::new(None, None);

        let mut btn = make_dom_element("button");
        btn.text = "Accept All".into();
        btn.classes = vec!["cookie-banner-btn".into()];

        let task = make_task("browse the site");
        let result = cascade.decide_action(&task, &[btn], None).unwrap();

        match &result.action {
            NavAction::Click { selector } => {
                assert!(selector.contains("cookie-banner-btn"));
            }
            other => panic!("Expected Click, got {:?}", other),
        }
        assert_eq!(result.source, "dom_heuristic:cookie_banner");
    }

    #[test]
    fn test_cascade_stats_tracking() {
        let mut cascade = NavCascade::new(None, None);

        let mut btn = make_dom_element("button");
        btn.text = "Accept All".into();
        btn.classes = vec!["cookie-banner-btn".into()];

        let task = make_task("browse the site");
        let _ = cascade.decide_action(&task, &[btn], None).unwrap();

        assert_eq!(cascade.stats().l0_decisions, 1);
        assert_eq!(cascade.stats().l1_detections, 0);
        assert_eq!(cascade.stats().l2_classifications, 0);
        assert_eq!(cascade.stats().l3_fallbacks, 0);
    }

    #[test]
    fn test_cascade_all_exhausted() {
        // Plain div with no matching patterns — should fall through to L3 fallback
        let mut cascade = NavCascade::new(None, None);
        let div = make_dom_element("div");
        let task = make_task("do something unusual");

        let result = cascade.decide_action(&task, &[div], None).unwrap();

        match &result.action {
            NavAction::Skip { reason } => {
                assert!(reason.contains("not configured") || reason.contains("could not"));
            }
            other => panic!("Expected Skip, got {:?}", other),
        }
        assert_eq!(cascade.stats().l3_fallbacks, 1);
    }

    #[test]
    fn test_cascade_with_llm_none_still_works() {
        // No LLM provider → same Skip behavior
        let mut cascade = NavCascade::new(None, None);
        assert!(cascade.llm_provider.is_none());

        let div = make_dom_element("div");
        let task = make_task("browse");
        let result = cascade.decide_action(&task, &[div], None).unwrap();
        assert_eq!(result.source, "cascade:l3_fallback");
    }

    #[test]
    fn test_cascade_builder_pattern() {
        use crate::browser::captcha_solver::CaptchaSolver;
        let cascade = NavCascade::new(None, None).with_captcha_solver(CaptchaSolver::default());
        assert!(!cascade.captcha_solver().is_enabled());
    }

    #[test]
    fn test_parse_llm_nav_response_click() {
        let json = r##"{"action": "click", "selector": "#submit-btn"}"##;
        let action = parse_llm_nav_response(json).unwrap();
        match action {
            NavAction::Click { selector } => assert_eq!(selector, "#submit-btn"),
            other => panic!("Expected Click, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_llm_nav_response_with_wrapper_text() {
        let text = r#"Here's the action: {"action": "scroll_down", "selector": ""} and that's it"#;
        let action = parse_llm_nav_response(text).unwrap();
        assert!(matches!(
            action,
            NavAction::Scroll {
                direction: ScrollDirection::Down
            }
        ));
    }

    #[test]
    fn test_parse_llm_nav_response_invalid() {
        assert!(parse_llm_nav_response("not json at all").is_none());
        assert!(parse_llm_nav_response(r#"{"action": "unknown_action"}"#).is_none());
    }

    #[test]
    fn test_detected_to_webbert_conversion() {
        let detected = vec![DetectedElement {
            element_type: ElementType::Button,
            bbox: BoundingBox {
                x: 0.1,
                y: 0.2,
                width: 0.4,
                height: 0.6,
            },
            confidence: 0.95,
            label: Some("Submit".into()),
        }];

        let wb = detected_to_webbert(&detected);
        assert_eq!(wb.len(), 1);
        assert_eq!(wb[0].element_type, "button");
        assert!((wb[0].bbox_cx - 0.3).abs() < 0.001); // center of 0.1 + 0.4/2
        assert!((wb[0].bbox_cy - 0.5).abs() < 0.001); // center of 0.2 + 0.6/2
        assert!((wb[0].confidence - 0.95).abs() < 0.001);
        assert_eq!(wb[0].label.as_deref(), Some("Submit"));
    }

    #[test]
    fn test_webbert_prediction_to_click() {
        let elements = vec![DetectedElement {
            element_type: ElementType::Button,
            bbox: BoundingBox {
                x: 0.1,
                y: 0.2,
                width: 0.3,
                height: 0.1,
            },
            confidence: 0.9,
            label: Some("Login".into()),
        }];

        let prediction = WebBertPrediction {
            action_class: "click".into(),
            target_element_idx: Some(0),
            confidence: 0.92,
            confident: true,
        };

        let action = webbert_prediction_to_action(&prediction, &elements);
        match action {
            NavAction::Click { selector } => {
                assert_eq!(selector, "Login");
            }
            other => panic!("Expected Click, got {:?}", other),
        }
    }

    #[test]
    fn test_training_data_recorded() {
        let mut cascade = NavCascade::new(None, None);
        let div = make_dom_element("div");
        let task = make_task("do something unusual");

        // This will fall through to L3, recording training data
        let _ = cascade.decide_action(&task, &[div], None).unwrap();

        let entries = cascade.drain_training_log();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source, "cascade:l3_fallback");
        assert_eq!(entries[0].task_context, "do something unusual");
        assert_eq!(entries[0].page_url, "https://example.com");

        // After drain, log should be empty
        assert!(cascade.drain_training_log().is_empty());
    }
}
