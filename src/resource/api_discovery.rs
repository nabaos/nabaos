use crate::core::error::Result;
use crate::resource::api_service::{
    api_service_from_config, ApiServiceCategory, ApiServiceConfig, ApiServiceResource,
};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ApiGuruEntry {
    pub id: String,
    pub title: String,
    pub description: String,
    pub preferred_version: String,
    pub swagger_url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiscoveredAuth {
    ApiKey { header: String },
    Bearer,
    Basic,
}

#[derive(Debug, Clone)]
pub struct DiscoveredApiConfig {
    pub title: String,
    pub description: String,
    pub base_url: String,
    pub auth_type: Option<DiscoveredAuth>,
    pub endpoint_count: usize,
}

// ---------------------------------------------------------------------------
// Directory parsing
// ---------------------------------------------------------------------------

/// Parse the APIs.guru directory JSON and filter entries by query substring.
pub fn parse_api_directory(directory_json: &serde_json::Value, query: &str) -> Vec<ApiGuruEntry> {
    let query_lower = query.to_lowercase();
    let obj = match directory_json.as_object() {
        Some(o) => o,
        None => return Vec::new(),
    };

    let mut results: Vec<ApiGuruEntry> = Vec::new();

    for (provider_id, provider_value) in obj {
        if !provider_id.to_lowercase().contains(&query_lower) {
            continue;
        }

        let preferred = provider_value
            .get("preferred")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let versions = match provider_value.get("versions").and_then(|v| v.as_object()) {
            Some(v) => v,
            None => continue,
        };

        let version_info = match versions.get(&preferred) {
            Some(v) => v,
            None => continue,
        };

        let title = version_info
            .get("info")
            .and_then(|i| i.get("title"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        let description = version_info
            .get("info")
            .and_then(|i| i.get("description"))
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string();

        let swagger_url = match version_info
            .get("swaggerUrl")
            .and_then(|s| s.as_str())
            .filter(|s| !s.is_empty())
        {
            Some(url) => url.to_string(),
            None => continue,
        };

        results.push(ApiGuruEntry {
            id: provider_id.clone(),
            title,
            description,
            preferred_version: preferred,
            swagger_url,
        });
    }

    // Sort: exact matches first, then by ID length (shorter = more relevant)
    results.sort_by(|a, b| {
        let a_exact = a.id.to_lowercase() == query_lower;
        let b_exact = b.id.to_lowercase() == query_lower;
        match (a_exact, b_exact) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.id.len().cmp(&b.id.len()),
        }
    });

    results
}

// ---------------------------------------------------------------------------
// OpenAPI spec parsing
// ---------------------------------------------------------------------------

/// Parse an OpenAPI (v2 or v3) spec into a DiscoveredApiConfig.
pub fn parse_openapi_spec(spec: &serde_json::Value) -> Result<DiscoveredApiConfig> {
    let title = spec
        .get("info")
        .and_then(|i| i.get("title"))
        .and_then(|t| t.as_str())
        .unwrap_or("Untitled API")
        .to_string();

    let description = spec
        .get("info")
        .and_then(|i| i.get("description"))
        .and_then(|d| d.as_str())
        .unwrap_or("")
        .to_string();

    let base_url = extract_base_url(spec);
    let auth_type = extract_auth(spec);

    let endpoint_count = spec
        .get("paths")
        .and_then(|p| p.as_object())
        .map(|o| o.len())
        .unwrap_or(0);

    Ok(DiscoveredApiConfig {
        title,
        description,
        base_url,
        auth_type,
        endpoint_count,
    })
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn extract_base_url(spec: &serde_json::Value) -> String {
    // OpenAPI 3.x: servers[0].url
    if let Some(url) = spec
        .get("servers")
        .and_then(|s| s.as_array())
        .and_then(|a| a.first())
        .and_then(|s| s.get("url"))
        .and_then(|u| u.as_str())
    {
        return url.to_string();
    }

    // Swagger 2.0: schemes[0]://host + basePath
    if let Some(host) = spec.get("host").and_then(|h| h.as_str()) {
        let scheme = spec
            .get("schemes")
            .and_then(|s| s.as_array())
            .and_then(|a| a.first())
            .and_then(|s| s.as_str())
            .unwrap_or("https");

        let base_path = spec.get("basePath").and_then(|b| b.as_str()).unwrap_or("");

        return format!("{scheme}://{host}{base_path}");
    }

    String::new()
}

fn extract_auth(spec: &serde_json::Value) -> Option<DiscoveredAuth> {
    // OpenAPI 3.x: components.securitySchemes
    let schemes = spec
        .get("components")
        .and_then(|c| c.get("securitySchemes"))
        .and_then(|s| s.as_object())
        // Swagger 2.0: securityDefinitions
        .or_else(|| spec.get("securityDefinitions").and_then(|s| s.as_object()));

    let schemes = schemes?;

    for (_name, scheme) in schemes {
        let scheme_type = scheme.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match scheme_type {
            "apiKey" => {
                let location = scheme.get("in").and_then(|i| i.as_str()).unwrap_or("");
                if location == "header" {
                    let header_name = scheme
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("X-API-Key")
                        .to_string();
                    return Some(DiscoveredAuth::ApiKey {
                        header: header_name,
                    });
                }
            }
            "http" => {
                let scheme_val = scheme.get("scheme").and_then(|s| s.as_str()).unwrap_or("");
                match scheme_val {
                    "bearer" => return Some(DiscoveredAuth::Bearer),
                    "basic" => return Some(DiscoveredAuth::Basic),
                    _ => {}
                }
            }
            "oauth2" => {
                return Some(DiscoveredAuth::Bearer);
            }
            _ => {}
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Category guessing
// ---------------------------------------------------------------------------

/// Guess the ApiServiceCategory from the API title and description.
pub fn guess_category(title: &str, description: &str) -> ApiServiceCategory {
    let combined = format!("{} {}", title, description).to_lowercase();

    if combined.contains("weather") || combined.contains("forecast") || combined.contains("climate")
    {
        ApiServiceCategory::Weather
    } else if combined.contains("calendar")
        || combined.contains("event")
        || combined.contains("schedule")
    {
        ApiServiceCategory::Calendar
    } else if combined.contains("captcha")
        || combined.contains("verify human")
        || combined.contains("bot detection")
        || combined.contains("bot protect")
    {
        ApiServiceCategory::Captcha
    } else if combined.contains("storage")
        || combined.contains("s3")
        || combined.contains("bucket")
        || combined.contains("file")
        || combined.contains("object")
    {
        ApiServiceCategory::Storage
    } else if combined.contains("embed")
        || combined.contains("vector")
        || combined.contains("similarity")
    {
        ApiServiceCategory::Embedding
    } else if combined.contains("translat") || combined.contains("language") {
        ApiServiceCategory::Translation
    } else if combined.contains("notify")
        || combined.contains("alert")
        || combined.contains("push notif")
        || combined.contains("sms")
        || combined.contains("message")
    {
        ApiServiceCategory::Notification
    } else if combined.contains("monitor")
        || combined.contains("uptime")
        || combined.contains("health check")
        || combined.contains("metric")
        || combined.contains("observ")
    {
        ApiServiceCategory::Monitoring
    } else if combined.contains("academic")
        || combined.contains("paper")
        || combined.contains("scholar")
        || combined.contains("research")
        || combined.contains("citation")
    {
        ApiServiceCategory::Academic
    } else if combined.contains("search")
        || combined.contains("query")
        || combined.contains("find")
        || combined.contains("lookup")
        || combined.contains("discover")
    {
        ApiServiceCategory::Search
    } else {
        ApiServiceCategory::Search
    }
}

// ---------------------------------------------------------------------------
// Resource builder
// ---------------------------------------------------------------------------

/// Build an ApiServiceResource from a DiscoveredApiConfig.
pub fn build_resource_from_discovery(
    id: &str,
    discovered: &DiscoveredApiConfig,
    credential_id: Option<String>,
    category_override: Option<ApiServiceCategory>,
) -> ApiServiceResource {
    let category = category_override
        .unwrap_or_else(|| guess_category(&discovered.title, &discovered.description));

    let auth_header = discovered.auth_type.as_ref().map(|auth| match auth {
        DiscoveredAuth::ApiKey { header } => header.clone(),
        DiscoveredAuth::Bearer | DiscoveredAuth::Basic => "Authorization".to_string(),
    });

    let config = ApiServiceConfig {
        category,
        provider: id.to_string(),
        api_endpoint: discovered.base_url.clone(),
        credential_id,
        auth_header,
        rate_limit_rpm: None,
        cost_per_call: None,
        timeout_secs: None,
        security_tier_override: None,
        default_quota: None,
    };

    api_service_from_config(id, &discovered.title, config)
}

// ---------------------------------------------------------------------------
// Async network functions
// ---------------------------------------------------------------------------

const APIS_GURU_LIST_URL: &str = "https://api.apis.guru/v2/list.json";

/// Search APIs.guru directory for APIs matching the query.
pub async fn search_api_directory(query: &str) -> crate::core::error::Result<Vec<ApiGuruEntry>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| crate::core::error::NyayaError::Config(format!("HTTP client error: {}", e)))?;

    let resp = client.get(APIS_GURU_LIST_URL).send().await.map_err(|e| {
        crate::core::error::NyayaError::Config(format!(
            "Failed to fetch APIs.guru directory: {}. Try manual config via YAML.",
            e
        ))
    })?;

    let json: serde_json::Value = resp.json().await.map_err(|e| {
        crate::core::error::NyayaError::Config(format!("Failed to parse APIs.guru response: {}", e))
    })?;

    let results = parse_api_directory(&json, query);
    if results.is_empty() {
        return Err(crate::core::error::NyayaError::Config(format!(
            "No APIs found matching '{}'. Try a different name or manual config via YAML.",
            query
        )));
    }

    Ok(results)
}

/// Fetch and parse an OpenAPI spec from a URL.
pub async fn discover_from_spec(
    swagger_url: &str,
) -> crate::core::error::Result<DiscoveredApiConfig> {
    if !swagger_url.starts_with("https://") {
        return Err(crate::core::error::NyayaError::Config(format!(
            "Refusing non-HTTPS spec URL: {}. Only HTTPS is supported.",
            swagger_url
        )));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| crate::core::error::NyayaError::Config(format!("HTTP client error: {}", e)))?;

    let resp = client.get(swagger_url).send().await.map_err(|e| {
        crate::core::error::NyayaError::Config(format!(
            "Failed to fetch OpenAPI spec from {}: {}",
            swagger_url, e
        ))
    })?;

    let spec: serde_json::Value = resp.json().await.map_err(|e| {
        crate::core::error::NyayaError::Config(format!(
            "Failed to parse OpenAPI spec: {}. The spec may be malformed. Try manual config.",
            e
        ))
    })?;

    parse_openapi_spec(&spec)
}

/// One-shot: search APIs.guru, fetch spec for top match, build ApiServiceResource.
pub async fn auto_configure(
    service_name: &str,
    credential_id: Option<&str>,
) -> crate::core::error::Result<ApiServiceResource> {
    let entries = search_api_directory(service_name).await?;
    let entry = &entries[0];

    let discovered = discover_from_spec(&entry.swagger_url).await?;

    Ok(build_resource_from_discovery(
        &entry.id,
        &discovered,
        credential_id.map(|s| s.to_string()),
        None,
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_apis_guru_entry() {
        let directory = json!({
            "stripe.com": {
                "preferred": "v1",
                "versions": {
                    "v1": {
                        "info": {
                            "title": "Stripe API",
                            "description": "Payment processing"
                        },
                        "swaggerUrl": "https://api.apis.guru/v2/specs/stripe.com/v1/swagger.json"
                    }
                }
            },
            "other-api.com": {
                "preferred": "v2",
                "versions": {
                    "v2": {
                        "info": {
                            "title": "Other API",
                            "description": "Something else"
                        },
                        "swaggerUrl": "https://example.com/spec.json"
                    }
                }
            }
        });

        let results = parse_api_directory(&directory, "stripe");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "stripe.com");
        assert_eq!(results[0].title, "Stripe API");
        assert_eq!(results[0].preferred_version, "v1");
        assert!(!results[0].swagger_url.is_empty());
    }

    #[test]
    fn test_extract_openapi_v2_config() {
        let spec = json!({
            "swagger": "2.0",
            "info": {
                "title": "Weather API",
                "description": "Current weather and forecasts"
            },
            "host": "api.weather.example.com",
            "basePath": "/v1",
            "schemes": ["https"],
            "paths": {
                "/current": {},
                "/forecast": {},
                "/alerts": {}
            },
            "securityDefinitions": {
                "api_key": {
                    "type": "apiKey",
                    "in": "header",
                    "name": "X-Weather-Key"
                }
            }
        });

        let config = parse_openapi_spec(&spec).unwrap();
        assert_eq!(config.title, "Weather API");
        assert_eq!(config.base_url, "https://api.weather.example.com/v1");
        assert_eq!(config.endpoint_count, 3);
        assert_eq!(
            config.auth_type,
            Some(DiscoveredAuth::ApiKey {
                header: "X-Weather-Key".to_string()
            })
        );
    }

    #[test]
    fn test_extract_openapi_v3_config() {
        let spec = json!({
            "openapi": "3.0.0",
            "info": {
                "title": "Stripe API",
                "description": "Payment processing API"
            },
            "servers": [
                { "url": "https://api.stripe.com/v1" }
            ],
            "paths": {
                "/charges": {},
                "/customers": {},
                "/invoices": {},
                "/subscriptions": {}
            },
            "components": {
                "securitySchemes": {
                    "bearerAuth": {
                        "type": "http",
                        "scheme": "bearer"
                    }
                }
            }
        });

        let config = parse_openapi_spec(&spec).unwrap();
        assert_eq!(config.title, "Stripe API");
        assert_eq!(config.base_url, "https://api.stripe.com/v1");
        assert_eq!(config.endpoint_count, 4);
        assert_eq!(config.auth_type, Some(DiscoveredAuth::Bearer));
    }

    #[test]
    fn test_auth_mapping_bearer() {
        let spec = json!({
            "openapi": "3.0.0",
            "info": { "title": "Test" },
            "components": {
                "securitySchemes": {
                    "bearer": {
                        "type": "http",
                        "scheme": "bearer"
                    }
                }
            }
        });

        let auth = extract_auth(&spec);
        assert_eq!(auth, Some(DiscoveredAuth::Bearer));
    }

    #[test]
    fn test_auth_mapping_apikey() {
        let spec = json!({
            "swagger": "2.0",
            "info": { "title": "Test" },
            "securityDefinitions": {
                "custom_key": {
                    "type": "apiKey",
                    "in": "header",
                    "name": "X-Custom-Auth"
                }
            }
        });

        let auth = extract_auth(&spec);
        assert_eq!(
            auth,
            Some(DiscoveredAuth::ApiKey {
                header: "X-Custom-Auth".to_string()
            })
        );
    }

    #[test]
    fn test_category_heuristic() {
        assert_eq!(
            guess_category("Weather API", "Get forecasts"),
            ApiServiceCategory::Weather
        );
        assert_eq!(
            guess_category("CalDAV", "Calendar event management"),
            ApiServiceCategory::Calendar
        );
        assert_eq!(
            guess_category("hCaptcha", "Bot detection service"),
            ApiServiceCategory::Captcha
        );
        assert_eq!(
            guess_category("S3", "Object storage bucket API"),
            ApiServiceCategory::Storage
        );
        assert_eq!(
            guess_category("Pinecone", "Vector embedding similarity search"),
            ApiServiceCategory::Embedding
        );
        assert_eq!(
            guess_category("Ntfy", "Push notification alerts"),
            ApiServiceCategory::Notification
        );
        assert_eq!(
            guess_category("UptimeRobot", "Uptime monitoring service"),
            ApiServiceCategory::Monitoring
        );
    }

    #[test]
    fn test_build_resource_from_discovery() {
        let discovered = DiscoveredApiConfig {
            title: "Weather API".to_string(),
            description: "Current weather and forecasts".to_string(),
            base_url: "https://api.weather.example.com/v1".to_string(),
            auth_type: Some(DiscoveredAuth::ApiKey {
                header: "X-Weather-Key".to_string(),
            }),
            endpoint_count: 3,
        };

        // Without category override — should guess Weather
        let resource = build_resource_from_discovery(
            "weather-api",
            &discovered,
            Some("weather-cred".to_string()),
            None,
        );
        assert_eq!(resource.id, "weather-api");
        assert_eq!(resource.name, "Weather API");
        assert_eq!(resource.config.category, ApiServiceCategory::Weather);
        assert_eq!(
            resource.config.api_endpoint,
            "https://api.weather.example.com/v1"
        );
        assert_eq!(
            resource.config.credential_id,
            Some("weather-cred".to_string())
        );
        assert_eq!(
            resource.config.auth_header,
            Some("X-Weather-Key".to_string())
        );

        // With category override
        let resource_override = build_resource_from_discovery(
            "weather-api",
            &discovered,
            None,
            Some(ApiServiceCategory::Monitoring),
        );
        assert_eq!(
            resource_override.config.category,
            ApiServiceCategory::Monitoring
        );
    }
}
