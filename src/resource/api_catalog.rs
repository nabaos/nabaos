use super::api_service::{
    api_service_from_config, ApiServiceCategory, ApiServiceConfig, ApiServiceResource,
};

// ---------------------------------------------------------------------------
// Preset factory functions
// ---------------------------------------------------------------------------

/// Brave Search API — web search with privacy focus
pub fn brave_search(credential_id: Option<&str>) -> ApiServiceResource {
    api_service_from_config(
        "brave-search",
        "Brave Search",
        ApiServiceConfig {
            category: ApiServiceCategory::Search,
            provider: "brave".to_string(),
            api_endpoint: "https://api.search.brave.com/res/v1/web/search".to_string(),
            credential_id: credential_id.map(|s| s.to_string()),
            auth_header: Some("X-Subscription-Token".to_string()),
            rate_limit_rpm: Some(60),
            cost_per_call: Some(0.005),
            timeout_secs: Some(30),
            security_tier_override: None,
            default_quota: None,
        },
    )
}

/// SerpApi — Google/Bing/DuckDuckGo search results API
pub fn serpapi(credential_id: Option<&str>) -> ApiServiceResource {
    api_service_from_config(
        "serpapi",
        "SerpApi",
        ApiServiceConfig {
            category: ApiServiceCategory::Search,
            provider: "serpapi".to_string(),
            api_endpoint: "https://serpapi.com/search".to_string(),
            credential_id: credential_id.map(|s| s.to_string()),
            auth_header: None,
            rate_limit_rpm: Some(100),
            cost_per_call: Some(0.01),
            timeout_secs: Some(30),
            security_tier_override: None,
            default_quota: None,
        },
    )
}

/// Semantic Scholar — academic paper search (free, no key needed for basic use)
pub fn semantic_scholar(credential_id: Option<&str>) -> ApiServiceResource {
    api_service_from_config(
        "semantic-scholar",
        "Semantic Scholar",
        ApiServiceConfig {
            category: ApiServiceCategory::Academic,
            provider: "semantic_scholar".to_string(),
            api_endpoint: "https://api.semanticscholar.org/graph/v1".to_string(),
            credential_id: credential_id.map(|s| s.to_string()),
            auth_header: Some("x-api-key".to_string()),
            rate_limit_rpm: Some(100),
            cost_per_call: None,
            timeout_secs: Some(30),
            security_tier_override: None,
            default_quota: None,
        },
    )
}

/// CrossRef — academic metadata and DOI resolution (free)
pub fn crossref(credential_id: Option<&str>) -> ApiServiceResource {
    api_service_from_config(
        "crossref",
        "CrossRef",
        ApiServiceConfig {
            category: ApiServiceCategory::Academic,
            provider: "crossref".to_string(),
            api_endpoint: "https://api.crossref.org/works".to_string(),
            credential_id: credential_id.map(|s| s.to_string()),
            auth_header: None,
            rate_limit_rpm: Some(50),
            cost_per_call: None,
            timeout_secs: Some(30),
            security_tier_override: None,
            default_quota: None,
        },
    )
}

/// 2Captcha — CAPTCHA solving service
pub fn two_captcha(credential_id: Option<&str>) -> ApiServiceResource {
    api_service_from_config(
        "two-captcha",
        "2Captcha",
        ApiServiceConfig {
            category: ApiServiceCategory::Captcha,
            provider: "2captcha".to_string(),
            api_endpoint: "https://api.2captcha.com".to_string(),
            credential_id: credential_id.map(|s| s.to_string()),
            auth_header: None,
            rate_limit_rpm: Some(30),
            cost_per_call: Some(0.003),
            timeout_secs: Some(120),
            security_tier_override: None,
            default_quota: None,
        },
    )
}

/// Anti-Captcha — CAPTCHA solving service
pub fn anti_captcha(credential_id: Option<&str>) -> ApiServiceResource {
    api_service_from_config(
        "anti-captcha",
        "Anti-Captcha",
        ApiServiceConfig {
            category: ApiServiceCategory::Captcha,
            provider: "anti-captcha".to_string(),
            api_endpoint: "https://api.anti-captcha.com".to_string(),
            credential_id: credential_id.map(|s| s.to_string()),
            auth_header: None,
            rate_limit_rpm: Some(30),
            cost_per_call: Some(0.002),
            timeout_secs: Some(120),
            security_tier_override: None,
            default_quota: None,
        },
    )
}

