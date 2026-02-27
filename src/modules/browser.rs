// Browser automation module — CDP-based headless browser control.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::core::error::{NyayaError, Result};

// ---------------------------------------------------------------------------
// Cookie & CookieJar — in-memory authenticated session state
// ---------------------------------------------------------------------------

/// A browser cookie for authenticated web sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    /// Unix timestamp (seconds). 0 = session cookie.
    pub expires: u64,
}

/// In-memory cookie jar keyed by domain.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CookieJar {
    /// domain → list of cookies for that domain.
    cookies: HashMap<String, Vec<Cookie>>,
}

impl CookieJar {
    pub fn new() -> Self {
        Self {
            cookies: HashMap::new(),
        }
    }

    /// Add a cookie, validating that its domain is well-formed.
    /// Returns Err if the domain looks malicious (empty, contains whitespace, etc.)
    pub fn set_cookie(&mut self, cookie: Cookie) -> std::result::Result<(), String> {
        validate_cookie_domain(&cookie.domain)?;
        self.cookies
            .entry(cookie.domain.clone())
            .or_default()
            .push(cookie);
        Ok(())
    }

    /// Get all cookies matching the given domain (exact or parent-domain match).
    pub fn get_cookies(&self, domain: &str) -> Vec<&Cookie> {
        let mut result = Vec::new();
        for (d, cookies) in &self.cookies {
            // A cookie with domain ".example.com" matches "sub.example.com"
            if d == domain || domain.ends_with(&format!(".{}", d.trim_start_matches('.'))) {
                result.extend(cookies.iter());
            }
        }
        result
    }

    /// Remove all cookies for a domain.
    pub fn clear_domain(&mut self, domain: &str) {
        self.cookies.remove(domain);
    }

    /// Remove all cookies.
    pub fn clear_all(&mut self) {
        self.cookies.clear();
    }
}

