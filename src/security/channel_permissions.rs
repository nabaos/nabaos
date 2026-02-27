//! Fine-grained channel permissions with per-contact/group/domain access control.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// Access level for a channel or default.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum AccessLevel {
    Full,
    Restricted,
    #[default]
    None,
}

impl AccessLevel {
    /// Returns a numeric rank for an access level (None=0, Restricted=1, Full=2).
    pub fn rank(&self) -> u8 {
        match self {
            AccessLevel::None => 0,
            AccessLevel::Restricted => 1,
            AccessLevel::Full => 2,
        }
    }
}

/// A single permission entry parsed from a string like `"foo"` or `"-foo"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionEntry {
    pub pattern: String,
    pub excluded: bool,
}

impl PermissionEntry {
    /// Parse a raw string. A leading `-` means excluded (only if non-empty pattern follows).
    pub fn parse(raw: &str) -> Self {
        if let Some(rest) = raw.strip_prefix('-') {
            if rest.is_empty() {
                // Bare "-" is treated as a literal pattern, not an exclude marker
                PermissionEntry {
                    pattern: raw.to_string(),
                    excluded: false,
                }
            } else {
                PermissionEntry {
                    pattern: rest.to_string(),
                    excluded: true,
                }
            }
        } else {
            PermissionEntry {
                pattern: raw.to_string(),
                excluded: false,
            }
        }
    }

    /// Convert back to the raw string form (prepends `-` if excluded).
    pub fn to_raw(&self) -> String {
        if self.excluded {
            format!("-{}", self.pattern)
        } else {
            self.pattern.clone()
        }
    }
}

/// Custom deserializer that takes a list of strings and parses each through `PermissionEntry::parse`.
fn deserialize_permission_entries<'de, D>(deserializer: D) -> Result<Vec<PermissionEntry>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw_strings: Vec<String> = Vec::deserialize(deserializer)?;
    Ok(raw_strings
        .iter()
        .map(|s| PermissionEntry::parse(s))
        .collect())
}

/// Serialize permission entries back to raw strings.
fn serialize_permission_entries<S>(
    entries: &Vec<PermissionEntry>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(Some(entries.len()))?;
    for entry in entries {
        seq.serialize_element(&entry.to_raw())?;
    }
    seq.end()
}

/// Access configuration for a single channel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelAccess {
    #[serde(default)]
    pub access: AccessLevel,
    #[serde(
        default,
        deserialize_with = "deserialize_permission_entries",
        serialize_with = "serialize_permission_entries"
    )]
    pub contacts: Vec<PermissionEntry>,
    #[serde(
        default,
        deserialize_with = "deserialize_permission_entries",
        serialize_with = "serialize_permission_entries"
    )]
    pub groups: Vec<PermissionEntry>,
    #[serde(
        default,
        deserialize_with = "deserialize_permission_entries",
        serialize_with = "serialize_permission_entries"
    )]
    pub domains: Vec<PermissionEntry>,
    #[serde(
        default,
        deserialize_with = "deserialize_permission_entries",
        serialize_with = "serialize_permission_entries"
    )]
    pub send_domains: Vec<PermissionEntry>,
    #[serde(
        default,
        deserialize_with = "deserialize_permission_entries",
        serialize_with = "serialize_permission_entries"
    )]
    pub servers: Vec<PermissionEntry>,
}

/// Top-level channel permissions configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelPermissions {
    #[serde(default)]
    pub default_access: AccessLevel,
    #[serde(default)]
    pub channels: HashMap<String, ChannelAccess>,
}

impl Default for ChannelPermissions {
    fn default() -> Self {
        ChannelPermissions {
            default_access: AccessLevel::None,
            channels: HashMap::new(),
        }
    }
}

/// Result of an access check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelAccessCheck {
    pub allowed: bool,
    pub reason: String,
}

