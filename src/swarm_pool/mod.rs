//! SwarmPool — pure Rust spawn heuristics for the Hatchery swarm.
//!
//! Deterministic heuristics for Queen spawning, zerg rush, elastic pool,
//! and retry policy. No LLM calls — called mechanically by Nydus.

use std::collections::HashMap;
use crate::core::types::QueenId;

/// Configuration for SwarmPool heuristics.
#[derive(Debug, Clone)]
pub struct SwarmPoolConfig {
    pub min_queens: usize,
    pub max_queens: usize,
    pub zerg_rush_threshold: usize,
    pub zerg_rush_queens: usize,
    pub max_retries_before_escalate: usize,
}

impl Default for SwarmPoolConfig {
    fn default() -> Self {
        Self {
            min_queens: 2,
            max_queens: 8,
            zerg_rush_threshold: 3,
            zerg_rush_queens: 3,
            max_retries_before_escalate: 1,
        }
    }
}

/// Actions that SwarmPool can request Nydus to execute.
#[derive(Debug, Clone)]
pub enum SwarmPoolAction {
    SpawnQueens(usize),
    ZergRush { task_id: String, num_queens: usize },
    KillQueen(QueenId),
    RetryTask { task_id: String },
    EscalateToOvermind { task_id: String, decline_count: usize, reasons: Vec<String> },
    Noop,
}

/// SwarmPool — deterministic spawn heuristics.
pub struct SwarmPool {
    config: SwarmPoolConfig,
    #[allow(dead_code)]
    decline_counts: HashMap<String, usize>,
    #[allow(dead_code)]
    decline_reasons: HashMap<String, Vec<String>>,
}

impl SwarmPool {
    pub fn new(config: SwarmPoolConfig) -> Self {
        Self {
            config,
            decline_counts: HashMap::new(),
            decline_reasons: HashMap::new(),
        }
    }

    pub fn config(&self) -> &SwarmPoolConfig {
        &self.config
    }

    /// Called when Overlord declines a task.
    /// 1st decline → RetryTask (auto-retry)
    /// 2nd decline → EscalateToOvermind
    pub fn on_decline(&mut self, task_id: &str, reason: &str) -> SwarmPoolAction {
        // Increment decline count
        let decline_count = self.decline_counts.entry(task_id.to_string()).or_insert(0);
        *decline_count += 1;

        // Track decline reason
        self.decline_reasons
            .entry(task_id.to_string())
            .or_insert_with(Vec::new)
            .push(reason.to_string());

        // If decline_count <= max_retries_before_escalate → retry
        if *decline_count <= self.config.max_retries_before_escalate {
            SwarmPoolAction::RetryTask {
                task_id: task_id.to_string(),
            }
        } else {
            // Escalate to Overmind
            let reasons = self.decline_reasons
                .get(task_id)
                .cloned()
                .unwrap_or_default();
            SwarmPoolAction::EscalateToOvermind {
                task_id: task_id.to_string(),
                decline_count: *decline_count,
                reasons,
            }
        }
    }

    /// Called when DAG changes (task completed, new ready tasks).
    /// Checks for bottleneck tasks → auto zerg rush.
    /// Checks if more Queens needed for ready tasks.
    pub fn on_dag_change(
        &self,
        ready_count: usize,
        active_count: usize,
        idle_count: usize,
        bottlenecks: Vec<(String, usize)>, // (task_id, blocks_count)
    ) -> Vec<SwarmPoolAction> {
        let mut actions = Vec::new();

        // Check for bottlenecks → zerg rush
        for (task_id, blocks_count) in bottlenecks {
            if blocks_count >= self.config.zerg_rush_threshold {
                actions.push(SwarmPoolAction::ZergRush {
                    task_id,
                    num_queens: self.config.zerg_rush_queens,
                });
            }
        }

        // Check if more Queens needed for ready tasks
        // If idle_count < ready_count and we're not at max_queens
        let total_queens = active_count + idle_count;
        if idle_count < ready_count && total_queens < self.config.max_queens {
            let needed = ready_count - idle_count;
            let can_spawn = self.config.max_queens - total_queens;
            let to_spawn = needed.min(can_spawn);
            if to_spawn > 0 {
                actions.push(SwarmPoolAction::SpawnQueens(to_spawn));
            }
        }

        actions
    }

    /// Called periodically for pool sizing.
    /// Maintains min/max Queens.
    pub fn maintenance(&self, active_count: usize, idle_count: usize) -> Vec<SwarmPoolAction> {
        let mut actions = Vec::new();
        let total_queens = active_count + idle_count;

        // Spawn if below min_queens
        if total_queens < self.config.min_queens {
            let to_spawn = self.config.min_queens - total_queens;
            actions.push(SwarmPoolAction::SpawnQueens(to_spawn));
        }

        // Note: Kill excess Queens logic is skipped for now since KillQueen
        // requires specific QueenId which SwarmPool doesn't track.
        // Nydus will handle pool downsizing directly.

        actions
    }

