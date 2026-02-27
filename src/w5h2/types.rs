use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// The action component of a W5H2 intent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Check,
    Send,
    Set,
    Control,
    Add,
    Search,
    Create,
    Delete,
    Analyze,
    Schedule,
    Generate,
}

/// The target component of a W5H2 intent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Email,
    Weather,
    Calendar,
    Lights,
    Shopping,
    Reminder,
    Price,
    Document,
    Code,
    Task,
    Contact,
    Invoice,
    Ticket,
    Course,
    Property,
    Health,
    Contract,
    Inventory,
    Portfolio,
    Shipment,
    Compliance,
    Campaign,
    Media,
    Grant,
    Asset,
    Vendor,
    Policy,
    Permit,
    Budget,
    Crop,
}

/// A classified W5H2 intent with action, target, and extracted parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct W5H2Intent {
    pub action: Action,
    pub target: Target,
    pub confidence: f32,
    pub params: HashMap<String, String>,
}

impl W5H2Intent {
    pub fn key(&self) -> IntentKey {
        IntentKey::from_action_target(self.action, self.target)
    }
}

impl fmt::Display for W5H2Intent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}_{} (confidence: {:.1}%)",
            self.action,
            self.target,
            self.confidence * 100.0
        )
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Check => write!(f, "check"),
            Action::Send => write!(f, "send"),
            Action::Set => write!(f, "set"),
            Action::Control => write!(f, "control"),
            Action::Add => write!(f, "add"),
            Action::Search => write!(f, "search"),
            Action::Create => write!(f, "create"),
            Action::Delete => write!(f, "delete"),
            Action::Analyze => write!(f, "analyze"),
            Action::Schedule => write!(f, "schedule"),
            Action::Generate => write!(f, "generate"),
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Email => write!(f, "email"),
            Target::Weather => write!(f, "weather"),
            Target::Calendar => write!(f, "calendar"),
            Target::Lights => write!(f, "lights"),
            Target::Shopping => write!(f, "shopping"),
            Target::Reminder => write!(f, "reminder"),
            Target::Price => write!(f, "price"),
            Target::Document => write!(f, "document"),
            Target::Code => write!(f, "code"),
            Target::Task => write!(f, "task"),
            Target::Contact => write!(f, "contact"),
            Target::Invoice => write!(f, "invoice"),
            Target::Ticket => write!(f, "ticket"),
            Target::Course => write!(f, "course"),
            Target::Property => write!(f, "property"),
            Target::Health => write!(f, "health"),
            Target::Contract => write!(f, "contract"),
            Target::Inventory => write!(f, "inventory"),
            Target::Portfolio => write!(f, "portfolio"),
            Target::Shipment => write!(f, "shipment"),
            Target::Compliance => write!(f, "compliance"),
            Target::Campaign => write!(f, "campaign"),
            Target::Media => write!(f, "media"),
            Target::Grant => write!(f, "grant"),
            Target::Asset => write!(f, "asset"),
            Target::Vendor => write!(f, "vendor"),
            Target::Policy => write!(f, "policy"),
            Target::Permit => write!(f, "permit"),
            Target::Budget => write!(f, "budget"),
            Target::Crop => write!(f, "crop"),
        }
    }
}

/// Deterministic cache key derived from (Action, Target)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IntentKey(pub String);

impl IntentKey {
    pub fn from_action_target(action: Action, target: Target) -> Self {
        Self(format!("{}_{}", action, target))
    }

