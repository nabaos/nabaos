//! VideoLooper — multi-shot video generation with vision LLM continuity.
//!
//! Pattern: generate clip -> extract last frame -> describe frame -> generate next clip -> repeat.

use crate::core::error::{NyayaError, Result};
use crate::media::shot_planner::{build_continuity_prompt, ShotPlan};
use crate::media::traits::*;
use std::path::{Path, PathBuf};

pub struct VideoLooper {
    ffmpeg_path: PathBuf,
    output_dir: PathBuf,
}

/// Result of a complete multi-shot video generation.
#[derive(Debug)]
pub struct LoopedVideoResult {
    pub output_path: PathBuf,
    pub total_duration_secs: f32,
    pub shot_count: usize,
    pub total_cost_usd: f64,
}

impl VideoLooper {
    pub fn new(ffmpeg_path: PathBuf, output_dir: PathBuf) -> Self {
        Self {
            ffmpeg_path,
            output_dir,
        }
    }

    /// Build the ffmpeg command to extract the last frame from a video file.
    pub fn build_extract_frame_command(
        ffmpeg_path: &Path,
        video_path: &Path,
        output_path: &Path,
    ) -> std::process::Command {
        let mut cmd = std::process::Command::new(ffmpeg_path);
        cmd.args(["-sseof", "-0.1", "-i"])
            .arg(video_path)
            .args(["-frames:v", "1", "-y"])
            .arg(output_path);
        cmd
    }

    /// Build the ffmpeg command to concatenate video clips.
    pub fn build_concat_command(
        ffmpeg_path: &Path,
        filelist_path: &Path,
        output_path: &Path,
    ) -> std::process::Command {
        let mut cmd = std::process::Command::new(ffmpeg_path);
        cmd.args(["-f", "concat", "-safe", "0", "-i"])
            .arg(filelist_path)
            .args(["-c", "copy", "-y"])
            .arg(output_path);
        cmd
    }

    /// Build the ffmpeg command to merge video + audio.
    pub fn build_merge_audio_command(
        ffmpeg_path: &Path,
        video_path: &Path,
        audio_path: &Path,
        output_path: &Path,
    ) -> std::process::Command {
        let mut cmd = std::process::Command::new(ffmpeg_path);
        cmd.arg("-i")
            .arg(video_path)
            .arg("-i")
            .arg(audio_path)
            .args(["-c:v", "copy", "-c:a", "aac", "-shortest", "-y"])
            .arg(output_path);
        cmd
    }

