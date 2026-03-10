//! Plugin system — extends the ability catalog with plugins, subprocesses, and cloud endpoints.
//!
//! Four ways to add an ability:
//!   A) Built-in: native Rust compiled into Nyaya binary
//!   B) Plugin: .so/.dylib loaded at runtime (future — currently manifest-only registration)
//!   C) Subprocess: shell out to existing CLI tool (ffmpeg, tesseract, etc.)
//!   D) Cloud: offload to remote HTTP endpoint
//!
//! Resolution order: built-in > plugin > subprocess > cloud > error

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::core::error::{NyayaError, Result};

/// How an ability is provided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AbilitySource {
    /// Native Rust compiled into the binary.
    BuiltIn,
    /// Native shared library (.so/.dylib) loaded at runtime.
    Plugin,
    /// External CLI tool spawned as subprocess.
    Subprocess,
    /// Remote HTTP endpoint.
    Cloud,
    /// Hardware GPIO/sensor/actuator on the device.
    Hardware,
}

impl std::fmt::Display for AbilitySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AbilitySource::BuiltIn => write!(f, "built-in"),
            AbilitySource::Plugin => write!(f, "plugin"),
            AbilitySource::Subprocess => write!(f, "subprocess"),
            AbilitySource::Cloud => write!(f, "cloud"),
            AbilitySource::Hardware => write!(f, "hardware"),
        }
    }
}

/// Trust level for plugins (controls what verification is required).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TrustLevel {
    /// User's own plugin — user's responsibility.
    Local = 0,
    /// Community-written, unreviewed — user must explicitly accept risk.
    Community = 1,
    /// Community-written, Nyaya-reviewed — mostly trusted.
    Verified = 2,
    /// Nyaya team authored/audited — fully trusted.
    Official = 3,
}

impl std::fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrustLevel::Local => write!(f, "LOCAL"),
            TrustLevel::Community => write!(f, "COMMUNITY"),
            TrustLevel::Verified => write!(f, "VERIFIED"),
            TrustLevel::Official => write!(f, "OFFICIAL"),
        }
    }
}

/// Security constraints for a plugin or subprocess ability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConstraints {
    /// Filesystem access level.
    #[serde(default = "default_fs_access")]
    pub filesystem_access: String,
    /// Whether network access is allowed.
    #[serde(default)]
    pub network_access: bool,
    /// Memory limit (e.g., "512MB").
    #[serde(default)]
    pub memory_limit: Option<String>,
    /// Execution timeout (e.g., "30s").
    #[serde(default)]
    pub timeout: Option<String>,
    /// Allowed read paths (for subprocess sandboxing).
    #[serde(default)]
    pub read_paths: Vec<String>,
    /// Allowed write paths (for subprocess sandboxing).
    #[serde(default)]
    pub write_paths: Vec<String>,
}

fn default_fs_access() -> String {
    "none".to_string()
}

impl Default for SecurityConstraints {
    fn default() -> Self {
        Self {
            filesystem_access: "none".to_string(),
            network_access: false,
            memory_limit: None,
            timeout: None,
            read_paths: vec![],
            write_paths: vec![],
        }
    }
}

/// A plugin manifest (loaded from YAML).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Plugin name.
    pub name: String,
    /// Plugin version.
    pub version: String,
    /// Author.
    #[serde(default)]
    pub author: Option<String>,
    /// License.
    #[serde(default)]
    pub license: Option<String>,
    /// Trust level.
    #[serde(default = "default_trust_level")]
    pub trust_level: TrustLevel,
    /// The ability this plugin provides.
    pub ability: PluginAbilityDef,
    /// Input parameter schema.
    #[serde(default)]
    pub input: HashMap<String, ParamSchema>,
    /// Output field schema.
    #[serde(default)]
    pub output: HashMap<String, String>,
    /// Fields to include in the receipt.
    #[serde(default)]
    pub receipt_fields: Vec<String>,
    /// Security constraints.
    #[serde(default)]
    pub security: SecurityConstraints,
}

fn default_trust_level() -> TrustLevel {
    TrustLevel::Local
}

/// The ability definition within a plugin manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAbilityDef {
    /// Fully qualified ability name (e.g., "files.read_psd").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Required permission tier (0-4, higher = more restricted).
    #[serde(default)]
    pub permission_tier: u8,
}

