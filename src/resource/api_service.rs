use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::error::Result;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ApiServiceCategory {
    Search,
    Academic,
    Captcha,
    Calendar,
    Storage,
    Embedding,
    Notification,
    Translation,
    Weather,
    Monitoring,
}

impl std::fmt::Display for ApiServiceCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiServiceCategory::Search => write!(f, "Search"),
            ApiServiceCategory::Academic => write!(f, "Academic"),
            ApiServiceCategory::Captcha => write!(f, "Captcha"),
            ApiServiceCategory::Calendar => write!(f, "Calendar"),
            ApiServiceCategory::Storage => write!(f, "Storage"),
            ApiServiceCategory::Embedding => write!(f, "Embedding"),
            ApiServiceCategory::Notification => write!(f, "Notification"),
            ApiServiceCategory::Translation => write!(f, "Translation"),
            ApiServiceCategory::Weather => write!(f, "Weather"),
            ApiServiceCategory::Monitoring => write!(f, "Monitoring"),
        }
    }
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiServiceConfig {
    pub category: ApiServiceCategory,
    pub provider: String,
    pub api_endpoint: String,
    pub credential_id: Option<String>,
    pub auth_header: Option<String>,
    pub rate_limit_rpm: Option<u32>,
    pub cost_per_call: Option<f64>,
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub security_tier_override: Option<super::SecurityTier>,
    #[serde(default)]
    pub default_quota: Option<super::registry::LeaseQuota>,
}

// ---------------------------------------------------------------------------
// ApiServiceResource
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ApiServiceResource {
    pub id: String,
    pub name: String,
    pub config: ApiServiceConfig,
    pub status: super::ResourceStatus,
}

// ---------------------------------------------------------------------------
// Resource trait implementation
// ---------------------------------------------------------------------------

impl super::Resource for ApiServiceResource {
    fn id(&self) -> &str {
        &self.id
    }

    fn resource_type(&self) -> super::ResourceType {
        super::ResourceType::ApiService
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn status(&self) -> &super::ResourceStatus {
        &self.status
    }

    fn capabilities(&self) -> Vec<super::ResourceCapability> {
        match self.config.category {
            ApiServiceCategory::Search
            | ApiServiceCategory::Academic
            | ApiServiceCategory::Embedding
            | ApiServiceCategory::Translation
            | ApiServiceCategory::Weather => {
                vec![super::ResourceCapability::ReadData]
            }
            ApiServiceCategory::Captcha => {
                vec![
                    super::ResourceCapability::ReadData,
                    super::ResourceCapability::Execute,
                ]
            }
            ApiServiceCategory::Calendar => {
                vec![
                    super::ResourceCapability::ReadData,
                    super::ResourceCapability::WriteData,
                ]
            }
            ApiServiceCategory::Storage => {
                vec![
                    super::ResourceCapability::ReadData,
                    super::ResourceCapability::WriteData,
                    super::ResourceCapability::Stream,
                ]
            }
            ApiServiceCategory::Notification => {
                vec![super::ResourceCapability::WriteData]
            }
            ApiServiceCategory::Monitoring => {
                vec![
                    super::ResourceCapability::ReadData,
                    super::ResourceCapability::Stream,
                ]
            }
        }
    }

    fn cost_model(&self) -> Option<super::CostModel> {
        match self.config.cost_per_call {
            Some(cost) if cost > 0.0 => Some(super::CostModel::PerCall(cost)),
            _ => Some(super::CostModel::Free),
        }
    }

    fn security_tier(&self, capability: &super::ResourceCapability) -> super::SecurityTier {
        if let Some(ref tier) = self.config.security_tier_override {
            return tier.clone();
        }
        super::default_security_tier(&super::ResourceType::ApiService, capability)
    }

    fn health_check(&self) -> Result<super::HealthReport> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        Ok(super::HealthReport {
            healthy: true,
            message: format!(
                "API service {} health check not implemented",
                self.config.provider
            ),
            checked_at: now,
        })
    }

