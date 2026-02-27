use super::super::device::{DeviceCommand, DeviceDriver, DeviceReading};
use crate::core::error::{NyayaError, Result};

#[derive(Debug)]
pub struct HomeAssistantDriver {
    pub entity_id: String,
}

impl DeviceDriver for HomeAssistantDriver {
    fn driver_name(&self) -> &str {
        "home_assistant"
    }

    fn read(&self) -> Result<DeviceReading> {
        let config = crate::modules::home_assistant::HaConfig::from_env()
            .map_err(|e| NyayaError::Config(format!("Home Assistant not configured: {}", e)))?;
        let entity = crate::modules::home_assistant::get_state(&config, &self.entity_id)
            .map_err(|e| NyayaError::Config(format!("HA entity read failed: {}", e)))?;

        let mut values = std::collections::HashMap::new();
        values.insert("state".to_string(), serde_json::Value::String(entity.state));
        values.insert("attributes".to_string(), entity.attributes);

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        Ok(DeviceReading {
            timestamp: ts,
            values,
        })
    }

    fn write(&self, command: &DeviceCommand) -> Result<()> {
        let config = crate::modules::home_assistant::HaConfig::from_env()
            .map_err(|e| NyayaError::Config(format!("Home Assistant not configured: {}", e)))?;

        // Extract domain from entity_id (e.g., "light" from "light.living_room")
        let domain = self.entity_id.split('.').next().unwrap_or("homeassistant");

        let params_value = serde_json::to_value(&command.params).ok();
        crate::modules::home_assistant::set_state(
            &config,
            domain,
            &command.action,
            &self.entity_id,
            params_value.as_ref(),
        )
        .map_err(|e| NyayaError::Config(format!("HA entity write failed: {}", e)))?;

        Ok(())
    }

    fn supports_stream(&self) -> bool {
        false
    }
}