impl ChannelPermissions {
    /// Check whether access is allowed on a channel for a given contact/group/domain.
    pub fn check_access(
        &self,
        channel: &str,
        contact: Option<&str>,
        group: Option<&str>,
        domain: Option<&str>,
    ) -> ChannelAccessCheck {
        let channel_access = self.channels.get(channel);
        let access_level = channel_access
            .map(|ca| &ca.access)
            .unwrap_or(&self.default_access);

        match access_level {
            AccessLevel::Full => ChannelAccessCheck {
                allowed: true,
                reason: "full access".to_string(),
            },
            AccessLevel::None => ChannelAccessCheck {
                allowed: false,
                reason: "no access".to_string(),
            },
            AccessLevel::Restricted => {
                let ca = match channel_access {
                    Some(ca) => ca,
                    None => {
                        return ChannelAccessCheck {
                            allowed: false,
                            reason: "restricted but no channel config".to_string(),
                        }
                    }
                };

                // Check contacts
                if let Some(c) = contact {
                    if let Some(result) = Self::check_entries(&ca.contacts, c) {
                        return result;
                    }
                }
                // Check groups
                if let Some(g) = group {
                    if let Some(result) = Self::check_entries(&ca.groups, g) {
                        return result;
                    }
                }
                // Check domains
                if let Some(d) = domain {
                    if let Some(result) = Self::check_entries(&ca.domains, d) {
                        return result;
                    }
                }

                // No match = deny
                ChannelAccessCheck {
                    allowed: false,
                    reason: "restricted: no matching entry".to_string(),
                }
            }
        }
    }

    /// Check send access. Uses `send_domains` if present, otherwise falls back to `domains`.
    pub fn check_send_access(
        &self,
        channel: &str,
        domain: Option<&str>,
        contact: Option<&str>,
    ) -> ChannelAccessCheck {
        let channel_access = self.channels.get(channel);
        let access_level = channel_access
            .map(|ca| &ca.access)
            .unwrap_or(&self.default_access);

        match access_level {
            AccessLevel::Full => ChannelAccessCheck {
                allowed: true,
                reason: "full access".to_string(),
            },
            AccessLevel::None => ChannelAccessCheck {
                allowed: false,
                reason: "no access".to_string(),
            },
            AccessLevel::Restricted => {
                let ca = match channel_access {
                    Some(ca) => ca,
                    None => {
                        return ChannelAccessCheck {
                            allowed: false,
                            reason: "restricted but no channel config".to_string(),
                        }
                    }
                };

                // Check contact
                if let Some(c) = contact {
                    if let Some(result) = Self::check_entries(&ca.contacts, c) {
                        return result;
                    }
                }

                // Use send_domains if non-empty, otherwise fall back to domains
                if let Some(d) = domain {
                    let domain_entries = if !ca.send_domains.is_empty() {
                        &ca.send_domains
                    } else {
                        &ca.domains
                    };
                    if let Some(result) = Self::check_entries(domain_entries, d) {
                        return result;
                    }
                }

                ChannelAccessCheck {
                    allowed: false,
                    reason: "restricted: no matching send entry".to_string(),
                }
            }
        }
    }

    /// Merge with another set of permissions, taking the most restrictive.
    /// Agent narrows constitution. Exclude entries are unioned (more restrictive).
    pub fn narrow_with(&self, other: &ChannelPermissions) -> ChannelPermissions {
        let default_access = if other.default_access.rank() < self.default_access.rank() {
            other.default_access.clone()
        } else {
            self.default_access.clone()
        };

        let mut channels = self.channels.clone();
        for (name, other_ca) in &other.channels {
            let merged = if let Some(self_ca) = channels.get(name) {
                // Take more restrictive access level
                let access = if other_ca.access.rank() < self_ca.access.rank() {
                    other_ca.access.clone()
                } else {
                    self_ca.access.clone()
                };
                // Union exclude entries from both sides (more restrictive)
                let contacts = Self::union_excludes(&self_ca.contacts, &other_ca.contacts);
                let groups = Self::union_excludes(&self_ca.groups, &other_ca.groups);
                let domains = Self::union_excludes(&self_ca.domains, &other_ca.domains);
                let send_domains =
                    Self::union_excludes(&self_ca.send_domains, &other_ca.send_domains);
                let servers = Self::union_excludes(&self_ca.servers, &other_ca.servers);
                ChannelAccess {
                    access,
                    contacts,
                    groups,
                    domains,
                    send_domains,
                    servers,
                }
            } else {
                other_ca.clone()
            };
            channels.insert(name.clone(), merged);
        }

        ChannelPermissions {
            default_access,
            channels,
        }
    }

