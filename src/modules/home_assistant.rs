//! Home Assistant REST API client.
//!
//! Provides typed wrappers around the Home Assistant REST API for:
//! - Listing entity states (`GET /api/states`)
//! - Getting a single entity state (`GET /api/states/{entity_id}`)
//! - Calling services (`POST /api/services/{domain}/{service}`)
//!
//! Configuration is read from `NABA_HA_URL` and `NABA_HA_TOKEN` env vars.
//! The long-lived access token should be stored in the vault in production.

use serde::{Deserialize, Serialize};

/// Home Assistant connection configuration.
#[derive(Debug, Clone)]
pub struct HaConfig {
    /// Base URL of the Home Assistant instance (e.g. `http://homeassistant.local:8123`).
    pub url: String,
    /// Long-lived access token for authentication.
    pub token: String,
}

impl HaConfig {
    /// Load configuration from environment variables.
    ///
    /// Requires:
    /// - `NABA_HA_URL` — Base URL (trailing slash stripped)
    /// - `NABA_HA_TOKEN` — Long-lived access token
    pub fn from_env() -> Result<Self, String> {
        let url = std::env::var("NABA_HA_URL").map_err(|_| {
            "NABA_HA_URL env var not set — required for Home Assistant integration".to_string()
        })?;

        if url.is_empty() {
            return Err("NABA_HA_URL must not be empty".into());
        }

        let token = std::env::var("NABA_HA_TOKEN").map_err(|_| {
            "NABA_HA_TOKEN env var not set — required for Home Assistant integration".to_string()
        })?;

        if token.is_empty() {
            return Err("NABA_HA_TOKEN must not be empty".into());
        }

        // Strip trailing slash for consistent URL joining
        let url = url.trim_end_matches('/').to_string();

        Ok(Self { url, token })
    }
}

/// A Home Assistant entity state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaEntity {
    /// Entity ID (e.g. `light.living_room`, `sensor.temperature`).
    pub entity_id: String,
    /// Current state value (e.g. `on`, `off`, `23.5`).
    pub state: String,
    /// Entity attributes (varies by entity type).
    pub attributes: serde_json::Value,
    /// ISO 8601 timestamp of last state change.
    #[serde(default)]
    pub last_changed: String,
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Validate an entity_id matches the `domain.object_id` format.
/// Both parts must be non-empty and contain only alphanumeric chars plus underscore.
pub fn validate_entity_id(entity_id: &str) -> Result<(), String> {
    if entity_id.is_empty() {
        return Err("entity_id must not be empty".into());
    }

    let parts: Vec<&str> = entity_id.splitn(2, '.').collect();
    if parts.len() != 2 {
        return Err(format!(
            "entity_id must be in 'domain.name' format, got '{}'",
            entity_id
        ));
    }

    let domain = parts[0];
    let name = parts[1];

    if domain.is_empty() || name.is_empty() {
        return Err(format!(
            "entity_id domain and name must both be non-empty, got '{}'",
            entity_id
        ));
    }

    if !domain
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(format!(
            "entity_id domain must be alphanumeric/underscore, got '{}'",
            domain
        ));
    }

    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!(
            "entity_id name must be alphanumeric/underscore, got '{}'",
            name
        ));
    }

    Ok(())
}

