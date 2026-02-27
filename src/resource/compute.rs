use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::error::Result;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ComputeProviderType {
    GenericHttp,
    Custom(String),
}

// ---------------------------------------------------------------------------
// PodSpec — lightweight specification for requesting a pod
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodSpec {
    pub gpu_type: String,
    pub gpu_count: u8,
    pub min_ram_gb: u32,
    pub image: Option<String>,
    pub max_cost_per_hour: f64,
    pub max_duration_hours: Option<u64>,
}

// ---------------------------------------------------------------------------
// ComputePodConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputePodConfig {
    pub provider: ComputeProviderType,
    pub api_endpoint: String,
    pub credential_id: Option<String>,
    pub gpu_type: Option<String>,
    pub gpu_count: Option<u8>,
    pub cpu_cores: Option<u16>,
    pub ram_gb: Option<u32>,
    pub cost_per_hour: Option<f64>,
    pub region: Option<String>,
    #[serde(default)]
    pub default_quota: Option<super::registry::LeaseQuota>,
}

// ---------------------------------------------------------------------------
// ComputePod
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ComputePod {
    pub id: String,
    pub name: String,
    pub config: ComputePodConfig,
    pub status: super::ResourceStatus,
    pub instance_id: Option<String>,
    pub ssh_endpoint: Option<String>,
}

// ---------------------------------------------------------------------------
// Resource trait implementation
// ---------------------------------------------------------------------------

impl super::Resource for ComputePod {
    fn id(&self) -> &str {
        &self.id
    }

    fn resource_type(&self) -> super::ResourceType {
        super::ResourceType::Compute
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn status(&self) -> &super::ResourceStatus {
        &self.status
    }

    fn capabilities(&self) -> Vec<super::ResourceCapability> {
        vec![
            super::ResourceCapability::ReadData,
            super::ResourceCapability::Provision,
            super::ResourceCapability::Execute,
        ]
    }

    fn cost_model(&self) -> Option<super::CostModel> {
        self.config.cost_per_hour.map(super::CostModel::PerHour)
    }

    fn security_tier(&self, capability: &super::ResourceCapability) -> super::SecurityTier {
        match capability {
            super::ResourceCapability::Provision => super::SecurityTier::Critical,
            super::ResourceCapability::Execute => super::SecurityTier::Critical,
            super::ResourceCapability::ReadData => super::SecurityTier::ExternalRead,
            other => super::default_security_tier(&super::ResourceType::Compute, other),
        }
    }

    fn health_check(&self) -> Result<super::HealthReport> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        Ok(super::HealthReport {
            healthy: true,
            message: "status check not implemented".into(),
            checked_at: now,
        })
    }

    fn metadata(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        if let Some(ref gpu) = self.config.gpu_type {
            map.insert("gpu_type".to_string(), gpu.clone());
        }
        if let Some(count) = self.config.gpu_count {
            map.insert("gpu_count".to_string(), count.to_string());
        }
        if let Some(cores) = self.config.cpu_cores {
            map.insert("cpu_cores".to_string(), cores.to_string());
        }
        if let Some(ram) = self.config.ram_gb {
            map.insert("ram_gb".to_string(), ram.to_string());
        }
        if let Some(ref region) = self.config.region {
            map.insert("region".to_string(), region.clone());
        }
        if let Some(cost) = self.config.cost_per_hour {
            map.insert("cost_per_hour".to_string(), cost.to_string());
        }
        if let Some(ref instance) = self.instance_id {
            map.insert("instance_id".to_string(), instance.clone());
        }
        if let Some(ref ssh) = self.ssh_endpoint {
            map.insert("ssh_endpoint".to_string(), ssh.clone());
        }
        map
    }
}

// ---------------------------------------------------------------------------
// Factory function
// ---------------------------------------------------------------------------

