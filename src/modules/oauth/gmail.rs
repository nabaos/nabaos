// Gmail OAuth connector — list and read emails via the Gmail REST API.

use super::{ConnectorConfig, OAuthToken};
use base64::Engine as _;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Required OAuth scopes for Gmail access.
pub const SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/gmail.readonly",
    // gmail.send is included to support the pre-existing `email.send` ability
    // (exec_email_send in host_functions.rs). Only read operations are
    // implemented in this module; send will be added when email.send is wired up.
    "https://www.googleapis.com/auth/gmail.send",
];

/// Maximum Gmail API calls per minute.
const RATE_LIMIT_MAX: u64 = 20;

/// Maximum email body size in bytes (100 KB).
const BODY_MAX_BYTES: usize = 100_000;

/// Google OAuth2 token endpoint.
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

// ---------------------------------------------------------------------------
// Rate limiter — sliding-window counter protected by a Mutex
//
// The previous implementation used two separate AtomicU64s (window_start and
// count), which had a TOCTOU race: another thread could reset the window
// between the load of window_start and the fetch_add on count, leading to
// incorrect window accounting. A single Mutex<(window_start, count)> makes
// the read-check-update of both fields atomic under the lock.
// ---------------------------------------------------------------------------

/// Rate-limit state: (window_start_secs, call_count_in_window).
static RATE_LIMIT_STATE: OnceLock<Mutex<(u64, u64)>> = OnceLock::new();

fn rate_limit_state() -> &'static Mutex<(u64, u64)> {
    RATE_LIMIT_STATE.get_or_init(|| Mutex::new((0, 0)))
}

/// Check and increment the rate limiter. Returns Err if the limit is exceeded.
fn check_rate_limit() -> Result<(), String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut state = rate_limit_state()
        .lock()
        .map_err(|_| "Rate limiter mutex poisoned".to_string())?;

    let (window_start, count) = &mut *state;

    // If more than 60 seconds have elapsed, reset the window.
    if now.saturating_sub(*window_start) >= 60 {
        *window_start = now;
        *count = 1;
        return Ok(());
    }

    if *count >= RATE_LIMIT_MAX {
        Err(format!(
            "Gmail API rate limit exceeded: {} calls in current 60s window (max {})",
            count, RATE_LIMIT_MAX
        ))
    } else {
        *count += 1;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Summary returned by list_messages (id + threadId only from the Gmail API).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GmailMessageSummary {
    pub id: String,
    pub thread_id: String,
}

/// Full message returned by get_message.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GmailMessage {
    pub id: String,
    pub thread_id: String,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub date: String,
    pub snippet: String,
    pub body: String,
    pub label_ids: Vec<String>,
}

/// Result from list_messages including the total estimated count.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GmailListResult {
    pub messages: Vec<GmailMessageSummary>,
    pub result_size_estimate: u32,
}

// ---------------------------------------------------------------------------
// Gmail connector
// ---------------------------------------------------------------------------

/// Gmail connector for reading and sending email via the Gmail API.
pub struct GmailConnector {
    config: ConnectorConfig,
}

impl GmailConnector {
    /// Create a new Gmail connector from its configuration.
    pub fn new(config: ConnectorConfig) -> Self {
        Self { config }
    }

    /// Returns true if client credentials are present and the connector is enabled.
    pub fn is_configured(&self) -> bool {
        self.config.enabled
            && self.config.client_id.is_some()
            && self.config.client_secret.is_some()
    }

    /// Build the Google OAuth2 authorization URL for Gmail scopes.
    pub fn auth_url(&self) -> Option<String> {
        let client_id = self.config.client_id.as_ref()?;
        let scopes = SCOPES.join(" ");
        Some(format!(
            "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&scope={}&response_type=code&access_type=offline",
            client_id, scopes
        ))
    }
}

// ---------------------------------------------------------------------------
// Gmail API functions (blocking, for use from sync exec_ dispatch)
// ---------------------------------------------------------------------------

