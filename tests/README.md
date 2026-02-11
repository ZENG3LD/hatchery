# Hatchery Integration Tests

## Overview

This directory contains integration tests for the Nydus event processor that simulate real swarm scenarios WITHOUT spawning actual Claude processes. Tests inject mock events directly into Nydus to verify bug fixes and core functionality.

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
| **test_infestor_review_flow** | Validating → Approved → Completed | TaskCompleted puts task in Validating; approval completes it; deps unblock only after approval |
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
8. **Infestor Workflow** - Validating state prevents premature dependency unblocking

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

## Maintenance

When adding new Nydus features:

1. Add integration test here if it involves multi-task scenarios
2. Add unit test in `src/nydus/mod.rs` if it's a single-method test
3. Follow the existing pattern: create DAG, inject events/state, verify outcome
4. Document which real-world bug scenario the test prevents
