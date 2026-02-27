//! Shot planner — LLM generates a structured shot list from a user description.

use crate::core::error::{NyayaError, Result};
use serde::{Deserialize, Serialize};

/// A planned video shot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shot {
    pub description: String,
    pub duration_secs: u8,
}

/// Audio plan for the final video.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AudioPlan {
    Narration { script: String, voice: String },
    Music { prompt: String },
    None,
}

/// A complete shot plan for multi-shot video generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotPlan {
    pub shots: Vec<Shot>,
    pub style_prompt: String,
    #[serde(default)]
    pub target_duration_secs: u32,
    pub audio: AudioPlan,
}

impl ShotPlan {
    pub fn total_duration(&self) -> u32 {
        self.shots.iter().map(|s| s.duration_secs as u32).sum()
    }

    pub fn estimated_cost(&self, cost_per_second: f64) -> f64 {
        self.total_duration() as f64 * cost_per_second
    }
}

/// Build the LLM prompt that generates a shot plan from a user description.
pub fn build_shot_plan_prompt(description: &str, target_duration_secs: u32) -> String {
    let max_shots = (target_duration_secs / 5).max(1).min(30);
    format!(
        r#"You are a video director. Create a shot list for a {target_duration_secs}-second video.

User's description: {description}

Output a JSON object with this exact structure:
{{
  "shots": [
    {{"description": "visual description of this shot", "duration_secs": 10}},
    ...
  ],
  "style_prompt": "consistent visual style applied to all shots (e.g., cinematic, 4K, warm color grading)",
  "audio": {{"type": "none"}}
}}

Rules:
- Each shot is 5 or 10 seconds
- Maximum {max_shots} shots
- Total duration should be close to {target_duration_secs} seconds
- Descriptions should be vivid and visual (what the camera SEES)
- Style prompt should ensure visual consistency across all shots
- For audio, use {{"type": "narration", "script": "...", "voice": "alloy"}} or {{"type": "music", "prompt": "..."}} or {{"type": "none"}}
- Output ONLY the JSON, no explanation"#
    )
}

/// Parse the LLM response into a ShotPlan.
pub fn parse_shot_plan(response: &str, target_duration_secs: u32) -> Result<ShotPlan> {
    let json_str = extract_json(response)?;
    let mut plan: ShotPlan = serde_json::from_str(&json_str)
        .map_err(|e| NyayaError::Config(format!("Failed to parse shot plan: {e}")))?;
    plan.target_duration_secs = target_duration_secs;

    for shot in &mut plan.shots {
        if shot.duration_secs < 5 {
            shot.duration_secs = 5;
        }
        if shot.duration_secs > 10 {
            shot.duration_secs = 10;
        }
    }

    if plan.shots.is_empty() {
        return Err(NyayaError::Config("Shot plan has no shots".to_string()));
    }

    Ok(plan)
}

/// Extract JSON object from a string that may contain markdown code fences.
fn extract_json(text: &str) -> Result<String> {
    let trimmed = text.trim();
    if trimmed.starts_with('{') {
        return Ok(trimmed.to_string());
    }
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            return Ok(trimmed[start..=end].to_string());
        }
    }
    Err(NyayaError::Config(
        "No JSON object found in LLM response".to_string(),
    ))
}

/// Build a continuity prompt given the last frame description and next shot.
pub fn build_continuity_prompt(
    frame_description: &str,
    next_shot: &Shot,
    style_prompt: &str,
) -> String {
    format!(
        "{}, {}. The previous scene ended with: {}. Continue naturally.",
        next_shot.description, style_prompt, frame_description
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shot_plan_total_duration() {
        let plan = ShotPlan {
            shots: vec![
                Shot {
                    description: "a".to_string(),
                    duration_secs: 10,
                },
                Shot {
                    description: "b".to_string(),
                    duration_secs: 5,
                },
                Shot {
                    description: "c".to_string(),
                    duration_secs: 10,
                },
            ],
            style_prompt: "cinematic".to_string(),
            target_duration_secs: 25,
            audio: AudioPlan::None,
        };
        assert_eq!(plan.total_duration(), 25);
    }

    #[test]
    fn test_shot_plan_estimated_cost() {
        let plan = ShotPlan {
            shots: vec![
                Shot {
                    description: "a".to_string(),
                    duration_secs: 10,
                },
                Shot {
                    description: "b".to_string(),
                    duration_secs: 10,
                },
            ],
            style_prompt: "cinematic".to_string(),
            target_duration_secs: 20,
            audio: AudioPlan::None,
        };
        let cost = plan.estimated_cost(0.12);
        assert!((cost - 2.40).abs() < 0.01);
    }

    #[test]
    fn test_parse_shot_plan_valid_json() {
        let json = r#"{"shots": [{"description": "sunset", "duration_secs": 10}], "style_prompt": "cinematic", "audio": {"type": "none"}}"#;
        let plan = parse_shot_plan(json, 120).unwrap();
        assert_eq!(plan.shots.len(), 1);
        assert_eq!(plan.shots[0].duration_secs, 10);
        assert_eq!(plan.style_prompt, "cinematic");
    }

    #[test]
    fn test_parse_shot_plan_from_markdown() {
        let response = "Here's the plan:\n```json\n{\"shots\": [{\"description\": \"test\", \"duration_secs\": 5}], \"style_prompt\": \"warm\", \"audio\": {\"type\": \"none\"}}\n```";
        let plan = parse_shot_plan(response, 60).unwrap();
        assert_eq!(plan.shots.len(), 1);
    }

    #[test]
    fn test_parse_shot_plan_enforces_duration_limits() {
        let json = r#"{"shots": [{"description": "a", "duration_secs": 3}, {"description": "b", "duration_secs": 20}], "style_prompt": "x", "audio": {"type": "none"}}"#;
        let plan = parse_shot_plan(json, 120).unwrap();
        assert_eq!(plan.shots[0].duration_secs, 5);
        assert_eq!(plan.shots[1].duration_secs, 10);
    }

    #[test]
    fn test_build_continuity_prompt() {
        let prompt = build_continuity_prompt(
            "cherry blossoms against blue sky",
            &Shot {
                description: "a temple in Kyoto".to_string(),
                duration_secs: 10,
            },
            "cinematic, 4K",
        );
        assert!(prompt.contains("cherry blossoms"));
        assert!(prompt.contains("temple in Kyoto"));
        assert!(prompt.contains("cinematic"));
    }
}
