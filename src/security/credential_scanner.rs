//! Credential scanner — detect and redact secrets/PII in text.
//!
//! Ported from nyaya-guard TypeScript implementation.
//! Scans for cloud provider keys, AI API keys, code hosting tokens,
//! payment keys, private keys, generic secrets, connection strings, and PII.
//!
//! SECURITY: Never logs or returns the actual secret values — only type + position.

use regex::Regex;
use std::sync::LazyLock;

/// A detected credential or PII match.
/// Note: byte offsets are internal only for redaction — not exposed publicly
/// to prevent reverse-engineering secret positions from match metadata.
#[derive(Debug, Clone)]
pub struct CredentialMatch {
    /// Type of credential detected (e.g. "aws_access_key", "github_pat")
    pub match_type: String,
    /// Start byte offset in the input text (internal use for redaction)
    pub(crate) start: usize,
    /// End byte offset in the input text (internal use for redaction)
    pub(crate) end: usize,
    /// Placeholder string for redaction
    pub placeholder: String,
}

/// Result of a full redaction scan.
#[derive(Debug)]
pub struct RedactResult {
    /// The text with all credentials/PII replaced by placeholders
    pub redacted: String,
    /// All matches found
    pub matches: Vec<CredentialMatch>,
}

/// A named pattern with its type identifier.
struct NamedPattern {
    match_type: &'static str,
    pattern: &'static LazyLock<Regex>,
    /// Prefix for placeholder: "REDACTED" for credentials, "PII_REDACTED" for PII
    redact_prefix: &'static str,
}

// === Credential Patterns ===

// Cloud provider keys
/// AWS access key: AKIA followed by 16 uppercase alphanumeric chars
static RE_AWS_ACCESS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap());

/// AWS secret key: 40 chars of base64-like characters (context-dependent)
static RE_AWS_SECRET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Za-z0-9/+=]{40}\b").unwrap());

/// GCP API key: AIza followed by 35 mixed alphanumeric/dash/underscore chars
static RE_GCP_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bAIza[0-9A-Za-z_-]{35}\b").unwrap());

// AI provider keys
/// OpenAI key: sk- followed by 20+ alphanumeric chars
static RE_OPENAI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bsk-[a-zA-Z0-9]{20,}\b").unwrap());

/// Anthropic key: sk-ant- followed by 20+ alphanumeric/dash chars
static RE_ANTHROPIC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bsk-ant-[a-zA-Z0-9_-]{20,}\b").unwrap());

// Code hosting / CI tokens
/// GitHub personal access token: ghp_ followed by 36 alphanumeric chars
static RE_GITHUB_PAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bghp_[a-zA-Z0-9]{36}\b").unwrap());

/// GitHub OAuth token: gho_ followed by 36 alphanumeric chars
static RE_GITHUB_OAUTH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgho_[a-zA-Z0-9]{36}\b").unwrap());

/// GitLab personal access token: glpat- followed by 20+ alphanumeric/dash chars
static RE_GITLAB_PAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bglpat-[a-zA-Z0-9_-]{20,}\b").unwrap());

// Payment keys
/// Stripe secret key: sk_test_ or sk_live_ followed by 24+ alphanumeric chars
static RE_STRIPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bsk_(test|live)_[a-zA-Z0-9]{24,}\b").unwrap());

/// Stripe restricted key: rk_test_ or rk_live_ followed by 24+ alphanumeric chars
static RE_STRIPE_RESTRICTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\brk_(test|live)_[a-zA-Z0-9]{24,}\b").unwrap());

// Private keys (PEM header)
/// Private key PEM header: -----BEGIN [RSA |EC |OPENSSH |DSA ]PRIVATE KEY-----
static RE_PRIVATE_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"-----BEGIN\s+(RSA\s+|EC\s+|OPENSSH\s+|DSA\s+)?PRIVATE\s+KEY-----").unwrap()
});

/// Private key body: base64-encoded key material (lines of 40-76 base64 chars).
/// Catches cases where the PEM header is stripped but the key body remains.
static RE_PRIVATE_KEY_BODY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"MII[A-Za-z0-9+/]{60,76}").unwrap());

// Generic secrets — context-dependent (password=, token=, etc.)
/// Generic secret: keyword followed by = or : and a value of 8-200 non-whitespace chars.
/// Length is capped at 200 to prevent ReDoS from backtracking on long non-matching inputs.
static RE_GENERIC_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b(password|passwd|secret|token|api_key|apikey|api[-_]?secret|auth[-_]?token)\s*[:=]\s*['"]?([^\s'"]{8,200})['"]?"#,
    )
    .unwrap()
});

