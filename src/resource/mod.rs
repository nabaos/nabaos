pub mod api_catalog;
pub mod api_discovery;
pub mod api_service;
pub mod compute;
pub mod device;
pub mod drivers;
pub mod financial;
pub mod registry;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

// ---------------------------------------------------------------------------
// Core enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    Compute,
    Financial,
    Device,
    ApiService,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceStatus {
    Available,
    InUse { agent_id: String },
    Provisioning,
    Degraded,
    Offline,
    Terminated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceCapability {
    ReadData,
    WriteData,
    Stream,
    Provision,
    Execute,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SecurityTier {
    ReadOnly,
    ExternalRead,
    ExternalWrite,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CostModel {
    Free,
    PerHour(f64),
    PerCall(f64),
    PerUnit { unit: String, cost: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub healthy: bool,
    pub message: String,
    pub checked_at: i64,
}

// ---------------------------------------------------------------------------
// Display implementations
// ---------------------------------------------------------------------------

impl fmt::Display for ResourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceType::Compute => write!(f, "Compute"),
            ResourceType::Financial => write!(f, "Financial"),
            ResourceType::Device => write!(f, "Device"),
            ResourceType::ApiService => write!(f, "ApiService"),
        }
    }
}

impl fmt::Display for ResourceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceStatus::Available => write!(f, "Available"),
            ResourceStatus::InUse { agent_id } => write!(f, "InUse({})", agent_id),
            ResourceStatus::Provisioning => write!(f, "Provisioning"),
            ResourceStatus::Degraded => write!(f, "Degraded"),
            ResourceStatus::Offline => write!(f, "Offline"),
            ResourceStatus::Terminated => write!(f, "Terminated"),
        }
    }
}

impl fmt::Display for ResourceCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceCapability::ReadData => write!(f, "ReadData"),
            ResourceCapability::WriteData => write!(f, "WriteData"),
            ResourceCapability::Stream => write!(f, "Stream"),
            ResourceCapability::Provision => write!(f, "Provision"),
            ResourceCapability::Execute => write!(f, "Execute"),
        }
    }
}

impl fmt::Display for CostModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CostModel::Free => write!(f, "Free"),
            CostModel::PerHour(rate) => write!(f, "${:.4}/hr", rate),
            CostModel::PerCall(rate) => write!(f, "${:.4}/call", rate),
            CostModel::PerUnit { unit, cost } => write!(f, "${:.4}/{}", cost, unit),
        }
    }
}

// ---------------------------------------------------------------------------
// Resource trait
// ---------------------------------------------------------------------------

pub trait Resource: Send + Sync + std::fmt::Debug {
    fn id(&self) -> &str;
    fn resource_type(&self) -> ResourceType;
    fn name(&self) -> &str;
    fn status(&self) -> &ResourceStatus;
    fn capabilities(&self) -> Vec<ResourceCapability>;
    fn cost_model(&self) -> Option<CostModel>;
    fn health_check(&self) -> crate::core::error::Result<HealthReport>;
    fn metadata(&self) -> HashMap<String, String>;
    fn security_tier(&self, capability: &ResourceCapability) -> SecurityTier;
}

// ---------------------------------------------------------------------------
// Helper function
// ---------------------------------------------------------------------------

pub fn default_security_tier(
    resource_type: &ResourceType,
    capability: &ResourceCapability,
) -> SecurityTier {
    match (resource_type, capability) {
        (_, ResourceCapability::Provision) => SecurityTier::Critical,
        (_, ResourceCapability::Execute) => SecurityTier::Critical,
        (ResourceType::Financial, ResourceCapability::WriteData) => SecurityTier::Critical,
        (ResourceType::Device, ResourceCapability::WriteData) => SecurityTier::ExternalWrite,
        (_, ResourceCapability::ReadData) => SecurityTier::ExternalRead,
        (_, ResourceCapability::Stream) => SecurityTier::ExternalRead,
        (_, ResourceCapability::WriteData) => SecurityTier::ExternalWrite,
    }
}

