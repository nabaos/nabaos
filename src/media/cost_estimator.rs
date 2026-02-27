//! Cost estimation for media generation — shown before execution.

use crate::media::shot_planner::ShotPlan;

/// Per-second cost for video generation by provider.
pub fn video_cost_per_second(provider: &str) -> f64 {
    match provider {
        "fal.ai" => 0.10,
        "runway" => 0.12,
        "openai" => 0.20,
        "comfyui" => 0.00,
        _ => 0.10,
    }
}

/// Per-image cost by provider.
pub fn image_cost(provider: &str) -> f64 {
    match provider {
        "fal.ai" => 0.025,
        "openai" => 0.04,
        "comfyui" => 0.00,
        _ => 0.04,
    }
}

/// Per-1000-characters cost for TTS by provider.
pub fn tts_cost_per_1k_chars(provider: &str) -> f64 {
    match provider {
        "openai" => 0.015,
        "elevenlabs" => 0.30,
        _ => 0.015,
    }
}

/// Estimate cost for a video shot plan.
pub fn estimate_video_cost(plan: &ShotPlan, provider: &str) -> CostEstimate {
    let video_cost = plan.total_duration() as f64 * video_cost_per_second(provider);
    let audio_cost = match &plan.audio {
        crate::media::shot_planner::AudioPlan::Narration { script, .. } => {
            script.len() as f64 / 1000.0 * tts_cost_per_1k_chars("openai")
        }
        _ => 0.0,
    };
    let vision_cost = (plan.shots.len().saturating_sub(1)) as f64 * 0.01;

    CostEstimate {
        video_usd: video_cost,
        audio_usd: audio_cost,
        vision_usd: vision_cost,
        total_usd: video_cost + audio_cost + vision_cost,
        provider: provider.to_string(),
        duration_secs: plan.total_duration(),
        shot_count: plan.shots.len(),
    }
}

/// Estimate cost for image generation.
pub fn estimate_image_cost(provider: &str, count: usize) -> CostEstimate {
    let total = image_cost(provider) * count as f64;
    CostEstimate {
        video_usd: 0.0,
        audio_usd: 0.0,
        vision_usd: 0.0,
        total_usd: total,
        provider: provider.to_string(),
        duration_secs: 0,
        shot_count: count,
    }
}

#[derive(Debug, Clone)]
pub struct CostEstimate {
    pub video_usd: f64,
    pub audio_usd: f64,
    pub vision_usd: f64,
    pub total_usd: f64,
    pub provider: String,
    pub duration_secs: u32,
    pub shot_count: usize,
}

impl std::fmt::Display for CostEstimate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} shots, {}s via {} — estimated ${:.2}",
            self.shot_count, self.duration_secs, self.provider, self.total_usd
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::shot_planner::{AudioPlan, Shot, ShotPlan};

    #[test]
    fn test_estimate_video_cost_12_shots() {
        let plan = ShotPlan {
            shots: (0..12)
                .map(|_| Shot {
                    description: "test".to_string(),
                    duration_secs: 10,
                })
                .collect(),
            style_prompt: "cinematic".to_string(),
            target_duration_secs: 120,
            audio: AudioPlan::None,
        };
        let est = estimate_video_cost(&plan, "fal.ai");
        assert_eq!(est.duration_secs, 120);
        assert_eq!(est.shot_count, 12);
        // 120s * $0.10/s = $12.00 video + 11 * $0.01 vision = $12.11
        assert!((est.total_usd - 12.11).abs() < 0.01);
    }

    #[test]
    fn test_estimate_comfyui_free() {
        let plan = ShotPlan {
            shots: vec![Shot {
                description: "test".to_string(),
                duration_secs: 10,
            }],
            style_prompt: "x".to_string(),
            target_duration_secs: 10,
            audio: AudioPlan::None,
        };
        let est = estimate_video_cost(&plan, "comfyui");
        assert_eq!(est.video_usd, 0.0);
    }

    #[test]
    fn test_cost_estimate_display() {
        let est = CostEstimate {
            video_usd: 1.20,
            audio_usd: 0.10,
            vision_usd: 0.05,
            total_usd: 1.35,
            provider: "runway".to_string(),
            duration_secs: 60,
            shot_count: 6,
        };
        let display = format!("{}", est);
        assert!(display.contains("6 shots"));
        assert!(display.contains("60s"));
        assert!(display.contains("runway"));
        assert!(display.contains("$1.35"));
    }
}