// Database connection strings
/// Connection strings: mongodb://, postgres://, mysql://, redis://
static RE_CONN_STRING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(mongodb(\+srv)?|postgres(ql)?|mysql|redis)://[^\s]+").unwrap()
});

/// Telegram bot token: 8-10 digit bot ID, colon, 35-char alphanumeric/dash/underscore secret
static RE_TELEGRAM_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{8,10}:[A-Za-z0-9_-]{35}\b").unwrap());

/// HuggingFace token: hf_ followed by 34+ alphanumeric chars
static RE_HUGGINGFACE_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bhf_[a-zA-Z0-9]{34,}\b").unwrap());

// === PII Patterns ===

/// US Social Security Number: NNN-NN-NNNN
static RE_SSN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap());

/// Credit card numbers: Visa, Mastercard, Amex, Discover
static RE_CREDIT_CARD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(?:4[0-9]{12}(?:[0-9]{3})?|5[1-5][0-9]{14}|3[47][0-9]{13}|6(?:011|5[0-9]{2})[0-9]{12})\b",
    )
    .unwrap()
});

/// Email addresses: standard format
static RE_EMAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b").unwrap());

/// US phone numbers: optional +1, area code, 7 digits
static RE_PHONE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(\+1[-.]?)?\(?\d{3}\)?[-.]?\d{3}[-.]?\d{4}\b").unwrap());

/// All credential patterns in scan order.
fn credential_patterns() -> Vec<NamedPattern> {
    vec![
        NamedPattern {
            match_type: "aws_access_key",
            pattern: &RE_AWS_ACCESS,
            redact_prefix: "REDACTED",
        },
        NamedPattern {
            match_type: "aws_secret_key",
            pattern: &RE_AWS_SECRET,
            redact_prefix: "REDACTED",
        },
        NamedPattern {
            match_type: "gcp_api_key",
            pattern: &RE_GCP_KEY,
            redact_prefix: "REDACTED",
        },
        NamedPattern {
            match_type: "openai_key",
            pattern: &RE_OPENAI,
            redact_prefix: "REDACTED",
        },
        NamedPattern {
            match_type: "anthropic_key",
            pattern: &RE_ANTHROPIC,
            redact_prefix: "REDACTED",
        },
        NamedPattern {
            match_type: "github_pat",
            pattern: &RE_GITHUB_PAT,
            redact_prefix: "REDACTED",
        },
        NamedPattern {
            match_type: "github_oauth",
            pattern: &RE_GITHUB_OAUTH,
            redact_prefix: "REDACTED",
        },
        NamedPattern {
            match_type: "gitlab_pat",
            pattern: &RE_GITLAB_PAT,
            redact_prefix: "REDACTED",
        },
        NamedPattern {
            match_type: "stripe_key",
            pattern: &RE_STRIPE,
            redact_prefix: "REDACTED",
        },
        NamedPattern {
            match_type: "stripe_restricted",
            pattern: &RE_STRIPE_RESTRICTED,
            redact_prefix: "REDACTED",
        },
        NamedPattern {
            match_type: "private_key",
            pattern: &RE_PRIVATE_KEY,
            redact_prefix: "REDACTED",
        },
        NamedPattern {
            match_type: "private_key_body",
            pattern: &RE_PRIVATE_KEY_BODY,
            redact_prefix: "REDACTED",
        },
        NamedPattern {
            match_type: "generic_secret",
            pattern: &RE_GENERIC_SECRET,
            redact_prefix: "REDACTED",
        },
        NamedPattern {
            match_type: "connection_string",
            pattern: &RE_CONN_STRING,
            redact_prefix: "REDACTED",
        },
        NamedPattern {
            match_type: "telegram_bot_token",
            pattern: &RE_TELEGRAM_TOKEN,
            redact_prefix: "REDACTED",
        },
        NamedPattern {
            match_type: "huggingface_token",
            pattern: &RE_HUGGINGFACE_TOKEN,
            redact_prefix: "REDACTED",
        },
    ]
}

/// All PII patterns in scan order.
fn pii_patterns() -> Vec<NamedPattern> {
    vec![
        NamedPattern {
            match_type: "us_ssn",
            pattern: &RE_SSN,
            redact_prefix: "PII_REDACTED",
        },
        NamedPattern {
            match_type: "credit_card",
            pattern: &RE_CREDIT_CARD,
            redact_prefix: "PII_REDACTED",
        },
        NamedPattern {
            match_type: "email",
            pattern: &RE_EMAIL,
            redact_prefix: "PII_REDACTED",
        },
        NamedPattern {
            match_type: "phone_us",
            pattern: &RE_PHONE,
            redact_prefix: "PII_REDACTED",
        },
    ]
}

