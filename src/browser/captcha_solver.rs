//! Advanced CAPTCHA Solver — tiered approach:
//!
//! 1. VLM (vision language model) — screenshot the CAPTCHA, ask the model to solve it
//! 2. CapSolver (external API) — if VLM fails AND user configured capsolver_api_key
//! 3. Give up — return SolveResult::Failed
//!
//! The entire feature is opt-in. If CaptchaSolverConfig is absent or
//! enabled=false, CaptchaSolver::solve() returns Failed immediately.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::browser::captcha::{CaptchaDetection, CaptchaType};
use crate::core::error::Result;
use crate::llm_router::provider::{ContentBlock, ImageSource, LlmProvider};
use crate::security::constitution::CaptchaSolverConfig;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result of a CAPTCHA solve attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveResult {
    /// CAPTCHA was solved successfully.
    Solved {
        method: SolveMethod,
        token: Option<String>,
    },
    /// All tiers failed or feature is disabled.
    Failed { reason: String },
}

/// Which method solved the CAPTCHA.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolveMethod {
    Vlm,
    CapSolver,
}

// ---------------------------------------------------------------------------
// CaptchaSolver
// ---------------------------------------------------------------------------

/// Tiered CAPTCHA solver — VLM first, CapSolver fallback, entirely opt-in.
#[derive(Default)]
pub struct CaptchaSolver {
    config: Option<CaptchaSolverConfig>,
    llm_provider: Option<Arc<LlmProvider>>,
}

impl CaptchaSolver {
    pub fn new(config: Option<CaptchaSolverConfig>) -> Self {
        Self {
            config,
            llm_provider: None,
        }
    }

    pub fn with_llm_provider(mut self, provider: Arc<LlmProvider>) -> Self {
        self.llm_provider = Some(provider);
        self
    }

    /// Is advanced solving enabled at all?
    pub fn is_enabled(&self) -> bool {
        self.config.as_ref().map(|c| c.enabled).unwrap_or(false)
    }

    /// Attempt to solve the detected CAPTCHA.
    ///
    /// Tier 1: VLM screenshot analysis (if vlm_enabled).
    /// Tier 2: CapSolver API (if capsolver_api_key configured and VLM failed).
    /// Returns Failed if neither succeeds or feature is disabled.
    pub fn solve(
        &self,
        detection: &CaptchaDetection,
        screenshot_base64: Option<&str>,
    ) -> Result<SolveResult> {
        let config = match &self.config {
            Some(c) if c.enabled => c,
            _ => {
                return Ok(SolveResult::Failed {
                    reason: "Advanced CAPTCHA solving not enabled".into(),
                })
            }
        };

        // Tier 1: VLM attempt
        if config.vlm_enabled {
            if let Some(screenshot) = screenshot_base64 {
                match self.try_vlm(detection, screenshot) {
                    Ok(result @ SolveResult::Solved { .. }) => return Ok(result),
                    Ok(SolveResult::Failed { .. }) => { /* fall through to tier 2 */ }
                    Err(_) => { /* VLM error, fall through */ }
                }
            }
        }

        // Tier 2: CapSolver (only if configured)
        if let Some(api_key) = config.capsolver_api_key.as_deref() {
            if !api_key.is_empty() {
                return self.try_capsolver(detection, api_key);
            }
        }

        Ok(SolveResult::Failed {
            reason: "All CAPTCHA solving tiers exhausted".into(),
        })
    }

    /// Tier 1: VLM-based solving — sends the screenshot to LlmProvider::complete_with_images.
    pub(crate) fn try_vlm(
        &self,
        detection: &CaptchaDetection,
        screenshot_base64: &str,
    ) -> Result<SolveResult> {
        let provider = match &self.llm_provider {
            Some(p) => p,
            None => {
                return Ok(SolveResult::Failed {
                    reason: "No LlmProvider configured for VLM solving".into(),
                })
            }
        };

        let system_prompt = "You are analyzing a web page screenshot containing a CAPTCHA. \
            Identify the CAPTCHA type and provide the solution. \
            For image CAPTCHAs, describe what you see and provide the answer text. \
            For checkbox CAPTCHAs, confirm the checkbox location. \
            Respond with ONLY the solution token/text, nothing else.";

        let captcha_desc = format!(
            "This screenshot contains a {:?} CAPTCHA (confidence: {:.2}). Solve it.",
            detection.captcha_type, detection.confidence
        );

        let content = vec![
            ContentBlock::Image {
                source: ImageSource {
                    source_type: "base64".into(),
                    media_type: "image/png".into(),
                    data: screenshot_base64.to_string(),
                },
            },
            ContentBlock::Text { text: captcha_desc },
        ];

        match provider.complete_with_images(system_prompt, content, None) {
            Ok(response) => {
                let token = response.text.trim().to_string();
                if token.is_empty() {
                    Ok(SolveResult::Failed {
                        reason: "VLM returned empty response".into(),
                    })
                } else {
                    Ok(SolveResult::Solved {
                        method: SolveMethod::Vlm,
                        token: Some(token),
                    })
                }
            }
            Err(e) => Ok(SolveResult::Failed {
                reason: format!("VLM call failed: {}", e),
            }),
        }
    }

