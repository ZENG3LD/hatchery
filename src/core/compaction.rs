//! CompactionStrategy — Progressive context compression for long-running swarms.
//!
//! Based on Goose's compaction pattern, this module implements progressive
//! context window management that prevents token overflow during extended
//! agent conversations.
//!
//! ## Strategy
//!
//! Progressive compaction levels:
//! - 80%: remove old tool responses
//! - 85%: remove more recent tool responses
//! - 90%: summarize old conversation
//! - 95%: fresh start with minimal carry-over

use serde::{Serialize, Deserialize};

/// Scope of content that should be protected from compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompactionScope {
    /// System prompt — never remove
    SystemPrompt,
    /// Task assignments — current work
    TaskAssignments,
    /// Last N messages — keep recent context
    RecentMessages(usize),
    /// Knowledge entries — accumulated wisdom
    Knowledge,
    /// Active task context (files, deps, etc.)
    ActiveTaskContext,
    /// Orchestration discipline rules — never remove.
    /// These rules ensure agents maintain discipline after context compression:
    /// update PRD checkboxes, follow protocols, report progress.
    OrchestrationRules,
}

/// Action to take at a given compaction level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompactionAction {
    /// Remove tool call responses older than N turns
    RemoveToolResponses { older_than_turns: usize },
    /// Summarize conversation older than N turns
    SummarizeOlderThan { turns: usize },
    /// Drop all thinking/reasoning blocks
    DropThinkingBlocks,
    /// Fresh start: only carry over specified scopes
    FreshStart { carry_over: Vec<CompactionScope> },
}

/// A single level of progressive compaction.
#[derive(Debug, Clone)]
pub struct CompactionLevel {
    /// At what context usage % to trigger this level
    pub trigger_pct: f32,
    /// What action to take
    pub action: CompactionAction,
}

/// CompactionStrategy manages progressive context compression.
///
/// Based on Goose's compaction pattern:
/// - 80%: remove old tool responses
/// - 85%: remove more recent tool responses
/// - 90%: summarize old conversation
/// - 95%: fresh start with minimal carry-over
pub struct CompactionStrategy {
    /// Trigger compaction at this % of context window (0.80 = 80%)
    threshold_pct: f32,
    /// What to preserve (never compact)
    protected: Vec<CompactionScope>,
    /// Progressive removal levels (ordered by trigger_pct ascending)
    levels: Vec<CompactionLevel>,
}

impl CompactionStrategy {
    /// Create a new CompactionStrategy with custom settings.
    pub fn new(threshold_pct: f32, protected: Vec<CompactionScope>, levels: Vec<CompactionLevel>) -> Self {
        Self { threshold_pct, protected, levels }
    }

    /// Default strategy for SwarmHost coordinator sessions.
    /// Based on Goose's DEFAULT_COMPACTION_THRESHOLD = 0.8
    pub fn default_swarm_host() -> Self {
        Self {
            threshold_pct: 0.80,
            protected: vec![
                CompactionScope::SystemPrompt,
                CompactionScope::TaskAssignments,
                CompactionScope::Knowledge,
                CompactionScope::OrchestrationRules,
                CompactionScope::RecentMessages(5),
            ],
            levels: vec![
                CompactionLevel { trigger_pct: 0.80, action: CompactionAction::RemoveToolResponses { older_than_turns: 20 } },
                CompactionLevel { trigger_pct: 0.85, action: CompactionAction::RemoveToolResponses { older_than_turns: 10 } },
                CompactionLevel { trigger_pct: 0.90, action: CompactionAction::SummarizeOlderThan { turns: 5 } },
                CompactionLevel { trigger_pct: 0.95, action: CompactionAction::FreshStart {
                    carry_over: vec![
                        CompactionScope::SystemPrompt,
                        CompactionScope::TaskAssignments,
                        CompactionScope::Knowledge,
                        CompactionScope::OrchestrationRules,
                        CompactionScope::RecentMessages(3),
                    ],
                }},
            ],
        }
    }

