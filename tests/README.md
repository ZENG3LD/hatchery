# Hatchery Integration Tests

## Overview

This directory contains integration tests for both **classic mode** (Nydus event processor) and **V4 modular architecture** (pipeline modules). Tests simulate real scenarios WITHOUT spawning actual Claude processes.

### Test Categories

1. **Classic Mode Tests** (`nydus_integration.rs`, `overlord_parsers_test.rs`, `swarm_pool_integration.rs`) — V3 integrated components
2. **V4 Module Tests** (new) — Trait contract tests, pipeline integration tests, hot-swap tests
3. **Session Parsers** (`overseer_parsing_test.rs`) — Claude Code session parsing (used in both modes)

---

## Classic Mode Tests

Integration tests for the Nydus event processor that simulate real swarm scenarios WITHOUT spawning actual Claude processes. Tests inject mock events directly into Nydus to verify bug fixes and core functionality.

## Test File: `nydus_integration.rs`

Comprehensive integration tests covering 10 scenarios based on real bugs from production swarm runs.

### Test Coverage

| Test | Description | What It Validates |
|------|-------------|-------------------|
| **test_sequential_task_completion** | Happy path with 3 sequential tasks (prd-1 → prd-2 → prd-3) | Task dependencies resolve correctly; blocked tasks become Ready after deps complete |
| **test_rate_limit_cascade_shutdown** | 2+ rate limit failures within 60s | Multiple rate limit errors trigger shutdown detection; DAG state updates correctly |
| **test_prd_checkbox_update** | Task completion updates PRD file | PRD checkboxes marked [x] atomically after task completion |
| **test_zerg_rush_winner_losers** | Bottleneck task assigned to 3 Queens | First Queen to complete becomes winner; losers identified; dependent tasks unblock |
| **test_dead_queen_recovery** | Queen dies with assigned task | Dead Queen detected; task recovered to Ready state; can be reassigned |
| **test_verify_scan_bonus_completion** | Post-merge verify scan | Tasks with same verify command get bonus completion when one satisfies both |
| **test_dependency_chain_with_failure** | prd-1 → prd-2 → prd-3 with prd-2 failing | Failed task keeps dependents blocked; requeue works; retry succeeds |
| **test_overlord_review_flow** | Validating → Approved → Completed | TaskCompleted puts task in Validating; approval completes it; deps unblock only after approval |
| **test_parallel_independent_tasks** | 3 independent tasks complete simultaneously | Multiple Queens work without conflicts; all tasks complete successfully |
| **test_diamond_dependency** | Diamond pattern (A → B/C, B/C → D) | D only becomes Ready when BOTH B and C complete |

### Key Bugs Tested

These tests validate fixes for real production issues:

1. **Sequential Dependencies** - Blocked tasks properly transition to Ready when dependencies complete
2. **Rate Limit Cascade** - Graceful shutdown when multiple Queens hit global rate limit
3. **PRD Checkbox Sync** - Atomic PRD file updates after task completion
4. **Zerg Rush** - Winner/loser handling when multiple Queens race on bottleneck tasks
5. **Dead Queen Recovery** - Stuck tasks recovered when Queen subprocess dies
6. **Verify Scan Bonus** - Post-merge scan detects bonus completions via verify commands
7. **Dependency Chain Failures** - Failed tasks properly block dependents; requeue works
8. **Overlord Workflow** - Validating state prevents premature dependency unblocking

## Running Tests

```bash
# Run all integration tests
cargo test --test nydus_integration

# Run specific test
cargo test --test nydus_integration test_zerg_rush_winner_losers

# Run with output
cargo test --test nydus_integration -- --nocapture
```

## Implementation Details

### Mock Infrastructure

- **Mock Queens**: Created via `create_mock_queen()` helper with channels but no subprocess
- **Direct DAG Access**: Tests use `task_dag()` and `task_dag_mut()` accessors to manipulate state
- **No Real Events**: Tests directly call DAG methods instead of injecting events (since `handle_event` is private)

### Test-Only Public Methods

Added to `Nydus` for testing (in `src/nydus/mod.rs`):

```rust
pub fn task_dag(&self) -> &TaskDag
pub fn task_dag_mut(&mut self) -> &mut TaskDag
pub fn register_queen(&mut self, queen_id: QueenId, handle: QueenHandle)
```

**WARNING**: These are test-only accessors and should not be used in production code.

### Testing Philosophy

- **No subprocess spawning**: Tests are fast and deterministic
- **Focus on state transitions**: Verify DAG state changes, not implementation details
- **Real bug scenarios**: Each test maps to a specific bug that was fixed
- **Regression prevention**: Tests fail if corresponding fix is reverted

## Notes

- Tests in `tests/` directory are integration tests (separate compilation)
- Tests in `src/*/mod.rs` with `#[cfg(test)]` are unit tests (same compilation)
- Integration tests cannot access private fields/methods (hence public accessors)
- All tests pass without spawning real Queens or Claude processes
- Pre-existing failures in `safety::worktree` module are unrelated to these tests

