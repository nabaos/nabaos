use serde::{Deserialize, Serialize};
use std::fmt;

/// Supported export targets for nabaos deployments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportTarget {
    CloudRun,
    RaspberryPi,
    Esp32,
    Ros2,
}

impl fmt::Display for ExportTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportTarget::CloudRun => write!(f, "CloudRun"),
            ExportTarget::RaspberryPi => write!(f, "RaspberryPi"),
            ExportTarget::Esp32 => write!(f, "Esp32"),
            ExportTarget::Ros2 => write!(f, "Ros2"),
        }
    }
}

impl ExportTarget {
    /// Returns a static slice of all export target variants.
    pub fn all() -> &'static [ExportTarget] {
        &[
            ExportTarget::CloudRun,
            ExportTarget::RaspberryPi,
            ExportTarget::Esp32,
            ExportTarget::Ros2,
        ]
    }

    /// Returns the platform capabilities for this export target.
    pub fn capabilities(&self) -> PlatformCapabilities {
        match self {
            ExportTarget::CloudRun => PlatformCapabilities {
                target: ExportTarget::CloudRun,
                has_network: true,
                has_filesystem: true,
                has_std: true,
                max_memory_kb: 524_288, // 512 MB
                supports_wasm: true,
                supports_native: true,
                has_gpio: false,
            },
            ExportTarget::RaspberryPi => PlatformCapabilities {
                target: ExportTarget::RaspberryPi,
                has_network: true,
                has_filesystem: true,
                has_std: true,
                max_memory_kb: 262_144, // 256 MB
                supports_wasm: true,
                supports_native: true,
                has_gpio: true,
            },
            ExportTarget::Esp32 => PlatformCapabilities {
                target: ExportTarget::Esp32,
                has_network: true, // WiFi
                has_filesystem: false,
                has_std: false,
                max_memory_kb: 320,
                supports_wasm: true,
                supports_native: false,
                has_gpio: true,
            },
            ExportTarget::Ros2 => PlatformCapabilities {
                target: ExportTarget::Ros2,
                has_network: true, // DDS
                has_filesystem: true,
                has_std: true,
                max_memory_kb: 524_288, // 512 MB
                supports_wasm: false,
                supports_native: true,
                has_gpio: true, // via ROS 2 topic bridge
            },
        }
    }
}

/// Describes the capabilities and constraints of a deployment platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformCapabilities {
    pub target: ExportTarget,
    pub has_network: bool,
    pub has_filesystem: bool,
    pub has_std: bool,
    pub max_memory_kb: u64,
    pub supports_wasm: bool,
    pub supports_native: bool,
    pub has_gpio: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capabilities_cloud_run() {
        let caps = ExportTarget::CloudRun.capabilities();
        assert_eq!(caps.target, ExportTarget::CloudRun);
        assert!(caps.has_network);
        assert!(caps.has_filesystem);
        assert!(caps.has_std);
        assert_eq!(caps.max_memory_kb, 524_288);
        assert!(caps.supports_wasm);
        assert!(caps.supports_native);
        assert!(!caps.has_gpio);
    }

    #[test]
    fn test_capabilities_raspberry_pi() {
        let caps = ExportTarget::RaspberryPi.capabilities();
        assert_eq!(caps.target, ExportTarget::RaspberryPi);
        assert!(caps.has_network);
        assert!(caps.has_filesystem);
        assert!(caps.has_std);
        assert_eq!(caps.max_memory_kb, 262_144);
        assert!(caps.supports_wasm);
        assert!(caps.supports_native);
        assert!(caps.has_gpio);
    }

    #[test]
    fn test_capabilities_esp32() {
        let caps = ExportTarget::Esp32.capabilities();
        assert_eq!(caps.target, ExportTarget::Esp32);
        assert!(caps.has_network);
        assert!(!caps.has_filesystem);
        assert!(!caps.has_std);
        assert_eq!(caps.max_memory_kb, 320);
        assert!(caps.supports_wasm);
        assert!(!caps.supports_native);
        assert!(caps.has_gpio);
    }

    #[test]
    fn test_display_formatting() {
        assert_eq!(ExportTarget::CloudRun.to_string(), "CloudRun");
        assert_eq!(ExportTarget::RaspberryPi.to_string(), "RaspberryPi");
        assert_eq!(ExportTarget::Esp32.to_string(), "Esp32");
        assert_eq!(ExportTarget::Ros2.to_string(), "Ros2");
    }

    #[test]
    fn test_all_returns_all_variants() {
        let all = ExportTarget::all();
        assert_eq!(all.len(), 4);
        assert!(all.contains(&ExportTarget::CloudRun));
        assert!(all.contains(&ExportTarget::RaspberryPi));
        assert!(all.contains(&ExportTarget::Esp32));
        assert!(all.contains(&ExportTarget::Ros2));
    }

    #[test]
    fn test_serde_roundtrip() {
        for target in ExportTarget::all() {
            let json = serde_json::to_string(target).unwrap();
            let deserialized: ExportTarget = serde_json::from_str(&json).unwrap();
            assert_eq!(*target, deserialized);
        }
    }

    #[test]
    fn test_parse_from_string() {
        let cloud_run: ExportTarget = serde_json::from_str("\"cloud_run\"").unwrap();
        assert_eq!(cloud_run, ExportTarget::CloudRun);

        let rpi: ExportTarget = serde_json::from_str("\"raspberry_pi\"").unwrap();
        assert_eq!(rpi, ExportTarget::RaspberryPi);

        let esp: ExportTarget = serde_json::from_str("\"esp32\"").unwrap();
        assert_eq!(esp, ExportTarget::Esp32);

        let ros2: ExportTarget = serde_json::from_str("\"ros2\"").unwrap();
        assert_eq!(ros2, ExportTarget::Ros2);
    }

    #[test]
    fn test_capabilities_ros2() {
        let caps = ExportTarget::Ros2.capabilities();
        assert_eq!(caps.target, ExportTarget::Ros2);
        assert!(caps.has_network);
        assert!(caps.has_filesystem);
        assert!(caps.has_std);
        assert_eq!(caps.max_memory_kb, 524_288);
        assert!(!caps.supports_wasm);
        assert!(caps.supports_native);
        assert!(caps.has_gpio);
    }
}