    /// Check if compaction should be triggered at current usage %.
    pub fn should_compact(&self, context_usage_pct: f32) -> bool {
        context_usage_pct >= self.threshold_pct
    }

    /// Get the compaction action for the current context usage level.
    /// Returns the highest-level action that the current usage qualifies for.
    pub fn get_action(&self, context_usage_pct: f32) -> Option<&CompactionAction> {
        self.levels
            .iter()
            .filter(|level| context_usage_pct >= level.trigger_pct)
            .max_by(|a, b| a.trigger_pct.partial_cmp(&b.trigger_pct).unwrap())
            .map(|level| &level.action)
    }

    /// Get the protected scopes (content that should never be compacted).
    pub fn protected_scopes(&self) -> &[CompactionScope] {
        &self.protected
    }

    /// Get the threshold percentage.
    pub fn threshold(&self) -> f32 {
        self.threshold_pct
    }

    /// Apply compaction to a list of context entries.
    /// Returns the entries that should be KEPT.
    ///
    /// Each entry is (turn_number, content, is_tool_response, is_thinking_block, scope).
    /// Scope=None means no special protection.
    pub fn compact(&self, context_usage_pct: f32, entries: &[ContextEntry]) -> Vec<usize> {
        // Returns indices of entries to keep
        let action = match self.get_action(context_usage_pct) {
            Some(a) => a,
            None => return (0..entries.len()).collect(), // keep all
        };

        let max_turn = entries.iter().map(|e| e.turn).max().unwrap_or(0);

        match action {
            CompactionAction::RemoveToolResponses { older_than_turns } => {
                entries.iter().enumerate()
                    .filter(|(_, e)| {
                        !e.is_tool_response || (max_turn - e.turn) < *older_than_turns
                    })
                    .map(|(i, _)| i)
                    .collect()
            }
            CompactionAction::SummarizeOlderThan { turns } => {
                // Keep only entries within last N turns (or protected)
                entries.iter().enumerate()
                    .filter(|(_, e)| {
                        (max_turn - e.turn) < *turns || e.is_protected
                    })
                    .map(|(i, _)| i)
                    .collect()
            }
            CompactionAction::DropThinkingBlocks => {
                entries.iter().enumerate()
                    .filter(|(_, e)| !e.is_thinking_block)
                    .map(|(i, _)| i)
                    .collect()
            }
            CompactionAction::FreshStart { carry_over: _ } => {
                // Only keep protected entries
                entries.iter().enumerate()
                    .filter(|(_, e)| e.is_protected)
                    .map(|(i, _)| i)
                    .collect()
            }
        }
    }
}

/// A context entry for compaction decisions.
#[derive(Debug, Clone)]
pub struct ContextEntry {
    pub turn: usize,
    pub is_tool_response: bool,
    pub is_thinking_block: bool,
    pub is_protected: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_compact() {
        let s = CompactionStrategy::default_swarm_host();
        assert!(!s.should_compact(0.5));
        assert!(!s.should_compact(0.79));
        assert!(s.should_compact(0.80));
        assert!(s.should_compact(0.95));
    }

    #[test]
    fn test_get_action_levels() {
        let s = CompactionStrategy::default_swarm_host();
        // Below threshold — no action
        assert!(s.get_action(0.5).is_none());
        // At 80% — RemoveToolResponses(20)
        let a = s.get_action(0.80).unwrap();
        assert!(matches!(a, CompactionAction::RemoveToolResponses { older_than_turns: 20 }));
        // At 86% — RemoveToolResponses(10)
        let a = s.get_action(0.86).unwrap();
        assert!(matches!(a, CompactionAction::RemoveToolResponses { older_than_turns: 10 }));
        // At 91% — Summarize(5)
        let a = s.get_action(0.91).unwrap();
        assert!(matches!(a, CompactionAction::SummarizeOlderThan { turns: 5 }));
        // At 96% — FreshStart
        let a = s.get_action(0.96).unwrap();
        assert!(matches!(a, CompactionAction::FreshStart { .. }));
    }