/// List messages matching an optional query.
///
/// GET https://gmail.googleapis.com/gmail/v1/users/me/messages
///
/// Returns message id/threadId pairs. Use `get_message()` to fetch full content.
pub fn list_messages(
    token: &str,
    query: Option<&str>,
    max_results: Option<u32>,
    label: Option<&str>,
) -> Result<GmailListResult, String> {
    check_rate_limit()?;

    let max = max_results.unwrap_or(10).min(100);
    let mut url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages?maxResults={}",
        max
    );
    if let Some(q) = query {
        url.push_str(&format!("&q={}", urlencoding_encode(q)));
    }
    if let Some(l) = label {
        url.push_str(&format!("&labelIds={}", urlencoding_encode(l)));
    }

    let resp = gmail_get(token, &url)?;
    parse_list_response(&resp)
}

/// Fetch a single message by ID with full content.
///
/// GET https://gmail.googleapis.com/gmail/v1/users/me/messages/{id}?format=full
pub fn get_message(token: &str, message_id: &str) -> Result<GmailMessage, String> {
    check_rate_limit()?;

    // Validate message_id — must be alphanumeric/hex
    if message_id.is_empty()
        || message_id.len() > 64
        || !message_id.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Err(format!("Invalid Gmail message ID: '{}'", message_id));
    }

    let url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=full",
        message_id
    );

    let resp = gmail_get(token, &url)?;
    parse_message_response(&resp)
}

/// Refresh an expired OAuth token using the Google token endpoint.
///
/// SECURITY: client_secret and refresh_token are never logged.
pub fn refresh_token_blocking(
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<OAuthToken, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id),
        ("client_secret", client_secret),
    ];

    let resp = client
        .post(GOOGLE_TOKEN_URL)
        .form(&params)
        .send()
        .map_err(|e| format!("Token refresh request failed: {}", e))?;

    let status = resp.status().as_u16();
    let body = resp
        .text()
        .map_err(|e| format!("Token refresh read error: {}", e))?;

    if status >= 400 {
        // Log status only — body may contain secrets
        return Err(format!("Token refresh failed with HTTP {}", status));
    }

    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("Token refresh JSON parse error: {}", e))?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Ok(OAuthToken {
        access_token: json["access_token"].as_str().unwrap_or("").to_string(),
        refresh_token: json["refresh_token"].as_str().map(|s| s.to_string()),
        expires_at: Some(now + json["expires_in"].as_u64().unwrap_or(3600)),
        scopes: json["scope"]
            .as_str()
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default(),
    })
}

/// Maximum reply body size in bytes (50 KB).
const REPLY_BODY_MAX_BYTES: usize = 50_000;

