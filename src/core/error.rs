use thiserror::Error;

#[derive(Error, Debug)]
pub enum NyayaError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Model loading error: {0}")]
    ModelLoad(String),

    #[error("Inference error: {0}")]
    Inference(String),

    #[error("Cache error: {0}")]
    Cache(String),

    #[error("Vault error: {0}")]
    Vault(String),

    #[error("Constitution violation: {0}")]
    ConstitutionViolation(String),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("WASM runtime error: {0}")]
    Wasm(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Export error: {0}")]
    Export(String),
}

impl NyayaError {
    /// Returns a user-friendly hint with actionable fix commands.
    pub fn user_hint(&self) -> Option<String> {
        match self {
            NyayaError::Config(msg) if msg.contains("API_KEY") || msg.contains("api_key") => {
                Some(
                    "Set your API key with:\n  export NABA_LLM_API_KEY=your-key-here\n\n\
                     Or run the setup wizard:\n  nabaos setup --interactive\n\n\
                     Docs: https://nabaos.github.io/nabaos/getting-started/configuration.html"
                        .to_string(),
                )
            }
            NyayaError::Config(msg) if msg.contains("TELEGRAM") || msg.contains("telegram") => {
                Some(format!(
                    "Telegram configuration issue: {}\n\n\
                     Set up Telegram:\n  export NABA_TELEGRAM_BOT_TOKEN=your-token\n\n\
                     Full guide: https://nabaos.github.io/nabaos/guides/telegram-setup.html",
                    msg
                ))
            }
            NyayaError::Config(_) => None,
            NyayaError::ModelLoad(msg) => {
                Some(format!(
                    "The ML model could not be loaded: {}\n\n\
                     Download models with:\n  nabaos setup --download-models\n\n\
                     Or set a custom model path:\n  export NABA_MODEL_PATH=/path/to/models\n\n\
                     Docs: https://nabaos.github.io/nabaos/troubleshooting/common-errors.html#model-not-found",
                    msg
                ))
            }
            NyayaError::ConstitutionViolation(msg) => {
                Some(format!(
                    "The active constitution blocked this action: {}\n\n\
                     Check your constitution rules:\n  nabaos constitution show\n\n\
                     Modify rules:\n  nabaos constitution use-template <template>\n\n\
                     Docs: https://nabaos.github.io/nabaos/concepts/constitutions.html",
                    msg
                ))
            }
            NyayaError::Vault(msg) => {
                Some(format!(
                    "Vault error: {}\n\n\
                     Set vault passphrase:\n  export NABA_VAULT_PASSPHRASE=your-passphrase\n\n\
                     Docs: https://nabaos.github.io/nabaos/guides/secrets-management.html",
                    msg
                ))
            }
            NyayaError::PermissionDenied(msg) => {
                Some(format!(
                    "Permission denied: {}\n\n\
                     Check agent permissions:\n  nabaos agent permissions <agent-name>\n\n\
                     Docs: https://nabaos.github.io/nabaos/concepts/agent-packages.html",
                    msg
                ))
            }
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, NyayaError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_error_display() {
        let err = NyayaError::Export("test".into());
        assert_eq!(format!("{}", err), "Export error: test");
    }
}
