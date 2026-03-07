//! Interactive confirmation channel for sensitive actions.
//!
//! When the constitution, circuit breaker, or privilege guard flags an action
//! as requiring user confirmation, a `ConfirmationRequest` is sent through the
//! TUI's `AppMessage` channel. The TUI renders a modal and sends back a
//! `ConfirmationResponse` via a oneshot-style `mpsc::Sender`.

use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonically increasing request ID generator.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A request for interactive user confirmation before executing a sensitive action.
#[derive(Debug, Clone)]
pub struct ConfirmationRequest {
    /// Unique request ID (auto-assigned).
    pub id: u64,
    /// Which agent triggered the action.
    pub agent_id: String,
    /// The ability being requested, possibly scoped (e.g. "email.send:bob@example.com").
    pub ability: String,
    /// Human-readable reason for the confirmation prompt.
    pub reason: String,
    /// What triggered the confirmation requirement.
    pub source: ConfirmationSource,
}

impl ConfirmationRequest {
    /// Create a new request with an auto-assigned ID.
    pub fn new(
        agent_id: impl Into<String>,
        ability: impl Into<String>,
        reason: impl Into<String>,
        source: ConfirmationSource,
    ) -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            agent_id: agent_id.into(),
            ability: ability.into(),
            reason: reason.into(),
            source,
        }
    }
}

/// What triggered the confirmation requirement.
#[derive(Debug, Clone)]
pub enum ConfirmationSource {
    /// A constitution rule with `Enforcement::Confirm`.
    Constitution { rule_name: String },
    /// A circuit breaker with `BreakerAction::Confirm`.
    CircuitBreaker { breaker_id: String },
    /// A privilege level escalation.
    Privilege { required_level: u8 },
}

/// The user's response to a confirmation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationResponse {
    /// Allow this one execution only — not persisted.
    AllowOnce,
    /// Allow for this session — elevates privilege via `PrivilegeGuard::elevate()`.
    AllowSession,
    /// Always allow this ability for this agent — persisted via `PermissionManager::grant()`.
    AllowAlwaysAgent,
    /// Deny the action.
    Deny,
}

impl ConfirmationResponse {
    /// All variants in display order (for modal rendering).
    pub const ALL: [Self; 4] = [
        Self::AllowOnce,
        Self::AllowSession,
        Self::AllowAlwaysAgent,
        Self::Deny,
    ];

    /// Human-readable label for the modal.
    pub fn label(&self) -> &'static str {
        match self {
            Self::AllowOnce => "Allow once",
            Self::AllowSession => "Allow for this session",
            Self::AllowAlwaysAgent => "Always allow for this agent",
            Self::Deny => "Deny",
        }
    }
}

/// Callback type for sending confirmation requests from the orchestrator.
///
/// The orchestrator calls this with a `ConfirmationRequest`. The implementation
/// (typically in the TUI) shows a modal and returns the user's response.
/// Returns `None` if the channel is broken or timed out.
pub type ConfirmFn =
    Box<dyn Fn(ConfirmationRequest) -> Option<ConfirmationResponse> + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_ids_are_unique() {
        let r1 = ConfirmationRequest::new("a1", "email.send", "test", ConfirmationSource::Constitution { rule_name: "r1".into() });
        let r2 = ConfirmationRequest::new("a1", "email.send", "test", ConfirmationSource::Constitution { rule_name: "r1".into() });
        assert_ne!(r1.id, r2.id);
    }

    #[test]
    fn test_response_labels() {
        assert_eq!(ConfirmationResponse::AllowOnce.label(), "Allow once");
        assert_eq!(ConfirmationResponse::Deny.label(), "Deny");
    }

    #[test]
    fn test_all_variants_count() {
        assert_eq!(ConfirmationResponse::ALL.len(), 4);
    }
}