/// CalDAV — generic calendar protocol (user provides endpoint)
pub fn caldav(api_endpoint: &str, credential_id: Option<&str>) -> ApiServiceResource {
    api_service_from_config(
        "caldav",
        "CalDAV Calendar",
        ApiServiceConfig {
            category: ApiServiceCategory::Calendar,
            provider: "caldav".to_string(),
            api_endpoint: api_endpoint.to_string(),
            credential_id: credential_id.map(|s| s.to_string()),
            auth_header: Some("Authorization".to_string()),
            rate_limit_rpm: None,
            cost_per_call: None,
            timeout_secs: Some(30),
            security_tier_override: None,
            default_quota: None,
        },
    )
}

/// S3-Compatible — object storage (AWS S3, MinIO, R2, etc.)
pub fn s3_compatible(api_endpoint: &str, credential_id: Option<&str>) -> ApiServiceResource {
    api_service_from_config(
        "s3-storage",
        "S3-Compatible Storage",
        ApiServiceConfig {
            category: ApiServiceCategory::Storage,
            provider: "s3".to_string(),
            api_endpoint: api_endpoint.to_string(),
            credential_id: credential_id.map(|s| s.to_string()),
            auth_header: Some("Authorization".to_string()),
            rate_limit_rpm: None,
            cost_per_call: Some(0.0004),
            timeout_secs: Some(60),
            security_tier_override: None,
            default_quota: None,
        },
    )
}

/// OpenAI Embeddings — text embedding API
pub fn openai_embeddings(credential_id: Option<&str>) -> ApiServiceResource {
    api_service_from_config(
        "openai-embeddings",
        "OpenAI Embeddings",
        ApiServiceConfig {
            category: ApiServiceCategory::Embedding,
            provider: "openai".to_string(),
            api_endpoint: "https://api.openai.com/v1/embeddings".to_string(),
            credential_id: credential_id.map(|s| s.to_string()),
            auth_header: Some("Authorization".to_string()),
            rate_limit_rpm: Some(3000),
            cost_per_call: Some(0.0001),
            timeout_secs: Some(30),
            security_tier_override: None,
            default_quota: None,
        },
    )
}

/// Ntfy — simple push notification service (free, self-hostable)
pub fn ntfy(credential_id: Option<&str>) -> ApiServiceResource {
    api_service_from_config(
        "ntfy",
        "Ntfy Notifications",
        ApiServiceConfig {
            category: ApiServiceCategory::Notification,
            provider: "ntfy".to_string(),
            api_endpoint: "https://ntfy.sh".to_string(),
            credential_id: credential_id.map(|s| s.to_string()),
            auth_header: Some("Authorization".to_string()),
            rate_limit_rpm: Some(250),
            cost_per_call: None,
            timeout_secs: Some(10),
            security_tier_override: None,
            default_quota: None,
        },
    )
}

/// DeepL — machine translation API
pub fn deepl(credential_id: Option<&str>) -> ApiServiceResource {
    api_service_from_config(
        "deepl",
        "DeepL Translation",
        ApiServiceConfig {
            category: ApiServiceCategory::Translation,
            provider: "deepl".to_string(),
            api_endpoint: "https://api-free.deepl.com/v2/translate".to_string(),
            credential_id: credential_id.map(|s| s.to_string()),
            auth_header: Some("Authorization".to_string()),
            rate_limit_rpm: Some(60),
            cost_per_call: Some(0.00002),
            timeout_secs: Some(30),
            security_tier_override: None,
            default_quota: None,
        },
    )
}

/// OpenWeatherMap — weather data API (free tier available)
pub fn openweathermap(credential_id: Option<&str>) -> ApiServiceResource {
    api_service_from_config(
        "openweathermap",
        "OpenWeatherMap",
        ApiServiceConfig {
            category: ApiServiceCategory::Weather,
            provider: "openweathermap".to_string(),
            api_endpoint: "https://api.openweathermap.org/data/2.5".to_string(),
            credential_id: credential_id.map(|s| s.to_string()),
            auth_header: None,
            rate_limit_rpm: Some(60),
            cost_per_call: None,
            timeout_secs: Some(15),
            security_tier_override: None,
            default_quota: None,
        },
    )
}

