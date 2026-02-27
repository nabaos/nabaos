use std::collections::HashMap;

use crate::core::error::{NyayaError, Result};
use crate::llm_router::provider::{LlmProvider, ProviderType};

use super::catalog::builtin_providers;

/// API format used by a provider.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiFormat {
    Anthropic,
    OpenAI,
    Google,
    Bedrock,
    Azure,
}

/// A model definition with pricing metadata.
#[derive(Debug, Clone)]
pub struct ModelDef {
    pub id: String,
    pub display_name: String,
    pub context_window: u32,
    pub input_price_per_mtok: f64,
    pub output_price_per_mtok: f64,
}

/// A provider definition including its API format, base URL, and available models.
#[derive(Debug, Clone)]
pub struct ProviderDef {
    pub id: String,
    pub display_name: String,
    pub api_format: ApiFormat,
    pub base_url: String,
    pub models: Vec<ModelDef>,
    pub default_model: String,
    pub supports_tools: bool,
    pub supports_vision: bool,
}

/// Registry of LLM providers and their API keys.
#[derive(Default)]
pub struct ProviderRegistry {
    providers: HashMap<String, ProviderDef>,
    api_keys: HashMap<String, String>,
}

impl ProviderRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            api_keys: HashMap::new(),
        }
    }

    /// Create a registry pre-loaded with all built-in providers.
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        for def in builtin_providers() {
            registry.add(def);
        }
        registry
    }

    /// Add a provider definition to the registry.
    pub fn add(&mut self, def: ProviderDef) {
        self.providers.insert(def.id.clone(), def);
    }

    /// Get a provider definition by ID.
    pub fn get(&self, id: &str) -> Option<&ProviderDef> {
        self.providers.get(id)
    }

    /// Set the API key for a provider.
    pub fn set_api_key(&mut self, provider_id: &str, key: String) {
        self.api_keys.insert(provider_id.to_string(), key);
    }

    /// Get the API key for a provider.
    pub fn get_api_key(&self, provider_id: &str) -> Option<&str> {
        self.api_keys.get(provider_id).map(|s| s.as_str())
    }

    /// List provider IDs that have API keys configured.
    pub fn list_configured(&self) -> Vec<String> {
        self.api_keys.keys().cloned().collect()
    }

    /// List all provider IDs in the registry.
    pub fn list_all(&self) -> Vec<&str> {
        self.providers.keys().map(|s| s.as_str()).collect()
    }

    /// Build an `LlmProvider` from registry data.
    ///
    /// For Anthropic format the base_url gets `/v1/messages` appended.
    /// For OpenAI-compatible formats the base_url gets `/chat/completions` appended.
    /// Timeout is set to 30 seconds.
    pub fn build_provider(
        &self,
        provider_id: &str,
        model_override: Option<&str>,
    ) -> Result<LlmProvider> {
        let def = self
            .providers
            .get(provider_id)
            .ok_or_else(|| NyayaError::Config(format!("Unknown provider: {}", provider_id)))?;

        let api_key = self
            .api_keys
            .get(provider_id)
            .ok_or_else(|| {
                NyayaError::Config(format!(
                    "No API key configured for provider: {}",
                    provider_id
                ))
            })?
            .clone();

        let model = model_override
            .map(|s| s.to_string())
            .unwrap_or_else(|| def.default_model.clone());

        let (provider_type, base_url) = match def.api_format {
            ApiFormat::Anthropic => {
                let url = format!("{}/v1/messages", def.base_url.trim_end_matches('/'));
                (ProviderType::Anthropic, url)
            }
            ApiFormat::OpenAI | ApiFormat::Google | ApiFormat::Azure => {
                let url = format!("{}/chat/completions", def.base_url.trim_end_matches('/'));
                (ProviderType::OpenAI, url)
            }
            ApiFormat::Bedrock => {
                let url = format!("{}/chat/completions", def.base_url.trim_end_matches('/'));
                (ProviderType::OpenAI, url)
            }
        };

        Ok(LlmProvider {
            provider: provider_type,
            api_key,
            model,
            base_url,
            timeout_secs: Some(30),
        })
    }

    /// Try to build a provider from `preferred`, falling back through `fallback` list.
    pub fn build_with_fallback(
        &self,
        preferred: &str,
        fallback: &[&str],
        model_override: Option<&str>,
    ) -> Result<LlmProvider> {
        if let Ok(provider) = self.build_provider(preferred, model_override) {
            return Ok(provider);
        }

        for &fb in fallback {
            if let Ok(provider) = self.build_provider(fb, model_override) {
                return Ok(provider);
            }
        }

        Err(NyayaError::Config(format!(
            "No configured provider found. Tried: {} and fallbacks {:?}",
            preferred, fallback
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_registry_with_builtin_providers() {
        let registry = ProviderRegistry::with_builtins();
        let all = registry.list_all();
        assert!(
            all.len() >= 50,
            "Expected at least 50 providers, got {}",
            all.len()
        );
        assert!(registry.get("anthropic").is_some());
        assert!(registry.get("openai").is_some());
        assert!(registry.get("deepseek").is_some());
        assert!(registry.get("openrouter").is_some());
    }

    #[test]
    fn test_add_custom_provider() {
        let mut registry = ProviderRegistry::new();
        registry.add(ProviderDef {
            id: "custom".to_string(),
            display_name: "Custom Provider".to_string(),
            api_format: ApiFormat::OpenAI,
            base_url: "https://custom.example.com".to_string(),
            models: vec![],
            default_model: "custom-model".to_string(),
            supports_tools: false,
            supports_vision: false,
        });
        assert!(registry.get("custom").is_some());
        assert_eq!(
            registry.get("custom").unwrap().display_name,
            "Custom Provider"
        );
    }

    #[test]
    fn test_list_configured_providers() {
        let mut registry = ProviderRegistry::with_builtins();
        assert_eq!(registry.list_configured().len(), 0);

        registry.set_api_key("anthropic", "sk-test-key".to_string());
        assert_eq!(registry.list_configured().len(), 1);
        assert!(registry
            .list_configured()
            .contains(&"anthropic".to_string()));
    }

    #[test]
    fn test_build_llm_provider() {
        let mut registry = ProviderRegistry::with_builtins();
        registry.set_api_key("anthropic", "sk-ant-test123".to_string());

        let provider = registry.build_provider("anthropic", None).unwrap();
        assert!(matches!(provider.provider, ProviderType::Anthropic));
        assert_eq!(provider.model, "claude-sonnet-4-6");
        assert!(provider.base_url.ends_with("/v1/messages"));
        assert_eq!(provider.timeout_secs, Some(30));
    }

    #[test]
    fn test_build_provider_with_model_override() {
        let mut registry = ProviderRegistry::with_builtins();
        registry.set_api_key("anthropic", "sk-ant-test123".to_string());

        let provider = registry
            .build_provider("anthropic", Some("claude-opus-4-6"))
            .unwrap();
        assert_eq!(provider.model, "claude-opus-4-6");
    }

    #[test]
    fn test_build_provider_unconfigured_fails() {
        let registry = ProviderRegistry::with_builtins();
        let result = registry.build_provider("anthropic", None);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("No API key"));
    }
}
