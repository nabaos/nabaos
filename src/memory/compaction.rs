//! Conversation compaction — reduces token count by summarizing older turns.
//!
//! Current strategy: local truncation heuristic (no LLM call).
//! Phase 3 can add LLM-powered summarization.

use super::memory_store::ConversationTurn;
use crate::core::error::Result;

/// Result of a compaction operation.
#[derive(Debug)]
pub struct CompactionResult {
    /// Number of original turns that were compacted.
    pub original_turns: usize,
    /// Estimated tokens in the summary.
    pub summary_token_estimate: u32,
    /// Tokens saved by compaction.
    pub tokens_saved: u32,
    /// The compacted summary text.
    pub summary_text: String,
}

/// Compact a sequence of conversation turns into a summary string.
///
/// Strategy: concatenate turns as "role: content" lines, truncate to
/// `max_summary_tokens * 4` chars. No LLM call — pure local heuristic.
pub fn compact_turns(
    turns: &[ConversationTurn],
    max_summary_tokens: u32,
) -> Result<CompactionResult> {
    if turns.is_empty() {
        return Ok(CompactionResult {
            original_turns: 0,
            summary_token_estimate: 0,
            tokens_saved: 0,
            summary_text: String::new(),
        });
    }

    let original_tokens: u32 = turns.iter().map(|t| t.token_estimate).sum();

    let mut summary_parts = Vec::new();
    for turn in turns {
        summary_parts.push(format!("{}: {}", turn.role.as_str(), turn.content));
    }
    let full_summary = summary_parts.join("\n");

    let max_chars = (max_summary_tokens as usize) * 4;
    let truncated = if full_summary.len() > max_chars {
        format!(
            "[Previous conversation summary]\n{}",
            &full_summary[..max_chars]
        )
    } else {
        format!("[Previous conversation summary]\n{}", full_summary)
    };

    let summary_token_estimate = (truncated.len() as u32) / 4;

    Ok(CompactionResult {
        original_turns: turns.len(),
        summary_token_estimate,
        tokens_saved: original_tokens.saturating_sub(summary_token_estimate),
        summary_text: truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::memory_store::TurnRole;

    fn make_turn(role: TurnRole, content: &str) -> ConversationTurn {
        ConversationTurn {
            id: 0,
            session_id: "test".to_string(),
            role,
            content: content.to_string(),
            token_estimate: (content.len() as u32) / 4,
            created_at: 0,
        }
    }

    #[test]
    fn test_compact_basic() {
        let turns = vec![
            make_turn(TurnRole::User, "Hello, how are you?"),
            make_turn(TurnRole::Assistant, "I'm doing great, thanks!"),
            make_turn(TurnRole::User, "What's the weather?"),
            make_turn(TurnRole::Assistant, "It's sunny and warm today."),
            make_turn(TurnRole::User, "Thanks!"),
        ];

        let result = compact_turns(&turns, 1000).unwrap();
        assert_eq!(result.original_turns, 5);
        assert!(result.summary_text.contains("user: Hello"));
        assert!(result.summary_text.contains("assistant: I'm doing great"));
        assert!(result
            .summary_text
            .starts_with("[Previous conversation summary]"));
    }

    #[test]
    fn test_compact_truncation() {
        let turns = vec![
            make_turn(TurnRole::User, &"a".repeat(200)),
            make_turn(TurnRole::Assistant, &"b".repeat(200)),
            make_turn(TurnRole::User, &"c".repeat(200)),
        ];

        // max_summary_tokens=10 -> max_chars=40
        let result = compact_turns(&turns, 10).unwrap();
        assert_eq!(result.original_turns, 3);
        // The summary should be truncated
        assert!(result.tokens_saved > 0);
    }

    #[test]
    fn test_compact_empty() {
        let result = compact_turns(&[], 100).unwrap();
        assert_eq!(result.original_turns, 0);
        assert_eq!(result.summary_token_estimate, 0);
        assert_eq!(result.tokens_saved, 0);
    }
}