/// Send a reply within an existing Gmail thread.
///
/// POST https://gmail.googleapis.com/gmail/v1/users/me/messages/send
///
/// Builds an RFC 2822 message with In-Reply-To and References headers so Gmail
/// threads the reply correctly.  The raw message is base64url-encoded and sent
/// with the `threadId` field.
///
/// Returns the sent message ID on success.
pub fn send_reply(
    token: &str,
    thread_id: &str,
    message_id: &str,
    to: &str,
    subject: Option<&str>,
    body: &str,
) -> Result<String, String> {
    check_rate_limit()?;

    // Validate thread_id: alphanumeric, max 64 chars
    if thread_id.is_empty()
        || thread_id.len() > 64
        || !thread_id.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Err(format!("Invalid Gmail thread ID: '{}'", thread_id));
    }

    // Validate message_id: alphanumeric, max 64 chars
    if message_id.is_empty()
        || message_id.len() > 64
        || !message_id.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Err(format!("Invalid Gmail message ID: '{}'", message_id));
    }

    // Validate recipient email
    if to.contains('\n') || to.contains('\r') || to.contains('\0') {
        return Err("send_reply: recipient address contains illegal characters".into());
    }
    let parts: Vec<&str> = to.split('@').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() || !parts[1].contains('.') {
        return Err(format!("send_reply: invalid email address '{}'", to));
    }

    // Body cap: 50 KB
    if body.len() > REPLY_BODY_MAX_BYTES {
        return Err(format!(
            "send_reply: body too large ({} bytes, max {})",
            body.len(),
            REPLY_BODY_MAX_BYTES
        ));
    }

    let subject_line = subject.unwrap_or("Re:");

    // Build RFC 2822 message with In-Reply-To and References headers.
    // The message_id from Gmail is a hex string; wrap it in angle brackets for
    // the RFC 2822 Message-ID reference format.
    let rfc_message_id = format!("<{}>", message_id);

    let raw_message = format!(
        "To: {}\r\nSubject: {}\r\nIn-Reply-To: {}\r\nReferences: {}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}",
        to, subject_line, rfc_message_id, rfc_message_id, body
    );

    // Base64url encode (no padding)
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw_message.as_bytes());

    let payload = serde_json::json!({
        "raw": encoded,
        "threadId": thread_id,
    });

    // POST to Gmail send endpoint
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let resp = client
        .post("https://gmail.googleapis.com/gmail/v1/users/me/messages/send")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .map_err(|e| format!("Gmail send_reply request failed: {}", e))?;

    let status = resp.status().as_u16();
    let resp_body = resp
        .text()
        .map_err(|e| format!("Gmail send_reply read error: {}", e))?;

    match status {
        200 => {
            let json: serde_json::Value = serde_json::from_str(&resp_body)
                .map_err(|e| format!("Gmail send_reply parse error: {}", e))?;
            let sent_id = json["id"].as_str().unwrap_or("").to_string();
            Ok(sent_id)
        }
        401 => Err("Gmail API: unauthorized (token expired or revoked)".into()),
        403 => Err("Gmail API: forbidden (insufficient scopes or quota exceeded)".into()),
        429 => Err("Gmail API: rate limited by Google".into()),
        _ => Err(format!("Gmail send_reply error: HTTP {}", status)),
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Make a GET request to the Gmail API with Bearer auth.
fn gmail_get(token: &str, url: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .map_err(|e| format!("Gmail API request failed: {}", e))?;

    let status = resp.status().as_u16();
    let body = resp
        .text()
        .map_err(|e| format!("Gmail API read error: {}", e))?;

    match status {
        200 => Ok(body),
        401 => Err("Gmail API: unauthorized (token expired or revoked)".into()),
        403 => Err("Gmail API: forbidden (insufficient scopes or quota exceeded)".into()),
        404 => Err("Gmail API: message not found".into()),
        429 => Err("Gmail API: rate limited by Google".into()),
        _ => Err(format!("Gmail API error: HTTP {}", status)),
    }
}

/// Parse the list_messages API response.
pub fn parse_list_response(body: &str) -> Result<GmailListResult, String> {
    let json: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("Gmail list parse error: {}", e))?;

    let result_size = json["resultSizeEstimate"].as_u64().unwrap_or(0) as u32;

    let messages = match json.get("messages") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|m| {
                let id = m["id"].as_str()?.to_string();
                let thread_id = m["threadId"].as_str()?.to_string();
                Some(GmailMessageSummary { id, thread_id })
            })
            .collect(),
        _ => Vec::new(),
    };

    Ok(GmailListResult {
        messages,
        result_size_estimate: result_size,
    })
}

/// Parse a full message response from the Gmail API.
pub fn parse_message_response(body: &str) -> Result<GmailMessage, String> {
    let json: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("Gmail message parse error: {}", e))?;

    let id = json["id"].as_str().unwrap_or("").to_string();
    let thread_id = json["threadId"].as_str().unwrap_or("").to_string();
    let snippet = json["snippet"].as_str().unwrap_or("").to_string();
    let label_ids: Vec<String> = json["labelIds"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Extract headers
    let headers = extract_headers(&json);
    let from = headers.get("From").cloned().unwrap_or_default();
    let to = headers.get("To").cloned().unwrap_or_default();
    let subject = headers.get("Subject").cloned().unwrap_or_default();
    let date = headers.get("Date").cloned().unwrap_or_default();

    // Extract body — prefer text/plain, fallback to text/html with tag stripping
    let email_body = extract_body(&json);

    // Truncate body to BODY_MAX_BYTES
    let body_truncated = safe_truncate_utf8(&email_body, BODY_MAX_BYTES);

    Ok(GmailMessage {
        id,
        thread_id,
        from,
        to,
        subject,
        date,
        snippet,
        body: body_truncated,
        label_ids,
    })
}