## Maintenance (Classic Mode)

When adding new Nydus features:

1. Add integration test here if it involves multi-task scenarios
2. Add unit test in `src/nydus/mod.rs` if it's a single-method test
3. Follow the existing pattern: create DAG, inject events/state, verify outcome
4. Document which real-world bug scenario the test prevents

---

## V4 Modular Architecture Tests

The V4 architecture introduces 50+ composable modules across 8 categories. Testing strategy ensures correctness at trait, module, and pipeline levels.

### Test Levels

#### Level 1: Trait Contract Tests

Each of the 7 core traits has a contract test suite that all implementations must pass. These verify that every module correctly implements its trait.

**Location**: `tests/trait_contracts/`

**Files**:
- `topology_contract.rs` — Tests all 7 topology modules (Centralized, Hierarchical, P2P, Blackboard, GraphDAG, Conversational, Hybrid)
- `decomposition_contract.rs` — Tests all 6 decomposition modules (DAG, HTN, TDAG, Emergent, RoleBased, Capability)
- `communication_contract.rs` — Tests all 8+4 communication modules (Direct, Broadcast, Blackboard, MessageBus, etc.)
- `memory_contract.rs` — Tests all 9 memory modules (SharedState, Conversation, MultiTier, etc.)
- `scheduling_contract.rs` — Tests all 7 scheduling modules (EventDriven, TimerBased, Hybrid, etc.)
- `resilience_contract.rs` — Tests all 7 resilience modules (Retry, Recovery, Degradation, etc.)
- `scaling_contract.rs` — Tests all 6 scaling modules (SmallScale, MediumScale, LargeScale, etc.)

**Example**:
```rust
// tests/trait_contracts/topology_contract.rs

#[test]
fn test_centralized_topology_contract() {
    test_topology_contract(CentralizedTopology::new(CentralizedConfig::default()));
}

#[test]
fn test_hierarchical_topology_contract() {
    test_topology_contract(HierarchicalTopology::new(HierarchicalConfig::default()));
}

// ... same for all 7 topology modules

fn test_topology_contract<T: Topology>(mut topology: T) {
    // Add agent
    let agent = AgentId::Queen(QueenId(0));
    topology.add_agent(agent.clone()).unwrap();
    assert_eq!(topology.agents().len(), 1);

    // Assign task
    let task = TaskId::new("task-1");
    let assigned = topology.assign_task(&task).unwrap();
    assert_eq!(assigned.len(), 1);

    // Handle completion
    topology.handle_completion(agent, task, TaskResult::Success);

    // ... more contract assertions
}
```

**Run contract tests**:
```bash
# Test all trait contracts (all 50+ modules)
cargo test --package hatchery --test trait_contracts

# Test specific trait contract
cargo test --package hatchery --test topology_contract
cargo test --package hatchery --test resilience_contract
```

#### Level 2: Module-Specific Tests

Individual modules may have additional tests beyond the trait contract (e.g., GraphDAG cycle detection, HTN method decomposition, RAG similarity search).

**Location**: `tests/modules/`

**Files**:
- `graph_dag_cycles_test.rs` — Cycle detection in GraphDAG topology
- `htn_planning_test.rs` — HTN method expansion, operator application
- `rag_similarity_test.rs` — TF-IDF similarity search in RAG memory
- `circuit_breaker_state_test.rs` — State transitions in CircuitBreaker resilience
- `work_stealing_test.rs` — Steal algorithm in WorkStealing scheduler
- ... more as needed

**Run module-specific tests**:
```bash
cargo test --package hatchery --test graph_dag_cycles_test
```

#### Level 3: Pipeline Integration Tests

End-to-end tests for preset pipelines (carousel, ralph, blackboard, consensus, swarm, minimal). Verify that composed modules work together correctly.

**Location**: `tests/pipeline_integration/`

**Files**:
- `carousel_integration_test.rs` — Multi-phase workflow (research → implement → test → debug)
- `ralph_integration_test.rs` — Autonomous iterative tasks with PRD checkboxes
- `blackboard_integration_test.rs` — Self-selecting research agents
- `consensus_integration_test.rs` — Multi-agent voting and agreement
- `swarm_integration_test.rs` — Embarrassingly parallel tasks with work stealing
- `minimal_integration_test.rs` — Simple single-agent tasks