// ---------------------------------------------------------------------------
// Discovery helper
// ---------------------------------------------------------------------------

/// Returns metadata for all preset API services: (function_name, category, description)
pub fn all_presets() -> Vec<(&'static str, ApiServiceCategory, &'static str)> {
    vec![
        (
            "brave_search",
            ApiServiceCategory::Search,
            "Web search with privacy focus",
        ),
        (
            "serpapi",
            ApiServiceCategory::Search,
            "Google/Bing/DuckDuckGo search results",
        ),
        (
            "semantic_scholar",
            ApiServiceCategory::Academic,
            "Academic paper search",
        ),
        (
            "crossref",
            ApiServiceCategory::Academic,
            "Academic metadata and DOI resolution",
        ),
        (
            "two_captcha",
            ApiServiceCategory::Captcha,
            "CAPTCHA solving service",
        ),
        (
            "anti_captcha",
            ApiServiceCategory::Captcha,
            "CAPTCHA solving service",
        ),
        (
            "caldav",
            ApiServiceCategory::Calendar,
            "CalDAV calendar protocol",
        ),
        (
            "s3_compatible",
            ApiServiceCategory::Storage,
            "S3-compatible object storage",
        ),
        (
            "openai_embeddings",
            ApiServiceCategory::Embedding,
            "Text embedding API",
        ),
        (
            "ntfy",
            ApiServiceCategory::Notification,
            "Push notification service",
        ),
        (
            "deepl",
            ApiServiceCategory::Translation,
            "Machine translation",
        ),
        (
            "openweathermap",
            ApiServiceCategory::Weather,
            "Weather data API",
        ),
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::Resource;
    use std::collections::HashSet;

    #[test]
    fn test_all_presets_returns_twelve() {
        let presets = all_presets();
        assert_eq!(presets.len(), 12);

        // All names are unique
        let names: HashSet<&str> = presets.iter().map(|(name, _, _)| *name).collect();
        assert_eq!(names.len(), 12);
    }

    #[test]
    fn test_preset_brave_search() {
        let svc = brave_search(Some("my-key"));

        assert_eq!(svc.id(), "brave-search");
        assert_eq!(svc.name(), "Brave Search");
        assert_eq!(svc.config.category, ApiServiceCategory::Search);
        assert_eq!(svc.config.provider, "brave");
        assert!(svc.config.api_endpoint.contains("brave.com"));
        assert_eq!(svc.config.credential_id, Some("my-key".to_string()));
        assert_eq!(
            svc.config.auth_header,
            Some("X-Subscription-Token".to_string())
        );
        assert_eq!(svc.config.rate_limit_rpm, Some(60));
        assert_eq!(svc.config.cost_per_call, Some(0.005));
    }

    #[test]
    fn test_preset_categories_cover_all() {
        let presets = all_presets();
        let categories: HashSet<String> = presets
            .iter()
            .map(|(_, cat, _)| format!("{:?}", cat))
            .collect();

        // 9 categories covered in catalog (Monitoring not preset but exists in enum)
        assert!(categories.contains("Search"));
        assert!(categories.contains("Academic"));
        assert!(categories.contains("Captcha"));
        assert!(categories.contains("Calendar"));
        assert!(categories.contains("Storage"));
        assert!(categories.contains("Embedding"));
        assert!(categories.contains("Notification"));
        assert!(categories.contains("Translation"));
        assert!(categories.contains("Weather"));
    }

    #[test]
    fn test_factory_with_credential() {
        // With credential
        let with = semantic_scholar(Some("key-123"));
        assert_eq!(with.config.credential_id, Some("key-123".to_string()));

        // Without credential
        let without = semantic_scholar(None);
        assert_eq!(without.config.credential_id, None);

        // Both should be valid resources
        assert_eq!(
            with.resource_type(),
            crate::resource::ResourceType::ApiService
        );
        assert_eq!(
            without.resource_type(),
            crate::resource::ResourceType::ApiService
        );
    }
}