    /// Reset decline counts for a task (called when task completes).
    pub fn on_task_completed(&mut self, task_id: &str) {
        self.decline_counts.remove(task_id);
        self.decline_reasons.remove(task_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_first_decline_retries() {
        let mut pool = SwarmPool::new(SwarmPoolConfig::default());
        let action = pool.on_decline("task1", "reason1");

        match action {
            SwarmPoolAction::RetryTask { task_id } => {
                assert_eq!(task_id, "task1");
            }
            _ => panic!("Expected RetryTask action"),
        }

        assert_eq!(pool.decline_counts.get("task1"), Some(&1));
        assert_eq!(pool.decline_reasons.get("task1").unwrap().len(), 1);
    }

    #[test]
    fn test_second_decline_escalates() {
        let mut pool = SwarmPool::new(SwarmPoolConfig::default());

        // First decline → retry
        pool.on_decline("task1", "reason1");

        // Second decline → escalate
        let action = pool.on_decline("task1", "reason2");

        match action {
            SwarmPoolAction::EscalateToOvermind { task_id, decline_count, reasons } => {
                assert_eq!(task_id, "task1");
                assert_eq!(decline_count, 2);
                assert_eq!(reasons.len(), 2);
                assert_eq!(reasons[0], "reason1");
                assert_eq!(reasons[1], "reason2");
            }
            _ => panic!("Expected EscalateToOvermind action"),
        }
    }

    #[test]
    fn test_decline_tracks_reasons() {
        let mut pool = SwarmPool::new(SwarmPoolConfig::default());

        pool.on_decline("task1", "reason1");
        pool.on_decline("task1", "reason2");
        pool.on_decline("task1", "reason3");

        let reasons = pool.decline_reasons.get("task1").unwrap();
        assert_eq!(reasons.len(), 3);
        assert_eq!(reasons[0], "reason1");
        assert_eq!(reasons[1], "reason2");
        assert_eq!(reasons[2], "reason3");
    }

    #[test]
    fn test_bottleneck_zerg_rush() {
        let pool = SwarmPool::new(SwarmPoolConfig {
            zerg_rush_threshold: 3,
            zerg_rush_queens: 4,
            ..SwarmPoolConfig::default()
        });

        let bottlenecks = vec![
            ("task1".to_string(), 5), // Above threshold
            ("task2".to_string(), 3), // At threshold
            ("task3".to_string(), 2), // Below threshold
        ];

        let actions = pool.on_dag_change(0, 0, 0, bottlenecks);

        // Should zerg rush task1 and task2 (both >= threshold)
        assert_eq!(actions.len(), 2);

        for action in &actions {
            match action {
                SwarmPoolAction::ZergRush { task_id, num_queens } => {
                    assert!(*task_id == "task1" || *task_id == "task2");
                    assert_eq!(*num_queens, 4);
                }
                _ => panic!("Expected ZergRush action"),
            }
        }
    }

    #[test]
    fn test_no_bottleneck_no_zerg() {
        let pool = SwarmPool::new(SwarmPoolConfig {
            zerg_rush_threshold: 3,
            ..SwarmPoolConfig::default()
        });

        let bottlenecks = vec![
            ("task1".to_string(), 2),
            ("task2".to_string(), 1),
        ];

        let actions = pool.on_dag_change(0, 0, 0, bottlenecks);

        // No zerg rush actions
        for action in &actions {
            assert!(!matches!(action, SwarmPoolAction::ZergRush { .. }));
        }
    }

    #[test]
    fn test_spawn_for_ready_tasks() {
        let pool = SwarmPool::new(SwarmPoolConfig {
            max_queens: 8,
            ..SwarmPoolConfig::default()
        });

        // 5 ready tasks, 1 active, 2 idle → need 3 more
        let actions = pool.on_dag_change(5, 1, 2, vec![]);

        // Should have SpawnQueens action
        let spawn_actions: Vec<_> = actions.iter()
            .filter_map(|a| match a {
                SwarmPoolAction::SpawnQueens(n) => Some(*n),
                _ => None,
            })
            .collect();

        assert_eq!(spawn_actions.len(), 1);
        assert_eq!(spawn_actions[0], 3); // 5 ready - 2 idle = 3 needed
    }

    #[test]
    fn test_no_spawn_at_max() {
        let pool = SwarmPool::new(SwarmPoolConfig {
            max_queens: 5,
            ..SwarmPoolConfig::default()
        });

        // 10 ready tasks, 3 active, 2 idle (total = 5, at max)
        let actions = pool.on_dag_change(10, 3, 2, vec![]);

        // Should NOT spawn (at max)
        let spawn_actions: Vec<_> = actions.iter()
            .filter_map(|a| match a {
                SwarmPoolAction::SpawnQueens(_) => Some(()),
                _ => None,
            })
            .collect();

        assert_eq!(spawn_actions.len(), 0);
    }

    #[test]
    fn test_spawn_respects_max() {
        let pool = SwarmPool::new(SwarmPoolConfig {
            max_queens: 5,
            ..SwarmPoolConfig::default()
        });

        // 10 ready tasks, 2 active, 1 idle → can spawn 2 more (max 5 total)
        let actions = pool.on_dag_change(10, 2, 1, vec![]);

        let spawn_actions: Vec<_> = actions.iter()
            .filter_map(|a| match a {
                SwarmPoolAction::SpawnQueens(n) => Some(*n),
                _ => None,
            })
            .collect();

        assert_eq!(spawn_actions.len(), 1);
        assert_eq!(spawn_actions[0], 2); // Can only spawn 2 (5 max - 3 current)
    }

    #[test]
    fn test_maintenance_spawn_to_min() {
        let pool = SwarmPool::new(SwarmPoolConfig {
            min_queens: 3,
            ..SwarmPoolConfig::default()
        });

        // 1 active, 0 idle → below min
        let actions = pool.maintenance(1, 0);

        assert_eq!(actions.len(), 1);
        match &actions[0] {
            SwarmPoolAction::SpawnQueens(n) => {
                assert_eq!(*n, 2); // Need 2 more to reach min of 3
            }
            _ => panic!("Expected SpawnQueens action"),
        }
    }

    #[test]
    fn test_maintenance_at_min_noop() {
        let pool = SwarmPool::new(SwarmPoolConfig {
            min_queens: 3,
            ..SwarmPoolConfig::default()
        });

        // 2 active, 1 idle (total = 3, at min)
        let actions = pool.maintenance(2, 1);

        // No actions needed
        assert_eq!(actions.len(), 0);
    }

    #[test]
    fn test_maintenance_above_min_noop() {
        let pool = SwarmPool::new(SwarmPoolConfig {
            min_queens: 2,
            ..SwarmPoolConfig::default()
        });

        // 3 active, 2 idle (total = 5, above min)
        let actions = pool.maintenance(3, 2);

        // No actions needed (kill logic not implemented)
        assert_eq!(actions.len(), 0);
    }

    #[test]
    fn test_task_completed_resets_declines() {
        let mut pool = SwarmPool::new(SwarmPoolConfig::default());

        // Decline twice
        pool.on_decline("task1", "reason1");
        pool.on_decline("task1", "reason2");

        assert_eq!(pool.decline_counts.get("task1"), Some(&2));
        assert_eq!(pool.decline_reasons.get("task1").unwrap().len(), 2);

        // Task completed
        pool.on_task_completed("task1");

        assert_eq!(pool.decline_counts.get("task1"), None);
        assert_eq!(pool.decline_reasons.get("task1"), None);
    }

    #[test]
    fn test_combined_bottleneck_and_spawn() {
        let pool = SwarmPool::new(SwarmPoolConfig {
            min_queens: 2,
            max_queens: 10,
            zerg_rush_threshold: 3,
            zerg_rush_queens: 3,
            ..SwarmPoolConfig::default()
        });

        let bottlenecks = vec![
            ("task1".to_string(), 5), // Bottleneck
        ];

        // 5 ready tasks, 2 active, 1 idle
        let actions = pool.on_dag_change(5, 2, 1, bottlenecks);

        // Should have both ZergRush and SpawnQueens
        let zerg_count = actions.iter()
            .filter(|a| matches!(a, SwarmPoolAction::ZergRush { .. }))
            .count();
        let spawn_count = actions.iter()
            .filter(|a| matches!(a, SwarmPoolAction::SpawnQueens(_)))
            .count();

        assert_eq!(zerg_count, 1);
        assert_eq!(spawn_count, 1);
    }

    #[test]
    fn test_multiple_bottlenecks() {
        let pool = SwarmPool::new(SwarmPoolConfig {
            zerg_rush_threshold: 2,
            zerg_rush_queens: 3,
            ..SwarmPoolConfig::default()
        });

        let bottlenecks = vec![
            ("task1".to_string(), 4),
            ("task2".to_string(), 3),
            ("task3".to_string(), 2),
        ];

        let actions = pool.on_dag_change(0, 0, 0, bottlenecks);

        // All 3 should trigger zerg rush
        let zerg_actions: Vec<_> = actions.iter()
            .filter_map(|a| match a {
                SwarmPoolAction::ZergRush { task_id, num_queens } => {
                    Some((task_id.clone(), *num_queens))
                }
                _ => None,
            })
            .collect();

        assert_eq!(zerg_actions.len(), 3);
        for (_, num_queens) in zerg_actions {
            assert_eq!(num_queens, 3);
        }
    }

    #[test]
    fn test_custom_max_retries() {
        let mut pool = SwarmPool::new(SwarmPoolConfig {
            max_retries_before_escalate: 3,
            ..SwarmPoolConfig::default()
        });

        // Should retry 3 times before escalating
        assert!(matches!(pool.on_decline("task1", "r1"), SwarmPoolAction::RetryTask { .. }));
        assert!(matches!(pool.on_decline("task1", "r2"), SwarmPoolAction::RetryTask { .. }));
        assert!(matches!(pool.on_decline("task1", "r3"), SwarmPoolAction::RetryTask { .. }));

        // 4th decline should escalate
        let action = pool.on_decline("task1", "r4");
        match action {
            SwarmPoolAction::EscalateToOvermind { decline_count, .. } => {
                assert_eq!(decline_count, 4);
            }
            _ => panic!("Expected escalation"),
        }
    }
}
