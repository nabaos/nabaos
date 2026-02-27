use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::error::Result;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    Sensor,
    Camera,
    Actuator,
    Display,
    Composite,
}

// ---------------------------------------------------------------------------
// DeviceReading / DeviceCommand
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceReading {
    pub timestamp: i64,
    pub values: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCommand {
    pub action: String,
    pub params: HashMap<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// DeviceDriver trait
// ---------------------------------------------------------------------------

pub trait DeviceDriver: Send + Sync + std::fmt::Debug {
    fn driver_name(&self) -> &str;
    fn read(&self) -> Result<DeviceReading>;
    fn write(&self, command: &DeviceCommand) -> Result<()>;
    fn supports_stream(&self) -> bool;
}

// ---------------------------------------------------------------------------
// DeviceConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub device_type: DeviceType,
    pub driver: String, // "home_assistant", "http"
    pub endpoint: Option<String>,
    pub entity_id: Option<String>, // for HA driver
    pub location: Option<String>,
    #[serde(default)]
    pub security_tier_override: Option<super::SecurityTier>,
    #[serde(default)]
    pub default_quota: Option<super::registry::LeaseQuota>,
}

// ---------------------------------------------------------------------------
// DeviceResource
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DeviceResource {
    pub id: String,
    pub name: String,
    pub config: DeviceConfig,
    pub status: super::ResourceStatus,
    pub last_reading: Option<DeviceReading>,
}

// ---------------------------------------------------------------------------
// Resource trait implementation
// ---------------------------------------------------------------------------

impl super::Resource for DeviceResource {
    fn id(&self) -> &str {
        &self.id
    }

    fn resource_type(&self) -> super::ResourceType {
        super::ResourceType::Device
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn status(&self) -> &super::ResourceStatus {
        &self.status
    }

    fn capabilities(&self) -> Vec<super::ResourceCapability> {
        match self.config.device_type {
            DeviceType::Sensor => vec![super::ResourceCapability::ReadData],
            DeviceType::Camera => vec![
                super::ResourceCapability::ReadData,
                super::ResourceCapability::Stream,
            ],
            DeviceType::Actuator => vec![
                super::ResourceCapability::ReadData,
                super::ResourceCapability::WriteData,
            ],
            DeviceType::Display => vec![super::ResourceCapability::WriteData],
            DeviceType::Composite => vec![
                super::ResourceCapability::ReadData,
                super::ResourceCapability::WriteData,
                super::ResourceCapability::Stream,
            ],
        }
    }

    fn cost_model(&self) -> Option<super::CostModel> {
        None
    }

    fn security_tier(&self, capability: &super::ResourceCapability) -> super::SecurityTier {
        if let Some(ref tier) = self.config.security_tier_override {
            return tier.clone();
        }
        super::default_security_tier(&super::ResourceType::Device, capability)
    }

    fn health_check(&self) -> Result<super::HealthReport> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        Ok(super::HealthReport {
            healthy: true,
            message: "device check not implemented".into(),
            checked_at: now,
        })
    }

    fn metadata(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        map.insert(
            "device_type".to_string(),
            format!("{:?}", self.config.device_type),
        );
        map.insert("driver".to_string(), self.config.driver.clone());
        if let Some(ref endpoint) = self.config.endpoint {
            map.insert("endpoint".to_string(), endpoint.clone());
        }
        if let Some(ref entity_id) = self.config.entity_id {
            map.insert("entity_id".to_string(), entity_id.clone());
        }
        if let Some(ref location) = self.config.location {
            map.insert("location".to_string(), location.clone());
        }
        map
    }
}

// ---------------------------------------------------------------------------
// Factory function
// ---------------------------------------------------------------------------

pub fn device_from_config(id: &str, name: &str, config: DeviceConfig) -> DeviceResource {
    DeviceResource {
        id: id.to_string(),
        name: name.to_string(),
        config,
        status: super::ResourceStatus::Available,
        last_reading: None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::Resource;

    #[test]
    fn test_device_type_serde() {
        let variants = vec![
            DeviceType::Sensor,
            DeviceType::Camera,
            DeviceType::Actuator,
            DeviceType::Display,
            DeviceType::Composite,
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant).unwrap();
            let back: DeviceType = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, back);
        }
    }

    #[test]
    fn test_device_reading_serde() {
        let mut values = HashMap::new();
        values.insert(
            "temperature".to_string(),
            serde_json::Value::Number(serde_json::Number::from_f64(22.5).unwrap()),
        );
        values.insert(
            "unit".to_string(),
            serde_json::Value::String("celsius".to_string()),
        );
        values.insert("active".to_string(), serde_json::Value::Bool(true));

        let reading = DeviceReading {
            timestamp: 1700000000,
            values,
        };

        let json = serde_json::to_string(&reading).unwrap();
        let back: DeviceReading = serde_json::from_str(&json).unwrap();

        assert_eq!(back.timestamp, 1700000000);
        assert_eq!(back.values.len(), 3);
        assert_eq!(
            back.values["unit"],
            serde_json::Value::String("celsius".to_string())
        );
        assert_eq!(back.values["active"], serde_json::Value::Bool(true));
    }

    #[test]
    fn test_device_command_serde() {
        let mut params = HashMap::new();
        params.insert(
            "brightness".to_string(),
            serde_json::Value::Number(serde_json::Number::from(255)),
        );
        params.insert(
            "color".to_string(),
            serde_json::Value::String("warm_white".to_string()),
        );

        let cmd = DeviceCommand {
            action: "turn_on".to_string(),
            params,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        let back: DeviceCommand = serde_json::from_str(&json).unwrap();

        assert_eq!(back.action, "turn_on");
        assert_eq!(back.params.len(), 2);
        assert_eq!(
            back.params["brightness"],
            serde_json::Value::Number(serde_json::Number::from(255))
        );
    }

    #[test]
    fn test_device_resource_capabilities_sensor() {
        let config = DeviceConfig {
            device_type: DeviceType::Sensor,
            driver: "http".to_string(),
            endpoint: Some("http://localhost:8080/sensor".to_string()),
            entity_id: None,
            location: Some("living_room".to_string()),
            security_tier_override: None,
            default_quota: None,
        };
        let device = device_from_config("dev-1", "Temp Sensor", config);
        let caps = device.capabilities();

        assert_eq!(caps.len(), 1);
        assert!(caps.contains(&crate::resource::ResourceCapability::ReadData));
    }

    #[test]
    fn test_device_resource_capabilities_actuator() {
        let config = DeviceConfig {
            device_type: DeviceType::Actuator,
            driver: "home_assistant".to_string(),
            endpoint: None,
            entity_id: Some("switch.garage_door".to_string()),
            location: Some("garage".to_string()),
            security_tier_override: None,
            default_quota: None,
        };
        let device = device_from_config("dev-2", "Garage Door", config);
        let caps = device.capabilities();

        assert_eq!(caps.len(), 2);
        assert!(caps.contains(&crate::resource::ResourceCapability::ReadData));
        assert!(caps.contains(&crate::resource::ResourceCapability::WriteData));
    }

    #[test]
    fn test_device_security_tier_override() {
        let config = DeviceConfig {
            device_type: DeviceType::Sensor,
            driver: "http".to_string(),
            endpoint: Some("http://localhost:8080/sensor".to_string()),
            entity_id: None,
            location: None,
            security_tier_override: Some(crate::resource::SecurityTier::Critical),
            default_quota: None,
        };
        let device = device_from_config("dev-3", "Critical Sensor", config);

        // Override should take precedence regardless of capability
        assert_eq!(
            device.security_tier(&crate::resource::ResourceCapability::ReadData),
            crate::resource::SecurityTier::Critical
        );
        assert_eq!(
            device.security_tier(&crate::resource::ResourceCapability::WriteData),
            crate::resource::SecurityTier::Critical
        );

        // Without override, default should apply
        let config_no_override = DeviceConfig {
            device_type: DeviceType::Sensor,
            driver: "http".to_string(),
            endpoint: None,
            entity_id: None,
            location: None,
            security_tier_override: None,
            default_quota: None,
        };
        let device_no_override = device_from_config("dev-4", "Normal Sensor", config_no_override);

        assert_eq!(
            device_no_override.security_tier(&crate::resource::ResourceCapability::ReadData),
            crate::resource::SecurityTier::ExternalRead
        );
        assert_eq!(
            device_no_override.security_tier(&crate::resource::ResourceCapability::WriteData),
            crate::resource::SecurityTier::ExternalWrite
        );
    }
}
