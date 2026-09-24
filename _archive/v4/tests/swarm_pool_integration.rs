// Integration tests for SwarmPool
use hatchery::swarm_pool::{SwarmPool, SwarmPoolConfig, SwarmPoolAction};

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
fn test_task_completed_resets_declines() {
    let mut pool = SwarmPool::new(SwarmPoolConfig::default());

    // Decline twice
    pool.on_decline("task1", "reason1");
    pool.on_decline("task1", "reason2");

    // Task completed
    pool.on_task_completed("task1");

    // Decline again - should be treated as first decline
    let action = pool.on_decline("task1", "reason3");
    match action {
        SwarmPoolAction::RetryTask { .. } => {} // Expected
        _ => panic!("Expected RetryTask after reset"),
    }
}