/// Validate a domain or service name (alphanumeric + underscore only).
pub fn validate_domain_service(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{} must not be empty", label));
    }
    if !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!(
            "{} must be alphanumeric/underscore only, got '{}'",
            label, value
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// API client functions
// ---------------------------------------------------------------------------

/// List all entity states from Home Assistant.
/// Optionally filters by domain prefix (e.g. `light`, `sensor`).
pub fn list_entities(
    config: &HaConfig,
    domain_filter: Option<&str>,
) -> Result<Vec<HaEntity>, String> {
    // Validate domain_filter if provided
    if let Some(df) = domain_filter {
        validate_domain_service(df, "domain_filter")?;
    }

    let url = format!("{}/api/states", config.url);
    let response = send_ha_request("GET", &url, &config.token, None)?;

    let entities: Vec<HaEntity> = serde_json::from_str(&response)
        .map_err(|e| format!("Failed to parse HA states response: {}", e))?;

    match domain_filter {
        Some(df) => {
            let prefix = format!("{}.", df);
            Ok(entities
                .into_iter()
                .filter(|e| e.entity_id.starts_with(&prefix))
                .collect())
        }
        None => Ok(entities),
    }
}

/// Get the state of a single entity.
pub fn get_state(config: &HaConfig, entity_id: &str) -> Result<HaEntity, String> {
    validate_entity_id(entity_id)?;

    let url = format!("{}/api/states/{}", config.url, entity_id);
    let response = send_ha_request("GET", &url, &config.token, None)?;

    let entity: HaEntity = serde_json::from_str(&response)
        .map_err(|e| format!("Failed to parse HA entity response: {}", e))?;

    Ok(entity)
}

/// Call a Home Assistant service.
///
/// - `domain`: Service domain (e.g. `light`, `switch`, `climate`)
/// - `service`: Service name (e.g. `turn_on`, `turn_off`, `set_temperature`)
/// - `entity_id`: Target entity
/// - `data`: Additional service data (merged with entity_id in the request body)
pub fn set_state(
    config: &HaConfig,
    domain: &str,
    service: &str,
    entity_id: &str,
    data: Option<&serde_json::Value>,
) -> Result<String, String> {
    validate_domain_service(domain, "domain")?;
    validate_domain_service(service, "service")?;
    validate_entity_id(entity_id)?;

    let url = format!("{}/api/services/{}/{}", config.url, domain, service);

    // Build request body: always include entity_id, merge additional data
    let mut body = match data {
        Some(serde_json::Value::Object(map)) => {
            let mut m = serde_json::Map::new();
            for (k, v) in map {
                m.insert(k.clone(), v.clone());
            }
            m
        }
        Some(_) => {
            return Err("data must be a JSON object if provided".into());
        }
        None => serde_json::Map::new(),
    };
    body.insert(
        "entity_id".to_string(),
        serde_json::Value::String(entity_id.to_string()),
    );

    let body_str = serde_json::to_string(&body)
        .map_err(|e| format!("Failed to serialize service call body: {}", e))?;

    let response = send_ha_request("POST", &url, &config.token, Some(&body_str))?;

    Ok(response)
}

// ---------------------------------------------------------------------------
// HTTP helper
// ---------------------------------------------------------------------------

/// Send an HTTP request to Home Assistant with Bearer token auth.
/// Uses `std::process::Command` calling `curl` to avoid adding HTTP client deps
/// for this module alone. The main runtime already has reqwest available, but
/// these host functions run synchronously.
fn send_ha_request(
    method: &str,
    url: &str,
    token: &str,
    body: Option<&str>,
) -> Result<String, String> {
    let mut cmd = std::process::Command::new("curl");
    cmd.arg("-s") // silent
        .arg("-S") // show errors
        .arg("--fail-with-body") // fail on HTTP errors but still capture body
        .arg("-X")
        .arg(method)
        .arg("-H")
        .arg(format!("Authorization: Bearer {}", token))
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("--max-time")
        .arg("30");

    if let Some(b) = body {
        cmd.arg("-d").arg(b);
    }

    cmd.arg(url);

    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to execute curl for HA request: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "HA API request failed ({}): {} {}",
            method,
            stderr.trim(),
            stdout.trim()
        ));
    }

    let response = String::from_utf8(output.stdout)
        .map_err(|e| format!("HA API response is not valid UTF-8: {}", e))?;

    Ok(response)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- HaConfig tests --
    // NOTE: env var tests combined into one test to avoid data races
    // (env vars are process-global and tests run in parallel).

    #[test]
    fn test_config_from_env_scenarios() {
        // 1. Missing URL
        std::env::remove_var("NABA_HA_URL");
        std::env::remove_var("NABA_HA_TOKEN");
        let result = HaConfig::from_env();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("NABA_HA_URL"));

        // 2. Missing token
        std::env::set_var("NABA_HA_URL", "http://localhost:8123");
        std::env::remove_var("NABA_HA_TOKEN");
        let result = HaConfig::from_env();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("NABA_HA_TOKEN"));

        // 3. Empty URL
        std::env::set_var("NABA_HA_URL", "");
        std::env::set_var("NABA_HA_TOKEN", "tok");
        let result = HaConfig::from_env();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must not be empty"));

        // 4. Empty token
        std::env::set_var("NABA_HA_URL", "http://ha:8123");
        std::env::set_var("NABA_HA_TOKEN", "");
        let result = HaConfig::from_env();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must not be empty"));

        // 5. Valid config — strips trailing slash
        std::env::set_var("NABA_HA_URL", "http://localhost:8123/");
        std::env::set_var("NABA_HA_TOKEN", "test_token");
        let config = HaConfig::from_env().unwrap();
        assert_eq!(config.url, "http://localhost:8123");
        assert_eq!(config.token, "test_token");

        // Cleanup
        std::env::remove_var("NABA_HA_URL");
        std::env::remove_var("NABA_HA_TOKEN");
    }

    // -- entity_id validation --

    #[test]
    fn test_validate_entity_id_valid() {
        assert!(validate_entity_id("light.living_room").is_ok());
        assert!(validate_entity_id("sensor.temperature_1").is_ok());
        assert!(validate_entity_id("switch.kitchen").is_ok());
        assert!(validate_entity_id("climate.bedroom").is_ok());
        assert!(validate_entity_id("input_boolean.away_mode").is_ok());
    }

    #[test]
    fn test_validate_entity_id_missing_dot() {
        assert!(validate_entity_id("lightliving_room").is_err());
    }

    #[test]
    fn test_validate_entity_id_empty() {
        assert!(validate_entity_id("").is_err());
    }

    #[test]
    fn test_validate_entity_id_empty_parts() {
        assert!(validate_entity_id(".living_room").is_err());
        assert!(validate_entity_id("light.").is_err());
    }

    #[test]
    fn test_validate_entity_id_special_chars() {
        assert!(validate_entity_id("light.living-room").is_err()); // hyphen
        assert!(validate_entity_id("light.living room").is_err()); // space
        assert!(validate_entity_id("light.living/room").is_err()); // slash
        assert!(validate_entity_id("light.living;room").is_err()); // semicolon
    }

    #[test]
    fn test_validate_entity_id_injection() {
        assert!(validate_entity_id("light.x/../../../etc/passwd").is_err());
        assert!(validate_entity_id("light.x\0y").is_err());
    }

    // -- domain/service validation --

    #[test]
    fn test_validate_domain_service_valid() {
        assert!(validate_domain_service("light", "domain").is_ok());
        assert!(validate_domain_service("turn_on", "service").is_ok());
        assert!(validate_domain_service("set_temperature", "service").is_ok());
    }

    #[test]
    fn test_validate_domain_service_empty() {
        assert!(validate_domain_service("", "domain").is_err());
    }

    #[test]
    fn test_validate_domain_service_special_chars() {
        assert!(validate_domain_service("light/dark", "domain").is_err());
        assert!(validate_domain_service("turn-on", "service").is_err());
        assert!(validate_domain_service("turn on", "service").is_err());
        assert!(validate_domain_service("turn;on", "service").is_err());
    }

    // -- HaEntity deserialization --

    #[test]
    fn test_entity_deserialization() {
        let json = r#"{
            "entity_id": "light.living_room",
            "state": "on",
            "attributes": {
                "brightness": 255,
                "friendly_name": "Living Room Light"
            },
            "last_changed": "2026-02-24T10:30:00+00:00"
        }"#;

        let entity: HaEntity = serde_json::from_str(json).unwrap();
        assert_eq!(entity.entity_id, "light.living_room");
        assert_eq!(entity.state, "on");
        assert_eq!(entity.attributes["brightness"], 255);
        assert_eq!(entity.last_changed, "2026-02-24T10:30:00+00:00");
    }

    #[test]
    fn test_entity_deserialization_missing_last_changed() {
        let json = r#"{
            "entity_id": "sensor.temp",
            "state": "22.5",
            "attributes": {}
        }"#;

        let entity: HaEntity = serde_json::from_str(json).unwrap();
        assert_eq!(entity.entity_id, "sensor.temp");
        assert_eq!(entity.state, "22.5");
        assert_eq!(entity.last_changed, ""); // default
    }

    #[test]
    fn test_entity_list_deserialization() {
        let json = r#"[
            {"entity_id": "light.a", "state": "on", "attributes": {}, "last_changed": ""},
            {"entity_id": "sensor.b", "state": "23", "attributes": {"unit": "C"}, "last_changed": ""}
        ]"#;

        let entities: Vec<HaEntity> = serde_json::from_str(json).unwrap();
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].entity_id, "light.a");
        assert_eq!(entities[1].entity_id, "sensor.b");
    }

    // -- set_state body building --

    #[test]
    fn test_set_state_validation() {
        // Domain with special chars should fail
        let config = HaConfig {
            url: "http://ha:8123".into(),
            token: "test".into(),
        };
        let result = set_state(&config, "light/evil", "turn_on", "light.x", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("domain"));
    }

    #[test]
    fn test_set_state_bad_service() {
        let config = HaConfig {
            url: "http://ha:8123".into(),
            token: "test".into(),
        };
        let result = set_state(&config, "light", "turn-on", "light.x", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("service"));
    }

    #[test]
    fn test_set_state_bad_entity() {
        let config = HaConfig {
            url: "http://ha:8123".into(),
            token: "test".into(),
        };
        let result = set_state(&config, "light", "turn_on", "bad_id", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("entity_id"));
    }

    #[test]
    fn test_set_state_data_must_be_object() {
        let config = HaConfig {
            url: "http://ha:8123".into(),
            token: "test".into(),
        };
        let data = serde_json::json!("not an object");
        // This should fail at data validation, not at the HTTP call
        let result = set_state(&config, "light", "turn_on", "light.room", Some(&data));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("data must be a JSON object"));
    }
}
