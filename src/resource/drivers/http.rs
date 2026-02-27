use super::super::device::{DeviceCommand, DeviceDriver, DeviceReading};
use crate::core::error::{NyayaError, Result};

#[derive(Debug)]
pub struct HttpDriver {
    pub read_url: String,
    pub write_url: Option<String>,
}

impl DeviceDriver for HttpDriver {
    fn driver_name(&self) -> &str {
        "http"
    }

    fn read(&self) -> Result<DeviceReading> {
        let output = std::process::Command::new("curl")
            .args(["-s", "-f", "--max-time", "10", &self.read_url])
            .output()
            .map_err(|e| NyayaError::Config(format!("HTTP read failed: {}", e)))?;

        if !output.status.success() {
            return Err(NyayaError::Config(format!(
                "HTTP read failed: status {}",
                output.status
            )));
        }

        let body = String::from_utf8_lossy(&output.stdout);
        let values: std::collections::HashMap<String, serde_json::Value> =
            serde_json::from_str(&body).unwrap_or_else(|_| {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "raw".to_string(),
                    serde_json::Value::String(body.to_string()),
                );
                m
            });

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
        let url = self.write_url.as_deref().unwrap_or(&self.read_url);
        let body = serde_json::json!({
            "action": command.action,
            "params": command.params,
        });

        let output = std::process::Command::new("curl")
            .args([
                "-s",
                "-f",
                "--max-time",
                "10",
                "-X",
                "POST",
                "-H",
                "Content-Type: application/json",
                "-d",
                &body.to_string(),
                url,
            ])
            .output()
            .map_err(|e| NyayaError::Config(format!("HTTP write failed: {}", e)))?;

        if !output.status.success() {
            return Err(NyayaError::Config(format!(
                "HTTP write failed: status {}",
                output.status
            )));
        }

        Ok(())
    }

    fn supports_stream(&self) -> bool {
        false
    }
}