/// Validate a cookie domain to prevent abuse.
pub fn validate_cookie_domain(domain: &str) -> std::result::Result<(), String> {
    if domain.is_empty() {
        return Err("Cookie domain cannot be empty".into());
    }
    if domain.contains(char::is_whitespace) {
        return Err("Cookie domain cannot contain whitespace".into());
    }
    if domain.contains('\0') {
        return Err("Cookie domain cannot contain null bytes".into());
    }
    // Reject overly broad domains (just a TLD like ".com")
    let trimmed = domain.trim_start_matches('.');
    if !trimmed.contains('.') && trimmed.len() <= 6 {
        return Err(format!("Cookie domain too broad: {}", domain));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Selector & value sanitization
// ---------------------------------------------------------------------------

/// Sanitize a CSS selector to prevent script injection.
/// Rejects selectors containing dangerous patterns.
pub fn sanitize_selector(sel: &str) -> std::result::Result<String, String> {
    if sel.is_empty() {
        return Err("Selector cannot be empty".into());
    }
    if sel.len() > 500 {
        return Err("Selector too long (max 500 characters)".into());
    }

    let lower = sel.to_lowercase();

    // Reject backticks (template literals → code execution)
    if sel.contains('`') {
        return Err("Selector contains backtick — possible template literal injection".into());
    }
    // Reject javascript: pseudo-protocol
    if lower.contains("javascript:") {
        return Err("Selector contains 'javascript:' — injection blocked".into());
    }
    // Reject <script> tags
    if lower.contains("<script") || lower.contains("</script") {
        return Err("Selector contains <script> — injection blocked".into());
    }
    // Reject event handlers (onclick=, onerror=, onload=, etc.)
    if lower.contains("on") {
        // More precise: check for on<eventname>= pattern
        let on_pattern =
            regex::Regex::new(r"(?i)\bon[a-z]+\s*=").map_err(|e| format!("Regex error: {}", e))?;
        if on_pattern.is_match(sel) {
            return Err("Selector contains event handler attribute — injection blocked".into());
        }
    }
    // Reject braces (object literals, code blocks)
    if sel.contains('{') || sel.contains('}') {
        return Err("Selector contains braces — possible code injection".into());
    }
    // Reject semicolons (statement separator)
    if sel.contains(';') {
        return Err("Selector contains semicolon — possible code injection".into());
    }

    Ok(sel.to_string())
}

/// Escape a value for safe insertion into a JS string literal (single-quoted).
/// Escapes backslashes, single quotes, newlines, and other control characters.
pub fn sanitize_form_value(val: &str) -> String {
    let mut escaped = String::with_capacity(val.len() + 8);
    for ch in val.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\'' => escaped.push_str("\\'"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\0' => escaped.push_str("\\0"),
            // Escape </  to prevent closing script tags in HTML context
            '<' => escaped.push_str("\\u003c"),
            '>' => escaped.push_str("\\u003e"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// A Chrome DevTools Protocol command.
#[derive(Debug, Clone, Serialize)]
pub struct CdpCommand {
    pub id: u64,
    pub method: String,
    pub params: serde_json::Value,
}

impl CdpCommand {
    /// Create a Page.navigate command.
    pub fn navigate(url: &str) -> Self {
        CdpCommand {
            id: 1,
            method: "Page.navigate".into(),
            params: serde_json::json!({ "url": url }),
        }
    }

    /// Create a DOM.getDocument command.
    pub fn get_document() -> Self {
        CdpCommand {
            id: 2,
            method: "DOM.getDocument".into(),
            params: serde_json::json!({}),
        }
    }

    /// Create a Runtime.evaluate command.
    pub fn evaluate(expression: &str) -> Self {
        CdpCommand {
            id: 3,
            method: "Runtime.evaluate".into(),
            params: serde_json::json!({ "expression": expression }),
        }
    }

    /// Create a Page.captureScreenshot command.
    pub fn screenshot(format: &str, quality: u32) -> Self {
        CdpCommand {
            id: 4,
            method: "Page.captureScreenshot".into(),
            params: serde_json::json!({ "format": format, "quality": quality }),
        }
    }

    /// Create a Network.setCookies command from a slice of Cookie structs.
    pub fn set_cookies(cookies: &[Cookie]) -> Self {
        let cdp_cookies: Vec<serde_json::Value> = cookies
            .iter()
            .map(|c| {
                let mut obj = serde_json::json!({
                    "name": c.name,
                    "value": c.value,
                    "domain": c.domain,
                    "path": c.path,
                    "secure": c.secure,
                    "httpOnly": c.http_only,
                });
                if c.expires > 0 {
                    obj.as_object_mut().unwrap().insert(
                        "expires".into(),
                        serde_json::Value::Number(c.expires.into()),
                    );
                }
                obj
            })
            .collect();

        CdpCommand {
            id: 5,
            method: "Network.setCookies".into(),
            params: serde_json::json!({ "cookies": cdp_cookies }),
        }
    }

    /// Create a CDP evaluate command that fills a form field.
    /// Validates the selector and escapes the value for safe JS execution.
    pub fn fill_form(selector: &str, value: &str) -> std::result::Result<Self, String> {
        let safe_sel = sanitize_selector(selector)?;
        let safe_val = sanitize_form_value(value);
        let safe_sel_escaped = sanitize_form_value(&safe_sel);

        let js = format!(
            "(() => {{ \
                const el = document.querySelector('{}'); \
                if (!el) throw new Error('Element not found: {}'); \
                el.value = '{}'; \
                el.dispatchEvent(new Event('input', {{ bubbles: true }})); \
                el.dispatchEvent(new Event('change', {{ bubbles: true }})); \
                return 'ok'; \
            }})()",
            safe_sel_escaped, safe_sel_escaped, safe_val,
        );

        Ok(CdpCommand {
            id: 6,
            method: "Runtime.evaluate".into(),
            params: serde_json::json!({ "expression": js }),
        })
    }

    /// Create a CDP evaluate command that clicks an element.
    /// Validates the selector to prevent injection.
    pub fn click(selector: &str) -> std::result::Result<Self, String> {
        let safe_sel = sanitize_selector(selector)?;
        let safe_sel_escaped = sanitize_form_value(&safe_sel);

        let js = format!(
            "(() => {{ \
                const el = document.querySelector('{}'); \
                if (!el) throw new Error('Element not found: {}'); \
                el.click(); \
                return 'ok'; \
            }})()",
            safe_sel_escaped, safe_sel_escaped,
        );

        Ok(CdpCommand {
            id: 7,
            method: "Runtime.evaluate".into(),
            params: serde_json::json!({ "expression": js }),
        })
    }
}

/// Configuration for headless browser sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserConfig {
    /// Path to Chromium binary. If None, auto-detected.
    pub chromium_path: Option<String>,
    /// Timeout in seconds for page operations.
    pub timeout_secs: u64,
    /// Maximum number of pages per session.
    pub max_pages_per_session: u32,
    /// Run in headless mode.
    pub headless: bool,
    /// Allowed domain patterns. Supports "*" (all) and "*.example.com" globs.
    pub allowed_domains: Vec<String>,
    /// Blocked domain patterns (checked before allowed).
    pub blocked_domains: Vec<String>,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            chromium_path: None,
            timeout_secs: 30,
            max_pages_per_session: 20,
            headless: true,
            allowed_domains: vec!["*".into()],
            blocked_domains: Vec::new(),
        }
    }
}

impl BrowserConfig {
    /// Check whether a URL is allowed by the domain constitution.
    /// Blocked domains are checked first; then allowed domains.
    pub fn is_url_allowed(&self, url: &str) -> bool {
        let domain = match extract_domain(url) {
            Some(d) => d,
            None => return false,
        };

        // Check blocked list first.
        for pattern in &self.blocked_domains {
            if domain_matches(&domain, pattern) {
                return false;
            }
        }

        // Check allowed list.
        for pattern in &self.allowed_domains {
            if domain_matches(&domain, pattern) {
                return true;
            }
        }

        false
    }
}

/// Result of a browser operation.
#[derive(Debug, Clone, Serialize)]
pub struct BrowserResult {
    pub success: bool,
    pub url: String,
    pub title: Option<String>,
    pub text_content: Option<String>,
    pub screenshot_base64: Option<String>,
    pub error: Option<String>,
}

/// Extract the domain (host) from a URL string.
pub fn extract_domain(url: &str) -> Option<String> {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
}

/// Check whether a domain matches a glob pattern.
/// Supports "*" (matches everything) and "*.suffix" (matches any subdomain).
pub fn domain_matches(domain: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        // Match the suffix itself or any subdomain of it.
        domain == suffix || domain.ends_with(&format!(".{}", suffix))
    } else {
        domain == pattern
    }
}

/// Launch a Chromium process with CDP remote debugging enabled.
pub fn launch_chrome(config: &BrowserConfig) -> Result<std::process::Child> {
    let chromium = config
        .chromium_path
        .clone()
        .or_else(|| {
            super::hardware::detect_tool(&[
                "chromium",
                "chromium-browser",
                "google-chrome",
                "google-chrome-stable",
            ])
            .map(|p| p.to_string_lossy().into_owned())
        })
        .ok_or_else(|| NyayaError::Config("No Chromium binary found".into()))?;

    let mut cmd = std::process::Command::new(&chromium);
    cmd.env_clear();
    cmd.arg("--remote-debugging-port=9222");
    cmd.arg("--no-first-run");
    cmd.arg("--no-default-browser-check");
    cmd.arg("--disable-gpu");

    if config.headless {
        cmd.arg("--headless");
    }

    let child = cmd.spawn().map_err(|e| {
        NyayaError::Config(format!(
            "Failed to launch Chromium at '{}': {}",
            chromium, e
        ))
    })?;

    Ok(child)
}

// ---------------------------------------------------------------------------
// CdpTransport — WebSocket transport for Chrome DevTools Protocol
// ---------------------------------------------------------------------------

/// WebSocket transport for Chrome DevTools Protocol
#[derive(Debug)]
pub struct CdpTransport {
    ws_url: String,
    next_id: AtomicU64,
}

/// A CDP target (page/tab) discovered via the /json endpoint
#[derive(Debug, Clone, Deserialize)]
pub struct CdpTarget {
    pub id: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    pub ws_url: Option<String>,
    pub title: String,
    pub url: String,
    #[serde(rename = "type")]
    pub target_type: String,
}

impl CdpTransport {
    pub fn new(ws_url: &str) -> Self {
        Self {
            ws_url: ws_url.to_string(),
            next_id: AtomicU64::new(1),
        }
    }

    /// Send a CDP command and wait for the matching response.
    pub async fn send_command(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, Box<dyn std::error::Error>> {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::connect_async;
        use tokio_tungstenite::tungstenite::Message;

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let msg = serde_json::json!({
            "id": id,
            "method": method,
            "params": params,
        });

        let (mut ws, _) = connect_async(&self.ws_url).await?;
        ws.send(Message::Text(msg.to_string())).await?;

        while let Some(Ok(response)) = ws.next().await {
            if let Message::Text(text) = response {
                let parsed: serde_json::Value = serde_json::from_str(&text)?;
                if parsed.get("id").and_then(|v| v.as_u64()) == Some(id) {
                    if let Some(error) = parsed.get("error") {
                        return Err(format!("CDP error: {}", error).into());
                    }
                    return Ok(parsed
                        .get("result")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null));
                }
            }
        }
        Err("WebSocket closed without response".into())
    }

    /// Discover available CDP targets from Chrome's /json endpoint
    pub async fn discover_targets(
        host: &str,
        port: u16,
    ) -> std::result::Result<Vec<CdpTarget>, Box<dyn std::error::Error>> {
        let url = format!("http://{}:{}/json", host, port);
        let resp = reqwest::get(&url).await?;
        let targets: Vec<CdpTarget> = resp.json().await?;
        Ok(targets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cdp_command_serialization() {
        let cmd = CdpCommand::navigate("https://example.com");
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("Page.navigate"));
        assert!(json.contains("https://example.com"));
    }

    #[test]
    fn test_browser_config_default() {
        let config = BrowserConfig::default();
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.max_pages_per_session, 20);
        assert!(config.headless);
    }

    #[test]
    fn test_url_allowed_by_constitution() {
        let config = BrowserConfig {
            allowed_domains: vec!["*.example.com".into(), "github.com".into()],
            blocked_domains: vec!["evil.com".into()],
            ..Default::default()
        };
        assert!(config.is_url_allowed("https://sub.example.com/page"));
        assert!(config.is_url_allowed("https://github.com/repo"));
        assert!(!config.is_url_allowed("https://evil.com/hack"));
        assert!(!config.is_url_allowed("https://random.com/page"));
    }

    #[test]
    fn test_url_allowed_wildcard_all() {
        let config = BrowserConfig {
            allowed_domains: vec!["*".into()],
            blocked_domains: vec!["evil.com".into()],
            ..Default::default()
        };
        assert!(config.is_url_allowed("https://anything.com"));
        assert!(!config.is_url_allowed("https://evil.com/hack"));
    }

    #[test]
    fn test_extract_domain_from_url() {
        assert_eq!(
            extract_domain("https://sub.example.com/path"),
            Some("sub.example.com".into())
        );
        assert_eq!(extract_domain("not-a-url"), None);
    }

    // --- Selector sanitization tests ---

    #[test]
    fn test_sanitize_selector_valid() {
        assert!(sanitize_selector("#login-form input[name='email']").is_ok());
        assert!(sanitize_selector(".btn-primary").is_ok());
        assert!(sanitize_selector("div > span.class").is_ok());
        assert!(sanitize_selector("input[type=\"submit\"]").is_ok());
    }

    #[test]
    fn test_sanitize_selector_rejects_empty() {
        assert!(sanitize_selector("").is_err());
    }

    #[test]
    fn test_sanitize_selector_rejects_backtick() {
        assert!(sanitize_selector("`injection`").is_err());
    }

    #[test]
    fn test_sanitize_selector_rejects_javascript() {
        assert!(sanitize_selector("javascript:alert(1)").is_err());
        assert!(sanitize_selector("JAVASCRIPT:void(0)").is_err());
    }

    #[test]
    fn test_sanitize_selector_rejects_script_tag() {
        assert!(sanitize_selector("<script>alert(1)</script>").is_err());
        assert!(sanitize_selector("div<script>").is_err());
    }

    #[test]
    fn test_sanitize_selector_rejects_event_handlers() {
        assert!(sanitize_selector("div onclick=alert(1)").is_err());
        assert!(sanitize_selector("img onerror=fetch('x')").is_err());
    }

    #[test]
    fn test_sanitize_selector_rejects_braces() {
        assert!(sanitize_selector("div{background:red}").is_err());
    }

    #[test]
    fn test_sanitize_selector_rejects_semicolon() {
        assert!(sanitize_selector("div; alert(1)").is_err());
    }

    #[test]
    fn test_sanitize_selector_rejects_too_long() {
        let long = "a".repeat(501);
        assert!(sanitize_selector(&long).is_err());
    }

    // --- Form value sanitization tests ---

    #[test]
    fn test_sanitize_form_value_escapes_quotes() {
        assert_eq!(sanitize_form_value("it's"), "it\\'s");
        assert_eq!(sanitize_form_value("say \"hi\""), "say \\\"hi\\\"");
    }

    #[test]
    fn test_sanitize_form_value_escapes_backslash() {
        assert_eq!(sanitize_form_value("a\\b"), "a\\\\b");
    }

    #[test]
    fn test_sanitize_form_value_escapes_newlines() {
        assert_eq!(sanitize_form_value("line1\nline2"), "line1\\nline2");
        assert_eq!(sanitize_form_value("cr\r"), "cr\\r");
    }

    #[test]
    fn test_sanitize_form_value_escapes_angle_brackets() {
        let escaped = sanitize_form_value("<script>alert(1)</script>");
        assert!(!escaped.contains('<'));
        assert!(!escaped.contains('>'));
    }

    // --- Cookie domain validation tests ---

    #[test]
    fn test_validate_cookie_domain_valid() {
        assert!(validate_cookie_domain("example.com").is_ok());
        assert!(validate_cookie_domain(".example.com").is_ok());
        assert!(validate_cookie_domain("sub.example.com").is_ok());
    }

    #[test]
    fn test_validate_cookie_domain_rejects_empty() {
        assert!(validate_cookie_domain("").is_err());
    }

    #[test]
    fn test_validate_cookie_domain_rejects_whitespace() {
        assert!(validate_cookie_domain("evil .com").is_err());
    }

    #[test]
    fn test_validate_cookie_domain_rejects_null() {
        assert!(validate_cookie_domain("evil\0.com").is_err());
    }

    #[test]
    fn test_validate_cookie_domain_rejects_too_broad() {
        assert!(validate_cookie_domain("com").is_err());
        assert!(validate_cookie_domain(".com").is_err());
    }

    // --- CookieJar tests ---

    #[test]
    fn test_cookie_jar_set_and_get() {
        let mut jar = CookieJar::new();
        jar.set_cookie(Cookie {
            name: "session".into(),
            value: "abc123".into(),
            domain: "example.com".into(),
            path: "/".into(),
            secure: true,
            http_only: true,
            expires: 0,
        })
        .unwrap();

        let cookies = jar.get_cookies("example.com");
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].name, "session");

        // Subdomain should also match
        let cookies = jar.get_cookies("sub.example.com");
        assert_eq!(cookies.len(), 1);

        // Unrelated domain should not match
        let cookies = jar.get_cookies("other.com");
        assert_eq!(cookies.len(), 0);
    }

    #[test]
    fn test_cookie_jar_clear() {
        let mut jar = CookieJar::new();
        jar.set_cookie(Cookie {
            name: "a".into(),
            value: "1".into(),
            domain: "example.com".into(),
            path: "/".into(),
            secure: false,
            http_only: false,
            expires: 0,
        })
        .unwrap();
        jar.clear_domain("example.com");
        assert!(jar.get_cookies("example.com").is_empty());
    }

    // --- CdpCommand new methods tests ---

    #[test]
    fn test_cdp_set_cookies() {
        let cookies = vec![Cookie {
            name: "sid".into(),
            value: "xyz".into(),
            domain: "example.com".into(),
            path: "/".into(),
            secure: true,
            http_only: true,
            expires: 1700000000,
        }];
        let cmd = CdpCommand::set_cookies(&cookies);
        assert_eq!(cmd.method, "Network.setCookies");
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"name\":\"sid\""));
        assert!(json.contains("\"expires\":1700000000"));
    }

    #[test]
    fn test_cdp_fill_form() {
        let cmd = CdpCommand::fill_form("#email", "user@example.com").unwrap();
        assert_eq!(cmd.method, "Runtime.evaluate");
        let expr = cmd.params["expression"].as_str().unwrap();
        assert!(expr.contains("user@example.com"));
        assert!(expr.contains("querySelector"));
        assert!(expr.contains("dispatchEvent"));
    }

    #[test]
    fn test_cdp_fill_form_rejects_bad_selector() {
        assert!(CdpCommand::fill_form("<script>", "val").is_err());
        assert!(CdpCommand::fill_form("div`", "val").is_err());
    }

    #[test]
    fn test_cdp_click() {
        let cmd = CdpCommand::click("#submit-btn").unwrap();
        assert_eq!(cmd.method, "Runtime.evaluate");
        let expr = cmd.params["expression"].as_str().unwrap();
        assert!(expr.contains("click()"));
        assert!(expr.contains("querySelector"));
    }

    #[test]
    fn test_cdp_click_rejects_bad_selector() {
        assert!(CdpCommand::click("").is_err());
        assert!(CdpCommand::click("javascript:alert(1)").is_err());
    }

    // --- CdpTransport tests ---

    #[test]
    fn test_cdp_transport_command_serialization() {
        let transport = CdpTransport::new("ws://localhost:9222/devtools/page/1");
        let id = transport.next_id.load(Ordering::Relaxed);
        let msg = serde_json::json!({
            "id": id,
            "method": "Page.navigate",
            "params": {"url": "https://example.com"}
        });
        assert_eq!(msg["method"], "Page.navigate");
        assert_eq!(msg["params"]["url"], "https://example.com");
    }

    #[test]
    fn test_cdp_target_parse() {
        let json = r#"[{
            "id": "ABC123",
            "webSocketDebuggerUrl": "ws://localhost:9222/devtools/page/ABC123",
            "title": "Example",
            "url": "https://example.com",
            "type": "page"
        }]"#;
        let targets: Vec<CdpTarget> = serde_json::from_str(json).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].id, "ABC123");
        assert_eq!(
            targets[0].ws_url.as_deref(),
            Some("ws://localhost:9222/devtools/page/ABC123")
        );
        assert_eq!(targets[0].target_type, "page");
    }

    #[test]
    fn test_command_id_increments() {
        let transport = CdpTransport::new("ws://localhost:9222");
        let id1 = transport.next_id.fetch_add(1, Ordering::Relaxed);
        let id2 = transport.next_id.fetch_add(1, Ordering::Relaxed);
        assert_eq!(id2, id1 + 1);
    }
}
