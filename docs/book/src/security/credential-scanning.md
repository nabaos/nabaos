# Credential Scanning

> **What you'll learn**
>
> - The 16 credential patterns and 4 PII patterns NabaOS detects
> - How to test the scanner from the command line
> - How redaction works and what the output looks like
> - How to verify detection with specific pattern examples

---

## Overview

The credential scanner runs on every piece of text that enters or leaves the
system -- user input, LLM responses, chain step outputs, and log messages. It
uses compiled regex patterns to detect secrets and personally identifiable
information (PII) in under 1ms.

When a match is found, the scanner replaces it with a type-safe placeholder.
The original secret value is never logged, stored, or returned in any API
response. Byte offsets are kept `pub(crate)` to prevent external code from
reverse-engineering secret positions from match metadata.

---

## 16 Credential Patterns

The scanner detects the following credential types, listed in scan order:

| # | Pattern ID | What it matches | Example prefix |
|---|---|---|---|
| 1 | `aws_access_key` | AWS access key ID | `AKIA` + 16 alphanumeric |
| 2 | `aws_secret_key` | AWS secret access key | 40-char base64-like string |
| 3 | `gcp_api_key` | Google Cloud Platform API key | `AIza` + 35 chars |
| 4 | `openai_key` | OpenAI API key | `sk-` + 20+ chars |
| 5 | `anthropic_key` | Anthropic API key | `sk-ant-` + 20+ chars |
| 6 | `github_pat` | GitHub personal access token | `ghp_` + 36 chars |
| 7 | `github_oauth` | GitHub OAuth token | `gho_` + 36 chars |
| 8 | `gitlab_pat` | GitLab personal access token | `glpat-` + 20+ chars |
| 9 | `stripe_key` | Stripe secret key | `sk_test_` or `sk_live_` + 24+ chars |
| 10 | `stripe_restricted` | Stripe restricted key | `rk_test_` or `rk_live_` + 24+ chars |
| 11 | `private_key` | PEM private key header | `-----BEGIN [RSA] PRIVATE KEY-----` |
| 12 | `private_key_body` | Base64 private key material (no header) | `MII` + 60+ base64 chars |
| 13 | `generic_secret` | Keyword-value pairs (password=, token=, etc.) | `password = "..."` |
| 14 | `connection_string` | Database connection URIs | `postgres://`, `mongodb://`, `redis://` |
| 15 | `telegram_bot_token` | Telegram bot API token | 8-10 digit ID + `:` + 35-char secret |
| 16 | `huggingface_token` | HuggingFace API token | `hf_` + 34+ chars |

### Pattern details

**Cloud provider keys** (patterns 1-3): These have distinctive prefixes that
make false positives rare. AWS access keys always start with `AKIA`, and GCP
keys always start with `AIza`.

**AI provider keys** (patterns 4-5): OpenAI keys start with `sk-` and Anthropic
keys start with `sk-ant-`. The scanner requires at least 20 characters after the
prefix to avoid matching short strings.

**Code hosting tokens** (patterns 6-8): GitHub PATs use `ghp_` (personal) and
`gho_` (OAuth) prefixes with exactly 36 trailing characters. GitLab uses
`glpat-` with 20+ characters.

**Payment keys** (patterns 9-10): Stripe keys use `sk_test_`/`sk_live_` and
`rk_test_`/`rk_live_` prefixes, requiring 24+ trailing characters.

**Private keys** (patterns 11-12): Pattern 11 detects PEM headers
(`-----BEGIN RSA PRIVATE KEY-----`). Pattern 12 catches key material without
headers -- base64-encoded bodies starting with `MII` followed by 60+ characters
of base64 content.

**Generic secrets** (pattern 13): Matches `password`, `passwd`, `secret`,
`token`, `api_key`, `apikey`, `api_secret`, and `auth_token` followed by `=` or
`:` and a value of 8-200 characters. The 200-character cap prevents ReDoS from
backtracking on long non-matching inputs.

**Connection strings** (pattern 14): Detects `mongodb://`, `postgres://`,
`mysql://`, and `redis://` URIs that typically contain embedded credentials.

**Messaging tokens** (pattern 15): Telegram bot tokens follow a specific format:
8-10 digit bot ID, colon, then a 35-character alphanumeric secret.

**ML platform tokens** (pattern 16): HuggingFace tokens start with `hf_`
followed by 34+ alphanumeric characters.

---

## 4 PII Patterns

| # | Pattern ID | What it matches | Example |
|---|---|---|---|
| 1 | `us_ssn` | US Social Security Number | `123-45-6789` |
| 2 | `credit_card` | Visa, Mastercard, Amex, Discover | `4111111111111111` |
| 3 | `email` | Email addresses | `alice@example.com` |
| 4 | `phone_us` | US phone numbers | `(555) 123-4567`, `+1-555-123-4567` |

PII matches use the `PII_REDACTED` prefix in placeholders instead of `REDACTED`,
so downstream code can distinguish between credential leaks and personal data
exposure.

---

## How to Test

Use the `security-scan` command to test the scanner against any input:

```bash
nyaya security-scan "my AWS key is AKIAIOSFODNN7EXAMPLE and email is alice@example.com"
```