    fn metadata(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        map.insert("category".to_string(), self.config.category.to_string());
        map.insert("provider".to_string(), self.config.provider.clone());
        map.insert("api_endpoint".to_string(), self.config.api_endpoint.clone());
        if let Some(ref auth) = self.config.auth_header {
            map.insert("auth_header".to_string(), auth.clone());
        }
        if let Some(rpm) = self.config.rate_limit_rpm {
            map.insert("rate_limit_rpm".to_string(), rpm.to_string());
        }
        if let Some(timeout) = self.config.timeout_secs {
            map.insert("timeout_secs".to_string(), timeout.to_string());
        }
        map
    }
}

// ---------------------------------------------------------------------------
// Factory function
// ---------------------------------------------------------------------------

pub fn api_service_from_config(
    id: &str,
    name: &str,
    config: ApiServiceConfig,
) -> ApiServiceResource {
    ApiServiceResource {
        id: id.to_string(),
        name: name.to_string(),
        config,
        status: super::ResourceStatus::Available,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::{CostModel, Resource, ResourceCapability, ResourceType, SecurityTier};

    fn search_config() -> ApiServiceConfig {
        ApiServiceConfig {
            category: ApiServiceCategory::Search,
            provider: "brave".to_string(),
            api_endpoint: "https://api.search.brave.com/res/v1/web/search".to_string(),
            credential_id: Some("brave-key".to_string()),
            auth_header: Some("X-Subscription-Token".to_string()),
            rate_limit_rpm: Some(60),
            cost_per_call: Some(0.005),
            timeout_secs: Some(30),
            security_tier_override: None,
            default_quota: None,
        }
    }

    #[test]
    fn test_api_service_category_serde() {
        let variants = vec![
            ApiServiceCategory::Search,
            ApiServiceCategory::Academic,
            ApiServiceCategory::Captcha,
            ApiServiceCategory::Calendar,
            ApiServiceCategory::Storage,
            ApiServiceCategory::Embedding,
            ApiServiceCategory::Notification,
            ApiServiceCategory::Translation,
            ApiServiceCategory::Weather,
            ApiServiceCategory::Monitoring,
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant).unwrap();
            let back: ApiServiceCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, back);
        }
    }

    #[test]
    fn test_api_service_config_serde() {
        let config = search_config();
        let json = serde_json::to_string(&config).unwrap();
        let back: ApiServiceConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(back.category, ApiServiceCategory::Search);
        assert_eq!(back.provider, "brave");
        assert_eq!(
            back.api_endpoint,
            "https://api.search.brave.com/res/v1/web/search"
        );
        assert_eq!(back.credential_id, Some("brave-key".to_string()));
        assert_eq!(back.auth_header, Some("X-Subscription-Token".to_string()));
        assert_eq!(back.rate_limit_rpm, Some(60));
        assert_eq!(back.cost_per_call, Some(0.005));
        assert_eq!(back.timeout_secs, Some(30));
    }

    #[test]
    fn test_api_service_resource_trait() {
        let svc = api_service_from_config("svc-1", "Brave Search", search_config());

        assert_eq!(svc.id(), "svc-1");
        assert_eq!(svc.name(), "Brave Search");
        assert_eq!(svc.resource_type(), ResourceType::ApiService);
        assert_eq!(*svc.status(), crate::resource::ResourceStatus::Available);
    }

    #[test]
    fn test_capabilities_by_category() {
        // Search → ReadData only
        let search = api_service_from_config("s", "s", search_config());
        assert_eq!(search.capabilities(), vec![ResourceCapability::ReadData]);

        // Captcha → ReadData + Execute
        let captcha_cfg = ApiServiceConfig {
            category: ApiServiceCategory::Captcha,
            provider: "2captcha".to_string(),
            api_endpoint: "https://api.2captcha.com".to_string(),
            credential_id: None,
            auth_header: None,
            rate_limit_rpm: None,
            cost_per_call: Some(0.003),
            timeout_secs: None,
            security_tier_override: None,
            default_quota: None,
        };
        let captcha = api_service_from_config("c", "c", captcha_cfg);
        let caps = captcha.capabilities();
        assert!(caps.contains(&ResourceCapability::ReadData));
        assert!(caps.contains(&ResourceCapability::Execute));
        assert_eq!(caps.len(), 2);

        // Storage → ReadData + WriteData + Stream
        let storage_cfg = ApiServiceConfig {
            category: ApiServiceCategory::Storage,
            provider: "s3".to_string(),
            api_endpoint: "https://s3.amazonaws.com".to_string(),
            credential_id: None,
            auth_header: None,
            rate_limit_rpm: None,
            cost_per_call: None,
            timeout_secs: None,
            security_tier_override: None,
            default_quota: None,
        };
        let storage = api_service_from_config("st", "st", storage_cfg);
        let caps = storage.capabilities();
        assert!(caps.contains(&ResourceCapability::ReadData));
        assert!(caps.contains(&ResourceCapability::WriteData));
        assert!(caps.contains(&ResourceCapability::Stream));
        assert_eq!(caps.len(), 3);

        // Notification → WriteData only
        let notif_cfg = ApiServiceConfig {
            category: ApiServiceCategory::Notification,
            provider: "ntfy".to_string(),
            api_endpoint: "https://ntfy.sh".to_string(),
            credential_id: None,
            auth_header: None,
            rate_limit_rpm: None,
            cost_per_call: None,
            timeout_secs: None,
            security_tier_override: None,
            default_quota: None,
        };
        let notif = api_service_from_config("n", "n", notif_cfg);
        assert_eq!(notif.capabilities(), vec![ResourceCapability::WriteData]);

        // Monitoring → ReadData + Stream
        let mon_cfg = ApiServiceConfig {
            category: ApiServiceCategory::Monitoring,
            provider: "uptime".to_string(),
            api_endpoint: "https://api.uptimerobot.com".to_string(),
            credential_id: None,
            auth_header: None,
            rate_limit_rpm: None,
            cost_per_call: None,
            timeout_secs: None,
            security_tier_override: None,
            default_quota: None,
        };
        let mon = api_service_from_config("m", "m", mon_cfg);
        let caps = mon.capabilities();
        assert!(caps.contains(&ResourceCapability::ReadData));
        assert!(caps.contains(&ResourceCapability::Stream));
        assert_eq!(caps.len(), 2);

        // Calendar → ReadData + WriteData
        let cal_cfg = ApiServiceConfig {
            category: ApiServiceCategory::Calendar,
            provider: "caldav".to_string(),
            api_endpoint: "https://caldav.example.com".to_string(),
            credential_id: None,
            auth_header: None,
            rate_limit_rpm: None,
            cost_per_call: None,
            timeout_secs: None,
            security_tier_override: None,
            default_quota: None,
        };
        let cal = api_service_from_config("cal", "cal", cal_cfg);
        let caps = cal.capabilities();
        assert!(caps.contains(&ResourceCapability::ReadData));
        assert!(caps.contains(&ResourceCapability::WriteData));
        assert_eq!(caps.len(), 2);
    }

    #[test]
    fn test_security_tier_override() {
        // Without override — uses default
        let svc = api_service_from_config("s", "s", search_config());
        assert_eq!(
            svc.security_tier(&ResourceCapability::ReadData),
            SecurityTier::ExternalRead
        );

        // With override — override wins
        let mut config = search_config();
        config.security_tier_override = Some(SecurityTier::Critical);
        let svc_override = api_service_from_config("s2", "s2", config);
        assert_eq!(
            svc_override.security_tier(&ResourceCapability::ReadData),
            SecurityTier::Critical
        );
        assert_eq!(
            svc_override.security_tier(&ResourceCapability::WriteData),
            SecurityTier::Critical
        );
    }

    #[test]
    fn test_cost_model_derivation() {
        // cost_per_call > 0 → PerCall
        let svc = api_service_from_config("s", "s", search_config());
        assert_eq!(svc.cost_model(), Some(CostModel::PerCall(0.005)));

        // cost_per_call = None → Free
        let mut config = search_config();
        config.cost_per_call = None;
        let svc_free = api_service_from_config("s2", "s2", config);
        assert_eq!(svc_free.cost_model(), Some(CostModel::Free));

        // cost_per_call = 0.0 → Free
        let mut config_zero = search_config();
        config_zero.cost_per_call = Some(0.0);
        let svc_zero = api_service_from_config("s3", "s3", config_zero);
        assert_eq!(svc_zero.cost_model(), Some(CostModel::Free));
    }
}