    #[test]
    fn test_compact_removes_old_tool_responses() {
        let s = CompactionStrategy::default_swarm_host();
        let entries = vec![
            ContextEntry { turn: 0, is_tool_response: true, is_thinking_block: false, is_protected: false },
            ContextEntry { turn: 5, is_tool_response: true, is_thinking_block: false, is_protected: false },
            ContextEntry { turn: 15, is_tool_response: false, is_thinking_block: false, is_protected: false },
            ContextEntry { turn: 19, is_tool_response: true, is_thinking_block: false, is_protected: false },
            ContextEntry { turn: 20, is_tool_response: true, is_thinking_block: false, is_protected: false },
        ];
        let kept = s.compact(0.80, &entries);
        // At 80%, remove tool responses older than 20 turns
        // max_turn=20, so entries at turn 0 and 5 are tool responses older than 20 turns from max
        assert!(kept.contains(&2)); // non-tool response always kept
        assert!(kept.contains(&4)); // turn 20, 0 turns old — kept
    }

    #[test]
    fn test_compact_fresh_start() {
        let s = CompactionStrategy::default_swarm_host();
        let entries = vec![
            ContextEntry { turn: 0, is_tool_response: false, is_thinking_block: false, is_protected: true },
            ContextEntry { turn: 1, is_tool_response: false, is_thinking_block: false, is_protected: false },
            ContextEntry { turn: 2, is_tool_response: false, is_thinking_block: false, is_protected: false },
        ];
        let kept = s.compact(0.96, &entries);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0], 0); // only protected entry
    }

    #[test]
    fn test_no_compact_below_threshold() {
        let s = CompactionStrategy::default_swarm_host();
        let entries = vec![
            ContextEntry { turn: 0, is_tool_response: true, is_thinking_block: false, is_protected: false },
            ContextEntry { turn: 1, is_tool_response: true, is_thinking_block: false, is_protected: false },
        ];
        let kept = s.compact(0.5, &entries);
        assert_eq!(kept.len(), 2); // all kept
    }

    #[test]
    fn test_compact_drop_thinking_blocks() {
        let s = CompactionStrategy::new(
            0.80,
            vec![],
            vec![CompactionLevel {
                trigger_pct: 0.80,
                action: CompactionAction::DropThinkingBlocks,
            }],
        );
        let entries = vec![
            ContextEntry { turn: 0, is_tool_response: false, is_thinking_block: true, is_protected: false },
            ContextEntry { turn: 1, is_tool_response: false, is_thinking_block: false, is_protected: false },
            ContextEntry { turn: 2, is_tool_response: false, is_thinking_block: true, is_protected: false },
        ];
        let kept = s.compact(0.80, &entries);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0], 1); // only non-thinking entry
    }

    #[test]
    fn test_compact_summarize_protects_recent() {
        let s = CompactionStrategy::default_swarm_host();
        let entries = vec![
            ContextEntry { turn: 0, is_tool_response: false, is_thinking_block: false, is_protected: false },
            ContextEntry { turn: 5, is_tool_response: false, is_thinking_block: false, is_protected: false },
            ContextEntry { turn: 15, is_tool_response: false, is_thinking_block: false, is_protected: false },
            ContextEntry { turn: 17, is_tool_response: false, is_thinking_block: false, is_protected: false },
            ContextEntry { turn: 20, is_tool_response: false, is_thinking_block: false, is_protected: false },
        ];
        let kept = s.compact(0.91, &entries); // Triggers SummarizeOlderThan(5)
        // max_turn=20, keep entries within last 5 turns (16-20)
        assert!(kept.contains(&4)); // turn 20
        assert!(kept.contains(&3)); // turn 17
        // Older entries should be removed unless protected
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn test_protected_scopes_accessor() {
        let s = CompactionStrategy::default_swarm_host();
        let scopes = s.protected_scopes();
        assert_eq!(scopes.len(), 5);
    }

    #[test]
    fn test_threshold_accessor() {
        let s = CompactionStrategy::default_swarm_host();
        assert_eq!(s.threshold(), 0.80);
    }
}