/// Parameter schema for plugin input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamSchema {
    /// Parameter type (string, int, bool, filepath, etc.).
    #[serde(rename = "type")]
    pub param_type: String,
    /// Default value.
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    /// Whether this parameter is required.
    #[serde(default)]
    pub required: Option<bool>,
    /// Auto-generate pattern (for output paths).
    #[serde(default)]
    pub auto: Option<String>,
}

/// Configuration for a subprocess ability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubprocessAbilityConfig {
    /// The command template with {param} placeholders.
    pub command: String,
    /// Parameter definitions.
    #[serde(default)]
    pub params: HashMap<String, ParamSchema>,
    /// Sandbox constraints.
    #[serde(default)]
    pub sandbox: SecurityConstraints,
    /// Fields to include in the receipt.
    #[serde(default)]
    pub receipt_fields: Vec<String>,
    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,
}

/// Configuration for a cloud ability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudAbilityConfig {
    /// The endpoint URL (with optional {param} placeholders).
    pub endpoint: String,
    /// HTTP method (GET, POST, etc.).
    #[serde(default = "default_method")]
    pub method: String,
    /// Headers to include.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Parameter definitions.
    #[serde(default)]
    pub params: HashMap<String, ParamSchema>,
    /// Timeout in seconds.
    #[serde(default = "default_cloud_timeout")]
    pub timeout_secs: u64,
    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,
    /// Fields to include in the receipt.
    #[serde(default)]
    pub receipt_fields: Vec<String>,
}

fn default_method() -> String {
    "POST".to_string()
}

fn default_cloud_timeout() -> u64 {
    30
}

/// A registered external ability (non-built-in).
#[derive(Debug, Clone)]
pub struct ExternalAbility {
    /// Ability name.
    pub name: String,
    /// Description.
    pub description: String,
    /// How this ability is provided.
    pub source: AbilitySource,
    /// Trust level.
    pub trust_level: TrustLevel,
    /// The execution config.
    pub config: ExternalAbilityConfig,
    /// Security constraints.
    pub security: SecurityConstraints,
    /// Receipt fields.
    pub receipt_fields: Vec<String>,
}

/// Execution configuration for an external ability.
#[derive(Debug, Clone)]
pub enum ExternalAbilityConfig {
    /// Plugin shared library path.
    Plugin { library_path: PathBuf },
    /// Subprocess command template.
    Subprocess(SubprocessAbilityConfig),
    /// Cloud HTTP endpoint.
    Cloud(CloudAbilityConfig),
    /// Hardware GPIO/sensor/actuator resource.
    Hardware {
        pin: u16,
        mode: String, // "digital_read", "analog_read", "digital_write", etc.
        transform_scale: f64,
        transform_offset: f64,
    },
}

/// Trait for dynamically loaded plugins.
/// Plugins implement this trait to expose abilities to the orchestrator.
pub trait Plugin: Send + Sync {
    /// The plugin's unique name.
    fn name(&self) -> &str;

    /// List of abilities this plugin provides.
    fn abilities(&self) -> Vec<String>;

    /// Execute an ability with the given input.
    fn execute(&self, ability: &str, input: serde_json::Value) -> std::result::Result<serde_json::Value, String>;

    /// Health check — returns true if the plugin is operational.
    fn health_check(&self) -> bool;
}

/// The plugin registry — stores all non-built-in abilities.
pub struct PluginRegistry {
    /// External abilities keyed by name.
    abilities: HashMap<String, ExternalAbility>,
    /// Directory where plugins are installed.
    plugin_dir: PathBuf,
    /// Dynamic plugin instances.
    plugins: Vec<Box<dyn Plugin>>,
}

impl PluginRegistry {
    /// Create a new plugin registry, scanning the plugin directory.
    pub fn new(plugin_dir: &Path) -> Self {
        let mut registry = Self {
            abilities: HashMap::new(),
            plugin_dir: plugin_dir.to_path_buf(),
            plugins: Vec::new(),
        };

        // Scan plugin directory for manifests
        if plugin_dir.exists() {
            if let Err(e) = registry.scan_plugins() {
                eprintln!("[warn] Failed to scan plugins: {}", e);
            }
        }

        registry
    }

    /// Create an empty registry (no plugin directory).
    pub fn empty() -> Self {
        Self {
            abilities: HashMap::new(),
            plugin_dir: PathBuf::from("/dev/null"),
            plugins: Vec::new(),
        }
    }

    /// Register a dynamic plugin instance.
    pub fn register_plugin(&mut self, plugin: Box<dyn Plugin>) {
        tracing::info!(name = %plugin.name(), abilities = ?plugin.abilities(), "Dynamic plugin registered");
        self.plugins.push(plugin);
    }