/// Extract specific headers from a Gmail message payload.
fn extract_headers(json: &serde_json::Value) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let wanted = ["From", "To", "Subject", "Date"];

    if let Some(headers) = json["payload"]["headers"].as_array() {
        for h in headers {
            if let (Some(name), Some(value)) = (h["name"].as_str(), h["value"].as_str()) {
                if wanted.contains(&name) {
                    map.insert(name.to_string(), value.to_string());
                }
            }
        }
    }
    map
}

/// Extract the email body from a Gmail message payload.
///
/// Walks the MIME parts tree, preferring text/plain over text/html.
/// Base64-decodes the body data. Strips HTML tags if only HTML is available.
fn extract_body(json: &serde_json::Value) -> String {
    let payload = &json["payload"];

    // Try top-level body first (simple messages)
    if let Some(body_data) = payload["body"]["data"].as_str() {
        let mime = payload["mimeType"].as_str().unwrap_or("");
        if let Some(decoded) = base64url_decode(body_data) {
            if mime.contains("text/html") {
                return strip_html_tags(&decoded);
            }
            return decoded;
        }
    }

    // Walk parts for multipart messages
    let mut plain_body: Option<String> = None;
    let mut html_body: Option<String> = None;

    collect_body_parts(payload, &mut plain_body, &mut html_body);

    if let Some(plain) = plain_body {
        return plain;
    }
    if let Some(html) = html_body {
        return strip_html_tags(&html);
    }

    String::new()
}

/// Recursively collect text/plain and text/html bodies from MIME parts.
fn collect_body_parts(
    part: &serde_json::Value,
    plain: &mut Option<String>,
    html: &mut Option<String>,
) {
    if let Some(parts) = part["parts"].as_array() {
        for p in parts {
            collect_body_parts(p, plain, html);
        }
    }

    let mime = part["mimeType"].as_str().unwrap_or("");
    if let Some(data) = part["body"]["data"].as_str() {
        if let Some(decoded) = base64url_decode(data) {
            if mime == "text/plain" && plain.is_none() {
                *plain = Some(decoded);
            } else if mime == "text/html" && html.is_none() {
                *html = Some(decoded);
            }
        }
    }
}

/// Decode base64url-encoded data (Gmail uses URL-safe base64 without padding).
///
/// Uses the `base64` crate's `URL_SAFE_NO_PAD` engine — the project already
/// depends on this crate, so there is no need for a hand-rolled decoder.
fn base64url_decode(data: &str) -> Option<String> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(data)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

/// Strip HTML tags from a string, producing plain text.
/// Also decodes common HTML entities.
pub fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut chars = html.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '<' {
            in_tag = true;
            // Insert newline for block-level tags
            let rest: String = chars.clone().take(5).collect();
            let lower = rest.to_lowercase();
            if lower.starts_with("br")
                || lower.starts_with("/p")
                || lower.starts_with("/div")
                || lower.starts_with("/tr")
                || lower.starts_with("/li")
            {
                result.push('\n');
            }
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(c);
        }
    }

    // Decode common HTML entities
    let result = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ");

    // Collapse multiple blank lines
    let mut collapsed = String::with_capacity(result.len());
    let mut prev_blank = false;
    for line in result.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_blank {
                collapsed.push('\n');
                prev_blank = true;
            }
        } else {
            if prev_blank {
                collapsed.push('\n');
            }
            collapsed.push_str(trimmed);
            collapsed.push('\n');
            prev_blank = false;
        }
    }

    collapsed.trim().to_string()
}

/// UTF-8 safe truncation at a byte boundary.
fn safe_truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        let truncate_at = s
            .char_indices()
            .take_while(|&(i, _)| i <= max_bytes)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        s[..truncate_at].to_string()
    }
}