    /// Union exclude entries from `other` into `base`, keeping base's include entries
    /// and adding any exclude entries from `other` that are not already present.
    fn union_excludes(base: &[PermissionEntry], other: &[PermissionEntry]) -> Vec<PermissionEntry> {
        let mut result: Vec<PermissionEntry> = base.to_vec();
        for entry in other {
            if entry.excluded {
                let already_present = result.iter().any(|e| {
                    e.excluded && e.pattern.to_lowercase() == entry.pattern.to_lowercase()
                });
                if !already_present {
                    result.push(entry.clone());
                }
            }
        }
        result
    }

    /// Workflow can widen self but not exceed ceiling.
    pub fn widen_within(
        &self,
        ceiling: &ChannelPermissions,
        workflow_perms: &ChannelPermissions,
    ) -> ChannelPermissions {
        // Start from self, widen toward workflow_perms, capped by ceiling
        let default_access = {
            let widened_rank = self
                .default_access
                .rank()
                .max(workflow_perms.default_access.rank());
            let ceiling_rank = ceiling.default_access.rank();
            let final_rank = widened_rank.min(ceiling_rank);
            rank_to_access_level(final_rank)
        };

        let mut channels = self.channels.clone();

        // Merge in workflow channels
        for (name, wf_ca) in &workflow_perms.channels {
            let ceiling_ca = ceiling.channels.get(name);
            let ceiling_access_rank = ceiling_ca
                .map(|c| c.access.rank())
                .unwrap_or(ceiling.default_access.rank());

            let self_ca = channels.get(name);
            let self_access_rank = self_ca
                .map(|c| c.access.rank())
                .unwrap_or(self.default_access.rank());

            let widened_rank = self_access_rank.max(wf_ca.access.rank());
            let final_rank = widened_rank.min(ceiling_access_rank);
            let access = rank_to_access_level(final_rank);

            let base = self_ca.cloned().unwrap_or(ChannelAccess {
                access: access.clone(),
                contacts: vec![],
                groups: vec![],
                domains: vec![],
                send_domains: vec![],
                servers: vec![],
            });

            channels.insert(
                name.clone(),
                ChannelAccess {
                    access,
                    contacts: if base.contacts.is_empty() {
                        wf_ca.contacts.clone()
                    } else {
                        base.contacts
                    },
                    groups: if base.groups.is_empty() {
                        wf_ca.groups.clone()
                    } else {
                        base.groups
                    },
                    domains: if base.domains.is_empty() {
                        wf_ca.domains.clone()
                    } else {
                        base.domains
                    },
                    send_domains: if base.send_domains.is_empty() {
                        wf_ca.send_domains.clone()
                    } else {
                        base.send_domains
                    },
                    servers: if base.servers.is_empty() {
                        wf_ca.servers.clone()
                    } else {
                        base.servers
                    },
                },
            );
        }

        ChannelPermissions {
            default_access,
            channels,
        }
    }

