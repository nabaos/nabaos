//! Constitution enforcement — YAML rules matched against W5H2 intents.
//!
//! The constitution is a set of rules that gate actions before any LLM or tool
//! execution. Each rule has a trigger (action groups), optional conditions
//! (scope, amount), and an enforcement level (Block, Warn, Confirm).

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::core::error::{NyayaError, Result};
use crate::w5h2::types::W5H2Intent;

/// Enforcement action when a rule matches
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Enforcement {
    /// Silently block the action
    Block,
    /// Allow but log a warning
    Warn,
    /// Require user confirmation before proceeding
    Confirm,
    /// Allow unconditionally
    Allow,
}

/// A single constitution rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    pub description: Option<String>,
    /// Actions that trigger this rule (e.g., ["send", "control"])
    pub trigger_actions: Vec<String>,
    /// Targets that trigger this rule (e.g., ["email", "lights"])
    /// If empty, matches any target
    pub trigger_targets: Vec<String>,
    /// Keywords in query that trigger this rule
    #[serde(default)]
    pub trigger_keywords: Vec<String>,
    /// What to do when the rule matches
    pub enforcement: Enforcement,
    /// Human-readable reason for the rule
    pub reason: Option<String>,
}

/// Browser stealth configuration for NyayaBrowser fingerprint resistance
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserStealthConfig {
    pub enabled: bool,
    pub canvas_noise: bool,
    pub webgl_spoof: bool,
    pub max_concurrent_tabs: usize,
}

/// Swarm-level constitution constraints
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SwarmConstitutionConfig {
    pub enabled: bool,
    pub max_workers: usize,
    pub allowed_worker_types: Vec<String>,
    pub max_result_chars: usize,
}

/// Optional Ollama (local LLM) configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OllamaConfig {
    pub enabled: bool,
    pub base_url: Option<String>,
    pub default_model: Option<String>,
    pub vision_model: Option<String>,
}

/// Advanced CAPTCHA solver configuration (entirely opt-in).
/// If not present, CAPTCHA handling falls back to basic detection only.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CaptchaSolverConfig {
    /// Master switch — must be true to enable any solving.
    pub enabled: bool,
    /// Try VLM (vision model) first. Uses the configured Ollama vision_model
    /// or the primary LLM's vision endpoint.
    #[serde(default = "default_vlm_enabled")]
    pub vlm_enabled: bool,
    /// CapSolver API key. Only used as fallback when VLM fails.
    /// If empty/absent, CapSolver is never attempted.
    pub capsolver_api_key: Option<String>,
    /// CapSolver API endpoint (default: https://api.capsolver.com)
    pub capsolver_base_url: Option<String>,
    /// Max attempts before giving up.
    #[serde(default = "default_captcha_max_attempts")]
    pub max_attempts: u32,
}

fn default_vlm_enabled() -> bool {
    true
}
fn default_captcha_max_attempts() -> u32 {
    3
}

/// The full constitution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constitution {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    pub rules: Vec<Rule>,
    /// Default enforcement for unmatched intents
    #[serde(default = "default_enforcement")]
    pub default_enforcement: Enforcement,
    /// Optional channel-level permissions (per-contact/group/domain access control)
    #[serde(default)]
    pub channel_permissions: Option<crate::security::channel_permissions::ChannelPermissions>,
    /// Browser stealth / fingerprint-resistance settings for NyayaBrowser
    #[serde(default)]
    pub browser_stealth: Option<BrowserStealthConfig>,
    /// Swarm worker constraints (max workers, allowed types, result budget)
    #[serde(default)]
    pub swarm_config: Option<SwarmConstitutionConfig>,
    /// Ollama local LLM configuration (optional — not required)
    #[serde(default)]
    pub ollama_config: Option<OllamaConfig>,
    /// Advanced CAPTCHA solver configuration (opt-in)
    #[serde(default)]
    pub captcha_solver: Option<CaptchaSolverConfig>,
    /// Ed25519 signature over the YAML content (hex-encoded, optional for backward compat)
    #[serde(default)]
    pub signature: Option<String>,
    /// Ed25519 public key used to sign this constitution (hex-encoded)
    #[serde(default)]
    pub public_key: Option<String>,
}

/// SECURITY: Default to Block (deny-by-default) so unmatched intents are rejected.
/// Warn was fail-open: it logged but allowed execution, effectively bypassing the constitution.
fn default_enforcement() -> Enforcement {
    Enforcement::Block
}

/// Result of checking an intent against the constitution
#[derive(Debug)]
pub struct ConstitutionCheck {
    pub allowed: bool,
    pub enforcement: Enforcement,
    pub matched_rule: Option<String>,
    pub reason: Option<String>,
}

/// The constitution enforcer
pub struct ConstitutionEnforcer {
    constitution: Constitution,
}

impl ConstitutionEnforcer {
    /// Load constitution from a YAML file.
    /// If the constitution has a signature, it is verified against the public key.
    /// If signature is present but invalid, loading fails.
    /// If signature is absent, a warning is logged (backward compat).
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let constitution: Constitution = serde_yaml::from_str(&content)?;

        // Verify signature if present
        if let (Some(sig_hex), Some(pk_hex)) =
            (&constitution.signature, &constitution.public_key)
        {
            // Strip signature/public_key fields from YAML to get the signed content
            let signable = ConstitutionSigner::strip_signature_fields(&content);
            if !ConstitutionSigner::verify_hex(&signable, sig_hex, pk_hex) {
                return Err(NyayaError::Config(
                    "Constitution signature verification failed — file may have been tampered with"
                        .to_string(),
                ));
            }
            tracing::info!(
                name = %constitution.name,
                rules = constitution.rules.len(),
                "Constitution loaded (signature verified)"
            );
        } else {
            tracing::warn!(
                name = %constitution.name,
                rules = constitution.rules.len(),
                "Constitution loaded (unsigned — consider signing with `nabaos constitution sign`)"
            );
        }