    /// Tier 2: CapSolver external API — creates a task and polls for the result.
    pub(crate) fn try_capsolver(
        &self,
        detection: &CaptchaDetection,
        api_key: &str,
    ) -> Result<SolveResult> {
        let task_type = match detection.captcha_type {
            CaptchaType::RecaptchaCheckbox | CaptchaType::RecaptchaV2 => "ReCaptchaV2TaskProxyLess",
            CaptchaType::HCaptcha => "HCaptchaTaskProxyLess",
            CaptchaType::Turnstile => "AntiTurnstileTaskProxyLess",
            CaptchaType::Unknown => {
                return Ok(SolveResult::Failed {
                    reason: "Unknown CAPTCHA type cannot be sent to CapSolver".into(),
                })
            }
        };

        let base_url = self
            .config
            .as_ref()
            .and_then(|c| c.capsolver_base_url.as_deref())
            .unwrap_or("https://api.capsolver.com");

        let client = reqwest::blocking::Client::new();

        // Create task
        let create_body = serde_json::json!({
            "clientKey": api_key,
            "task": { "type": task_type }
        });

        let create_resp = match client
            .post(format!("{}/createTask", base_url))
            .json(&create_body)
            .send()
        {
            Ok(r) => r,
            Err(e) => {
                return Ok(SolveResult::Failed {
                    reason: format!("CapSolver createTask failed: {}", e),
                })
            }
        };

        let create_json: serde_json::Value = match create_resp.json() {
            Ok(j) => j,
            Err(e) => {
                return Ok(SolveResult::Failed {
                    reason: format!("CapSolver parse failed: {}", e),
                })
            }
        };

        let task_id = match create_json.get("taskId").and_then(|t| t.as_str()) {
            Some(id) => id.to_string(),
            None => {
                let error_desc = create_json
                    .get("errorDescription")
                    .and_then(|e| e.as_str())
                    .unwrap_or("unknown error");
                return Ok(SolveResult::Failed {
                    reason: format!("CapSolver createTask error: {}", error_desc),
                });
            }
        };

        // Poll for result
        let max_attempts = self
            .config
            .as_ref()
            .map(|c| c.max_attempts.max(1))
            .unwrap_or(40);
        let poll_body = serde_json::json!({
            "clientKey": api_key,
            "taskId": task_id,
        });

        for _ in 0..max_attempts {
            std::thread::sleep(std::time::Duration::from_secs(3));

            let poll_resp = match client
                .post(format!("{}/getTaskResult", base_url))
                .json(&poll_body)
                .send()
            {
                Ok(r) => r,
                Err(e) => {
                    return Ok(SolveResult::Failed {
                        reason: format!("CapSolver poll failed: {}", e),
                    })
                }
            };

            let poll_json: serde_json::Value = match poll_resp.json() {
                Ok(j) => j,
                Err(e) => {
                    return Ok(SolveResult::Failed {
                        reason: format!("CapSolver poll parse failed: {}", e),
                    })
                }
            };

            let status = poll_json
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            if status == "ready" {
                let solution = &poll_json["solution"];
                let token = solution
                    .get("gRecaptchaResponse")
                    .or_else(|| solution.get("token"))
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string());
                return Ok(SolveResult::Solved {
                    method: SolveMethod::CapSolver,
                    token,
                });
            } else if status == "failed" {
                let reason = poll_json
                    .get("errorDescription")
                    .and_then(|e| e.as_str())
                    .unwrap_or("Task failed");
                return Ok(SolveResult::Failed {
                    reason: format!("CapSolver: {}", reason),
                });
            }
        }

