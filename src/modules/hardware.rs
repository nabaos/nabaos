use std::path::{Path, PathBuf};
use std::process::Command;

use crate::modules::profile::{ModuleProfile, VoiceMode};

/// GPU information detected on the host.
#[derive(Debug, Clone)]
pub enum GpuInfo {
    None,
    Nvidia { vram_mb: u64 },
    Amd { vram_mb: u64 },
    Other(String),
}

impl std::fmt::Display for GpuInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpuInfo::None => write!(f, "None"),
            GpuInfo::Nvidia { vram_mb } => write!(f, "NVIDIA ({} MB VRAM)", vram_mb),
            GpuInfo::Amd { vram_mb } => write!(f, "AMD ({} MB VRAM)", vram_mb),
            GpuInfo::Other(s) => write!(f, "Other ({})", s),
        }
    }
}

/// Summary of the host machine's hardware and available tools.
#[derive(Debug, Clone)]
pub struct HardwareInfo {
    pub cpu_cores: usize,
    pub ram_mb: u64,
    pub gpu: GpuInfo,
    pub disk_free_gb: u64,
    pub os: String,
    pub arch: String,
    pub chromium_path: Option<PathBuf>,
    pub node_path: Option<PathBuf>,
    pub ffmpeg_path: Option<PathBuf>,
    pub latex_path: Option<PathBuf>,
}

/// Search a list of candidate binary names and return the path of the first
/// one found on the system (via `which`).
pub fn detect_tool(candidates: &[&str]) -> Option<PathBuf> {
    for name in candidates {
        if let Ok(output) = Command::new("which").arg(name).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(PathBuf::from(path));
                }
            }
        }
    }
    None
}

impl HardwareInfo {
    /// Scan the host machine and populate a `HardwareInfo` struct.
    pub fn scan() -> Self {
        let cpu_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);

        let ram_mb = Self::detect_ram_mb();
        let gpu = Self::detect_gpu();
        let disk_free_gb = Self::detect_disk_free_gb();

        let os = std::env::consts::OS.to_string();
        let arch = std::env::consts::ARCH.to_string();

        let chromium_path = detect_tool(&[
            "chromium",
            "chromium-browser",
            "google-chrome",
            "google-chrome-stable",
        ]);
        let node_path = detect_tool(&["node", "nodejs"]);
        let ffmpeg_path = detect_tool(&["ffmpeg"]);
        let latex_path = detect_tool(&["pdflatex", "xelatex", "lualatex"]);

