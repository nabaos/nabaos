//! CAPTCHA Detection and Resolution Handler.
//!
//! Detects common CAPTCHA types (reCAPTCHA, hCaptcha, Cloudflare Turnstile)
//! from DOM elements and recommends a handling strategy.

use serde::{Deserialize, Serialize};

use crate::browser::dom_heuristics::{element_to_selector, DomElement};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Types of CAPTCHAs that can be detected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptchaType {
    /// Simple checkbox "I'm not a robot".
    RecaptchaCheckbox,
    /// Image selection puzzle.
    RecaptchaV2,
    /// hCaptcha challenge.
    HCaptcha,
    /// Cloudflare Turnstile.
    Turnstile,
    /// Generic CAPTCHA detected but type unclear.
    Unknown,
}

/// Strategy for handling a detected CAPTCHA.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptchaStrategy {
    /// Try to solve via vision model (simple checkboxes).
    VisionAttempt,
    /// Escalate to human via Telegram.
    HumanInLoop,
    /// Skip and try alternate route.
    Skip,
}

/// Result of CAPTCHA detection on a page.
#[derive(Debug, Clone)]
pub struct CaptchaDetection {
    pub captcha_type: CaptchaType,
    pub strategy: CaptchaStrategy,
    pub confidence: f32,
    /// CSS selector to the CAPTCHA element.
    pub selector: Option<String>,
}

// ---------------------------------------------------------------------------
// Detector
// ---------------------------------------------------------------------------

/// Detects CAPTCHA presence from DOM elements and recommends handling strategies.
pub struct CaptchaDetector;

impl CaptchaDetector {
    pub fn new() -> Self {
        Self
    }

    /// Detect CAPTCHA presence from DOM elements.
    ///
    /// Checks for reCAPTCHA, hCaptcha, Cloudflare Turnstile, and generic
    /// CAPTCHA indicators in order. Returns the first match found.
    pub fn detect(&self, elements: &[DomElement]) -> Option<CaptchaDetection> {
        if let Some(det) = self.detect_recaptcha(elements) {
            return Some(det);
        }
        if let Some(det) = self.detect_hcaptcha(elements) {
            return Some(det);
        }
        if let Some(det) = self.detect_turnstile(elements) {
            return Some(det);
        }
        if let Some(det) = self.detect_generic_captcha(elements) {
            return Some(det);
        }
        None
    }

    /// Determine strategy based on CAPTCHA type.
    pub fn strategy_for(captcha_type: &CaptchaType) -> CaptchaStrategy {
        match captcha_type {
            CaptchaType::RecaptchaCheckbox => CaptchaStrategy::VisionAttempt,
            CaptchaType::Turnstile => CaptchaStrategy::VisionAttempt,
            CaptchaType::RecaptchaV2 => CaptchaStrategy::HumanInLoop,
            CaptchaType::HCaptcha => CaptchaStrategy::HumanInLoop,
            CaptchaType::Unknown => CaptchaStrategy::Skip,
        }
    }

    /// Detect reCAPTCHA: iframe with src/href containing "recaptcha",
    /// div with class "g-recaptcha", or iframe with class containing "recaptcha".
    fn detect_recaptcha(&self, elements: &[DomElement]) -> Option<CaptchaDetection> {
        for el in elements {
            let classes_lower: Vec<String> = el.classes.iter().map(|c| c.to_lowercase()).collect();

            // div with class "g-recaptcha" => checkbox style
            if el.tag == "div" && classes_lower.iter().any(|c| c == "g-recaptcha") {
                let captcha_type = CaptchaType::RecaptchaCheckbox;
                return Some(CaptchaDetection {
                    strategy: Self::strategy_for(&captcha_type),
                    captcha_type,
                    confidence: 0.95,
                    selector: Some(element_to_selector(el)),
                });
            }

            // iframe with href containing "recaptcha" (href used as src stand-in)
            if el.tag == "iframe" {
                let href_match = el
                    .href
                    .as_deref()
                    .map(|h| h.to_lowercase().contains("recaptcha"))
                    .unwrap_or(false);

                let class_match = classes_lower.iter().any(|c| c.contains("recaptcha"));

                if href_match || class_match {
                    // If href contains "anchor" or class hints at checkbox, it's likely checkbox;
                    // otherwise treat as full v2 puzzle.
                    let is_checkbox = el
                        .href
                        .as_deref()
                        .map(|h| h.to_lowercase().contains("anchor"))
                        .unwrap_or(false);

                    let captcha_type = if is_checkbox {
                        CaptchaType::RecaptchaCheckbox
                    } else {
                        CaptchaType::RecaptchaV2
                    };

                    return Some(CaptchaDetection {
                        strategy: Self::strategy_for(&captcha_type),
                        captcha_type,
                        confidence: 0.92,
                        selector: Some(element_to_selector(el)),
                    });
                }
            }
        }
        None
    }