        Ok(SolveResult::Failed {
            reason: format!("CapSolver {} timed out after polling", task_type),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::captcha::CaptchaStrategy;

    fn make_detection() -> CaptchaDetection {
        CaptchaDetection {
            captcha_type: CaptchaType::RecaptchaV2,
            strategy: CaptchaStrategy::HumanInLoop,
            confidence: 0.92,
            selector: Some(".g-recaptcha".into()),
        }
    }

    #[test]
    fn test_solver_disabled_by_default() {
        let solver = CaptchaSolver::default();
        assert!(!solver.is_enabled());
        let result = solver.solve(&make_detection(), None).unwrap();
        assert!(matches!(result, SolveResult::Failed { .. }));
    }

    #[test]
    fn test_solver_disabled_explicitly() {
        let solver = CaptchaSolver::new(Some(CaptchaSolverConfig {
            enabled: false,
            ..Default::default()
        }));
        assert!(!solver.is_enabled());
        let result = solver.solve(&make_detection(), Some("base64data")).unwrap();
        match result {
            SolveResult::Failed { reason } => assert!(reason.contains("not enabled")),
            _ => panic!("Expected Failed"),
        }
    }

    #[test]
    fn test_solver_vlm_first() {
        let solver = CaptchaSolver::new(Some(CaptchaSolverConfig {
            enabled: true,
            vlm_enabled: true,
            capsolver_api_key: None,
            capsolver_base_url: None,
            max_attempts: 3,
        }));
        assert!(solver.is_enabled());
        // VLM placeholder returns Failed, no capsolver configured => overall Failed
        let result = solver
            .solve(&make_detection(), Some("screenshot_data"))
            .unwrap();
        match result {
            SolveResult::Failed { reason } => assert!(reason.contains("exhausted")),
            _ => panic!("Expected Failed after VLM placeholder"),
        }
    }

    #[test]
    fn test_solver_capsolver_fallback() {
        let solver = CaptchaSolver::new(Some(CaptchaSolverConfig {
            enabled: true,
            vlm_enabled: true,
            capsolver_api_key: Some("test-key-123".into()),
            capsolver_base_url: None,
            max_attempts: 3,
        }));
        // VLM fails, then CapSolver placeholder runs
        let result = solver
            .solve(&make_detection(), Some("screenshot_data"))
            .unwrap();
        match result {
            SolveResult::Failed { reason } => assert!(reason.contains("CapSolver")),
            _ => panic!("Expected Failed from CapSolver placeholder"),
        }
    }

    #[test]
    fn test_solver_no_capsolver_without_key() {
        let solver = CaptchaSolver::new(Some(CaptchaSolverConfig {
            enabled: true,
            vlm_enabled: false,      // VLM disabled
            capsolver_api_key: None, // No CapSolver key
            capsolver_base_url: None,
            max_attempts: 3,
        }));
        let result = solver
            .solve(&make_detection(), Some("screenshot_data"))
            .unwrap();
        match result {
            SolveResult::Failed { reason } => assert!(reason.contains("exhausted")),
            _ => panic!("Expected Failed"),
        }
    }

    #[test]
    fn test_captcha_solver_config_serde() {
        let yaml = r#"
enabled: true
vlm_enabled: true
capsolver_api_key: "cap-key-abc"
capsolver_base_url: "https://custom.capsolver.com"
max_attempts: 5
"#;
        let config: CaptchaSolverConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.enabled);
        assert!(config.vlm_enabled);
        assert_eq!(config.capsolver_api_key.as_deref(), Some("cap-key-abc"));
        assert_eq!(config.max_attempts, 5);

        // Roundtrip
        let yaml_out = serde_yaml::to_string(&config).unwrap();
        let config2: CaptchaSolverConfig = serde_yaml::from_str(&yaml_out).unwrap();
        assert_eq!(config2.capsolver_api_key, config.capsolver_api_key);
    }

    #[test]
    fn test_vlm_no_provider_returns_failed() {
        let solver = CaptchaSolver::new(Some(CaptchaSolverConfig {
            enabled: true,
            vlm_enabled: true,
            capsolver_api_key: None,
            capsolver_base_url: None,
            max_attempts: 3,
        }));
        // No llm_provider — try_vlm returns Failed
        let result = solver.try_vlm(&make_detection(), "fake_base64").unwrap();
        match result {
            SolveResult::Failed { reason } => assert!(reason.contains("No LlmProvider")),
            _ => panic!("Expected Failed"),
        }
    }

    #[test]
    fn test_capsolver_unknown_type_rejected() {
        let solver = CaptchaSolver::new(Some(CaptchaSolverConfig {
            enabled: true,
            vlm_enabled: false,
            capsolver_api_key: Some("test-key".into()),
            capsolver_base_url: None,
            max_attempts: 3,
        }));
        let mut detection = make_detection();
        detection.captcha_type = CaptchaType::Unknown;
        let result = solver.try_capsolver(&detection, "test-key").unwrap();
        match result {
            SolveResult::Failed { reason } => assert!(reason.contains("Unknown")),
            _ => panic!("Expected Failed for Unknown type"),
        }
    }
}