        Ok(Self { constitution })
    }

    /// Create from an in-memory constitution
    pub fn from_constitution(constitution: Constitution) -> Self {
        Self { constitution }
    }

    /// Check an intent against the constitution (pre-LLM gate)
    pub fn check(&self, intent: &W5H2Intent, query: Option<&str>) -> ConstitutionCheck {
        let action_str = format!("{}", intent.action).to_lowercase();
        let target_str = format!("{}", intent.target).to_lowercase();
        let query_lower = query.map(|q| q.to_lowercase());

        for rule in &self.constitution.rules {
            let has_action_triggers = !rule.trigger_actions.is_empty();
            let has_target_triggers = !rule.trigger_targets.is_empty();
            let has_keyword_triggers = !rule.trigger_keywords.is_empty();

            // A rule with NO triggers at all matches nothing (skip it)
            if !has_action_triggers && !has_target_triggers && !has_keyword_triggers {
                continue;
            }

            // Check action/target triggers (must both match if specified)
            let action_match = !has_action_triggers
                || rule
                    .trigger_actions
                    .iter()
                    .any(|a| a.to_lowercase() == action_str || a == "*");
            let target_match = !has_target_triggers
                || rule
                    .trigger_targets
                    .iter()
                    .any(|t| t.to_lowercase() == target_str || t == "*");

            let intent_matches = has_action_triggers && action_match && target_match;

            // Check keyword trigger (independent of action/target)
            let keyword_matches = has_keyword_triggers
                && query_lower.as_ref().is_some_and(|q| {
                    rule.trigger_keywords
                        .iter()
                        .any(|kw| q.contains(&kw.to_lowercase()))
                });

            // Rule matches if either intent or keywords match
            let matches = intent_matches || keyword_matches;

            if matches {
                let allowed = matches!(rule.enforcement, Enforcement::Allow | Enforcement::Warn | Enforcement::Confirm);
                return ConstitutionCheck {
                    allowed,
                    enforcement: rule.enforcement,
                    matched_rule: Some(rule.name.clone()),
                    reason: rule.reason.clone(),
                };
            }
        }

        // No rule matched, use default
        let allowed = matches!(
            self.constitution.default_enforcement,
            Enforcement::Allow | Enforcement::Warn | Enforcement::Confirm
        );
        ConstitutionCheck {
            allowed,
            enforcement: self.constitution.default_enforcement,
            matched_rule: None,
            reason: None,
        }
    }

    /// Check query text against keyword-based constitution rules only.
    /// This is used BEFORE classification (Tier 0/1 cache lookups) to ensure
    /// cached results cannot bypass the constitution.
    pub fn check_query_text(&self, query: &str) -> ConstitutionCheck {
        let query_lower = query.to_lowercase();

        for rule in &self.constitution.rules {
            let has_keyword_triggers = !rule.trigger_keywords.is_empty();
            if !has_keyword_triggers {
                continue;
            }

            let keyword_matches = rule
                .trigger_keywords
                .iter()
                .any(|kw| query_lower.contains(&kw.to_lowercase()));

            if keyword_matches {
                let allowed = matches!(rule.enforcement, Enforcement::Allow | Enforcement::Warn | Enforcement::Confirm);
                return ConstitutionCheck {
                    allowed,
                    enforcement: rule.enforcement,
                    matched_rule: Some(rule.name.clone()),
                    reason: rule.reason.clone(),
                };
            }
        }

        // No keyword rule matched — allow (intent-based rules checked later)
        ConstitutionCheck {
            allowed: true,
            enforcement: Enforcement::Allow,
            matched_rule: None,
            reason: None,
        }
    }

    /// Get the constitution name
    pub fn name(&self) -> &str {
        &self.constitution.name
    }

    /// Get the channel permissions from the constitution (if any).
    pub fn channel_permissions(
        &self,
    ) -> Option<&crate::security::channel_permissions::ChannelPermissions> {
        self.constitution.channel_permissions.as_ref()
    }

    /// Get all rules
    pub fn rules(&self) -> &[Rule] {
        &self.constitution.rules
    }

    /// Check whether the given channel/contact/group/domain combination is allowed
    /// by the constitution's channel_permissions configuration.
    pub fn check_channel_access(
        &self,
        channel: &str,
        contact: Option<&str>,
        group: Option<&str>,
        domain: Option<&str>,
    ) -> crate::security::channel_permissions::ChannelAccessCheck {
        match &self.constitution.channel_permissions {
            Some(perms) => perms.check_access(channel, contact, group, domain),
            None => crate::security::channel_permissions::ChannelAccessCheck {
                allowed: true,
                reason: "no channel_permissions configured".into(),
            },
        }
    }

    /// Check an ability name against constitution rules.
    /// Used in MODE 2 to verify each step's ability is allowed before chain execution.
    ///
    /// Matching rules:
    /// - Extract the action part (before the first dot) for comparison
    /// - Use case-insensitive exact matching (not substring) to prevent
    ///   "send" from matching "transcend" or "sending"
    /// - Wildcard "*" matches any action
    pub fn check_ability(&self, ability: &str) -> ConstitutionCheck {
        // Extract the action part (before separator) for matching.
        // Intent keys use '_' (e.g. "check_email"), tool names may use '.' (e.g. "email.send").
        let sep = if ability.contains('.') { '.' } else { '_' };
        let action_part = ability.split(sep).next().unwrap_or(ability).to_lowercase();
        let target_part: Option<String> = ability.split(sep).nth(1).map(|s| s.to_lowercase());

        for rule in &self.constitution.rules {
            let has_action_triggers = !rule.trigger_actions.is_empty();
            let has_target_triggers = !rule.trigger_targets.is_empty();
            if !has_action_triggers {
                continue;
            }

            // Exact match (case-insensitive) on action part, or wildcard
            let action_match = rule.trigger_actions.iter().any(|a| {
                let trigger_lower = a.to_lowercase();
                trigger_lower == action_part || a == "*"
            });

            // If rule has target triggers, also check the target part
            let target_match = !has_target_triggers
                || target_part.as_ref().is_some_and(|tp| {
                    rule.trigger_targets
                        .iter()
                        .any(|t| t.to_lowercase() == *tp || t == "*")
                });

            if action_match && target_match {
                let allowed = matches!(rule.enforcement, Enforcement::Allow | Enforcement::Warn | Enforcement::Confirm);
                return ConstitutionCheck {
                    allowed,
                    enforcement: rule.enforcement,
                    matched_rule: Some(rule.name.clone()),
                    reason: rule.reason.clone(),
                };
            }
        }

        // No rule matched, use default
        let allowed = matches!(
            self.constitution.default_enforcement,
            Enforcement::Allow | Enforcement::Warn | Enforcement::Confirm
        );
        ConstitutionCheck {
            allowed,
            enforcement: self.constitution.default_enforcement,
            matched_rule: None,
            reason: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Ed25519 Constitution Signing
// ---------------------------------------------------------------------------

/// Handles Ed25519 signing and verification of constitution YAML files.
pub struct ConstitutionSigner;

impl ConstitutionSigner {
    /// Generate a new Ed25519 keypair. Returns (signing_key_bytes, verifying_key_bytes).
    pub fn generate_keypair() -> ([u8; 32], [u8; 32]) {
        use ed25519_dalek::SigningKey;
        use rand::rngs::OsRng;

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        (signing_key.to_bytes(), verifying_key.to_bytes())
    }

    /// Sign YAML bytes with an Ed25519 signing key. Returns signature bytes.
    pub fn sign(yaml_bytes: &[u8], signing_key_bytes: &[u8; 32]) -> Vec<u8> {
        use ed25519_dalek::{Signer, SigningKey};

        let signing_key = SigningKey::from_bytes(signing_key_bytes);
        let signature = signing_key.sign(yaml_bytes);
        signature.to_bytes().to_vec()
    }

    /// Verify a signature over YAML bytes using an Ed25519 verifying key.
    pub fn verify(
        yaml_bytes: &[u8],
        signature_bytes: &[u8],
        verifying_key_bytes: &[u8; 32],
    ) -> bool {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};

        let Ok(verifying_key) = VerifyingKey::from_bytes(verifying_key_bytes) else {
            return false;
        };
        let Ok(signature) = Signature::from_slice(signature_bytes) else {
            return false;
        };
        verifying_key.verify(yaml_bytes, &signature).is_ok()
    }

    /// Verify using hex-encoded signature and public key strings.
    pub fn verify_hex(yaml_content: &str, sig_hex: &str, pk_hex: &str) -> bool {
        let Ok(sig_bytes) = hex::decode(sig_hex) else {
            return false;
        };
        let Ok(pk_bytes) = hex::decode(pk_hex) else {
            return false;
        };
        if pk_bytes.len() != 32 {
            return false;
        }
        let pk: [u8; 32] = pk_bytes.try_into().unwrap();
        Self::verify(yaml_content.as_bytes(), &sig_bytes, &pk)
    }

    /// Strip `signature:` and `public_key:` fields from YAML content
    /// to produce the canonical content that was signed.
    pub fn strip_signature_fields(yaml_content: &str) -> String {
        yaml_content
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                !trimmed.starts_with("signature:") && !trimmed.starts_with("public_key:")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Sign a constitution YAML file and return the updated YAML with signature and public_key.
    pub fn sign_file(
        yaml_content: &str,
        signing_key_bytes: &[u8; 32],
    ) -> (String, String, String) {
        use ed25519_dalek::SigningKey;

        let signable = Self::strip_signature_fields(yaml_content);
        let sig_bytes = Self::sign(signable.as_bytes(), signing_key_bytes);
        let sig_hex = hex::encode(&sig_bytes);

        let signing_key = SigningKey::from_bytes(signing_key_bytes);
        let pk_hex = hex::encode(signing_key.verifying_key().to_bytes());

        (signable, sig_hex, pk_hex)
    }
}

/// Load the default constitution (built-in safe defaults)
pub fn default_constitution() -> Constitution {
    Constitution {
        name: "default".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Default safety constitution with common-sense boundaries".to_string()),
        rules: vec![
            Rule {
                name: "block_destructive_keywords".to_string(),
                description: Some("Block queries containing destructive keywords".to_string()),
                trigger_actions: vec![],
                trigger_targets: vec![],
                trigger_keywords: vec![
                    "delete all".to_string(),
                    "rm -rf".to_string(),
                    "drop table".to_string(),
                    "format disk".to_string(),
                    "wipe".to_string(),
                    "destroy".to_string(),
                ],
                enforcement: Enforcement::Block,
                reason: Some("Destructive operations require explicit confirmation".to_string()),
            },
            Rule {
                name: "confirm_send_actions".to_string(),
                description: Some("Require confirmation for sending messages/emails".to_string()),
                trigger_actions: vec!["send".to_string()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Sending actions can have external effects".to_string()),
            },
            Rule {
                name: "warn_control_actions".to_string(),
                description: Some("Warn on device control actions".to_string()),
                trigger_actions: vec!["control".to_string()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Warn,
                reason: Some("Device control changes physical state".to_string()),
            },
            Rule {
                name: "allow_check_actions".to_string(),
                description: Some("Allow all read-only check actions".to_string()),
                trigger_actions: vec!["check".to_string()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Read-only operations are safe".to_string()),
            },
            Rule {
                name: "confirm_delete_actions".to_string(),
                description: Some("Require confirmation for delete actions".to_string()),
                trigger_actions: vec!["delete".to_string()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Delete actions are destructive and need confirmation".to_string()),
            },
            Rule {
                name: "allow_search_actions".to_string(),
                description: Some("Allow search/query actions".to_string()),
                trigger_actions: vec![
                    "search".to_string(),
                    "query".to_string(),
                    "list".to_string(),
                    "get".to_string(),
                    "flow".to_string(),
                    "schedule".to_string(),
                    "notify".to_string(),
                    "nlp".to_string(),
                    "data".to_string(),
                    "storage".to_string(),
                    "trading".to_string(),
                    "browser".to_string(),
                    "calendar".to_string(),
                    "memory".to_string(),
                    "docs".to_string(),
                    "channel".to_string(),
                    "files".to_string(),
                    "create".to_string(),
                    "analyze".to_string(),
                    "generate".to_string(),
                    "script".to_string(),
                    "shell".to_string(),
                    "llm".to_string(),
                    "email".to_string(),
                    "sms".to_string(),
                    "deep".to_string(),
                    "write".to_string(),
                    "read".to_string(),
                    "store".to_string(),
                    "recall".to_string(),
                    "save".to_string(),
                    "copy".to_string(),
                    "rename".to_string(),
                    "move".to_string(),
                    "run".to_string(),
                    "compute".to_string(),
                    "calculate".to_string(),
                    "count".to_string(),
                    "extract".to_string(),
                    "summarize".to_string(),
                    "fetch".to_string(),
                    "explain".to_string(),
                    "convert".to_string(),
                    "sort".to_string(),
                    "filter".to_string(),
                    "find".to_string(),
                    "show".to_string(),
                    "print".to_string(),
                    "parse".to_string(),
                    "format".to_string(),
                    "build".to_string(),
                    "test".to_string(),
                    "check".to_string(),
                    "validate".to_string(),
                    "describe".to_string(),
                    "compare".to_string(),
                    "update".to_string(),
                    "add".to_string(),
                    "set".to_string(),
                    "remove".to_string(),
                    "install".to_string(),
                    "configure".to_string(),
                    "download".to_string(),
                    "upload".to_string(),
                    "encode".to_string(),
                    "decode".to_string(),
                    "encrypt".to_string(),
                    "decrypt".to_string(),
                    "hash".to_string(),
                    "zip".to_string(),
                    "unzip".to_string(),
                    "backup".to_string(),
                    "restore".to_string(),
                    "monitor".to_string(),
                    "debug".to_string(),
                    "profile".to_string(),
                    "benchmark".to_string(),
                    "deploy".to_string(),
                    "ping".to_string(),
                    "scan".to_string(),
                    "inspect".to_string(),
                    "log".to_string(),
                    "audit".to_string(),
                    "report".to_string(),
                    "collect".to_string(),
                    "aggregate".to_string(),
                    "transform".to_string(),
                    "clean".to_string(),
                    "merge".to_string(),
                    "split".to_string(),
                    "map".to_string(),
                    "reduce".to_string(),
                    "sample".to_string(),
                    "visualize".to_string(),
                    "plot".to_string(),
                    "chart".to_string(),
                    "diagram".to_string(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard agent operations are allowed".to_string()),
            },
        ],
        // SECURITY: Block-by-default — unmatched intents are denied.
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Solopreneur assistant — business planning, drafting, research
pub fn solopreneur_constitution() -> Constitution {
    Constitution {
        name: "solopreneur".to_string(),
        version: "1.0.0".to_string(),
        description: Some(
            "Solopreneur assistant — business planning, drafting, research".to_string(),
        ),
        rules: vec![
            Rule {
                name: "allow_business_ops".to_string(),
                description: Some("Allow standard business operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "create".into(),
                    "analyze".into(),
                    "generate".into(),
                    "schedule".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "calendar".into(),
                    "files".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard solopreneur operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send".to_string(),
                description: Some("Require confirmation before sending".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Sending actions can have external effects".to_string()),
            },
            Rule {
                name: "confirm_delete".to_string(),
                description: Some("Require confirmation before deleting".to_string()),
                trigger_actions: vec!["delete".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Delete actions are destructive and need confirmation".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Freelancer assistant — invoicing, client comms, time tracking
pub fn freelancer_constitution() -> Constitution {
    Constitution {
        name: "freelancer".to_string(),
        version: "1.0.0".to_string(),
        description: Some(
            "Freelancer assistant — invoicing, client comms, time tracking".to_string(),
        ),
        rules: vec![
            Rule {
                name: "allow_freelance_ops".to_string(),
                description: Some("Allow standard freelance operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "create".into(),
                    "analyze".into(),
                    "generate".into(),
                    "schedule".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "calendar".into(),
                    "files".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard freelancer operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send".to_string(),
                description: Some("Require confirmation before sending".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Sending actions can have external effects".to_string()),
            },
            Rule {
                name: "confirm_delete".to_string(),
                description: Some("Require confirmation before deleting".to_string()),
                trigger_actions: vec!["delete".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Delete actions are destructive and need confirmation".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Digital marketing assistant — analytics, content creation, SEO
pub fn marketer_constitution() -> Constitution {
    Constitution {
        name: "digital-marketer".to_string(),
        version: "1.0.0".to_string(),
        description: Some(
            "Digital marketing assistant — analytics, content creation, SEO".to_string(),
        ),
        rules: vec![
            Rule {
                name: "allow_marketing_ops".to_string(),
                description: Some("Allow standard marketing operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "create".into(),
                    "analyze".into(),
                    "generate".into(),
                    "schedule".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "browser".into(),
                    "files".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard marketing operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send".to_string(),
                description: Some("Require confirmation before sending".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Sending actions can have external effects".to_string()),
            },
            Rule {
                name: "block_financial".to_string(),
                description: Some("Block all financial operations".to_string()),
                trigger_actions: vec!["*".into()],
                trigger_targets: vec!["price".into(), "portfolio".into()],
                trigger_keywords: vec![],
                enforcement: Enforcement::Block,
                reason: Some("Marketing assistant cannot access financial data".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Student assistant — research, study aids, assignment help
pub fn student_constitution() -> Constitution {
    Constitution {
        name: "student".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Student assistant — research, study aids, assignment help".to_string()),
        rules: vec![
            Rule {
                name: "block_financial".to_string(),
                description: Some("Block all financial operations".to_string()),
                trigger_actions: vec!["*".into()],
                trigger_targets: vec!["price".into(), "portfolio".into(), "invoice".into()],
                trigger_keywords: vec![],
                enforcement: Enforcement::Block,
                reason: Some("Student assistant cannot access financial data".to_string()),
            },
            Rule {
                name: "allow_learning_ops".to_string(),
                description: Some("Allow standard learning operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "analyze".into(),
                    "generate".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "browser".into(),
                    "files".into(),
                    "calendar".into(),
                    "schedule".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard learning operations are allowed".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Sales assistant — lead management, outreach, pipeline tracking
pub fn sales_constitution() -> Constitution {
    Constitution {
        name: "sales".to_string(),
        version: "1.0.0".to_string(),
        description: Some(
            "Sales assistant — lead management, outreach, pipeline tracking".to_string(),
        ),
        rules: vec![
            Rule {
                name: "allow_sales_ops".to_string(),
                description: Some("Allow standard sales operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "create".into(),
                    "analyze".into(),
                    "generate".into(),
                    "schedule".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "calendar".into(),
                    "files".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard sales operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_outreach".to_string(),
                description: Some("Require confirmation before sending outreach".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Outreach sends need confirmation to avoid spam".to_string()),
            },
            Rule {
                name: "confirm_delete".to_string(),
                description: Some("Require confirmation before deleting".to_string()),
                trigger_actions: vec!["delete".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Delete actions are destructive and need confirmation".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Support assistant — ticket triage, KB search, response drafting
pub fn support_constitution() -> Constitution {
    Constitution {
        name: "customer-support".to_string(),
        version: "1.0.0".to_string(),
        description: Some(
            "Support assistant — ticket triage, KB search, response drafting".to_string(),
        ),
        rules: vec![
            Rule {
                name: "allow_support_ops".to_string(),
                description: Some("Allow standard support operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "create".into(),
                    "analyze".into(),
                    "generate".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "files".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard support operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send".to_string(),
                description: Some("Require confirmation before sending".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Sending responses needs confirmation".to_string()),
            },
            Rule {
                name: "block_destructive".to_string(),
                description: Some("Block destructive and control operations".to_string()),
                trigger_actions: vec!["delete".into(), "control".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Block,
                reason: Some("Support agents cannot delete data or control systems".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Legal assistant — contract analysis, case research, document drafting
pub fn legal_constitution() -> Constitution {
    Constitution {
        name: "legal".to_string(),
        version: "1.0.0".to_string(),
        description: Some(
            "Legal assistant — contract analysis, case research, document drafting".to_string(),
        ),
        rules: vec![
            Rule {
                name: "allow_legal_ops".to_string(),
                description: Some("Allow standard legal operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "analyze".into(),
                    "generate".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "browser".into(),
                    "files".into(),
                    "create".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard legal operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send".to_string(),
                description: Some("Require confirmation before sending".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Sending legal communications needs confirmation".to_string()),
            },
            Rule {
                name: "block_financial_control".to_string(),
                description: Some("Block control and delete operations".to_string()),
                trigger_actions: vec!["control".into(), "delete".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Block,
                reason: Some("Legal assistant cannot delete data or control systems".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// E-commerce assistant — inventory, orders, product listings, analytics
pub fn ecommerce_constitution() -> Constitution {
    Constitution {
        name: "ecommerce".to_string(),
        version: "1.0.0".to_string(),
        description: Some(
            "E-commerce assistant — inventory, orders, product listings, analytics".to_string(),
        ),
        rules: vec![
            Rule {
                name: "allow_ecommerce_ops".to_string(),
                description: Some("Allow standard e-commerce operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "create".into(),
                    "analyze".into(),
                    "generate".into(),
                    "schedule".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "files".into(),
                    "calendar".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard e-commerce operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send_and_delete".to_string(),
                description: Some(
                    "Require confirmation for send and delete operations".to_string(),
                ),
                trigger_actions: vec!["send".into(), "delete".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Send and delete actions need confirmation in e-commerce".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// HR assistant — recruitment, onboarding, employee engagement
pub fn hr_constitution() -> Constitution {
    Constitution {
        name: "hr".to_string(),
        version: "1.0.0".to_string(),
        description: Some(
            "HR assistant — recruitment, onboarding, employee engagement".to_string(),
        ),
        rules: vec![
            Rule {
                name: "allow_hr_ops".to_string(),
                description: Some("Allow standard HR operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "create".into(),
                    "analyze".into(),
                    "generate".into(),
                    "schedule".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "calendar".into(),
                    "files".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard HR operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send".to_string(),
                description: Some("Require confirmation before sending".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Sending actions need confirmation".to_string()),
            },
            Rule {
                name: "block_financial".to_string(),
                description: Some("Block financial operations".to_string()),
                trigger_actions: vec!["*".into()],
                trigger_targets: vec!["price".into(), "portfolio".into(), "invoice".into()],
                trigger_keywords: vec![],
                enforcement: Enforcement::Block,
                reason: Some("HR assistant cannot access financial data".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Finance assistant — accounting, tax, audit, budgeting
pub fn finance_constitution() -> Constitution {
    Constitution {
        name: "finance".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Finance assistant — accounting, tax, audit, budgeting".to_string()),
        rules: vec![
            Rule {
                name: "allow_finance_ops".to_string(),
                description: Some("Allow standard finance operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "create".into(),
                    "analyze".into(),
                    "generate".into(),
                    "schedule".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "calendar".into(),
                    "files".into(),
                    "trading".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard finance operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send".to_string(),
                description: Some("Require confirmation before sending".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Outbound communications need confirmation".to_string()),
            },
            Rule {
                name: "confirm_delete".to_string(),
                description: Some("Require confirmation before deleting".to_string()),
                trigger_actions: vec!["delete".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Delete actions are destructive and need confirmation".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Healthcare assistant — clinical summaries, triage, drug interactions
pub fn healthcare_constitution() -> Constitution {
    Constitution {
        name: "healthcare".to_string(),
        version: "1.0.0".to_string(),
        description: Some(
            "Healthcare assistant — clinical summaries, triage, drug interactions".to_string(),
        ),
        rules: vec![
            Rule {
                name: "allow_healthcare_ops".to_string(),
                description: Some("Allow standard healthcare operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "analyze".into(),
                    "generate".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "calendar".into(),
                    "files".into(),
                    "browser".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard healthcare operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send".to_string(),
                description: Some("Require confirmation before sending".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Patient communications need confirmation".to_string()),
            },
            Rule {
                name: "block_destructive".to_string(),
                description: Some("Block destructive and control operations".to_string()),
                trigger_actions: vec!["delete".into(), "control".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Block,
                reason: Some("Patient safety requires blocking destructive actions".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Engineering assistant — inspections, maintenance, project tracking
pub fn engineering_constitution() -> Constitution {
    Constitution {
        name: "engineering".to_string(),
        version: "1.0.0".to_string(),
        description: Some(
            "Engineering assistant — inspections, maintenance, project tracking".to_string(),
        ),
        rules: vec![
            Rule {
                name: "allow_engineering_ops".to_string(),
                description: Some("Allow standard engineering operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "create".into(),
                    "analyze".into(),
                    "generate".into(),
                    "schedule".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "calendar".into(),
                    "files".into(),
                    "browser".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard engineering operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send".to_string(),
                description: Some("Require confirmation before sending".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Sending actions need confirmation".to_string()),
            },
            Rule {
                name: "confirm_delete".to_string(),
                description: Some("Require confirmation before deleting".to_string()),
                trigger_actions: vec!["delete".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Delete actions are destructive and need confirmation".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Media assistant — journalism, PR, content production
pub fn media_constitution() -> Constitution {
    Constitution {
        name: "media".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Media assistant — journalism, PR, content production".to_string()),
        rules: vec![
            Rule {
                name: "allow_media_ops".to_string(),
                description: Some("Allow standard media operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "create".into(),
                    "analyze".into(),
                    "generate".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "browser".into(),
                    "files".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard media operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send".to_string(),
                description: Some("Require confirmation before sending".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Publishing needs confirmation".to_string()),
            },
            Rule {
                name: "block_financial".to_string(),
                description: Some("Block financial operations".to_string()),
                trigger_actions: vec!["*".into()],
                trigger_targets: vec!["price".into(), "portfolio".into(), "invoice".into()],
                trigger_keywords: vec![],
                enforcement: Enforcement::Block,
                reason: Some("Media assistant cannot access financial data".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Government assistant — policy analysis, regulatory monitoring, compliance
pub fn government_constitution() -> Constitution {
    Constitution {
        name: "government".to_string(),
        version: "1.0.0".to_string(),
        description: Some(
            "Government assistant — policy analysis, regulatory monitoring, compliance".to_string(),
        ),
        rules: vec![
            Rule {
                name: "allow_government_ops".to_string(),
                description: Some("Allow standard government operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "analyze".into(),
                    "generate".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "browser".into(),
                    "files".into(),
                    "calendar".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard government operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send".to_string(),
                description: Some("Require confirmation before sending".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Government communications need confirmation".to_string()),
            },
            Rule {
                name: "block_destructive".to_string(),
                description: Some("Block destructive, control, and add operations".to_string()),
                trigger_actions: vec!["delete".into(), "control".into(), "add".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Block,
                reason: Some("Government assistant has strict action restrictions".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// NGO assistant — grant writing, donor reports, program monitoring
pub fn ngo_constitution() -> Constitution {
    Constitution {
        name: "ngo".to_string(),
        version: "1.0.0".to_string(),
        description: Some(
            "NGO assistant — grant writing, donor reports, program monitoring".to_string(),
        ),
        rules: vec![
            Rule {
                name: "allow_ngo_ops".to_string(),
                description: Some("Allow standard NGO operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "create".into(),
                    "analyze".into(),
                    "generate".into(),
                    "schedule".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "calendar".into(),
                    "files".into(),
                    "browser".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard NGO operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send".to_string(),
                description: Some("Require confirmation before sending".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Sending actions need confirmation".to_string()),
            },
            Rule {
                name: "confirm_delete".to_string(),
                description: Some("Require confirmation before deleting".to_string()),
                trigger_actions: vec!["delete".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Delete actions are destructive and need confirmation".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Logistics assistant — shipment tracking, route optimization, customs
pub fn logistics_constitution() -> Constitution {
    Constitution {
        name: "logistics".to_string(),
        version: "1.0.0".to_string(),
        description: Some(
            "Logistics assistant — shipment tracking, route optimization, customs".to_string(),
        ),
        rules: vec![
            Rule {
                name: "allow_logistics_ops".to_string(),
                description: Some("Allow standard logistics operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "create".into(),
                    "analyze".into(),
                    "generate".into(),
                    "schedule".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "calendar".into(),
                    "files".into(),
                    "browser".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard logistics operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send".to_string(),
                description: Some("Require confirmation before sending".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Sending actions need confirmation".to_string()),
            },
            Rule {
                name: "confirm_delete".to_string(),
                description: Some("Require confirmation before deleting".to_string()),
                trigger_actions: vec!["delete".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Delete actions are destructive and need confirmation".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Research assistant — literature review, data analysis, paper summaries
pub fn research_constitution() -> Constitution {
    Constitution {
        name: "research".to_string(),
        version: "1.0.0".to_string(),
        description: Some(
            "Research assistant — literature review, data analysis, paper summaries".to_string(),
        ),
        rules: vec![
            Rule {
                name: "block_financial".to_string(),
                description: Some("Block financial operations".to_string()),
                trigger_actions: vec!["*".into()],
                trigger_targets: vec!["price".into(), "portfolio".into(), "invoice".into()],
                trigger_keywords: vec![],
                enforcement: Enforcement::Block,
                reason: Some("Research assistant cannot access financial data".to_string()),
            },
            Rule {
                name: "block_destructive_actions".to_string(),
                description: Some("Block destructive and control operations".to_string()),
                trigger_actions: vec!["delete".into(), "control".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Block,
                reason: Some("Research assistant cannot perform destructive actions".to_string()),
            },
            Rule {
                name: "allow_research_ops".to_string(),
                description: Some("Allow standard research operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "analyze".into(),
                    "generate".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "browser".into(),
                    "files".into(),
                    "calendar".into(),
                    "schedule".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard research operations are allowed".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Consulting assistant — competitive analysis, due diligence, strategy
pub fn consulting_constitution() -> Constitution {
    Constitution {
        name: "consulting".to_string(),
        version: "1.0.0".to_string(),
        description: Some(
            "Consulting assistant — competitive analysis, due diligence, strategy".to_string(),
        ),
        rules: vec![
            Rule {
                name: "allow_consulting_ops".to_string(),
                description: Some("Allow standard consulting operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "create".into(),
                    "analyze".into(),
                    "generate".into(),
                    "schedule".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "calendar".into(),
                    "files".into(),
                    "browser".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard consulting operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send".to_string(),
                description: Some("Require confirmation before sending".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Sending actions need confirmation".to_string()),
            },
            Rule {
                name: "confirm_delete".to_string(),
                description: Some("Require confirmation before deleting".to_string()),
                trigger_actions: vec!["delete".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Delete actions are destructive and need confirmation".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Creative assistant — design, trends, spec sheets, content
pub fn creative_constitution() -> Constitution {
    Constitution {
        name: "creative".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Creative assistant — design, trends, spec sheets, content".to_string()),
        rules: vec![
            Rule {
                name: "allow_creative_ops".to_string(),
                description: Some("Allow standard creative operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "create".into(),
                    "analyze".into(),
                    "generate".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "browser".into(),
                    "files".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard creative operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send".to_string(),
                description: Some("Require confirmation before sending".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Sending actions need confirmation".to_string()),
            },
            Rule {
                name: "block_financial".to_string(),
                description: Some("Block financial operations".to_string()),
                trigger_actions: vec!["*".into()],
                trigger_targets: vec!["price".into(), "portfolio".into(), "invoice".into()],
                trigger_keywords: vec![],
                enforcement: Enforcement::Block,
                reason: Some("Creative assistant cannot access financial data".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Agriculture assistant — crop monitoring, market prices, weather
pub fn agriculture_constitution() -> Constitution {
    Constitution {
        name: "agriculture".to_string(),
        version: "1.0.0".to_string(),
        description: Some(
            "Agriculture assistant — crop monitoring, market prices, weather".to_string(),
        ),
        rules: vec![
            Rule {
                name: "allow_agriculture_ops".to_string(),
                description: Some("Allow standard agriculture operations".to_string()),
                trigger_actions: vec![
                    "check".into(),
                    "search".into(),
                    "analyze".into(),
                    "generate".into(),
                    "nlp".into(),
                    "data".into(),
                    "storage".into(),
                    "docs".into(),
                    "memory".into(),
                    "flow".into(),
                    "notify".into(),
                    "calendar".into(),
                    "files".into(),
                    "browser".into(),
                    "trading".into(),
                ],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Allow,
                reason: Some("Standard agriculture operations are allowed".to_string()),
            },
            Rule {
                name: "confirm_send".to_string(),
                description: Some("Require confirmation before sending".to_string()),
                trigger_actions: vec!["send".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Sending actions need confirmation".to_string()),
            },
            Rule {
                name: "confirm_delete".to_string(),
                description: Some("Require confirmation before deleting".to_string()),
                trigger_actions: vec!["delete".into()],
                trigger_targets: vec![],
                trigger_keywords: vec![],
                enforcement: Enforcement::Confirm,
                reason: Some("Delete actions are destructive and need confirmation".to_string()),
            },
        ],
        default_enforcement: Enforcement::Block,
        channel_permissions: None,
        browser_stealth: None,
        swarm_config: None,
        ollama_config: None,
        captcha_solver: None,
        signature: None,
        public_key: None,
    }
}

/// Look up a constitution template by name.
/// Returns `None` if the name is not recognized.
pub fn get_constitution_template(name: &str) -> Option<Constitution> {
    match name {
        "default" => Some(default_constitution()),
        "solopreneur" => Some(solopreneur_constitution()),
        "freelancer" => Some(freelancer_constitution()),
        "digital-marketer" | "marketer" => Some(marketer_constitution()),
        "student" => Some(student_constitution()),
        "sales" => Some(sales_constitution()),
        "customer-support" | "support" => Some(support_constitution()),
        "legal" => Some(legal_constitution()),
        "ecommerce" | "e-commerce" => Some(ecommerce_constitution()),
        "hr" | "human-resources" => Some(hr_constitution()),
        "finance" | "accounting" => Some(finance_constitution()),
        "healthcare" | "medical" => Some(healthcare_constitution()),
        "engineering" | "construction" => Some(engineering_constitution()),
        "media" | "journalism" => Some(media_constitution()),
        "government" | "public-sector" => Some(government_constitution()),
        "ngo" | "nonprofit" => Some(ngo_constitution()),
        "logistics" | "supply-chain" => Some(logistics_constitution()),
        "research" | "academic" => Some(research_constitution()),
        "consulting" | "advisory" => Some(consulting_constitution()),
        "creative" | "design" => Some(creative_constitution()),
        "agriculture" | "farming" => Some(agriculture_constitution()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::w5h2::types::{Action, Target};
    use std::collections::HashMap;

    fn make_intent(action: Action, target: Target) -> W5H2Intent {
        W5H2Intent {
            action,
            target,
            confidence: 0.95,
            params: HashMap::new(),
        }
    }

    #[test]
    fn test_default_constitution_allows_checks() {
        let enforcer = ConstitutionEnforcer::from_constitution(default_constitution());

        let intent = make_intent(Action::Check, Target::Email);
        let result = enforcer.check(&intent, Some("check my email"));
        assert!(result.allowed);
        assert_eq!(result.enforcement, Enforcement::Allow);
    }

    #[test]
    fn test_default_constitution_confirms_sends() {
        let enforcer = ConstitutionEnforcer::from_constitution(default_constitution());

        let intent = make_intent(Action::Send, Target::Email);
        let result = enforcer.check(&intent, Some("send email to bob"));
        // Confirm enforcement is treated as allowed (user confirms in TUI)
        assert!(result.allowed);
        assert_eq!(result.enforcement, Enforcement::Confirm);
    }

    #[test]
    fn test_check_ability_unknown_blocked_by_default() {
        // Built-in default constitution is deny-by-default (Block)
        let enforcer = ConstitutionEnforcer::from_constitution(default_constitution());
        let result = enforcer.check_ability("unknown_unknown");
        assert!(!result.allowed, "unknown_unknown should be blocked by deny-by-default");
        assert_eq!(result.enforcement, Enforcement::Block);
    }

    #[test]
    fn test_check_ability_send_email_confirms() {
        let enforcer = ConstitutionEnforcer::from_constitution(default_constitution());
        let result = enforcer.check_ability("send_email");
        assert!(result.allowed, "send_email should be allowed (Confirm is allowed=true)");
        assert_eq!(result.enforcement, Enforcement::Confirm);
    }

    #[test]
    fn test_destructive_keywords_blocked() {
        let enforcer = ConstitutionEnforcer::from_constitution(default_constitution());

        let intent = make_intent(Action::Check, Target::Email);
        let result = enforcer.check(&intent, Some("delete all my emails"));
        assert!(!result.allowed);
        assert_eq!(result.enforcement, Enforcement::Block);
    }

    #[test]
    fn test_yaml_roundtrip() {
        let constitution = default_constitution();
        let yaml = serde_yaml::to_string(&constitution).unwrap();
        let parsed: Constitution = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.name, "default");
        assert_eq!(parsed.rules.len(), constitution.rules.len());
    }

    #[test]
    fn test_custom_constitution() {
        let constitution = Constitution {
            name: "trading-bot".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            rules: vec![
                Rule {
                    name: "allow_price_checks".to_string(),
                    description: None,
                    trigger_actions: vec!["check".to_string()],
                    trigger_targets: vec!["price".to_string()],
                    trigger_keywords: vec![],
                    enforcement: Enforcement::Allow,
                    reason: None,
                },
                Rule {
                    name: "block_non_trading".to_string(),
                    description: None,
                    trigger_actions: vec!["*".to_string()],
                    trigger_targets: vec!["email".to_string(), "calendar".to_string()],
                    trigger_keywords: vec![],
                    enforcement: Enforcement::Block,
                    reason: Some("Trading bot cannot access personal data".to_string()),
                },
            ],
            default_enforcement: Enforcement::Warn,
            channel_permissions: None,
            browser_stealth: None,
            swarm_config: None,
            ollama_config: None,
            captcha_solver: None,
            signature: None,
            public_key: None,
        };

        let enforcer = ConstitutionEnforcer::from_constitution(constitution);

        // Price check allowed
        let check = enforcer.check(&make_intent(Action::Check, Target::Price), None);
        assert!(check.allowed);

        // Email blocked
        let check = enforcer.check(&make_intent(Action::Check, Target::Email), None);
        assert!(!check.allowed);
        assert_eq!(check.enforcement, Enforcement::Block);
    }

    #[test]
    fn test_all_constitution_templates_valid() {
        let names = [
            "default",
            "solopreneur",
            "freelancer",
            "digital-marketer",
            "student",
            "sales",
            "customer-support",
            "legal",
            "ecommerce",
            "hr",
            "finance",
            "healthcare",
            "engineering",
            "media",
            "government",
            "ngo",
            "logistics",
            "research",
            "consulting",
            "creative",
            "agriculture",
        ];
        for name in &names {
            let c = get_constitution_template(name).unwrap();
            assert!(!c.rules.is_empty(), "Template {} has no rules", name);
            assert_eq!(
                c.default_enforcement,
                Enforcement::Block,
                "Template {} must be deny-by-default",
                name
            );
            let yaml = serde_yaml::to_string(&c).unwrap();
            let _parsed: Constitution = serde_yaml::from_str(&yaml).unwrap();
        }
    }

    #[test]
    fn test_get_constitution_template_unknown() {
        assert!(get_constitution_template("nonexistent").is_none());
    }

    #[test]
    fn test_student_blocks_financial() {
        let enforcer = ConstitutionEnforcer::from_constitution(student_constitution());
        let intent = make_intent(Action::Check, Target::Price);
        let result = enforcer.check(&intent, None);
        assert!(!result.allowed);
        assert_eq!(result.enforcement, Enforcement::Block);
    }

    #[test]
    fn test_support_blocks_delete() {
        let enforcer = ConstitutionEnforcer::from_constitution(support_constitution());
        let intent = make_intent(Action::Delete, Target::Email);
        let result = enforcer.check(&intent, None);
        assert!(!result.allowed);
        assert_eq!(result.enforcement, Enforcement::Block);
    }

    #[test]
    fn test_constitution_with_channel_permissions_yaml() {
        let yaml = r#"
name: test-with-channels
version: "1.0.0"
rules: []
default_enforcement: block
channel_permissions:
  default_access: none
  channels:
    telegram:
      access: restricted
      contacts:
        - "+919876543210"
      groups: []
      domains: []
      send_domains: []
      servers: []
"#;
        let constitution: Constitution = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(constitution.name, "test-with-channels");
        let perms = constitution.channel_permissions.unwrap();
        assert_eq!(
            perms.default_access,
            crate::security::channel_permissions::AccessLevel::None
        );
        assert!(perms.channels.contains_key("telegram"));
        let tg = &perms.channels["telegram"];
        assert_eq!(
            tg.access,
            crate::security::channel_permissions::AccessLevel::Restricted
        );
        assert_eq!(tg.contacts.len(), 1);
        assert_eq!(tg.contacts[0].pattern, "+919876543210");
    }

    #[test]
    fn test_constitution_without_channel_permissions() {
        let yaml = r#"
name: no-channels
version: "1.0.0"
rules: []
default_enforcement: block
"#;
        let constitution: Constitution = serde_yaml::from_str(yaml).unwrap();
        assert!(constitution.channel_permissions.is_none());
    }

    #[test]
    fn test_check_channel_access_blocks() {
        use crate::security::channel_permissions::*;
        let mut channels = std::collections::HashMap::new();
        channels.insert(
            "telegram".to_string(),
            ChannelAccess {
                access: AccessLevel::Restricted,
                contacts: vec![PermissionEntry::parse("+919876543210")],
                groups: vec![],
                domains: vec![],
                send_domains: vec![],
                servers: vec![],
                rate_limit_per_minute: None,
                cost_budget_usd: None,
            },
        );
        let perms = ChannelPermissions {
            default_access: AccessLevel::None,
            channels,
        };
        let constitution = Constitution {
            name: "test".into(),
            version: "1.0.0".into(),
            description: None,
            rules: vec![],
            default_enforcement: Enforcement::Block,
            channel_permissions: Some(perms),
            browser_stealth: None,
            swarm_config: None,
            ollama_config: None,
            captcha_solver: None,
            signature: None,
            public_key: None,
        };
        let enforcer = ConstitutionEnforcer::from_constitution(constitution);
        // Unknown contact should be blocked on restricted channel
        let result = enforcer.check_channel_access("telegram", Some("+44999999999"), None, None);
        assert!(!result.allowed);
    }

    #[test]
    fn test_check_channel_access_allows() {
        use crate::security::channel_permissions::*;
        let mut channels = std::collections::HashMap::new();
        channels.insert(
            "telegram".to_string(),
            ChannelAccess {
                access: AccessLevel::Restricted,
                contacts: vec![PermissionEntry::parse("+919876543210")],
                groups: vec![],
                domains: vec![],
                send_domains: vec![],
                servers: vec![],
                rate_limit_per_minute: None,
                cost_budget_usd: None,
            },
        );
        let perms = ChannelPermissions {
            default_access: AccessLevel::None,
            channels,
        };
        let constitution = Constitution {
            name: "test".into(),
            version: "1.0.0".into(),
            description: None,
            rules: vec![],
            default_enforcement: Enforcement::Block,
            channel_permissions: Some(perms),
            browser_stealth: None,
            swarm_config: None,
            ollama_config: None,
            captcha_solver: None,
            signature: None,
            public_key: None,
        };
        let enforcer = ConstitutionEnforcer::from_constitution(constitution);
        // Authorized contact should be allowed
        let result = enforcer.check_channel_access("telegram", Some("+919876543210"), None, None);
        assert!(result.allowed);
    }

    #[test]
    fn test_constitution_with_stealth_config() {
        let yaml = r#"
name: stealth-test
version: "1.0.0"
rules: []
default_enforcement: block
browser_stealth:
  enabled: true
  canvas_noise: true
  webgl_spoof: false
  max_concurrent_tabs: 8
"#;
        let constitution: Constitution = serde_yaml::from_str(yaml).unwrap();
        let stealth = constitution.browser_stealth.unwrap();
        assert!(stealth.enabled);
        assert!(stealth.canvas_noise);
        assert!(!stealth.webgl_spoof);
        assert_eq!(stealth.max_concurrent_tabs, 8);
    }

    #[test]
    fn test_swarm_config_defaults() {
        let config = SwarmConstitutionConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.max_workers, 0);
        assert!(config.allowed_worker_types.is_empty());
        assert_eq!(config.max_result_chars, 0);

        // Verify a constitution with swarm_config parses
        let yaml = r#"
name: swarm-test
version: "1.0.0"
rules: []
default_enforcement: block
swarm_config:
  enabled: true
  max_workers: 10
  allowed_worker_types: ["search", "academic", "web_crawl"]
  max_result_chars: 100000
"#;
        let constitution: Constitution = serde_yaml::from_str(yaml).unwrap();
        let sc = constitution.swarm_config.unwrap();
        assert!(sc.enabled);
        assert_eq!(sc.max_workers, 10);
        assert_eq!(sc.allowed_worker_types.len(), 3);
        assert_eq!(sc.max_result_chars, 100_000);
    }

    #[test]
    fn test_check_channel_access_no_config() {
        let constitution = Constitution {
            name: "test".into(),
            version: "1.0.0".into(),
            description: None,
            rules: vec![],
            default_enforcement: Enforcement::Block,
            channel_permissions: None,
            browser_stealth: None,
            swarm_config: None,
            ollama_config: None,
            captcha_solver: None,
            signature: None,
            public_key: None,
        };
        let enforcer = ConstitutionEnforcer::from_constitution(constitution);
        // No channel_permissions configured — should allow everything
        let result = enforcer.check_channel_access("telegram", Some("+44999999999"), None, None);
        assert!(result.allowed);
        assert_eq!(result.reason, "no channel_permissions configured");
    }

    #[test]
    fn test_ollama_config_serde() {
        let yaml = r#"
name: test
version: "1.0"
rules: []
ollama_config:
  enabled: true
  base_url: "http://localhost:11434"
  default_model: "llama3.2"
  vision_model: "llava"
"#;
        let c: Constitution = serde_yaml::from_str(yaml).unwrap();
        let ollama = c.ollama_config.unwrap();
        assert!(ollama.enabled);
        assert_eq!(ollama.default_model.as_deref(), Some("llama3.2"));
        assert_eq!(ollama.vision_model.as_deref(), Some("llava"));
    }

    #[test]
    fn test_constitution_without_ollama_still_parses() {
        let yaml = r#"
name: test
version: "1.0"
rules: []
"#;
        let c: Constitution = serde_yaml::from_str(yaml).unwrap();
        assert!(c.ollama_config.is_none());
    }

    // -----------------------------------------------------------------------
    // Ed25519 signing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_sign_verify_roundtrip() {
        let (sk, vk) = ConstitutionSigner::generate_keypair();
        let content = b"name: test\nversion: 1.0\nrules: []";
        let sig = ConstitutionSigner::sign(content, &sk);
        assert!(ConstitutionSigner::verify(content, &sig, &vk));
    }

    #[test]
    fn test_tamper_detection() {
        let (sk, vk) = ConstitutionSigner::generate_keypair();
        let content = b"name: test\nversion: 1.0\nrules: []";
        let sig = ConstitutionSigner::sign(content, &sk);
        let tampered = b"name: test\nversion: 1.0\nrules: [EVIL]";
        assert!(!ConstitutionSigner::verify(tampered, &sig, &vk));
    }

    #[test]
    fn test_unsigned_backward_compat() {
        // A constitution without signature/public_key should parse fine
        let yaml = r#"
name: unsigned
version: "1.0"
rules: []
"#;
        let c: Constitution = serde_yaml::from_str(yaml).unwrap();
        assert!(c.signature.is_none());
        assert!(c.public_key.is_none());
    }

    #[test]
    fn test_sign_file_produces_valid_signature() {
        let (sk, _vk) = ConstitutionSigner::generate_keypair();
        let yaml = "name: test\nversion: '1.0'\nrules: []\n";
        let (signable, sig_hex, pk_hex) = ConstitutionSigner::sign_file(yaml, &sk);
        assert!(ConstitutionSigner::verify_hex(&signable, &sig_hex, &pk_hex));
    }

    #[test]
    fn test_strip_signature_fields() {
        let yaml = "name: test\nsignature: abc123\npublic_key: def456\nrules: []\n";
        let stripped = ConstitutionSigner::strip_signature_fields(yaml);
        assert!(!stripped.contains("signature:"));
        assert!(!stripped.contains("public_key:"));
        assert!(stripped.contains("name: test"));
        assert!(stripped.contains("rules: []"));
    }
}