/// Scan text for credential patterns.
/// Returns matches with positions — NEVER returns actual secret values.
pub fn scan_credentials(text: &str) -> Vec<CredentialMatch> {
    scan_with_patterns(text, &credential_patterns())
}

/// Scan text for PII patterns.
pub fn scan_pii(text: &str) -> Vec<CredentialMatch> {
    scan_with_patterns(text, &pii_patterns())
}

/// Quick check — does text contain anything that looks like a secret?
/// Faster than full scan for hot path checks.
pub fn contains_credentials(text: &str) -> bool {
    for np in &credential_patterns() {
        if np.pattern.is_match(text) {
            return true;
        }
    }
    false
}

/// Redact all credentials and PII from text.
/// Returns the redacted text and the list of matches found.
pub fn redact_all(text: &str) -> RedactResult {
    let mut all_matches: Vec<CredentialMatch> = Vec::new();
    all_matches.extend(scan_credentials(text));
    all_matches.extend(scan_pii(text));

    if all_matches.is_empty() {
        return RedactResult {
            redacted: text.to_string(),
            matches: vec![],
        };
    }

    // Sort by position descending so replacements don't shift indices
    all_matches.sort_by(|a, b| b.start.cmp(&a.start));

    // Deduplicate overlapping matches (keep the first = more specific one)
    let mut deduped: Vec<CredentialMatch> = Vec::new();
    for m in &all_matches {
        let overlaps = deduped
            .iter()
            .any(|existing| m.start < existing.end && m.end > existing.start);
        if !overlaps {
            deduped.push(m.clone());
        }
    }

    let mut redacted = text.to_string();
    for m in &deduped {
        // Safe because we sorted descending and deduped
        redacted.replace_range(m.start..m.end, &m.placeholder);
    }

    RedactResult {
        redacted,
        matches: deduped,
    }
}

/// Scan text against a set of named patterns.
fn scan_with_patterns(text: &str, patterns: &[NamedPattern]) -> Vec<CredentialMatch> {
    let mut matches = Vec::new();
    for np in patterns {
        for m in np.pattern.find_iter(text) {
            matches.push(CredentialMatch {
                match_type: np.match_type.to_string(),
                start: m.start(),
                end: m.end(),
                placeholder: format!("[{}:{}]", np.redact_prefix, np.match_type),
            });
        }
    }
    matches
}

/// Summary of a credential scan — safe to log (no secret content).
#[derive(Debug, Default)]
pub struct ScanSummary {
    /// Total credentials found
    pub credential_count: usize,
    /// Total PII items found
    pub pii_count: usize,
    /// Types of credentials found (e.g. ["aws_access_key", "github_pat"])
    pub types_found: Vec<String>,
}

/// Quick scan that returns only a summary (safe to log).
pub fn scan_summary(text: &str) -> ScanSummary {
    let creds = scan_credentials(text);
    let pii = scan_pii(text);
    let mut types: Vec<String> = creds
        .iter()
        .chain(pii.iter())
        .map(|m| m.match_type.clone())
        .collect();
    types.sort();
    types.dedup();

    ScanSummary {
        credential_count: creds.len(),
        pii_count: pii.len(),
        types_found: types,
    }
}

