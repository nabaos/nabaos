use serde::{Deserialize, Serialize};

// PeaTask is the primary input type routed by this executor.
#[allow(unused_imports)]
use crate::pea::objective::PeaTask;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskRoute {
    Llm,
    Swarm,
    Media,
    FileSystem,
    Channel,
    Browser,
    ApiService,
    Schedule,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CapabilityStatus {
    Available,
    Missing { hint: String },
    Discoverable { api_name: String },
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub success: bool,
    pub output: String,
    pub artifacts: Vec<String>,
    pub cost_usd: f64,
}

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

/// Keyword-based classification of a task description into a [`TaskRoute`].
///
/// Order matters: more specific routes (Swarm, Media, Browser, Channel,
/// FileSystem, ApiService, Schedule) are checked before the broad Llm route.
pub fn classify_task(description: &str) -> TaskRoute {
    let lower = description.to_lowercase();

    // Swarm
    if lower.contains("research")
        || lower.contains("search web")
        || lower.contains("investigate")
        || lower.contains("find sources")
    {
        return TaskRoute::Swarm;
    }

    // Media
    if lower.contains("image")
        || lower.contains("video")
        || lower.contains("audio")
        || lower.contains("visual")
        || lower.contains("photo")
        || lower.contains("illustration")
        || lower.contains("media engine")
    {
        return TaskRoute::Media;
    }

    // Browser
    if lower.contains("browse")
        || lower.contains("navigate")
        || lower.contains("scrape")
        || lower.contains("visit url")
        || lower.contains("web page")
    {
        return TaskRoute::Browser;
    }

    // Channel
    if lower.contains("post to")
        || (lower.starts_with("post ") || lower.contains(" post "))
        || lower.contains("announce")
        || lower.contains("send message")
        || lower.contains("share on")
        || lower.contains("email")
    {
        return TaskRoute::Channel;
    }

    // FileSystem
    if lower.contains("read file")
        || lower.contains("write file")
        || lower.contains("save")
        || lower.contains("export")
        || lower.contains("format")
        || lower.contains("layout")
    {
        return TaskRoute::FileSystem;
    }

    // ApiService
    if lower.contains("api") || lower.contains("call service") || lower.contains("upload to") {
        return TaskRoute::ApiService;
    }

    // Schedule
    if lower.contains("schedule")
        || lower.contains("recurring")
        || lower.contains("every day")
        || lower.contains("monitor")
    {
        return TaskRoute::Schedule;
    }

    // Llm (broad catch-all for text generation tasks)
    if lower.contains("write")
        || lower.contains("draft")
        || lower.contains("summarize")
        || lower.contains("outline")
        || lower.contains("review")
        || lower.contains("plan")
        || lower.contains("create")
        || lower.contains("compose")
        || lower.contains("analyze")
        || lower.contains("synthesize")
        || lower.contains("compile")
        || lower.contains("select")
    {
        return TaskRoute::Llm;
    }

    TaskRoute::Unknown
}

/// Check whether the capability required by a task is available.
///
/// * If `capability_required` is `Some("media_engine")`, checks for known
///   media-related environment variables.
/// * If `capability_required` is `Some(other)`, returns `Discoverable`.
/// * If `capability_required` is `None`, performs an optimistic check based on
///   the route (only [`TaskRoute::Llm`] requires `NABA_LLM_API_KEY`).
pub fn check_capability(route: &TaskRoute, capability_required: Option<&str>) -> CapabilityStatus {
    match capability_required {
        Some("media_engine") => {
            if std::env::var("NABA_FAL_API_KEY").is_ok()
                || std::env::var("NABA_LLM_API_KEY").is_ok()
                || std::env::var("NABA_COMFYUI_URL").is_ok()
            {
                CapabilityStatus::Available
            } else {
                CapabilityStatus::Missing {
                    hint: "Set NABA_FAL_API_KEY, NABA_LLM_API_KEY, or NABA_COMFYUI_URL".to_string(),
                }
            }
        }
        Some(cap) => CapabilityStatus::Discoverable {
            api_name: cap.to_string(),
        },
        None => match route {
            TaskRoute::Llm => {
                if std::env::var("NABA_LLM_API_KEY").is_ok() {
                    CapabilityStatus::Available
                } else {
                    CapabilityStatus::Missing {
                        hint: "Set NABA_LLM_API_KEY".to_string(),
                    }
                }
            }
            _ => CapabilityStatus::Available,
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_research_task() {
        let route = classify_task("Search web for Indian recipes");
        assert_eq!(route, TaskRoute::Swarm);
    }

    #[test]
    fn test_classify_image_task() {
        let route = classify_task("Generate dish images using media engine");
        assert_eq!(route, TaskRoute::Media);
    }

    #[test]
    fn test_classify_write_task() {
        let route = classify_task("Write introduction chapter");
        assert_eq!(route, TaskRoute::Llm);
    }

    #[test]
    fn test_classify_post_task() {
        let route = classify_task("Post recipe teasers to Telegram");
        assert_eq!(route, TaskRoute::Channel);
    }

    #[test]
    fn test_classify_unknown_task() {
        let route = classify_task("dance the macarena");
        assert_eq!(route, TaskRoute::Unknown);
    }

    #[test]
    fn test_capability_check_media_missing() {
        // Clear the relevant env vars to ensure Missing result.
        unsafe { std::env::remove_var("NABA_FAL_API_KEY"); }
        unsafe { std::env::remove_var("NABA_LLM_API_KEY"); }
        unsafe { std::env::remove_var("NABA_COMFYUI_URL"); }

        let status = check_capability(&TaskRoute::Media, Some("media_engine"));
        match status {
            CapabilityStatus::Missing { hint } => {
                assert!(hint.contains("NABA_FAL_API_KEY"));
            }
            other => panic!("Expected Missing, got {:?}", other),
        }
    }
}