pub fn compute_pod_from_config(id: &str, name: &str, config: ComputePodConfig) -> ComputePod {
    ComputePod {
        id: id.to_string(),
        name: name.to_string(),
        config,
        status: super::ResourceStatus::Available,
        instance_id: None,
        ssh_endpoint: None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::Resource;

    fn sample_config() -> ComputePodConfig {
        ComputePodConfig {
            provider: ComputeProviderType::GenericHttp,
            api_endpoint: "https://api.runpod.io/v2".to_string(),
            credential_id: Some("cred-123".to_string()),
            gpu_type: Some("A100".to_string()),
            gpu_count: Some(4),
            cpu_cores: Some(16),
            ram_gb: Some(64),
            cost_per_hour: Some(3.89),
            region: Some("us-east-1".to_string()),
            default_quota: Some(super::super::registry::LeaseQuota {
                max_cost_usd: Some(10.0),
                max_calls: Some(100),
                max_duration_secs: Some(3600),
            }),
        }
    }

    #[test]
    fn test_compute_pod_resource_trait() {
        let pod = compute_pod_from_config("pod-1", "GPU Worker", sample_config());

        assert_eq!(pod.id(), "pod-1");
        assert_eq!(pod.name(), "GPU Worker");
        assert_eq!(pod.resource_type(), crate::resource::ResourceType::Compute);

        let caps = pod.capabilities();
        assert!(caps.contains(&crate::resource::ResourceCapability::ReadData));
        assert!(caps.contains(&crate::resource::ResourceCapability::Provision));
        assert!(caps.contains(&crate::resource::ResourceCapability::Execute));
        assert_eq!(caps.len(), 3);

        // cost model
        let cost = pod.cost_model().expect("should have cost model");
        assert_eq!(cost, crate::resource::CostModel::PerHour(3.89));

        // status
        assert_eq!(*pod.status(), crate::resource::ResourceStatus::Available);
    }

    #[test]
    fn test_compute_pod_security_tiers() {
        let pod = compute_pod_from_config("pod-2", "Secure Pod", sample_config());

        assert_eq!(
            pod.security_tier(&crate::resource::ResourceCapability::Provision),
            crate::resource::SecurityTier::Critical
        );
        assert_eq!(
            pod.security_tier(&crate::resource::ResourceCapability::Execute),
            crate::resource::SecurityTier::Critical
        );
        assert_eq!(
            pod.security_tier(&crate::resource::ResourceCapability::ReadData),
            crate::resource::SecurityTier::ExternalRead
        );
    }

    #[test]
    fn test_pod_spec_serde() {
        let spec = PodSpec {
            gpu_type: "A100".to_string(),
            gpu_count: 2,
            min_ram_gb: 80,
            image: Some("pytorch/pytorch:2.1".to_string()),
            max_cost_per_hour: 4.50,
            max_duration_hours: Some(24),
        };

        let json = serde_json::to_string(&spec).unwrap();
        let back: PodSpec = serde_json::from_str(&json).unwrap();

        assert_eq!(back.gpu_type, "A100");
        assert_eq!(back.gpu_count, 2);
        assert_eq!(back.min_ram_gb, 80);
        assert_eq!(back.image, Some("pytorch/pytorch:2.1".to_string()));
        assert_eq!(back.max_cost_per_hour, 4.50);
        assert_eq!(back.max_duration_hours, Some(24));
    }

    #[test]
    fn test_compute_pod_config_serde() {
        let config = sample_config();

        let json = serde_json::to_string(&config).unwrap();
        let back: ComputePodConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(back.provider, ComputeProviderType::GenericHttp);
        assert_eq!(back.api_endpoint, "https://api.runpod.io/v2");
        assert_eq!(back.credential_id, Some("cred-123".to_string()));
        assert_eq!(back.gpu_type, Some("A100".to_string()));
        assert_eq!(back.gpu_count, Some(4));
        assert_eq!(back.cpu_cores, Some(16));
        assert_eq!(back.ram_gb, Some(64));
        assert_eq!(back.cost_per_hour, Some(3.89));
        assert_eq!(back.region, Some("us-east-1".to_string()));
        assert!(back.default_quota.is_some());
        let quota = back.default_quota.unwrap();
        assert_eq!(quota.max_duration_secs, Some(3600));
        assert_eq!(quota.max_cost_usd, Some(10.0));
    }
}