**Expected output:**

```text
=== Security Scan Results ===

Credential matches: 1
  [1] aws_access_key

PII matches: 1
  [1] email

Redacted text:
  my AWS key is [REDACTED:aws_access_key] and email is [PII_REDACTED:email]
```

### Test each pattern type

Here are test commands for every credential category:

```bash
# AWS access key
nyaya security-scan "AKIAIOSFODNN7EXAMPLE"

# OpenAI key
nyaya security-scan "sk-abc123def456ghi789jkl012mno345"

# Anthropic key
nyaya security-scan "sk-ant-api03-abcdefghijklmnopqrst"

# GitHub PAT
nyaya security-scan "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij"

# GitLab PAT
nyaya security-scan "glpat-xxxxxxxxxxxxxxxxxxxx"

# Stripe key
nyaya security-scan "sk_live_abcdefghijklmnopqrstuvwx"

# Private key header
nyaya security-scan "-----BEGIN RSA PRIVATE KEY-----"

# Generic secret
nyaya security-scan 'password = "MyS3cretP@ssw0rd!"'

# Connection string
nyaya security-scan "postgres://user:pass@localhost:5432/mydb"

# Telegram bot token
nyaya security-scan "1234567890:ABCDefghIJKLmnopQRSTuvwxYZ123456789"

# HuggingFace token
nyaya security-scan "hf_abcdefghijklmnopqrstuvwxyz12345678"

# SSN
nyaya security-scan "SSN is 123-45-6789"

# Credit card
nyaya security-scan "Card: 4111111111111111"

# Email
nyaya security-scan "Contact alice@example.com"

# Phone
nyaya security-scan "Call (555) 123-4567"
```

---

## How Redaction Works

The redaction process operates in four steps:

1. **Scan credentials**: All 16 credential patterns are evaluated against the
   input text. Each match records its type, byte start offset, and byte end
   offset.

2. **Scan PII**: All 4 PII patterns are evaluated. Matches are added to the
   same list.

3. **Deduplicate overlaps**: Matches are sorted by position (descending). If
   two matches overlap in byte range, the more specific match (scanned first)
   is kept and the other is dropped.

4. **Replace**: Working from the end of the string backward (so byte offsets
   remain valid), each match is replaced with its placeholder string.

### Placeholder format

Credentials are replaced with:

```text
[REDACTED:pattern_id]
```

PII is replaced with:

```text
[PII_REDACTED:pattern_id]
```

**Example:**

Input:

```text
Key is AKIAIOSFODNN7EXAMPLE and SSN is 123-45-6789
```

Output:

```text
Key is [REDACTED:aws_access_key] and SSN is [PII_REDACTED:us_ssn]
```

### Where redaction runs

| Location | When | Why |
|---|---|---|
| **Input gate** | Before security classification | Prevent secrets from reaching the BERT classifier context |
| **LLM output** | After every LLM response | Catch secrets the model may have memorized or hallucinated |
| **Chain step output** | After each tool call returns | Catch secrets in API responses |
| **Log pipeline** | Before any text is written to logs | Ensure secrets never appear in log files |

---

## Quick Check API

For hot-path performance, the scanner provides a `contains_credentials` function
that returns a boolean without building the full match list. This is used for
the fast pre-check before running a complete scan:

```text
contains_credentials("normal text")          → false  (< 0.1ms)
contains_credentials("ghp_ABCDEF...")        → true   (< 0.1ms)
```

The full `redact_all` function, which builds match objects and performs string
replacement, completes in under 1ms for typical input lengths.

---

## Scan Summary (Safe to Log)

The `scan_summary` function returns a `ScanSummary` that contains only
metadata -- never the actual secret values:

```rust
ScanSummary {
    credential_count: 1,
    pii_count: 1,
    types_found: ["aws_access_key", "email"],
}
```

This summary is safe to include in log files, security alerts, and anomaly
detector records.

---

## Design Decisions

**Why regex instead of ML?** Credential patterns have rigid, well-defined
formats (fixed prefixes, known lengths). Regex detection is deterministic,
auditable, and runs in under 1ms. An ML classifier would add latency, require
training data, and introduce false-negative risk for a problem that regex solves
perfectly.

**Why cap generic_secret at 200 characters?** Without a length cap, the
`[^\s'"]{8,200}` quantifier could backtrack exponentially on long non-matching
strings, causing a regex denial-of-service (ReDoS). The 200-character cap bounds
worst-case execution time. This is verified by a dedicated test
(`test_generic_secret_no_redos`) that runs the scanner against a 10,000-character
input and asserts completion in under 2 seconds.

**Why are byte offsets `pub(crate)`?** Exposing match positions in a public API
would allow an attacker to infer secret length and location from redaction
metadata. By keeping offsets internal, the public interface reveals only the
type of credential found, not where it was in the input.

---

## Next Steps

- [Threat Model](threat-model.md) -- understand the full security architecture
- [Circuit Breakers](circuit-breakers.md) -- add safety limits to chain execution
- [Debug Mode](../troubleshooting/debug-mode.md) -- inspect security scan results in detail