    /// Execute an ability via a registered dynamic plugin.
    /// Returns None if no plugin provides this ability.
    pub fn execute_plugin(
        &self,
        ability: &str,
        input: serde_json::Value,
    ) -> Option<std::result::Result<serde_json::Value, String>> {
        for plugin in &self.plugins {
            if plugin.abilities().iter().any(|a| a == ability) {
                return Some(plugin.execute(ability, input));
            }
        }
        None
    }

    /// List all registered dynamic plugins.
    pub fn plugins(&self) -> &[Box<dyn Plugin>] {
        &self.plugins
    }

    /// Scan the plugin directory for manifest files.
    fn scan_plugins(&mut self) -> Result<()> {
        let entries = std::fs::read_dir(&self.plugin_dir)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            // Each plugin is a directory with a manifest.yaml
            if path.is_dir() {
                let manifest_path = path.join("manifest.yaml");
                if manifest_path.exists() {
                    match self.load_plugin_manifest(&manifest_path) {
                        Ok(()) => {}
                        Err(e) => {
                            eprintln!("[warn] Failed to load plugin {}: {}", path.display(), e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Load a plugin from its manifest file.
    fn load_plugin_manifest(&mut self, manifest_path: &Path) -> Result<()> {
        let content = std::fs::read_to_string(manifest_path)?;
        let manifest: PluginManifest = serde_yaml::from_str(&content)?;

        let ability = ExternalAbility {
            name: manifest.ability.name.clone(),
            description: manifest.ability.description.clone(),
            source: AbilitySource::Plugin,
            trust_level: manifest.trust_level,
            config: ExternalAbilityConfig::Plugin {
                library_path: manifest_path
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(format!("lib{}.so", manifest.name)),
            },
            security: manifest.security.clone(),
            receipt_fields: manifest.receipt_fields.clone(),
        };

        tracing::info!(
            name = %manifest.ability.name,
            plugin = %manifest.name,
            trust = %manifest.trust_level,
            "Plugin ability registered"
        );

        self.abilities
            .insert(manifest.ability.name.clone(), ability);
        Ok(())
    }

    /// Register a subprocess ability from config.
    pub fn register_subprocess(&mut self, name: &str, config: SubprocessAbilityConfig) {
        let description = config
            .description
            .clone()
            .unwrap_or_else(|| format!("Subprocess ability: {}", name));

        let ability = ExternalAbility {
            name: name.to_string(),
            description,
            source: AbilitySource::Subprocess,
            trust_level: TrustLevel::Local,
            config: ExternalAbilityConfig::Subprocess(config.clone()),
            security: config.sandbox.clone(),
            receipt_fields: config.receipt_fields.clone(),
        };

        tracing::info!(name = %name, "Subprocess ability registered");
        self.abilities.insert(name.to_string(), ability);
    }

    /// Register a cloud ability from config.
    pub fn register_cloud(&mut self, name: &str, config: CloudAbilityConfig) {
        let description = config
            .description
            .clone()
            .unwrap_or_else(|| format!("Cloud ability: {}", name));

        let ability = ExternalAbility {
            name: name.to_string(),
            description,
            source: AbilitySource::Cloud,
            trust_level: TrustLevel::Community,
            config: ExternalAbilityConfig::Cloud(config.clone()),
            security: SecurityConstraints {
                network_access: true,
                ..SecurityConstraints::default()
            },
            receipt_fields: config.receipt_fields.clone(),
        };

        tracing::info!(name = %name, "Cloud ability registered");
        self.abilities.insert(name.to_string(), ability);
    }

    /// Register an ability directly by name.
    pub fn register_ability(&mut self, name: &str, ability: ExternalAbility) {
        self.abilities.insert(name.to_string(), ability);
    }

    /// Load subprocess abilities from a YAML config file.
    pub fn load_subprocess_config(&mut self, path: &Path) -> Result<()> {
        let content = std::fs::read_to_string(path)?;
        let config: HashMap<String, SubprocessAbilityYaml> = serde_yaml::from_str(&content)?;

        for (name, yaml) in config {
            if yaml.ability_type != "subprocess" {
                continue;
            }
            let subprocess_config = SubprocessAbilityConfig {
                command: yaml.command,
                params: yaml.params,
                sandbox: yaml.sandbox,
                receipt_fields: yaml.receipt_fields,
                description: yaml.description,
            };
            self.register_subprocess(&name, subprocess_config);
        }

        Ok(())
    }

    /// Look up an external ability by name.
    pub fn get(&self, name: &str) -> Option<&ExternalAbility> {
        self.abilities.get(name)
    }

    /// List all registered external abilities.
    pub fn list(&self) -> Vec<&ExternalAbility> {
        let mut abilities: Vec<_> = self.abilities.values().collect();
        abilities.sort_by(|a, b| a.name.cmp(&b.name));
        abilities
    }

    /// Execute a subprocess ability.
    pub fn execute_subprocess(
        &self,
        ability: &ExternalAbility,
        input: &serde_json::Value,
    ) -> std::result::Result<SubprocessResult, String> {
        let config = match &ability.config {
            ExternalAbilityConfig::Subprocess(c) => c,
            _ => return Err("Not a subprocess ability".to_string()),
        };

        // Shell metacharacters that are NEVER allowed in parameter values
        const BLOCKED_CHARS: &[char] = &[
            ';', '|', '&', '`', '$', '\n', '\r', '\0', '(', ')', '<', '>', '{', '}', '\'', '"',
            '\\', ' ', '\t',
        ];

        // Build the command by interpolating parameters
        let mut command_str = config.command.clone();
        if let Some(obj) = input.as_object() {
            for (key, value) in obj {
                let placeholder = format!("{{{}}}", key);
                let val_str = match value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };

                // Security: validate parameter values don't contain shell metacharacters
                if val_str.chars().any(|c| BLOCKED_CHARS.contains(&c)) {
                    return Err(format!("Parameter '{}' contains shell metacharacters", key));
                }

                command_str = command_str.replace(&placeholder, &val_str);
            }
        }

        // Fill in defaults for missing params
        for (key, schema) in &config.params {
            let placeholder = format!("{{{}}}", key);
            if command_str.contains(&placeholder) {
                if let Some(ref default) = schema.default {
                    let val_str = match default {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    command_str = command_str.replace(&placeholder, &val_str);
                } else {
                    return Err(format!(
                        "Missing required parameter '{}' with no default",
                        key
                    ));
                }
            }
        }

        // Check for any remaining unresolved placeholders
        if command_str.contains('{') && command_str.contains('}') {
            let re = regex::Regex::new(r"\{[a-zA-Z_][a-zA-Z0-9_.]*\}").unwrap();
            if let Some(m) = re.find(&command_str) {
                return Err(format!("Unresolved parameter in command: {}", m.as_str()));
            }
        }

        // Parse timeout from security constraints
        let timeout_secs = ability
            .security
            .timeout
            .as_ref()
            .and_then(|t| parse_duration_secs(t))
            .unwrap_or(60);

        // Execute the command — NO sh -c. Split into program + args and exec directly.
        let start = std::time::Instant::now();

        let parts = subprocess_split(&command_str)?;
        if parts.is_empty() {
            return Err("Empty command after parameter resolution".to_string());
        }

        let mut child = Command::new(&parts[0])
            .args(&parts[1..])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn subprocess '{}': {}", parts[0], e))?;

        // Timeout enforcement via polling
        let deadline = std::time::Duration::from_secs(timeout_secs);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if start.elapsed() > deadline {
                        let _ = child.kill();
                        return Err(format!(
                            "Subprocess timed out after {}s (limit: {}s)",
                            start.elapsed().as_secs(),
                            timeout_secs
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => return Err(format!("Error waiting for subprocess: {}", e)),
            }
        }

        let output = child
            .wait_with_output()
            .map_err(|e| format!("Failed to read subprocess output: {}", e))?;
        let duration_ms = start.elapsed().as_millis() as u64;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(SubprocessResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout,
            stderr,
            duration_ms,
            command: command_str,
        })
    }

    /// Execute a cloud ability via HTTP.
    pub fn execute_cloud(
        &self,
        ability: &ExternalAbility,
        input: &serde_json::Value,
    ) -> std::result::Result<CloudResult, String> {
        let config = match &ability.config {
            ExternalAbilityConfig::Cloud(c) => c,
            _ => return Err("Not a cloud ability".to_string()),
        };

        // SSRF protection: validate endpoint URL
        let parsed_url = url::Url::parse(&config.endpoint)
            .map_err(|e| format!("Invalid cloud endpoint URL: {}", e))?;

        match parsed_url.host_str() {
            None => return Err("Cloud endpoint has no host".to_string()),
            Some(host) => {
                let blocked_hosts = [
                    "localhost",
                    "127.0.0.1",
                    "0.0.0.0",
                    "[::1]",
                    "169.254.169.254",
                    "metadata.google.internal",
                ];
                let host_lower = host.to_lowercase();
                if blocked_hosts.iter().any(|b| host_lower == *b) {
                    return Err(format!(
                        "Cloud endpoint host '{}' is blocked (SSRF protection)",
                        host
                    ));
                }
                // Block private IP ranges
                if host_lower.starts_with("10.")
                    || host_lower.starts_with("192.168.")
                    || host_lower.starts_with("172.")
                {
                    // Check 172.16.0.0/12 range
                    if host_lower.starts_with("172.") {
                        if let Some(second_octet) = host_lower
                            .strip_prefix("172.")
                            .and_then(|r| r.split('.').next())
                            .and_then(|s| s.parse::<u8>().ok())
                        {
                            if (16..=31).contains(&second_octet) {
                                return Err(format!(
                                    "Cloud endpoint host '{}' is a private IP (SSRF protection)",
                                    host
                                ));
                            }
                        }
                    } else {
                        return Err(format!(
                            "Cloud endpoint host '{}' is a private IP (SSRF protection)",
                            host
                        ));
                    }
                }
                // Block non-HTTPS schemes
                if parsed_url.scheme() != "https" {
                    return Err(format!(
                        "Cloud endpoint must use HTTPS (got '{}')",
                        parsed_url.scheme()
                    ));
                }
            }
        }

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| format!("HTTP client build failed: {}", e))?;

        let start = std::time::Instant::now();

        let mut request = match config.method.to_uppercase().as_str() {
            "GET" => client.get(&config.endpoint),
            "POST" => client.post(&config.endpoint).json(input),
            "PUT" => client.put(&config.endpoint).json(input),
            _ => return Err(format!("Unsupported HTTP method: {}", config.method)),
        };

        for (key, value) in &config.headers {
            request = request.header(key.as_str(), value.as_str());
        }

        let response = request
            .send()
            .map_err(|e| format!("Cloud request failed: {}", e))?;

        let status = response.status().as_u16();
        let body = response
            .text()
            .map_err(|e| format!("Failed to read cloud response: {}", e))?;

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(CloudResult {
            status_code: status,
            body,
            duration_ms,
        })
    }

    /// Get the plugin directory path.
    pub fn plugin_dir(&self) -> &Path {
        &self.plugin_dir
    }

    /// Remove a plugin by ability name.
    pub fn remove(&mut self, ability_name: &str) -> bool {
        self.abilities.remove(ability_name).is_some()
    }

    /// Count registered abilities.
    pub fn count(&self) -> usize {
        self.abilities.len()
    }
}

/// Result of a subprocess execution.
#[derive(Debug)]
pub struct SubprocessResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub command: String,
}

/// Result of a cloud ability execution.
#[derive(Debug)]
pub struct CloudResult {
    pub status_code: u16,
    pub body: String,
    pub duration_ms: u64,
}

/// Helper YAML struct for loading subprocess configs from file.
#[derive(Debug, Deserialize)]
struct SubprocessAbilityYaml {
    #[serde(rename = "type")]
    ability_type: String,
    command: String,
    #[serde(default)]
    params: HashMap<String, ParamSchema>,
    #[serde(default)]
    sandbox: SecurityConstraints,
    #[serde(default)]
    receipt_fields: Vec<String>,
    #[serde(default)]
    description: Option<String>,
}

/// Split a command string into program and arguments (simple whitespace splitting).
/// Does NOT support shell features — this is intentional for security.
fn subprocess_split(s: &str) -> std::result::Result<Vec<String>, String> {
    let parts: Vec<String> = s.split_whitespace().map(|p| p.to_string()).collect();
    if parts.is_empty() {
        return Err("Empty command".to_string());
    }
    Ok(parts)
}

/// Parse a duration string like "30s", "5m", "2h" into seconds.
fn parse_duration_secs(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Some(rest) = s.strip_suffix('s') {
        rest.parse().ok()
    } else if let Some(rest) = s.strip_suffix('m') {
        rest.parse::<u64>().ok().map(|v| v * 60)
    } else if let Some(rest) = s.strip_suffix('h') {
        rest.parse::<u64>().ok().map(|v| v * 3600)
    } else {
        s.parse().ok()
    }
}

/// Install a plugin from a manifest path into the plugin directory.
pub fn install_plugin(plugin_dir: &Path, manifest_path: &Path) -> Result<String> {
    // Read and validate the manifest
    let content = std::fs::read_to_string(manifest_path)?;
    let manifest: PluginManifest = serde_yaml::from_str(&content)
        .map_err(|e| NyayaError::Config(format!("Invalid plugin manifest: {}", e)))?;

    // Validate the ability name (only safe characters)
    if !manifest
        .ability
        .name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(NyayaError::Config(format!(
            "Invalid ability name: {}",
            manifest.ability.name
        )));
    }

    // Create plugin directory
    let dest = plugin_dir.join(&manifest.name);
    std::fs::create_dir_all(&dest)?;

    // Copy manifest
    std::fs::copy(manifest_path, dest.join("manifest.yaml"))?;

    // Copy library if it exists next to the manifest
    if let Some(parent) = manifest_path.parent() {
        let lib_name = format!("lib{}.so", manifest.name);
        let lib_path = parent.join(&lib_name);
        if lib_path.exists() {
            std::fs::copy(&lib_path, dest.join(&lib_name))?;
        }
    }

    Ok(manifest.ability.name)
}

/// Remove a plugin by name from the plugin directory.
pub fn remove_plugin(plugin_dir: &Path, plugin_name: &str) -> Result<bool> {
    // Validate name to prevent path traversal
    if plugin_name.contains('/') || plugin_name.contains('\\') || plugin_name.contains("..") {
        return Err(NyayaError::Config(
            "Invalid plugin name: path traversal detected".to_string(),
        ));
    }

    let plugin_path = plugin_dir.join(plugin_name);
    if plugin_path.exists() {
        std::fs::remove_dir_all(&plugin_path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_manifest_parse() {
        let yaml = r#"
name: psd_reader
version: 1.0.0
author: nyaya-community
license: MIT
trust_level: VERIFIED

ability:
  name: files.read_psd
  description: "Read Adobe PSD files, extract layers and metadata"
  permission_tier: 2

input:
  path: {type: string}
  extract_layers: {type: bool, default: true}

output:
  layers: "array"
  width: "int"
  height: "int"

receipt_fields: [file_hash, layers_count, dimensions]

security:
  filesystem_access: read_only
  network_access: false
  memory_limit: 512MB
  timeout: 30s
"#;

        let manifest: PluginManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.name, "psd_reader");
        assert_eq!(manifest.ability.name, "files.read_psd");
        assert_eq!(manifest.trust_level, TrustLevel::Verified);
        assert_eq!(manifest.ability.permission_tier, 2);
        assert!(!manifest.security.network_access);
        assert_eq!(manifest.security.filesystem_access, "read_only");
        assert_eq!(manifest.receipt_fields.len(), 3);
        assert_eq!(manifest.input.len(), 2);
    }

    #[test]
    fn test_subprocess_config_parse() {
        let yaml = r#"
command: "ffmpeg -i {input} -vf scale={width}:{height} {output}"
params:
  input: {type: filepath, required: true}
  width: {type: int, default: 1920}
  height: {type: int, default: 1080}
sandbox:
  read_paths: ["/tmp/input"]
  write_paths: ["/tmp/output"]
  network_access: false
  timeout: 300s
  memory_limit: 2GB
receipt_fields: [input_hash, output_hash, duration, exit_code]
description: "Transcode video using ffmpeg"
"#;

        let config: SubprocessAbilityConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.command.contains("ffmpeg"));
        assert_eq!(config.params.len(), 3);
        assert!(!config.sandbox.network_access);
        assert_eq!(config.receipt_fields.len(), 4);
    }

    #[test]
    fn test_cloud_config_parse() {
        let yaml = r#"
endpoint: "https://api.example.com/v1/generate"
method: POST
headers:
  Authorization: "Bearer test-key"
params:
  prompt: {type: string, required: true}
timeout_secs: 60
description: "Generate image via cloud API"
receipt_fields: [request_id, generation_time]
"#;

        let config: CloudAbilityConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.endpoint, "https://api.example.com/v1/generate");
        assert_eq!(config.method, "POST");
        assert_eq!(config.timeout_secs, 60);
        assert!(config.headers.contains_key("Authorization"));
    }

    #[test]
    fn test_empty_registry() {
        let registry = PluginRegistry::empty();
        assert_eq!(registry.count(), 0);
        assert!(registry.list().is_empty());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_register_subprocess() {
        let mut registry = PluginRegistry::empty();

        let config = SubprocessAbilityConfig {
            command: "echo {text}".to_string(),
            params: HashMap::new(),
            sandbox: SecurityConstraints::default(),
            receipt_fields: vec!["exit_code".to_string()],
            description: Some("Echo text".to_string()),
        };

        registry.register_subprocess("test.echo", config);
        assert_eq!(registry.count(), 1);

        let ability = registry.get("test.echo").unwrap();
        assert_eq!(ability.source, AbilitySource::Subprocess);
        assert_eq!(ability.trust_level, TrustLevel::Local);
    }

    #[test]
    fn test_register_cloud() {
        let mut registry = PluginRegistry::empty();

        let config = CloudAbilityConfig {
            endpoint: "https://api.example.com/v1/process".to_string(),
            method: "POST".to_string(),
            headers: HashMap::new(),
            params: HashMap::new(),
            timeout_secs: 30,
            description: Some("Process data".to_string()),
            receipt_fields: vec![],
        };

        registry.register_cloud("test.process", config);
        assert_eq!(registry.count(), 1);

        let ability = registry.get("test.process").unwrap();
        assert_eq!(ability.source, AbilitySource::Cloud);
    }

    #[test]
    fn test_subprocess_shell_injection_blocked() {
        let registry = PluginRegistry::empty();

        let ability = ExternalAbility {
            name: "test.echo".to_string(),
            description: "Echo test".to_string(),
            source: AbilitySource::Subprocess,
            trust_level: TrustLevel::Local,
            config: ExternalAbilityConfig::Subprocess(SubprocessAbilityConfig {
                command: "echo {text}".to_string(),
                params: HashMap::new(),
                sandbox: SecurityConstraints::default(),
                receipt_fields: vec![],
                description: None,
            }),
            security: SecurityConstraints::default(),
            receipt_fields: vec![],
        };

        // Attempt shell injection via semicolon
        let input = serde_json::json!({"text": "hello; rm -rf /"});
        let result = registry.execute_subprocess(&ability, &input);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("shell metacharacters"));

        // Attempt via pipe
        let input = serde_json::json!({"text": "hello | cat /etc/passwd"});
        let result = registry.execute_subprocess(&ability, &input);
        assert!(result.is_err());

        // Attempt via backtick
        let input = serde_json::json!({"text": "hello `whoami`"});
        let result = registry.execute_subprocess(&ability, &input);
        assert!(result.is_err());

        // Attempt via dollar sign
        let input = serde_json::json!({"text": "hello $(whoami)"});
        let result = registry.execute_subprocess(&ability, &input);
        assert!(result.is_err());
    }

    #[test]
    fn test_subprocess_execution_safe_input() {
        let registry = PluginRegistry::empty();

        let ability = ExternalAbility {
            name: "test.echo".to_string(),
            description: "Echo test".to_string(),
            source: AbilitySource::Subprocess,
            trust_level: TrustLevel::Local,
            config: ExternalAbilityConfig::Subprocess(SubprocessAbilityConfig {
                command: "echo {text}".to_string(),
                params: HashMap::new(),
                sandbox: SecurityConstraints::default(),
                receipt_fields: vec![],
                description: None,
            }),
            security: SecurityConstraints::default(),
            receipt_fields: vec![],
        };

        let input = serde_json::json!({"text": "helloworld"});
        let result = registry.execute_subprocess(&ability, &input).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("helloworld"));
    }

    #[test]
    fn test_trust_level_ordering() {
        assert!(TrustLevel::Local < TrustLevel::Community);
        assert!(TrustLevel::Community < TrustLevel::Verified);
        assert!(TrustLevel::Verified < TrustLevel::Official);
    }

    #[test]
    fn test_ability_source_display() {
        assert_eq!(format!("{}", AbilitySource::BuiltIn), "built-in");
        assert_eq!(format!("{}", AbilitySource::Plugin), "plugin");
        assert_eq!(format!("{}", AbilitySource::Subprocess), "subprocess");
        assert_eq!(format!("{}", AbilitySource::Cloud), "cloud");
    }

    #[test]
    fn test_ability_source_hardware_display() {
        assert_eq!(format!("{}", AbilitySource::Hardware), "hardware");
    }

    #[test]
    fn test_external_ability_config_hardware() {
        let config = ExternalAbilityConfig::Hardware {
            pin: 17,
            mode: "digital_read".to_string(),
            transform_scale: 3.3,
            transform_offset: -0.1,
        };
        match &config {
            ExternalAbilityConfig::Hardware {
                pin,
                mode,
                transform_scale,
                transform_offset,
            } => {
                assert_eq!(*pin, 17);
                assert_eq!(mode, "digital_read");
                assert!((transform_scale - 3.3).abs() < f64::EPSILON);
                assert!((transform_offset - (-0.1)).abs() < f64::EPSILON);
            }
            _ => panic!("Expected Hardware variant"),
        }
    }

    #[test]
    fn test_parse_duration_secs() {
        assert_eq!(parse_duration_secs("30s"), Some(30));
        assert_eq!(parse_duration_secs("5m"), Some(300));
        assert_eq!(parse_duration_secs("2h"), Some(7200));
        assert_eq!(parse_duration_secs("60"), Some(60));
        assert_eq!(parse_duration_secs("abc"), None);
    }

    #[test]
    fn test_remove_ability() {
        let mut registry = PluginRegistry::empty();
        let config = SubprocessAbilityConfig {
            command: "echo test".to_string(),
            params: HashMap::new(),
            sandbox: SecurityConstraints::default(),
            receipt_fields: vec![],
            description: None,
        };
        registry.register_subprocess("test.echo", config);
        assert_eq!(registry.count(), 1);

        assert!(registry.remove("test.echo"));
        assert_eq!(registry.count(), 0);
        assert!(!registry.remove("test.echo")); // Already removed
    }

    #[test]
    fn test_install_validates_ability_name() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("manifest.yaml");

        let yaml = r#"
name: evil_plugin
version: 1.0.0
ability:
  name: "../../etc/passwd"
  description: "Path traversal attempt"
"#;
        std::fs::write(&manifest_path, yaml).unwrap();

        let result = install_plugin(dir.path(), &manifest_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_plugin_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let result = remove_plugin(dir.path(), "../../../etc");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Plugin trait tests
    // -----------------------------------------------------------------------

    struct MockPlugin;

    impl Plugin for MockPlugin {
        fn name(&self) -> &str {
            "mock"
        }
        fn abilities(&self) -> Vec<String> {
            vec!["mock.greet".to_string(), "mock.add".to_string()]
        }
        fn execute(&self, ability: &str, input: serde_json::Value) -> std::result::Result<serde_json::Value, String> {
            match ability {
                "mock.greet" => {
                    let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("world");
                    Ok(serde_json::json!({"greeting": format!("Hello, {}!", name)}))
                }
                "mock.add" => {
                    let a = input.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let b = input.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    Ok(serde_json::json!({"result": a + b}))
                }
                _ => Err(format!("Unknown ability: {}", ability)),
            }
        }
        fn health_check(&self) -> bool {
            true
        }
    }

    #[test]
    fn test_register_and_execute_plugin() {
        let mut registry = PluginRegistry::empty();
        registry.register_plugin(Box::new(MockPlugin));
        assert_eq!(registry.plugins().len(), 1);

        let result = registry
            .execute_plugin("mock.greet", serde_json::json!({"name": "NabaOS"}))
            .unwrap()
            .unwrap();
        assert_eq!(result["greeting"], "Hello, NabaOS!");
    }

    #[test]
    fn test_plugin_execute_unknown_ability() {
        let mut registry = PluginRegistry::empty();
        registry.register_plugin(Box::new(MockPlugin));

        // Ability not provided by any plugin → None
        let result = registry.execute_plugin("nonexistent.ability", serde_json::json!({}));
        assert!(result.is_none());
    }

    #[test]
    fn test_plugin_health_check() {
        let plugin = MockPlugin;
        assert!(plugin.health_check());
    }

    #[test]
    fn test_plugin_dir_scan() {
        let dir = tempfile::tempdir().unwrap();
        let plugin_dir = dir.path().join("plugins");
        std::fs::create_dir_all(&plugin_dir).unwrap();

        // Create a plugin with manifest
        let test_plugin = plugin_dir.join("test_plugin");
        std::fs::create_dir_all(&test_plugin).unwrap();
        std::fs::write(
            test_plugin.join("manifest.yaml"),
            r#"
name: test_plugin
version: 1.0.0
ability:
  name: test.ability
  description: "A test ability"
"#,
        )
        .unwrap();

        let registry = PluginRegistry::new(&plugin_dir);
        assert_eq!(registry.count(), 1);
        let ability = registry.get("test.ability").unwrap();
        assert_eq!(ability.source, AbilitySource::Plugin);
    }
}