**Example**:
```rust
// tests/pipeline_integration/carousel_integration_test.rs

#[tokio::test]
async fn test_carousel_multi_phase_workflow() {
    // Build carousel preset
    let pipeline = carousel_preset().build().unwrap();

    // Add tasks: research → implement → test → debug
    let tasks = vec![
        Task::new("research-api", vec![], TaskType::Research),
        Task::new("implement-connector", vec!["research-api"], TaskType::Implement),
        Task::new("test-connector", vec!["implement-connector"], TaskType::Test),
        Task::new("debug-connector", vec!["test-connector"], TaskType::Debug),
    ];

    for task in tasks {
        pipeline.decomposition.decompose(task).unwrap();
    }

    // Add agents
    for i in 0..3 {
        let agent = AgentId::Queen(QueenId(i));
        pipeline.topology.add_agent(agent).unwrap();
    }

    // Run pipeline (mock execution)
    // ... verify tasks execute in correct order
    // ... verify DAG dependencies respected
    // ... verify memory state after each phase
}
```

**Run pipeline integration tests**:
```bash
# Test all preset pipelines
cargo test --package hatchery --test pipeline_integration

# Test specific preset
cargo test --package hatchery --test carousel_integration_test
```

#### Level 4: Hot-Swap Tests

Runtime component replacement tests. Verify that swapping modules mid-execution doesn't break pipeline state.

**Location**: `tests/hot_swap_test.rs`

**Example**:
```rust
#[tokio::test]
async fn test_hot_swap_topology() {
    let mut pipeline = minimal_preset().build().unwrap();

    // Start with Centralized topology
    assert!(matches!(pipeline.topology_name(), "centralized"));

    // Add tasks and agents
    // ...

    // Hot-swap to PeerToPeer topology
    pipeline.swap_topology(PeerToPeerTopology::new(PeerToPeerConfig::default())).unwrap();

    // Verify existing tasks continue
    // Verify new tasks use new topology
}
```

**Run hot-swap tests**:
```bash
cargo test --package hatchery --test hot_swap_test
```

### Test Coverage Goals

| Test Level | Coverage Target | Current |
|------------|-----------------|---------|
| Trait contracts | 100% (all 50+ modules) | 🚧 WIP |
| Module-specific | 80% (critical algorithms) | 🚧 WIP |
| Pipeline integration | 100% (all 6 presets) | 🚧 WIP |
| Hot-swap | 80% (all 7 traits) | 🚧 WIP |

### Running All V4 Tests

```bash
# Run all V4 module tests (contract + module + pipeline + hot-swap)
cargo test --package hatchery --lib  # Unit tests in src/
cargo test --package hatchery --test trait_contracts
cargo test --package hatchery --test modules
cargo test --package hatchery --test pipeline_integration
cargo test --package hatchery --test hot_swap_test

# Or run all tests
cargo test --package hatchery
```

### Regression Tests (V3 → V4)

Ensure classic mode (Nydus + SwarmPool + Overlord) still works after V4 refactor.

**Location**: `tests/regression/`

**Files**:
- `nydus_classic_mode_test.rs` — Verify Nydus classic mode unchanged
- `swarm_pool_classic_test.rs` — Verify SwarmPool classic mode unchanged
- `overlord_classic_test.rs` — Verify Overlord classic mode unchanged

**Run regression tests**:
```bash
cargo test --package hatchery --test regression
```

### Benchmarks (Performance Regression)

**Location**: `benches/`

**Files**:
- `pipeline_throughput.rs` — Tasks/sec for each preset
- `memory_latency.rs` — Get/insert latency for each memory module
- `scheduling_overhead.rs` — Scheduling latency for each scheduler

**Run benchmarks**:
```bash
cargo bench --package hatchery
```

### Test Data & Fixtures

**Location**: `tests/fixtures/`

**Files**:
- `sample_prds/` — Sample PRD markdown files for testing
- `mock_sessions/` — Mock Claude Code session logs
- `mock_diffs/` — Sample git diffs for Overlord testing

### CI/CD Integration

Tests run in GitHub Actions on every PR:

```yaml
# .github/workflows/hatchery_tests.yml
- name: Run classic mode tests
  run: cargo test --package hatchery --test nydus_integration

- name: Run V4 trait contract tests
  run: cargo test --package hatchery --test trait_contracts

- name: Run V4 pipeline integration tests
  run: cargo test --package hatchery --test pipeline_integration

- name: Run regression tests
  run: cargo test --package hatchery --test regression

- name: Run benchmarks
  run: cargo bench --package hatchery
```

### Maintenance (V4 Modules)

When adding a new module:

1. **Implement the trait** for your module
2. **Add trait contract test** in `tests/trait_contracts/{trait}_contract.rs`
3. **Add module-specific tests** (if needed) in `tests/modules/`
4. **Update preset** (if applicable) in `src/pipeline/presets.rs`
5. **Add pipeline integration test** (if new preset) in `tests/pipeline_integration/`
6. **Document** your module in `ARCHITECTURE_V4.md`

When modifying an existing module:

1. **Run trait contract test** to ensure trait compliance
2. **Run module-specific tests** (if any)
3. **Run regression tests** to ensure classic mode unaffected
4. **Run benchmarks** to detect performance regressions

See `ARCHITECTURE_V4.md` for full details on V4 modular architecture.