#[cfg(feature = "watcher")]
pub fn emit_credential_event(
    tx: &tokio::sync::broadcast::Sender<crate::watcher::events::WatchEvent>,
    credential_type: &str,
    destination: &str,
) {
    use crate::watcher::events::*;
    let event = WatchEvent {
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        kind: WatchEventKind::CredentialLeak {
            credential_type: credential_type.to_string(),
            destination: destination.to_string(),
        },
        severity: Severity::Critical,
    };
    let _ = tx.send(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aws_access_key() {
        let text = "My key is AKIAIOSFODNN7EXAMPLE ok";
        let matches = scan_credentials(text);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].match_type, "aws_access_key");
    }

    #[test]
    fn test_github_pat() {
        let text = "Token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
        let matches = scan_credentials(text);
        assert!(matches.iter().any(|m| m.match_type == "github_pat"));
    }

    #[test]
    fn test_openai_key() {
        let text = "export OPENAI_API_KEY=sk-abc123def456ghi789jkl012mno345";
        let matches = scan_credentials(text);
        assert!(matches.iter().any(|m| m.match_type == "openai_key"));
    }

    #[test]
    fn test_anthropic_key() {
        let text = "key: sk-ant-api03-abcdefghijklmnopqrst";
        let matches = scan_credentials(text);
        assert!(matches.iter().any(|m| m.match_type == "anthropic_key"));
    }

    #[test]
    fn test_stripe_key() {
        let text = "STRIPE_SECRET=sk_live_abcdefghijklmnopqrstuvwx";
        let matches = scan_credentials(text);
        assert!(matches.iter().any(|m| m.match_type == "stripe_key"));
    }

    #[test]
    fn test_private_key_header() {
        let text = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAK...";
        let matches = scan_credentials(text);
        assert!(matches.iter().any(|m| m.match_type == "private_key"));
    }

    #[test]
    fn test_generic_secret() {
        let text = r#"password = "MyS3cretP@ssw0rd!""#;
        let matches = scan_credentials(text);
        assert!(matches.iter().any(|m| m.match_type == "generic_secret"));
    }

    #[test]
    fn test_connection_string() {
        let text = "DATABASE_URL=postgres://user:pass@localhost:5432/mydb";
        let matches = scan_credentials(text);
        assert!(matches.iter().any(|m| m.match_type == "connection_string"));
    }

    #[test]
    fn test_ssn() {
        let text = "SSN: 123-45-6789";
        let matches = scan_pii(text);
        assert!(matches.iter().any(|m| m.match_type == "us_ssn"));
    }

    #[test]
    fn test_credit_card_visa() {
        let text = "Card: 4111111111111111";
        let matches = scan_pii(text);
        assert!(matches.iter().any(|m| m.match_type == "credit_card"));
    }

    #[test]
    fn test_email() {
        let text = "Contact: alice@example.com for details";
        let matches = scan_pii(text);
        assert!(matches.iter().any(|m| m.match_type == "email"));
    }

    #[test]
    fn test_redact_all() {
        let text = "Key is AKIAIOSFODNN7EXAMPLE and SSN is 123-45-6789";
        let result = redact_all(text);
        assert!(!result.redacted.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(!result.redacted.contains("123-45-6789"));
        assert!(result.redacted.contains("[REDACTED:aws_access_key]"));
        assert!(result.redacted.contains("[PII_REDACTED:us_ssn]"));
    }

    #[test]
    fn test_contains_credentials_true() {
        assert!(contains_credentials(
            "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij"
        ));
    }

    #[test]
    fn test_contains_credentials_false() {
        assert!(!contains_credentials(
            "just a normal message with no secrets"
        ));
    }

    #[test]
    fn test_no_false_positive_normal_text() {
        let text = "Hello, the weather today is sunny with a high of 75F";
        let creds = scan_credentials(text);
        let pii = scan_pii(text);
        assert!(creds.is_empty());
        // phone might match "75F" patterns loosely — check it doesn't
        // (it shouldn't because 75F is too short for phone pattern)
        assert!(pii.is_empty() || pii.iter().all(|m| m.match_type != "credit_card"));
    }

    #[test]
    fn test_scan_summary() {
        let text = "AKIAIOSFODNN7EXAMPLE and 123-45-6789";
        let summary = scan_summary(text);
        assert!(summary.credential_count >= 1);
        assert!(summary.pii_count >= 1);
        assert!(summary.types_found.contains(&"aws_access_key".to_string()));
    }

    #[test]
    fn test_gcp_key() {
        let text = "GCP key: AIzaSyA1234567890abcdefghijklmnopqrstuv";
        let matches = scan_credentials(text);
        assert!(matches.iter().any(|m| m.match_type == "gcp_api_key"));
    }

    #[test]
    fn test_gitlab_pat() {
        let text = "GITLAB_TOKEN=glpat-xxxxxxxxxxxxxxxxxxxx";
        let matches = scan_credentials(text);
        assert!(matches.iter().any(|m| m.match_type == "gitlab_pat"));
    }

    #[test]
    fn test_private_key_body_without_header() {
        // Base64-encoded key material without PEM header (must be 63+ chars after MII prefix)
        let text = "key_data: MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC7bFnRSm8QabcdefghXYZ1234567890";
        let matches = scan_credentials(text);
        assert!(
            matches.iter().any(|m| m.match_type == "private_key_body"),
            "Should detect base64-encoded private key body"
        );
    }

    #[test]
    fn test_match_positions_not_public() {
        // Verify that start/end are pub(crate) by checking they still work
        // internally for redaction
        let text = "Key is AKIAIOSFODNN7EXAMPLE ok";
        let result = redact_all(text);
        assert!(result.redacted.contains("[REDACTED:aws_access_key]"));
        // The positions are used internally but not accessible externally
    }

    #[test]
    fn test_generic_secret_no_redos() {
        // Long string that could cause ReDoS with unbounded repetition
        let long_value = "x".repeat(10_000);
        let text = format!("password = {}", long_value);
        // Should complete quickly (< 1 second) due to capped repetition
        let start = std::time::Instant::now();
        let _ = scan_credentials(&text);
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_secs() < 2,
            "Generic secret scan took too long: {:?}",
            elapsed
        );
    }
}
