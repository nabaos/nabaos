use std::path::{Path, PathBuf};

use crate::core::error::{NyayaError, Result};

/// Application configuration
#[derive(Debug, Clone)]
pub struct NyayaConfig {
    pub data_dir: PathBuf,
    pub model_path: PathBuf,
    pub constitution_path: Option<PathBuf>,
    pub llm_api_key: Option<String>,
    pub llm_provider: Option<String>,
    pub daily_budget_usd: Option<f64>,
    pub per_task_budget_usd: Option<f64>,
    /// Directory for installed plugins.
    pub plugin_dir: PathBuf,
    /// Path to subprocess abilities config file.
    pub subprocess_config: Option<PathBuf>,
    /// Named constitution template to use (alternative to constitution_path).
    /// Options: default, solopreneur, freelancer, marketer, student, sales, support, legal, ecommerce
    pub constitution_template: Option<String>,
    /// Module profile (which optional features are enabled).
    pub profile: crate::modules::profile::ModuleProfile,
}

impl NyayaConfig {
    /// Load configuration from environment and defaults
    pub fn load() -> Result<Self> {
        let data_dir = std::env::var("NABA_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| dirs_or_default().join(".nabaos"));

        let model_path = std::env::var("NABA_MODEL_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                // Look for models relative to the binary or in the project
                PathBuf::from("models/setfit-w5h2")
            });

        let constitution_path = std::env::var("NABA_CONSTITUTION_PATH")
            .map(PathBuf::from)
            .ok();

        let llm_api_key = std::env::var("NABA_LLM_API_KEY").ok();
        let llm_provider = std::env::var("NABA_LLM_PROVIDER").ok();

        let daily_budget_usd = std::env::var("NABA_DAILY_BUDGET_USD")
            .ok()
            .and_then(|v| v.parse().ok());
        let per_task_budget_usd = std::env::var("NABA_PER_TASK_BUDGET_USD")
            .ok()
            .and_then(|v| v.parse().ok());

        let plugin_dir = std::env::var("NABA_PLUGIN_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_dir.join("plugins"));

        let subprocess_config = std::env::var("NABA_SUBPROCESS_CONFIG")
            .map(PathBuf::from)
            .ok();

        let constitution_template = std::env::var("NABA_CONSTITUTION_TEMPLATE").ok();

        let profile =
            crate::modules::profile::ModuleProfile::load_or_default(&data_dir.join("profile.toml"));

        Ok(Self {
            data_dir,
            model_path,
            constitution_path,
            llm_api_key,
            llm_provider,
            daily_budget_usd,
            per_task_budget_usd,
            plugin_dir,
            subprocess_config,
            constitution_template,
            profile,
        })
    }

    /// Ensure required directories exist
    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        Ok(())
    }

    /// Get the database directory
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("nyaya.db")
    }
}

fn dirs_or_default() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

/// Resolve the model path, checking multiple locations
pub fn resolve_model_path(configured: &Path) -> Result<PathBuf> {
    // Check configured path first
    if configured.join("model.onnx").exists() {
        return Ok(configured.to_path_buf());
    }

    // Check relative to current dir
    let cwd = std::env::current_dir()?;
    let relative = cwd.join(configured);
    if relative.join("model.onnx").exists() {
        return Ok(relative);
    }

    // Check relative to binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let exe_relative = exe_dir.join(configured);
            if exe_relative.join("model.onnx").exists() {
                return Ok(exe_relative);
            }
        }
    }

    Err(NyayaError::ModelLoad(format!(
        "Model directory not found at '{}'. Run the ONNX export script first.",
        configured.display()
    )))
}