/// Minimal URL encoding for query parameters.
fn urlencoding_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", b));
            }
        }
    }
    result
}

/// Poll for messages received after the given Unix timestamp.
pub fn poll_new_messages(
    token: &str,
    since_epoch: u64,
) -> std::result::Result<Vec<GmailMessage>, String> {
    let query = format!("after:{}", since_epoch);
    let list = list_messages(token, Some(&query), Some(20), None)
        .map_err(|e| format!("Gmail poll error: {e}"))?;
    let mut messages = Vec::new();
    for summary in &list.messages {
        match get_message(token, &summary.id) {
            Ok(msg) => messages.push(msg),
            Err(e) => tracing::warn!(id = %summary.id, "Failed to fetch message: {e}"),
        }
    }
    Ok(messages)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Rate limiter tests ---

    /// Helper to reset the rate-limit state to a specific (window_start, count).
    fn set_rate_limit_state(window_start: u64, count: u64) {
        let mut state = rate_limit_state().lock().unwrap();
        *state = (window_start, count);
    }

    #[test]
    fn test_rate_limit_allows_within_limit() {
        // Reset window to zero so the first call re-initialises the window.
        set_rate_limit_state(0, 0);

        // First call should succeed (resets window)
        assert!(check_rate_limit().is_ok());
    }

    #[test]
    fn test_rate_limit_rejects_over_limit() {
        // Set window to current time with count already at the maximum.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        set_rate_limit_state(now, RATE_LIMIT_MAX);

        let result = check_rate_limit();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("rate limit"));
    }

    #[test]
    fn test_rate_limit_resets_after_window() {
        // Set window to 120 seconds ago (expired) with count at max.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        set_rate_limit_state(now.saturating_sub(120), RATE_LIMIT_MAX);

        // Should succeed because the window has expired
        assert!(check_rate_limit().is_ok());
    }

    // --- parse_list_response tests ---

    #[test]
    fn test_parse_list_empty() {
        let json = r#"{"resultSizeEstimate": 0}"#;
        let result = parse_list_response(json).unwrap();
        assert!(result.messages.is_empty());
        assert_eq!(result.result_size_estimate, 0);
    }

    #[test]
    fn test_parse_list_with_messages() {
        let json = r#"{
            "messages": [
                {"id": "abc123", "threadId": "thread1"},
                {"id": "def456", "threadId": "thread2"}
            ],
            "resultSizeEstimate": 42
        }"#;
        let result = parse_list_response(json).unwrap();
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.messages[0].id, "abc123");
        assert_eq!(result.messages[0].thread_id, "thread1");
        assert_eq!(result.messages[1].id, "def456");
        assert_eq!(result.result_size_estimate, 42);
    }

    #[test]
    fn test_parse_list_invalid_json() {
        let result = parse_list_response("not json");
        assert!(result.is_err());
    }

    // --- parse_message_response tests ---

    #[test]
    fn test_parse_message_simple() {
        // Base64url of "Hello, world!" is "SGVsbG8sIHdvcmxkIQ"
        let json = r#"{
            "id": "msg123",
            "threadId": "thread1",
            "snippet": "Hello, world!",
            "labelIds": ["INBOX", "UNREAD"],
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    {"name": "From", "value": "alice@example.com"},
                    {"name": "To", "value": "bob@example.com"},
                    {"name": "Subject", "value": "Test Subject"},
                    {"name": "Date", "value": "Mon, 1 Jan 2024 12:00:00 +0000"}
                ],
                "body": {
                    "data": "SGVsbG8sIHdvcmxkIQ"
                }
            }
        }"#;
        let msg = parse_message_response(json).unwrap();
        assert_eq!(msg.id, "msg123");
        assert_eq!(msg.thread_id, "thread1");
        assert_eq!(msg.from, "alice@example.com");
        assert_eq!(msg.to, "bob@example.com");
        assert_eq!(msg.subject, "Test Subject");
        assert_eq!(msg.date, "Mon, 1 Jan 2024 12:00:00 +0000");
        assert_eq!(msg.body, "Hello, world!");
        assert_eq!(msg.label_ids, vec!["INBOX", "UNREAD"]);
    }

    #[test]
    fn test_parse_message_multipart() {
        // Base64url of "Plain text body" is "UGxhaW4gdGV4dCBib2R5"
        // Base64url of "<p>HTML body</p>" is "PHA-SFRNTCB ib2R5PC9wPg" (approx)
        let json = r#"{
            "id": "msg456",
            "threadId": "thread2",
            "snippet": "Plain text",
            "labelIds": ["INBOX"],
            "payload": {
                "mimeType": "multipart/alternative",
                "headers": [
                    {"name": "From", "value": "sender@test.com"},
                    {"name": "To", "value": "recv@test.com"},
                    {"name": "Subject", "value": "Multi"},
                    {"name": "Date", "value": "Tue, 2 Jan 2024 08:00:00 +0000"}
                ],
                "parts": [
                    {
                        "mimeType": "text/plain",
                        "body": {"data": "UGxhaW4gdGV4dCBib2R5"}
                    },
                    {
                        "mimeType": "text/html",
                        "body": {"data": "PHA-SFRNTCB ib2R5PC9wPg"}
                    }
                ]
            }
        }"#;
        let msg = parse_message_response(json).unwrap();
        assert_eq!(msg.id, "msg456");
        // Should prefer text/plain
        assert_eq!(msg.body, "Plain text body");
    }

    #[test]
    fn test_parse_message_html_only() {
        // Base64url of "<p>Hello <b>World</b></p>" is "PHA+SGVsbG8gPGI+V29ybGQ8L2I+PC9wPg"
        let json = r#"{
            "id": "msg789",
            "threadId": "thread3",
            "snippet": "Hello World",
            "labelIds": [],
            "payload": {
                "mimeType": "text/html",
                "headers": [
                    {"name": "From", "value": "x@y.com"},
                    {"name": "To", "value": "a@b.com"},
                    {"name": "Subject", "value": "HTML only"},
                    {"name": "Date", "value": "Wed, 3 Jan 2024 00:00:00 +0000"}
                ],
                "body": {
                    "data": "PHA-SGVsbG8gPGI-V29ybGQ8L2I-PC9wPg"
                }
            }
        }"#;
        let msg = parse_message_response(json).unwrap();
        assert_eq!(msg.id, "msg789");
        // HTML tags should be stripped
        assert!(!msg.body.contains("<p>"));
        assert!(!msg.body.contains("<b>"));
    }

    // --- HTML tag stripping tests ---

    #[test]
    fn test_strip_html_simple() {
        assert_eq!(strip_html_tags("<p>Hello</p>"), "Hello");
    }

    #[test]
    fn test_strip_html_nested() {
        let html = "<div><p>First <b>bold</b> paragraph</p><p>Second</p></div>";
        let text = strip_html_tags(html);
        assert!(text.contains("First bold paragraph"));
        assert!(text.contains("Second"));
    }

    #[test]
    fn test_strip_html_entities() {
        let html = "&amp; &lt;test&gt; &quot;quoted&quot;";
        let text = strip_html_tags(html);
        assert_eq!(text, "& <test> \"quoted\"");
    }

    #[test]
    fn test_strip_html_br_newlines() {
        let html = "Line 1<br>Line 2<br/>Line 3";
        let text = strip_html_tags(html);
        assert!(text.contains("Line 1"));
        assert!(text.contains("Line 2"));
    }

    #[test]
    fn test_strip_html_empty() {
        assert_eq!(strip_html_tags(""), "");
    }

    // --- base64url decode tests ---

    #[test]
    fn test_base64url_decode_hello() {
        let decoded = base64url_decode("SGVsbG8sIHdvcmxkIQ").unwrap();
        assert_eq!(decoded, "Hello, world!");
    }

    #[test]
    fn test_base64url_decode_empty() {
        let decoded = base64url_decode("").unwrap();
        assert_eq!(decoded, "");
    }

    #[test]
    fn test_base64url_decode_padded() {
        // "Hi" base64 = "SGk=" , base64url = "SGk"
        let decoded = base64url_decode("SGk").unwrap();
        assert_eq!(decoded, "Hi");
    }

    // --- Body truncation tests ---

    #[test]
    fn test_safe_truncate_utf8_ascii() {
        let s = "Hello, world!";
        assert_eq!(safe_truncate_utf8(s, 5), "Hello");
    }

    #[test]
    fn test_safe_truncate_utf8_multibyte() {
        let s = "Hello \u{1F600} world";
        // The emoji is 4 bytes at position 6, so truncating at 7 should not split it.
        let truncated = safe_truncate_utf8(s, 7);
        assert!(truncated.is_char_boundary(truncated.len()));
    }

    #[test]
    fn test_safe_truncate_utf8_no_truncation() {
        let s = "short";
        assert_eq!(safe_truncate_utf8(s, 100), "short");
    }

    // --- URL encoding tests ---

    #[test]
    fn test_urlencoding_spaces() {
        assert_eq!(
            urlencoding_encode("from:alice subject:hello world"),
            "from%3Aalice%20subject%3Ahello%20world"
        );
    }

    #[test]
    fn test_urlencoding_safe_chars() {
        assert_eq!(
            urlencoding_encode("hello-world_1.0~test"),
            "hello-world_1.0~test"
        );
    }

    // --- send_reply validation tests ---

    #[test]
    fn test_send_reply_rejects_empty_thread_id() {
        set_rate_limit_state(0, 0);
        let result = send_reply("tok", "", "msg123", "a@b.com", None, "body");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid Gmail thread ID"));
    }

    #[test]
    fn test_send_reply_rejects_bad_thread_id() {
        set_rate_limit_state(0, 0);
        let result = send_reply("tok", "../bad", "msg123", "a@b.com", None, "body");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid Gmail thread ID"));
    }

    #[test]
    fn test_send_reply_rejects_empty_message_id() {
        set_rate_limit_state(0, 0);
        let result = send_reply("tok", "thread1", "", "a@b.com", None, "body");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid Gmail message ID"));
    }

    #[test]
    fn test_send_reply_rejects_invalid_email() {
        set_rate_limit_state(0, 0);
        let result = send_reply("tok", "thread1", "msg1", "not-an-email", None, "hi");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid email address"));
    }

    #[test]
    fn test_send_reply_rejects_newline_in_email() {
        set_rate_limit_state(0, 0);
        let result = send_reply(
            "tok",
            "thread1",
            "msg1",
            "a@b.com\nBcc: evil@x.com",
            None,
            "hi",
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("illegal characters"));
    }

    #[test]
    fn test_send_reply_rejects_oversized_body() {
        set_rate_limit_state(0, 0);
        let big_body = "x".repeat(REPLY_BODY_MAX_BYTES + 1);
        let result = send_reply("tok", "thread1", "msg1", "a@b.com", None, &big_body);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("body too large"));
    }

    // --- Message ID validation tests ---

    #[test]
    fn test_get_message_rejects_empty_id() {
        let result = get_message("fake_token", "");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid Gmail message ID"));
    }

    #[test]
    fn test_get_message_rejects_special_chars() {
        let result = get_message("fake_token", "../../../etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid Gmail message ID"));
    }

    // --- poll_new_messages query format test ---

    #[test]
    fn test_poll_query_format() {
        let since: u64 = 1740000000;
        let query = format!("after:{}", since);
        assert_eq!(query, "after:1740000000");
    }

    // --- extract_headers tests ---

    #[test]
    fn test_extract_headers() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
            "payload": {
                "headers": [
                    {"name": "From", "value": "test@example.com"},
                    {"name": "Subject", "value": "Test"},
                    {"name": "X-Custom", "value": "ignored"}
                ]
            }
        }"#,
        )
        .unwrap();
        let headers = extract_headers(&json);
        assert_eq!(headers.get("From").unwrap(), "test@example.com");
        assert_eq!(headers.get("Subject").unwrap(), "Test");
        assert!(headers.get("X-Custom").is_none());
    }
}