    /// Generate a filelist.txt for ffmpeg concat.
    pub fn generate_filelist(clip_paths: &[PathBuf]) -> String {
        clip_paths
            .iter()
            .map(|p| {
                let escaped = p.display().to_string().replace('\'', "'\\''");
                format!("file '{}'", escaped)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Execute the full multi-shot generation loop.
    pub async fn generate(
        &self,
        plan: &ShotPlan,
        video_gen: &dyn VideoGenerator,
        describe_frame: &dyn Fn(&[u8]) -> Result<String>,
    ) -> Result<LoopedVideoResult> {
        std::fs::create_dir_all(&self.output_dir)
            .map_err(|e| NyayaError::Config(format!("Failed to create output dir: {e}")))?;

        let video_config = VideoConfig::default();
        let mut clip_paths: Vec<PathBuf> = Vec::new();
        let mut last_frame: Option<Vec<u8>> = None;
        let mut total_cost = 0.0;

        for (i, shot) in plan.shots.iter().enumerate() {
            let config = VideoConfig {
                duration_secs: shot.duration_secs,
                ..video_config.clone()
            };

            let prompt = if let Some(ref frame_data) = last_frame {
                let description = describe_frame(frame_data)?;
                build_continuity_prompt(&description, shot, &plan.style_prompt)
            } else {
                format!("{}, {}", shot.description, plan.style_prompt)
            };

            let result = if let Some(ref frame_data) = last_frame {
                video_gen
                    .image_to_video(frame_data, &prompt, &config)
                    .await?
            } else {
                video_gen.text_to_video(&prompt, &config).await?
            };

            total_cost += result.cost_usd;

            let clip_path = self.output_dir.join(format!("clip_{:03}.mp4", i));
            std::fs::write(&clip_path, &result.data)
                .map_err(|e| NyayaError::Config(format!("Failed to write clip {i}: {e}")))?;
            clip_paths.push(clip_path.clone());

            let frame_path = self.output_dir.join(format!("frame_{:03}.png", i));
            let status =
                Self::build_extract_frame_command(&self.ffmpeg_path, &clip_path, &frame_path)
                    .status()
                    .map_err(|e| {
                        NyayaError::Config(format!("ffmpeg frame extraction error: {e}"))
                    })?;

            if status.success() {
                last_frame = Some(
                    std::fs::read(&frame_path)
                        .map_err(|e| NyayaError::Config(format!("Failed to read frame: {e}")))?,
                );
            } else {
                tracing::warn!(
                    "Frame extraction failed for clip {i}, continuing without continuity"
                );
                last_frame = None;
            }
        }

        let filelist_path = self.output_dir.join("filelist.txt");
        let filelist_content = Self::generate_filelist(&clip_paths);
        std::fs::write(&filelist_path, &filelist_content)
            .map_err(|e| NyayaError::Config(format!("Failed to write filelist: {e}")))?;

        let output_path = self.output_dir.join("output.mp4");
        let concat_status =
            Self::build_concat_command(&self.ffmpeg_path, &filelist_path, &output_path)
                .status()
                .map_err(|e| NyayaError::Config(format!("ffmpeg concat error: {e}")))?;

        if !concat_status.success() {
            return Err(NyayaError::Config("ffmpeg concat failed".to_string()));
        }

        let total_duration: f32 = plan.shots.iter().map(|s| s.duration_secs as f32).sum();

        Ok(LoopedVideoResult {
            output_path,
            total_duration_secs: total_duration,
            shot_count: clip_paths.len(),
            total_cost_usd: total_cost,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_filelist() {
        let paths = vec![
            PathBuf::from("/tmp/clip_000.mp4"),
            PathBuf::from("/tmp/clip_001.mp4"),
            PathBuf::from("/tmp/clip_002.mp4"),
        ];
        let filelist = VideoLooper::generate_filelist(&paths);
        assert!(filelist.contains("file '/tmp/clip_000.mp4'"));
        assert!(filelist.contains("file '/tmp/clip_002.mp4'"));
        assert_eq!(filelist.lines().count(), 3);
    }

    #[test]
    fn test_extract_frame_command() {
        let cmd = VideoLooper::build_extract_frame_command(
            Path::new("/usr/bin/ffmpeg"),
            Path::new("/tmp/clip.mp4"),
            Path::new("/tmp/frame.png"),
        );
        let program = cmd.get_program().to_str().unwrap();
        assert_eq!(program, "/usr/bin/ffmpeg");
        let args: Vec<&str> = cmd.get_args().map(|a| a.to_str().unwrap()).collect();
        assert!(args.contains(&"-sseof"));
        assert!(args.contains(&"-frames:v"));
    }

    #[test]
    fn test_concat_command() {
        let cmd = VideoLooper::build_concat_command(
            Path::new("/usr/bin/ffmpeg"),
            Path::new("/tmp/filelist.txt"),
            Path::new("/tmp/output.mp4"),
        );
        let args: Vec<&str> = cmd.get_args().map(|a| a.to_str().unwrap()).collect();
        assert!(args.contains(&"-f"));
        assert!(args.contains(&"concat"));
    }

    #[test]
    fn test_merge_audio_command() {
        let cmd = VideoLooper::build_merge_audio_command(
            Path::new("/usr/bin/ffmpeg"),
            Path::new("/tmp/video.mp4"),
            Path::new("/tmp/audio.mp3"),
            Path::new("/tmp/final.mp4"),
        );
        let args: Vec<&str> = cmd.get_args().map(|a| a.to_str().unwrap()).collect();
        assert!(args.contains(&"-c:a"));
        assert!(args.contains(&"aac"));
        assert!(args.contains(&"-shortest"));
    }
}