    /// Detect hCaptcha: div with class "h-captcha" or iframe with href
    /// containing "hcaptcha.com".
    fn detect_hcaptcha(&self, elements: &[DomElement]) -> Option<CaptchaDetection> {
        for el in elements {
            let classes_lower: Vec<String> = el.classes.iter().map(|c| c.to_lowercase()).collect();

            if el.tag == "div" && classes_lower.iter().any(|c| c == "h-captcha") {
                let captcha_type = CaptchaType::HCaptcha;
                return Some(CaptchaDetection {
                    strategy: Self::strategy_for(&captcha_type),
                    captcha_type,
                    confidence: 0.95,
                    selector: Some(element_to_selector(el)),
                });
            }

            if el.tag == "iframe" {
                let href_match = el
                    .href
                    .as_deref()
                    .map(|h| h.to_lowercase().contains("hcaptcha.com"))
                    .unwrap_or(false);

                if href_match {
                    let captcha_type = CaptchaType::HCaptcha;
                    return Some(CaptchaDetection {
                        strategy: Self::strategy_for(&captcha_type),
                        captcha_type,
                        confidence: 0.92,
                        selector: Some(element_to_selector(el)),
                    });
                }
            }
        }
        None
    }

    /// Detect Cloudflare Turnstile: div with class "cf-turnstile".
    fn detect_turnstile(&self, elements: &[DomElement]) -> Option<CaptchaDetection> {
        for el in elements {
            if el.tag == "div" {
                let classes_lower: Vec<String> =
                    el.classes.iter().map(|c| c.to_lowercase()).collect();

                if classes_lower.iter().any(|c| c == "cf-turnstile") {
                    let captcha_type = CaptchaType::Turnstile;
                    return Some(CaptchaDetection {
                        strategy: Self::strategy_for(&captcha_type),
                        captcha_type,
                        confidence: 0.93,
                        selector: Some(element_to_selector(el)),
                    });
                }
            }
        }
        None
    }

    /// Detect generic CAPTCHA by looking for elements with text, class, or id
    /// containing "captcha" (case-insensitive).
    fn detect_generic_captcha(&self, elements: &[DomElement]) -> Option<CaptchaDetection> {
        for el in elements {
            let text_lower = el.text.to_lowercase();
            let id_lower = el.id.as_deref().unwrap_or("").to_lowercase();
            let classes_lower: String = el.classes.join(" ").to_lowercase();

            if text_lower.contains("captcha")
                || id_lower.contains("captcha")
                || classes_lower.contains("captcha")
            {
                let captcha_type = CaptchaType::Unknown;
                return Some(CaptchaDetection {
                    strategy: Self::strategy_for(&captcha_type),
                    captcha_type,
                    confidence: 0.60,
                    selector: Some(element_to_selector(el)),
                });
            }
        }
        None
    }
}

impl Default for CaptchaDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_element(tag: &str) -> DomElement {
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

    #[test]
    fn test_detect_recaptcha_iframe() {
        let detector = CaptchaDetector::new();

        // iframe with g-recaptcha class
        let mut iframe = make_element("iframe");
        iframe.classes = vec!["g-recaptcha".into()];

        let result = detector.detect(&[iframe]);
        assert!(result.is_some());
        let det = result.unwrap();
        assert!(
            det.captcha_type == CaptchaType::RecaptchaCheckbox
                || det.captcha_type == CaptchaType::RecaptchaV2
        );
        assert!(det.confidence > 0.8);
        assert!(det.selector.is_some());

        // div with g-recaptcha class
        let mut div = make_element("div");
        div.classes = vec!["g-recaptcha".into()];

        let result2 = detector.detect(&[div]);
        assert!(result2.is_some());
        assert_eq!(
            result2.unwrap().captcha_type,
            CaptchaType::RecaptchaCheckbox
        );
    }

    #[test]
    fn test_detect_no_captcha() {
        let detector = CaptchaDetector::new();

        let button = make_element("button");
        let mut input = make_element("input");
        input.input_type = Some("text".into());
        input.name = Some("username".into());
        let div = make_element("div");

        let result = detector.detect(&[button, input, div]);
        assert!(result.is_none());
    }

    #[test]
    fn test_captcha_strategy_defaults() {
        assert_eq!(
            CaptchaDetector::strategy_for(&CaptchaType::RecaptchaCheckbox),
            CaptchaStrategy::VisionAttempt
        );
        assert_eq!(
            CaptchaDetector::strategy_for(&CaptchaType::RecaptchaV2),
            CaptchaStrategy::HumanInLoop
        );
        assert_eq!(
            CaptchaDetector::strategy_for(&CaptchaType::HCaptcha),
            CaptchaStrategy::HumanInLoop
        );
        assert_eq!(
            CaptchaDetector::strategy_for(&CaptchaType::Turnstile),
            CaptchaStrategy::VisionAttempt
        );
        assert_eq!(
            CaptchaDetector::strategy_for(&CaptchaType::Unknown),
            CaptchaStrategy::Skip
        );
    }
}