    /// Parse an IntentKey from a class label string (e.g., "check_email")
    pub fn from_label(label: &str) -> Option<Self> {
        let (action, target) = parse_label(label)?;
        Some(Self::from_action_target(action, target))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IntentKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub const W5H2_CLASSES: &[&str] = &[
    // Original 8
    "add_shopping",
    "check_calendar",
    "check_email",
    "check_price",
    "check_weather",
    "control_lights",
    "send_email",
    "set_reminder",
    // Round 1 expansion (16)
    "analyze_code",
    "analyze_document",
    "analyze_portfolio",
    "check_inventory",
    "check_task",
    "check_ticket",
    "create_document",
    "create_invoice",
    "create_task",
    "generate_course",
    "generate_document",
    "schedule_task",
    "search_contact",
    "search_contract",
    "search_property",
    "check_health",
    // Round 2 expansion - 100 user type coverage (30)
    "analyze_budget",
    "analyze_campaign",
    "analyze_compliance",
    "analyze_crop",
    "analyze_media",
    "analyze_policy",
    "analyze_shipment",
    "analyze_vendor",
    "check_asset",
    "check_budget",
    "check_compliance",
    "check_crop",
    "check_grant",
    "check_media",
    "check_permit",
    "check_policy",
    "check_shipment",
    "check_vendor",
    "create_campaign",
    "create_grant",
    "create_permit",
    "create_policy",
    "generate_budget",
    "generate_compliance",
    "generate_media",
    "schedule_shipment",
    "search_grant",
    "search_media",
    "search_permit",
    "search_vendor",
];

/// Parse a label string into (Action, Target)
pub fn parse_label(label: &str) -> Option<(Action, Target)> {
    let parts: Vec<&str> = label.splitn(2, '_').collect();
    if parts.len() != 2 {
        return None;
    }

    let action = match parts[0] {
        "check" => Action::Check,
        "send" => Action::Send,
        "set" => Action::Set,
        "control" => Action::Control,
        "add" => Action::Add,
        "search" => Action::Search,
        "create" => Action::Create,
        "delete" => Action::Delete,
        "analyze" => Action::Analyze,
        "schedule" => Action::Schedule,
        "generate" => Action::Generate,
        _ => return None,
    };

    let target = match parts[1] {
        "email" => Target::Email,
        "weather" => Target::Weather,
        "calendar" => Target::Calendar,
        "lights" => Target::Lights,
        "shopping" => Target::Shopping,
        "reminder" => Target::Reminder,
        "price" => Target::Price,
        "document" => Target::Document,
        "code" => Target::Code,
        "task" => Target::Task,
        "contact" => Target::Contact,
        "invoice" => Target::Invoice,
        "ticket" => Target::Ticket,
        "course" => Target::Course,
        "property" => Target::Property,
        "health" => Target::Health,
        "contract" => Target::Contract,
        "inventory" => Target::Inventory,
        "portfolio" => Target::Portfolio,
        "shipment" => Target::Shipment,
        "compliance" => Target::Compliance,
        "campaign" => Target::Campaign,
        "media" => Target::Media,
        "grant" => Target::Grant,
        "asset" => Target::Asset,
        "vendor" => Target::Vendor,
        "policy" => Target::Policy,
        "permit" => Target::Permit,
        "budget" => Target::Budget,
        "crop" => Target::Crop,
        _ => return None,
    };

    Some((action, target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intent_key_roundtrip() {
        let key = IntentKey::from_action_target(Action::Check, Target::Email);
        assert_eq!(key.as_str(), "check_email");

        let parsed = IntentKey::from_label("check_email").unwrap();
        assert_eq!(key, parsed);
    }

    #[test]
    fn test_all_classes_parseable() {
        for cls in W5H2_CLASSES {
            assert!(
                parse_label(cls).is_some(),
                "Failed to parse W5H2 class: {}",
                cls
            );
        }
    }

    #[test]
    fn test_invalid_label() {
        assert!(parse_label("invalid").is_none());
        assert!(parse_label("unknown_action").is_none());
    }

    #[test]
    fn test_intent_display() {
        let intent = W5H2Intent {
            action: Action::Check,
            target: Target::Email,
            confidence: 0.95,
            params: HashMap::new(),
        };
        assert_eq!(format!("{}", intent), "check_email (confidence: 95.0%)");
    }
}