    /// Check entries against a value. Excludes checked first (deny wins), then includes.
    /// Uses exact case-insensitive equality matching (safe default for permissions).
    fn check_entries(entries: &[PermissionEntry], value: &str) -> Option<ChannelAccessCheck> {
        let value_lower = value.to_lowercase();

        // Check excludes first — deny wins
        for entry in entries {
            if entry.excluded && value_lower == entry.pattern.to_lowercase() {
                return Some(ChannelAccessCheck {
                    allowed: false,
                    reason: format!("excluded by pattern: {}", entry.pattern),
                });
            }
        }

        // Then check includes
        for entry in entries {
            if !entry.excluded && value_lower == entry.pattern.to_lowercase() {
                return Some(ChannelAccessCheck {
                    allowed: true,
                    reason: format!("allowed by pattern: {}", entry.pattern),
                });
            }
        }

        None
    }
}

fn rank_to_access_level(rank: u8) -> AccessLevel {
    match rank {
        0 => AccessLevel::None,
        1 => AccessLevel::Restricted,
        _ => AccessLevel::Full,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_entry_parse() {
        let excluded = PermissionEntry::parse("-foo");
        assert!(excluded.excluded);
        assert_eq!(excluded.pattern, "foo");

        let included = PermissionEntry::parse("foo");
        assert!(!included.excluded);
        assert_eq!(included.pattern, "foo");
    }

    #[test]
    fn test_permission_entry_to_raw() {
        let entry = PermissionEntry::parse("-bar");
        assert_eq!(entry.to_raw(), "-bar");

        let entry2 = PermissionEntry::parse("baz");
        assert_eq!(entry2.to_raw(), "baz");

        // Roundtrip
        let raw = "-hello";
        assert_eq!(PermissionEntry::parse(raw).to_raw(), raw);
        let raw2 = "world";
        assert_eq!(PermissionEntry::parse(raw2).to_raw(), raw2);
    }

    #[test]
    fn test_check_access_full() {
        let perms = ChannelPermissions {
            default_access: AccessLevel::Full,
            channels: HashMap::new(),
        };
        let result = perms.check_access("telegram", None, None, None);
        assert!(result.allowed);
    }

    #[test]
    fn test_check_access_none() {
        let mut channels = HashMap::new();
        channels.insert(
            "telegram".to_string(),
            ChannelAccess {
                access: AccessLevel::None,
                contacts: vec![],
                groups: vec![],
                domains: vec![],
                send_domains: vec![],
                servers: vec![],
            },
        );
        let perms = ChannelPermissions {
            default_access: AccessLevel::Full,
            channels,
        };
        let result = perms.check_access("telegram", None, None, None);
        assert!(!result.allowed);
    }

    #[test]
    fn test_check_access_restricted_allow() {
        let mut channels = HashMap::new();
        channels.insert(
            "telegram".to_string(),
            ChannelAccess {
                access: AccessLevel::Restricted,
                contacts: vec![PermissionEntry::parse("+919876543210")],
                groups: vec![],
                domains: vec![],
                send_domains: vec![],
                servers: vec![],
            },
        );
        let perms = ChannelPermissions {
            default_access: AccessLevel::None,
            channels,
        };
        let result = perms.check_access("telegram", Some("+919876543210"), None, None);
        assert!(result.allowed);
    }

    #[test]
    fn test_check_access_restricted_exclude() {
        let mut channels = HashMap::new();
        channels.insert(
            "telegram".to_string(),
            ChannelAccess {
                access: AccessLevel::Restricted,
                contacts: vec![PermissionEntry {
                    pattern: "+919876543210".to_string(),
                    excluded: true,
                }],
                groups: vec![],
                domains: vec![],
                send_domains: vec![],
                servers: vec![],
            },
        );
        let perms = ChannelPermissions {
            default_access: AccessLevel::Full,
            channels,
        };
        let result = perms.check_access("telegram", Some("+919876543210"), None, None);
        assert!(!result.allowed);
    }

    #[test]
    fn test_check_access_exclude_wins() {
        let mut channels = HashMap::new();
        channels.insert(
            "telegram".to_string(),
            ChannelAccess {
                access: AccessLevel::Restricted,
                contacts: vec![
                    PermissionEntry::parse("+919876543210"), // include exact number
                    PermissionEntry::parse("-+919876543210"), // exclude same number
                ],
                groups: vec![],
                domains: vec![],
                send_domains: vec![],
                servers: vec![],
            },
        );
        let perms = ChannelPermissions {
            default_access: AccessLevel::None,
            channels,
        };
        // Exact match on both include and exclude — deny wins
        let result = perms.check_access("telegram", Some("+919876543210"), None, None);
        assert!(!result.allowed);
    }

    #[test]
    fn test_check_access_no_match_denied() {
        let mut channels = HashMap::new();
        channels.insert(
            "telegram".to_string(),
            ChannelAccess {
                access: AccessLevel::Restricted,
                contacts: vec![PermissionEntry::parse("+919876543210")],
                groups: vec![],
                domains: vec![],
                send_domains: vec![],
                servers: vec![],
            },
        );
        let perms = ChannelPermissions {
            default_access: AccessLevel::Full,
            channels,
        };
        // Different number doesn't match any entry — denied
        let result = perms.check_access("telegram", Some("+44123456789"), None, None);
        assert!(!result.allowed);
    }

    #[test]
    fn test_exact_match_prevents_subdomain_spoofing() {
        let mut channels = HashMap::new();
        channels.insert(
            "email".to_string(),
            ChannelAccess {
                access: AccessLevel::Restricted,
                contacts: vec![],
                groups: vec![],
                domains: vec![PermissionEntry::parse("example.com")],
                send_domains: vec![],
                servers: vec![],
            },
        );
        let perms = ChannelPermissions {
            default_access: AccessLevel::None,
            channels,
        };
        // Exact match works
        let result = perms.check_access("email", None, None, Some("example.com"));
        assert!(result.allowed);
        // Substring "evil-example.com" must NOT match
        let result2 = perms.check_access("email", None, None, Some("evil-example.com"));
        assert!(!result2.allowed);
    }

    #[test]
    fn test_bare_dash_is_literal() {
        let entry = PermissionEntry::parse("-");
        assert!(!entry.excluded);
        assert_eq!(entry.pattern, "-");
    }

    #[test]
    fn test_default_access_level_is_none() {
        assert_eq!(AccessLevel::default(), AccessLevel::None);
        let perms = ChannelPermissions::default();
        assert_eq!(perms.default_access, AccessLevel::None);
        // Default should deny
        let result = perms.check_access("telegram", None, None, None);
        assert!(!result.allowed);
    }

    #[test]
    fn test_check_send_access_send_domains() {
        let mut channels = HashMap::new();
        channels.insert(
            "email".to_string(),
            ChannelAccess {
                access: AccessLevel::Restricted,
                contacts: vec![],
                groups: vec![],
                domains: vec![PermissionEntry::parse("example.com")],
                send_domains: vec![PermissionEntry::parse("send.example.com")],
                servers: vec![],
            },
        );
        let perms = ChannelPermissions {
            default_access: AccessLevel::None,
            channels,
        };
        // send_domains should be used, not domains
        let result = perms.check_send_access("email", Some("send.example.com"), None);
        assert!(result.allowed);

        // domains entry should NOT match for send
        let result2 = perms.check_send_access("email", Some("example.com"), None);
        assert!(!result2.allowed);
    }

    #[test]
    fn test_narrow_with() {
        let constitution = ChannelPermissions {
            default_access: AccessLevel::Full,
            channels: HashMap::new(),
        };
        let mut agent_channels = HashMap::new();
        agent_channels.insert(
            "telegram".to_string(),
            ChannelAccess {
                access: AccessLevel::Restricted,
                contacts: vec![PermissionEntry::parse("+919876543210")],
                groups: vec![],
                domains: vec![],
                send_domains: vec![],
                servers: vec![],
            },
        );
        let agent = ChannelPermissions {
            default_access: AccessLevel::Restricted,
            channels: agent_channels,
        };

        let result = constitution.narrow_with(&agent);
        // Agent narrows: Restricted < Full, so Restricted wins
        assert_eq!(result.default_access, AccessLevel::Restricted);
        assert!(result.channels.contains_key("telegram"));
        assert_eq!(result.channels["telegram"].access, AccessLevel::Restricted);
    }

    #[test]
    fn test_narrow_with_unions_excludes() {
        let mut const_channels = HashMap::new();
        const_channels.insert(
            "telegram".to_string(),
            ChannelAccess {
                access: AccessLevel::Restricted,
                contacts: vec![
                    PermissionEntry::parse("+919876543210"),
                    PermissionEntry::parse("-+911111111111"),
                ],
                groups: vec![],
                domains: vec![],
                send_domains: vec![],
                servers: vec![],
            },
        );
        let constitution = ChannelPermissions {
            default_access: AccessLevel::Full,
            channels: const_channels,
        };

        let mut agent_channels = HashMap::new();
        agent_channels.insert(
            "telegram".to_string(),
            ChannelAccess {
                access: AccessLevel::Restricted,
                contacts: vec![PermissionEntry::parse("-+912222222222")],
                groups: vec![],
                domains: vec![],
                send_domains: vec![],
                servers: vec![],
            },
        );
        let agent = ChannelPermissions {
            default_access: AccessLevel::Full,
            channels: agent_channels,
        };

        let result = constitution.narrow_with(&agent);
        let tg = &result.channels["telegram"];
        // Should have: original include + both excludes (union)
        let exclude_patterns: Vec<&str> = tg
            .contacts
            .iter()
            .filter(|e| e.excluded)
            .map(|e| e.pattern.as_str())
            .collect();
        assert!(exclude_patterns.contains(&"+911111111111"));
        assert!(exclude_patterns.contains(&"+912222222222"));
    }

    #[test]
    fn test_widen_within_ceiling() {
        // Self is restrictive
        let current = ChannelPermissions {
            default_access: AccessLevel::None,
            channels: HashMap::new(),
        };
        // Ceiling allows full
        let ceiling = ChannelPermissions {
            default_access: AccessLevel::Full,
            channels: HashMap::new(),
        };
        // Workflow wants restricted
        let workflow = ChannelPermissions {
            default_access: AccessLevel::Restricted,
            channels: HashMap::new(),
        };

        let result = current.widen_within(&ceiling, &workflow);
        // Widened from None to Restricted (workflow), which is within Full ceiling
        assert_eq!(result.default_access, AccessLevel::Restricted);

        // Now test ceiling cap: workflow wants Full but ceiling is Restricted
        let ceiling2 = ChannelPermissions {
            default_access: AccessLevel::Restricted,
            channels: HashMap::new(),
        };
        let workflow2 = ChannelPermissions {
            default_access: AccessLevel::Full,
            channels: HashMap::new(),
        };
        let result2 = current.widen_within(&ceiling2, &workflow2);
        // Capped at Restricted (ceiling)
        assert_eq!(result2.default_access, AccessLevel::Restricted);
    }

    #[test]
    fn test_yaml_roundtrip() {
        let yaml = r#"
default_access: restricted
channels:
  telegram:
    access: restricted
    contacts:
      - "+919876543210"
      - "-+919999000000"
    groups: []
    domains: []
    send_domains: []
    servers: []
"#;

        let perms: ChannelPermissions = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(perms.default_access, AccessLevel::Restricted);
        let tg = &perms.channels["telegram"];
        assert_eq!(tg.contacts.len(), 2);
        assert!(!tg.contacts[0].excluded);
        assert_eq!(tg.contacts[0].pattern, "+919876543210");
        assert!(tg.contacts[1].excluded);
        assert_eq!(tg.contacts[1].pattern, "+919999000000");

        // Roundtrip: serialize and deserialize again
        let serialized = serde_yaml::to_string(&perms).unwrap();
        let perms2: ChannelPermissions = serde_yaml::from_str(&serialized).unwrap();
        assert_eq!(perms, perms2);
    }
}