        HardwareInfo {
            cpu_cores,
            ram_mb,
            gpu,
            disk_free_gb,
            os,
            arch,
            chromium_path,
            node_path,
            ffmpeg_path,
            latex_path,
        }
    }

    /// Read total RAM from /proc/meminfo (Linux) or fall back to 0.
    fn detect_ram_mb() -> u64 {
        let meminfo = Path::new("/proc/meminfo");
        if meminfo.exists() {
            if let Ok(contents) = std::fs::read_to_string(meminfo) {
                for line in contents.lines() {
                    if line.starts_with("MemTotal:") {
                        // Format: "MemTotal:       16384000 kB"
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            if let Ok(kb) = parts[1].parse::<u64>() {
                                return kb / 1024;
                            }
                        }
                    }
                }
            }
        }
        0
    }

    /// Attempt to detect an NVIDIA GPU via nvidia-smi.
    fn detect_gpu() -> GpuInfo {
        if let Ok(output) = Command::new("nvidia-smi")
            .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                if let Some(first_line) = text.lines().next() {
                    if let Ok(vram) = first_line.trim().parse::<u64>() {
                        return GpuInfo::Nvidia { vram_mb: vram };
                    }
                }
            }
        }
        // Could add AMD detection here in the future.
        GpuInfo::None
    }

    /// Detect free disk space on the root filesystem via `df`.
    fn detect_disk_free_gb() -> u64 {
        if let Ok(output) = Command::new("df")
            .args(["--output=avail", "-B1G", "/"])
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                // Skip header line.
                if let Some(line) = text.lines().nth(1) {
                    if let Ok(gb) = line.trim().parse::<u64>() {
                        return gb;
                    }
                }
            }
        }
        0
    }

    /// Suggest a `ModuleProfile` based on detected hardware and tools.
    pub fn suggest_profile(&self) -> ModuleProfile {
        let now = chrono::Utc::now().to_rfc3339();

        let mut profile = ModuleProfile {
            core: true,
            web: self.ram_mb >= 512,
            voice: if self.ffmpeg_path.is_some() {
                VoiceMode::Local
            } else {
                VoiceMode::Disabled
            },
            browser: self.chromium_path.is_some() && self.ram_mb >= 2048,
            oauth: Vec::new(),
            telegram: true,
            mobile: false,
            latex: self.latex_path.is_some(),
            name: "suggested".to_string(),
            generated_at: now,
        };

        // On very low RAM machines, disable optional heavy modules.
        if self.ram_mb < 1024 {
            profile.web = false;
            profile.browser = false;
            profile.voice = VoiceMode::Disabled;
        }

        profile
    }

    /// Return a human-readable report of the hardware scan.
    pub fn display_report(&self) -> String {
        use crate::tui::fmt;

        let ram_gb = self.ram_mb / 1024;
        let gpu_str = format!("{}", self.gpu);

        let tool_check = |opt: &Option<PathBuf>, name: &str| -> String {
            if opt.is_some() {
                fmt::ok(name)
            } else {
                fmt::skip(name)
            }
        };

        let mut lines = Vec::new();
        lines.push(fmt::header_line("Hardware"));
        lines.push(fmt::row_pair(
            "CPU",
            &format!("{} cores", self.cpu_cores),
            "RAM",
            &format!("{} GB", ram_gb),
        ));
        lines.push(fmt::row_pair(
            "GPU",
            &gpu_str,
            "Disk",
            &format!("{} GB free", self.disk_free_gb),
        ));
        lines.push(fmt::row_pair("OS", &self.os, "Arch", &self.arch));
        lines.push(fmt::section("Tools"));
        // Build tool status row
        let chromium_s = tool_check(&self.chromium_path, "Chromium");
        let node_s = tool_check(&self.node_path, "Node.js");
        let ffmpeg_s = tool_check(&self.ffmpeg_path, "FFmpeg");
        let latex_s = tool_check(&self.latex_path, "LaTeX");
        lines.push(chromium_s);
        lines.push(node_s);
        lines.push(ffmpeg_s);
        lines.push(latex_s);
        lines.push(fmt::footer());
        lines.join("\n")
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_returns_valid_hardware_info() {
        let hw = HardwareInfo::scan();
        assert!(hw.cpu_cores >= 1);
        // OS and arch should be non-empty on any platform.
        assert!(!hw.os.is_empty());
        assert!(!hw.arch.is_empty());
    }

    #[test]
    fn test_suggest_profile_low_ram() {
        let hw = HardwareInfo {
            cpu_cores: 1,
            ram_mb: 512,
            gpu: GpuInfo::None,
            disk_free_gb: 10,
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            chromium_path: None,
            node_path: None,
            ffmpeg_path: None,
            latex_path: None,
        };
        let p = hw.suggest_profile();
        assert!(p.core);
        assert!(!p.browser);
        assert!(!p.web);
        assert_eq!(p.voice, VoiceMode::Disabled);
        assert!(!p.latex);
    }

    #[test]
    fn test_suggest_profile_full_machine() {
        let hw = HardwareInfo {
            cpu_cores: 8,
            ram_mb: 16384,
            gpu: GpuInfo::Nvidia { vram_mb: 8192 },
            disk_free_gb: 200,
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            chromium_path: Some(PathBuf::from("/usr/bin/chromium")),
            node_path: Some(PathBuf::from("/usr/bin/node")),
            ffmpeg_path: Some(PathBuf::from("/usr/bin/ffmpeg")),
            latex_path: Some(PathBuf::from("/usr/bin/pdflatex")),
        };
        let p = hw.suggest_profile();
        assert!(p.core);
        assert!(p.web);
        assert!(p.browser);
        assert_eq!(p.voice, VoiceMode::Local);
        assert!(p.latex);
        assert!(p.telegram);
    }

    #[test]
    fn test_detect_tool_finds_existing() {
        // "sh" should exist on any Unix-like system.
        let result = detect_tool(&["sh"]);
        assert!(result.is_some());
    }

    #[test]
    fn test_detect_tool_returns_none_for_missing() {
        let result = detect_tool(&["this_tool_definitely_does_not_exist_xyz_999"]);
        assert!(result.is_none());
    }
}
