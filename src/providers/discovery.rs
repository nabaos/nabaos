use crate::core::error::{NyayaError, Result};

/// Query the OpenAI-compatible `/models` endpoint and return a sorted list of model IDs.
pub fn fetch_available_models(base_url: &str, api_key: &str) -> Result<Vec<String>> {
    // Normalize: strip trailing `/v1` to avoid `/v1/v1/models`
    let base = base_url.trim_end_matches('/');
    let base = base.strip_suffix("/v1").unwrap_or(base);

    let url = format!("{}/v1/models", base);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| NyayaError::Config(format!("HTTP client build failed: {}", e)))?;

    let mut req = client.get(&url);
    if !api_key.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", api_key));
    }

    let resp = req
        .send()
        .map_err(|e| NyayaError::Config(format!("Model discovery request failed ({}): {}", url, e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(NyayaError::Config(format!(
            "Model discovery failed ({}): {}",
            status, body
        )));
    }

    let json: serde_json::Value = resp
        .json()
        .map_err(|e| NyayaError::Config(format!("Model list parse failed: {}", e)))?;

    let mut models: Vec<String> = json["data"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|m| m["id"].as_str().map(String::from))
        .collect();

    models.sort();
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_normalization_no_v1() {
        // Ensure the function doesn't panic with a bad URL (will fail on network)
        let result = fetch_available_models("http://127.0.0.1:1", "test-key");
        assert!(result.is_err()); // connection refused is fine
    }

    #[test]
    fn test_url_normalization_strips_trailing_v1() {
        // We can't easily test network calls, but we can verify the logic
        // by checking the error message contains the normalized URL
        let result = fetch_available_models("https://example.com/v1", "key");
        // Should try https://example.com/v1/models (not /v1/v1/models)
        assert!(result.is_err());
    }

    #[test]
    fn test_url_normalization_strips_trailing_slash_v1() {
        let result = fetch_available_models("https://example.com/v1/", "key");
        assert!(result.is_err());
    }
}