// ---------------------------------------------------------------------------
// Config types for YAML loading
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceConfig {
    pub id: String,
    #[serde(rename = "type")]
    pub resource_type: ResourceType,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    #[serde(flatten)]
    pub type_config: serde_yaml::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SharedResourcesConfig {
    pub resources: Vec<ResourceConfig>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_type_serde_roundtrip() {
        let variants = vec![
            ResourceType::Compute,
            ResourceType::Financial,
            ResourceType::Device,
            ResourceType::ApiService,
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant).unwrap();
            let back: ResourceType = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, back);
        }
    }

    #[test]
    fn test_resource_status_serde() {
        let variants: Vec<ResourceStatus> = vec![
            ResourceStatus::Available,
            ResourceStatus::InUse {
                agent_id: "agent-42".to_string(),
            },
            ResourceStatus::Provisioning,
            ResourceStatus::Degraded,
            ResourceStatus::Offline,
            ResourceStatus::Terminated,
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant).unwrap();
            let back: ResourceStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, back);
        }
    }

    #[test]
    fn test_capability_serde() {
        let variants = vec![
            ResourceCapability::ReadData,
            ResourceCapability::WriteData,
            ResourceCapability::Stream,
            ResourceCapability::Provision,
            ResourceCapability::Execute,
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant).unwrap();
            let back: ResourceCapability = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, back);
        }
    }

    #[test]
    fn test_cost_model_serde() {
        let variants = vec![
            CostModel::Free,
            CostModel::PerHour(1.50),
            CostModel::PerCall(0.005),
            CostModel::PerUnit {
                unit: "token".to_string(),
                cost: 0.0001,
            },
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant).unwrap();
            let back: CostModel = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, back);
        }
    }

    #[test]
    fn test_default_security_tier_compute_provision() {
        assert_eq!(
            default_security_tier(&ResourceType::Compute, &ResourceCapability::Provision),
            SecurityTier::Critical
        );
    }

    #[test]
    fn test_default_security_tier_financial_write() {
        assert_eq!(
            default_security_tier(&ResourceType::Financial, &ResourceCapability::WriteData),
            SecurityTier::Critical
        );
    }

    #[test]
    fn test_default_security_tier_device_read() {
        assert_eq!(
            default_security_tier(&ResourceType::Device, &ResourceCapability::ReadData),
            SecurityTier::ExternalRead
        );
    }

    #[test]
    fn test_shared_resources_config_parse() {
        let yaml = r#"
resources:
  - id: gpu-a100
    type: compute
    name: "NVIDIA A100"
    gpu: true
    vram_gb: 80
  - id: stripe-main
    type: financial
    name: "Stripe Account"
  - id: arm-cam-01
    type: device
"#;
        let shared: SharedResourcesConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(shared.resources.len(), 3);
        assert_eq!(shared.resources[0].id, "gpu-a100");
        assert_eq!(shared.resources[0].resource_type, ResourceType::Compute);
        assert_eq!(shared.resources[0].name.as_deref(), Some("NVIDIA A100"));
        assert_eq!(shared.resources[1].id, "stripe-main");
        assert_eq!(shared.resources[1].resource_type, ResourceType::Financial);
        assert_eq!(shared.resources[2].id, "arm-cam-01");
        assert_eq!(shared.resources[2].resource_type, ResourceType::Device);
        assert!(shared.resources[2].name.is_none());
    }

    #[test]
    fn test_resource_prompt_section() {
        use registry::ResourceRecord;

        let records = vec![
            ResourceRecord {
                id: "gpu-a100".to_string(),
                name: "NVIDIA A100".to_string(),
                resource_type: ResourceType::Compute,
                status: ResourceStatus::Available,
                cost_model: None,
                metadata: HashMap::new(),
                config_json: "{}".to_string(),
                registered_at: 1000,
                last_health_check: None,
            },
            ResourceRecord {
                id: "stripe-main".to_string(),
                name: "Stripe Account".to_string(),
                resource_type: ResourceType::Financial,
                status: ResourceStatus::InUse {
                    agent_id: "agent-1".to_string(),
                },
                cost_model: None,
                metadata: HashMap::new(),
                config_json: "{}".to_string(),
                registered_at: 2000,
                last_health_check: None,
            },
        ];

        let mut section = String::new();
        section.push_str("AVAILABLE RESOURCES:\n\n");
        for r in &records {
            section.push_str(&format!(
                "- {} [{}] ({}): {}\n",
                r.id,
                r.resource_type_display(),
                r.status_display(),
                r.name
            ));
        }
        section.push_str("\n---\n\n");

        assert!(section.contains("- gpu-a100 [compute] (available): NVIDIA A100"));
        assert!(section.contains("- stripe-main [financial] (in_use:agent-1): Stripe Account"));
        assert!(section.starts_with("AVAILABLE RESOURCES:\n\n"));
        assert!(section.ends_with("\n---\n\n"));
    }

    #[test]
    fn test_shared_resources_config_parse_api_service() {
        let yaml = r#"
resources:
  - id: brave-search
    type: api_service
    name: "Brave Search"
    category: search
    provider: brave
    api_endpoint: "https://api.search.brave.com/res/v1/web/search"
    credential_id: brave-key
    auth_header: "X-Subscription-Token"
    rate_limit_rpm: 60
    cost_per_call: 0.005
  - id: ntfy-alerts
    type: api_service
    name: "Ntfy Alerts"
    category: notification
    provider: ntfy
    api_endpoint: "https://ntfy.sh"
"#;
        let shared: SharedResourcesConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(shared.resources.len(), 2);
        assert_eq!(shared.resources[0].id, "brave-search");
        assert_eq!(shared.resources[0].resource_type, ResourceType::ApiService);
        assert_eq!(shared.resources[0].name.as_deref(), Some("Brave Search"));
        assert_eq!(shared.resources[1].id, "ntfy-alerts");
        assert_eq!(shared.resources[1].resource_type, ResourceType::ApiService);
    }
}
