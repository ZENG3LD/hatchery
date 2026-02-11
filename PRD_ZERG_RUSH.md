# Zerg Rush: Multi-Queen Task Assignment

## Executive Summary

**Zerg Rush** enables assigning multiple Queens to the same bottleneck task simultaneously. The first Queen to complete the task wins — their work gets reviewed and merged. All other Queens working on the same task are cancelled and reassigned to newly-unblocked tasks. This eliminates pipeline stalls caused by critical-path bottleneck tasks.

## Problem Statement

Currently: 1 Queen per task. When a bottleneck task (a task that many downstream tasks depend on) is being worked on by a single Queen, all other Queens remain idle waiting for the bottleneck to complete. This creates pipeline stalls and underutilizes available Queens.

## Solution: Multi-Queen Racing

For critical-path tasks (tasks with many items in their `blocks` list), assign multiple Queens to work on the same task simultaneously. First to commit wins.

### Rules

1. **Identify bottleneck tasks**: Tasks that have many downstream dependents (large `blocks` list)
2. **Trigger condition**: When a bottleneck task becomes Ready AND there are more idle Queens than ready tasks
3. **Winner takes all**: First Queen to complete gets their work reviewed by Infestor
4. **Losers stop**: All other Queens on that task are cancelled, worktrees synced, reassigned to new tasks
5. **Normal tasks unaffected**: Non-bottleneck tasks still get 1:1 assignment

## Architecture Changes

### 1. Data Model Changes

#### `DagTaskStatus` enum (in `core/task_dag.rs`)

**Current:**
```rust
pub enum DagTaskStatus {
    Blocked,
    Ready,
    Assigned(QueenId),  // Single Queen
    InProgress,
    Validating,
    Completed,
    Failed { error: String, attempts: usize },
}
```

**New:**
```rust
pub enum DagTaskStatus {
    Blocked,
    Ready,
    /// Task assigned to one Queen (normal mode)
    Assigned(QueenId),
    /// Task assigned to multiple Queens (zerg rush mode)
    /// Queens are racing — first to complete wins
    ZergRush {
        /// All Queens assigned to this task
        queens: Vec<QueenId>,
        /// First Queen to complete (if any)
        winner: Option<QueenId>,
    },
    InProgress,
    Validating,
    Completed,
    Failed { error: String, attempts: usize },
}
```

#### `DagTask` struct (in `core/task_dag.rs`)

**Current:**
```rust
pub struct DagTask {
    pub assigned_to: Option<QueenId>,  // Single Queen
    // ... other fields
}
```

**New:**
```rust
pub struct DagTask {
    /// Which Queen(s) are assigned to this task
    /// - None: not assigned
    /// - Some(vec![queen]): single Queen (normal)
    /// - Some(vec![q1, q2, ...]): multiple Queens (zerg rush)
    pub assigned_to: Option<Vec<QueenId>>,
    // ... other fields
}
```

### 2. TaskDag API Changes

Add new methods to `TaskDag`:

```rust
impl TaskDag {
    /// Assign a task to multiple Queens (zerg rush mode).
    /// Returns true if successful.
    pub fn assign_zerg(&mut self, task_id: &str, queen_ids: Vec<QueenId>) -> bool {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.status = DagTaskStatus::ZergRush {
                queens: queen_ids.clone(),
                winner: None,
            };
            task.assigned_to = Some(queen_ids);
            task.started_at = Some(Utc::now());
            true
        } else {
            false
        }
    }

    /// Mark the first Queen to complete a zerg rush task as the winner.
    /// Returns list of loser Queens that need to be cancelled.
    pub fn zerg_winner(&mut self, task_id: &str, winner: QueenId) -> Vec<QueenId> {
        if let Some(task) = self.tasks.get_mut(task_id) {
            if let DagTaskStatus::ZergRush { queens, winner: ref mut w } = &mut task.status {
                *w = Some(winner.clone());
                // Return all Queens except the winner
                queens.iter()
                    .filter(|q| *q != &winner)
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    }

    /// Check if a task is in zerg rush mode.
    pub fn is_zerg_task(&self, task_id: &str) -> bool {
        self.tasks.get(task_id)
            .map(|t| matches!(t.status, DagTaskStatus::ZergRush { .. }))
            .unwrap_or(false)
    }

    /// Get all Queens assigned to a task (handles both normal and zerg mode).
    pub fn assigned_queens(&self, task_id: &str) -> Vec<QueenId> {
        self.tasks.get(task_id)
            .and_then(|t| t.assigned_to.clone())
            .unwrap_or_default()
    }

    /// Calculate bottleneck score for a task.
    /// Higher score = more important bottleneck.
    pub fn bottleneck_score(&self, task_id: &str) -> usize {
        self.tasks.get(task_id)
            .map(|t| t.blocks.len())
            .unwrap_or(0)
    }

    /// Get bottleneck tasks sorted by score (highest first).
    /// A bottleneck task is one that blocks many other tasks.
    pub fn bottleneck_tasks(&self, min_score: usize) -> Vec<&DagTask> {
        let mut tasks: Vec<&DagTask> = self.tasks.values()
            .filter(|t| matches!(t.status, DagTaskStatus::Ready) && t.blocks.len() >= min_score)
            .collect();
        tasks.sort_by_key(|t| std::cmp::Reverse(t.blocks.len()));
        tasks
    }
}
```

### 3. Scheduling Changes (in `nydus/mod.rs`)

#### New Config Field

```rust
pub struct NydusConfig {
    // ... existing fields

    /// Enable zerg rush for bottleneck tasks
    pub zerg_rush_enabled: bool,

    /// Minimum blocks count to trigger zerg rush
    pub zerg_rush_min_bottleneck: usize,

    /// Maximum Queens to assign to a single zerg task
    pub zerg_rush_max_queens: usize,
}

impl Default for NydusConfig {
    fn default() -> Self {
        Self {
            // ... existing defaults
            zerg_rush_enabled: true,
            zerg_rush_min_bottleneck: 3,  // Task must block 3+ tasks
            zerg_rush_max_queens: 3,       // Max 3 Queens per zerg task
        }
    }
}
```

#### Zerg Rush Detection Logic

Add new method to `Nydus`:

```rust
impl Nydus {
    /// Determine if we should trigger zerg rush for bottleneck tasks.
    ///
    /// Conditions:
    /// 1. Zerg rush is enabled in config
    /// 2. There are bottleneck tasks (blocks >= min_bottleneck)
    /// 3. There are more idle Queens than ready tasks (excess capacity)
    fn should_zerg_rush(&self) -> bool {
        if !self.config.zerg_rush_enabled {
            return false;
        }

        let ready_tasks = self.task_dag.ready_tasks();
        let idle_queens: Vec<_> = self.handles.iter()
            .filter(|(_, h)| matches!(h.status(), QueenStatus::Idle))
            .collect();

        // Need excess idle Queens
        if idle_queens.len() <= ready_tasks.len() {
            return false;
        }

        // Need at least one bottleneck task
        let bottleneck_tasks = self.task_dag.bottleneck_tasks(
            self.config.zerg_rush_min_bottleneck
        );

        !bottleneck_tasks.is_empty()
    }

    /// Plan zerg rush assignments.
    ///
    /// Returns (task_id, queen_ids) tuples for zerg rush assignments.
    fn plan_zerg_rush(&self, idle_queens: &[QueenId]) -> Vec<(String, Vec<QueenId>)> {
        let mut assignments = Vec::new();
        let mut available_queens = idle_queens.to_vec();

        let bottleneck_tasks = self.task_dag.bottleneck_tasks(
            self.config.zerg_rush_min_bottleneck
        );

        for task in bottleneck_tasks {
            if available_queens.len() < 2 {
                break;  // Need at least 2 Queens for zerg rush
            }

            // Assign 2 to max_queens Queens to this bottleneck
            let num_queens = std::cmp::min(
                available_queens.len(),
                self.config.zerg_rush_max_queens
            );

            let assigned: Vec<QueenId> = available_queens
                .drain(..num_queens)
                .collect();

            eprintln!(
                "[Nydus] ZERG RUSH: Assigning {} Queens to bottleneck task {} (blocks {} tasks)",
                assigned.len(),
                task.id,
                task.blocks.len()
            );

            assignments.push((task.id.clone(), assigned));
        }

        assignments
    }
}
```

#### Modified `try_schedule_with_preference`

Replace the current 1:1 zip logic with zerg-aware logic:

```rust
async fn try_schedule_with_preference(&mut self, preferred_queen: Option<&QueenId>) -> Result<usize> {
    let ready_tasks = self.task_dag.ready_tasks();
    if ready_tasks.is_empty() {
        return Ok(0);
    }

    let mut idle_queens: Vec<QueenId> = self.handles.iter()
        .filter(|(_, handle)| matches!(handle.status(), QueenStatus::Idle))
        .map(|(id, _)| id.clone())
        .collect();

    if idle_queens.is_empty() {
        return Ok(0);
    }

    // Work stealing optimization: put preferred queen first if idle
    if let Some(preferred) = preferred_queen {
        if let Some(pos) = idle_queens.iter().position(|q| q == preferred) {
            idle_queens.swap(0, pos);
        }
    }

    let mut assigned = 0;

    // PHASE 1: Zerg Rush (if enabled and conditions met)
    if self.should_zerg_rush() {
        let zerg_assignments = self.plan_zerg_rush(&idle_queens);

        for (task_id, queen_ids) in zerg_assignments {
            // Mark as zerg rush in DAG
            self.task_dag.assign_zerg(&task_id, queen_ids.clone());

            // Get task details
            let dag_task = self.task_dag.get(&task_id).unwrap();
            let description = dag_task.description.clone();
            let priority = dag_task.priority as u8;
            let blocked_by: Vec<TaskId> = dag_task.blocked_by.iter()
                .map(|id| TaskId(id.clone()))
                .collect();
            let created_at = dag_task.created_at;
            let skill_hint = dag_task.skill_hint.clone();

            // Build context
            let knowledge = self.build_knowledge_context();
            let knowledge_entries = self.build_knowledge_entries();
            let other_tasks_summary = self.build_other_tasks_summary(&task_id);

            // Assign to ALL Queens in parallel
            for queen_id in &queen_ids {
                let task = Task {
                    id: TaskId(task_id.clone()),
                    description: description.clone(),
                    status: TaskStatus::Assigned,
                    assigned_to: Some(queen_id.clone()),
                    priority,
                    blocked_by: blocked_by.clone(),
                    created_at,
                };

                let task_context = TaskContext {
                    knowledge: knowledge.clone(),
                    recent_messages: Vec::new(),
                    shared_state: HashMap::new(),
                    skill_hint: skill_hint.clone(),
                    knowledge_entries: knowledge_entries.clone(),
                    other_tasks_summary: Some(other_tasks_summary.clone()),
                    rejection_feedback: None,
                };

                if let Some(handle) = self.handles.get(queen_id) {
                    if let Err(e) = handle.assign(task, task_context).await {
                        eprintln!("[Nydus] Failed to assign zerg task {} to {}: {}",
                                  task_id, queen_id.0, e);
                        continue;
                    }
                    assigned += 1;

                    // Remove from idle_queens
                    if let Some(pos) = idle_queens.iter().position(|q| q == queen_id) {
                        idle_queens.remove(pos);
                    }
                } else {
                    eprintln!("[Nydus] No handle for queen {} in zerg rush", queen_id.0);
                }
            }
        }
    }

    // PHASE 2: Normal 1:1 assignment for remaining tasks and Queens
    let remaining_tasks: Vec<_> = ready_tasks.iter()
        .filter(|t| !self.task_dag.is_zerg_task(&t.id))
        .collect();

    let normal_assignments: Vec<_> = remaining_tasks.iter()
        .zip(idle_queens.iter())
        .map(|(task, queen)| (task.id.clone(), queen.clone()))
        .collect();

    for (task_id, queen_id) in normal_assignments {
        // ... existing 1:1 assignment logic (unchanged)
        self.task_dag.assign(&task_id, queen_id.clone());
        // ... rest of existing assignment code
        assigned += 1;
    }

    Ok(assigned)
}
```

### 4. Event Handling Changes (in `nydus/mod.rs`)

Modify the `QueenEvent::TaskCompleted` handler to detect zerg rush tasks:

```rust
async fn handle_queen_event(&mut self, event: QueenEvent) -> Result<()> {
    match event {
        QueenEvent::TaskCompleted { queen_id, task_id, result_text, cost_usd, .. } => {
            // Check if this is a zerg rush task
            if self.task_dag.is_zerg_task(&task_id.0) {
                // This is the FIRST Queen to complete the zerg task
                eprintln!(
                    "[Nydus] ZERG WINNER: {} completed zerg task {} first!",
                    queen_id.0, task_id.0
                );

                // Mark winner in DAG and get losers
                let losers = self.task_dag.zerg_winner(&task_id.0, queen_id.clone());

                eprintln!(
                    "[Nydus] Cancelling {} loser Queens: {:?}",
                    losers.len(),
                    losers.iter().map(|q| &q.0).collect::<Vec<_>>()
                );

                // Cancel loser Queens
                for loser_id in &losers {
                    if let Err(e) = self.cancel_queen_task(loser_id, &task_id.0).await {
                        eprintln!("[Nydus] Failed to cancel loser {}: {}", loser_id.0, e);
                    }
                }

                // Sync loser worktrees (before merge, so they can start new work clean)
                if let Some(ref worktree_mgr) = self.worktree_mgr {
                    for loser_id in &losers {
                        eprintln!("[Nydus] Syncing loser {} worktree with base", loser_id.0);
                        if let Err(e) = worktree_mgr.sync_with_base(loser_id) {
                            eprintln!("[Nydus] Failed to sync loser worktree: {}", e);
                        }
                    }
                }

                // Proceed with normal Infestor review for the winner
                // ... existing Infestor review logic
            } else {
                // Normal task completion (not zerg rush)
                // ... existing completion logic
            }

            // ... rest of existing handler
        }
        // ... other event handlers
    }
}
```

Add new method to cancel Queens:

```rust
impl Nydus {
    /// Cancel a Queen's current task.
    ///
    /// Sends a special cancellation message to the Queen, telling them to stop
    /// working on the specified task immediately.
    async fn cancel_queen_task(&mut self, queen_id: &QueenId, task_id: &str) -> Result<()> {
        if let Some(handle) = self.handles.get(queen_id) {
            let cancel_msg = SwarmMessage {
                id: format!("cancel-{}-{}", queen_id.0, task_id),
                from: AgentId::Nydus(self.id.clone()),
                to: AgentId::Queen(queen_id.clone()),
                msg_type: MessageType::Command,
                payload: serde_json::json!({
                    "command": "cancel_task",
                    "task_id": task_id,
                    "reason": "zerg_rush_loser"
                }),
                timestamp: Utc::now(),
                correlation_id: None,
                visibility: Visibility::default_internal(),
            };

            handle.send_message(cancel_msg).await?;
        }
        Ok(())
    }
}
```

### 5. Infestor Integration

The Infestor only reviews the FIRST completion (the winner). Subsequent completions from losers are ignored.

Add tracking to prevent duplicate reviews:

```rust
impl Nydus {
    /// Track which tasks are currently under Infestor review.
    /// Key: task_id, Value: queen_id of the winner being reviewed.
    in_review: HashMap<String, QueenId>,
}

// In handle_queen_event for TaskCompleted:
if self.task_dag.is_zerg_task(&task_id.0) {
    // Check if already under review
    if self.in_review.contains_key(&task_id.0) {
        eprintln!(
            "[Nydus] Ignoring late completion from {} for zerg task {} (already under review)",
            queen_id.0, task_id.0
        );
        return Ok(());
    }

    // Mark as under review
    self.in_review.insert(task_id.0.clone(), queen_id.clone());

    // Mark winner and cancel losers (existing code)
    let losers = self.task_dag.zerg_winner(&task_id.0, queen_id.clone());
    // ...
}

// In handle_infestor_approve / handle_infestor_reject:
// Remove from in_review after decision
self.in_review.remove(&task_id);
```

### 6. Edge Cases

#### A. First Queen's work is REJECTED by Infestor

When Infestor rejects the winner's work:

```rust
async fn handle_infestor_reject(&mut self, queen_id: &QueenId, task_id: &str, reason: &str) -> Result<()> {
    // Remove from in_review
    self.in_review.remove(task_id);

    // If this was a zerg task, check if there are other Queens still working
    if self.task_dag.is_zerg_task(task_id) {
        let assigned_queens = self.task_dag.assigned_queens(task_id);

        // If there are other Queens still assigned (we cancelled them but they might not
        // have stopped yet), wait for their completion
        if assigned_queens.len() > 1 {
            eprintln!(
                "[Nydus] Zerg winner {} rejected, waiting for other Queens' attempts",
                queen_id.0
            );
            // Reset zerg status but keep other Queens assigned
            // Next completion will trigger review again
            return Ok(());
        }
    }

    // Normal rejection flow: requeue with feedback
    let requeued = self.task_dag.requeue_with_feedback(task_id, reason.to_string());
    if requeued {
        eprintln!("[Nydus] Requeued task {} with rejection feedback", task_id);

        // Sync ALL worktrees to discard rejected changes
        if let Some(ref worktree_mgr) = self.worktree_mgr {
            worktree_mgr.sync_all_with_base()?;
        }
    }

    Ok(())
}
```

#### B. ALL Queens fail the zerg task

If all Queens fail to complete the task (all return TaskFailed), transition to normal retry logic:

```rust
QueenEvent::TaskFailed { queen_id, task_id, error, .. } => {
    if self.task_dag.is_zerg_task(&task_id.0) {
        // Check how many Queens have failed this zerg task
        let assigned_queens = self.task_dag.assigned_queens(&task_id.0);

        // Mark this Queen as failed (track separately)
        // ... tracking logic

        // If ALL Queens assigned to this zerg task have failed, mark task as failed
        if all_queens_failed {
            eprintln!("[Nydus] All Queens failed zerg task {}", task_id.0);
            self.task_dag.fail(&task_id.0, error);
        }
    } else {
        // Normal failure handling
        self.task_dag.fail(&task_id.0, error);
    }
}
```

#### C. Race condition: Multiple completions arrive simultaneously

The first `TaskCompleted` event to be processed wins. The `in_review` HashMap ensures only one review happens:

```rust
// This is already handled by the in_review check:
if self.in_review.contains_key(&task_id.0) {
    // Another Queen's completion is already being reviewed
    return Ok(());
}
```

#### D. Queen dies while working on zerg task

Use existing recovery logic — `recover_stuck_tasks` will reset the task to Ready, and next schedule will reassign (possibly with zerg rush again if still a bottleneck).

### 7. Worktree Management

After the winner's work is merged, sync all loser worktrees:

```rust
async fn handle_infestor_approve(&mut self, queen_id: &QueenId, task_id: &str, summary: &str) -> Result<()> {
    // Remove from in_review
    self.in_review.remove(task_id);

    // Merge winner's branch
    if let Some(ref worktree_mgr) = self.worktree_mgr {
        let merge_result = worktree_mgr.merge_to_base(queen_id)?;

        match merge_result {
            MergeResult::Merged { commit_sha } => {
                eprintln!("[Nydus] Merged {} to main: {}", queen_id.0, commit_sha);

                // Sync ALL other Queens' worktrees with new main
                worktree_mgr.sync_all_with_base()?;
                eprintln!("[Nydus] Synced all worktrees with merged changes");
            }
            _ => {
                eprintln!("[Nydus] Merge failed for winner {}", queen_id.0);
                // ... handle merge failure
            }
        }
    }

    // Mark task as completed
    self.task_dag.complete(task_id, DagTaskResult {
        success: true,
        output: summary.to_string(),
        files_modified: Vec::new(),
    });

    Ok(())
}
```

## Implementation Plan

### Phase 1: Data Model (1-2 hours)

- [ ] Update `DagTaskStatus` enum with `ZergRush` variant
- [ ] Update `DagTask.assigned_to` to `Option<Vec<QueenId>>`
- [ ] Add `TaskDag::assign_zerg`, `zerg_winner`, `is_zerg_task`, `bottleneck_score`, `bottleneck_tasks` methods
- [ ] Update `TaskDag::stats` to handle zerg tasks
- [ ] Fix compilation errors in existing code

### Phase 2: Scheduling Logic (2-3 hours)

- [ ] Add config fields: `zerg_rush_enabled`, `zerg_rush_min_bottleneck`, `zerg_rush_max_queens`
- [ ] Implement `should_zerg_rush()` method
- [ ] Implement `plan_zerg_rush()` method
- [ ] Update `try_schedule_with_preference` with two-phase logic (zerg + normal)
- [ ] Test scheduling with mock tasks and Queens

### Phase 3: Event Handling (2-3 hours)

- [ ] Add `in_review` HashMap to Nydus
- [ ] Update `handle_queen_event` for TaskCompleted with zerg detection
- [ ] Implement `cancel_queen_task()` method
- [ ] Update `handle_infestor_approve` to sync all worktrees
- [ ] Update `handle_infestor_reject` to handle zerg rejection case
- [ ] Handle TaskFailed for zerg tasks

### Phase 4: Edge Cases & Testing (2-3 hours)

- [ ] Test: Winner rejected, wait for other Queens
- [ ] Test: All Queens fail zerg task
- [ ] Test: Race condition with simultaneous completions
- [ ] Test: Queen dies during zerg task
- [ ] Test: Worktree sync after merge
- [ ] Test: Normal tasks unaffected when zerg is active

### Phase 5: Observability (1 hour)

- [ ] Add logging for zerg rush triggers
- [ ] Add metrics for zerg tasks (how many, success rate, etc.)
- [ ] Update `SwarmProgress` to show zerg task count
- [ ] Add IPC protocol support for querying zerg status

## Configuration Example

```rust
let config = NydusConfig {
    max_queens: 6,
    zerg_rush_enabled: true,
    zerg_rush_min_bottleneck: 3,      // Task must block 3+ downstream tasks
    zerg_rush_max_queens: 3,          // Assign max 3 Queens to bottleneck
    // ... other config
};
```

## Example Scenario

### Initial State

```
DAG:
  A (Ready, blocks: [B, C, D, E])  <- Bottleneck (blocks 4 tasks)
  B (Blocked by A)
  C (Blocked by A)
  D (Blocked by A)
  E (Blocked by A)
  F (Ready, blocks: [])

Queens: Q1, Q2, Q3, Q4 (all Idle)
```

### Zerg Rush Triggered

```
Schedule decision:
  - Bottleneck A blocks 4 tasks (>= min_bottleneck=3)
  - 4 idle Queens, 2 ready tasks (A, F)
  - Excess capacity: 4 - 2 = 2 extra Queens

Action:
  - Assign Q1, Q2, Q3 to task A (zerg rush, max_queens=3)
  - Assign Q4 to task F (normal)

DAG:
  A (ZergRush, queens: [Q1, Q2, Q3], winner: None)
  F (Assigned(Q4))
```

### Q2 Completes First

```
Event: TaskCompleted from Q2 for task A
Action:
  1. Mark Q2 as winner in DAG
  2. Get losers: [Q1, Q3]
  3. Cancel Q1 and Q3
  4. Sync Q1 and Q3 worktrees to main
  5. Send task A to Infestor for review

DAG:
  A (ZergRush, queens: [Q1, Q2, Q3], winner: Some(Q2))

Queens:
  Q1: Cancelled, worktree synced, Idle
  Q2: Waiting for Infestor review
  Q3: Cancelled, worktree synced, Idle
  Q4: Still working on F
```

### Infestor Approves Q2's Work

```
Event: Infestor TaskCompleted with VERDICT: APPROVE
Action:
  1. Merge Q2's branch to main
  2. Sync all worktrees (Q1, Q3, Q4) to new main
  3. Mark task A as Completed
  4. refresh_readiness() -> B, C, D, E become Ready
  5. Next schedule: Assign Q1, Q2, Q3 to B, C, D (normal 1:1)

DAG:
  A (Completed)
  B (Assigned(Q1))
  C (Assigned(Q2))
  D (Assigned(Q3))
  E (Ready)
  F (Assigned(Q4))

Pipeline unblocked — all Queens working!
```

## Performance Considerations

### Cost

- Zerg rush increases LLM API costs (multiple Queens working on same task)
- Tradeoff: Cost vs. time-to-completion
- Config knobs allow tuning (min_bottleneck, max_queens)

### When to Enable

- Large DAGs with clear critical paths
- High-value bottleneck tasks (architecture, API design)
- Abundant Queen capacity (many idle Queens)

### When to Disable

- Small DAGs (no bottlenecks)
- Cost-sensitive workloads
- Limited Queen capacity

## Metrics to Track

- **Zerg Rush Triggers**: How often zerg rush activates
- **Zerg Win Rate**: % of zerg tasks where first Queen's work is approved
- **Time Savings**: Wallclock time saved by parallelizing bottlenecks
- **Cost Overhead**: Extra LLM costs from cancelled work
- **Bottleneck Distribution**: Which tasks trigger zerg rush most often

## Future Enhancements

### Dynamic Threshold

Adjust `zerg_rush_min_bottleneck` based on DAG size and Queen pool size.

### Winner Prediction

Use historical data to predict which Queen is most likely to succeed, assign more weight to them.

### Partial Credit

If a loser's work is similar to the winner's, offer them a related unblocked task (avoid wasted context).

### Cost Budget

Limit zerg rush based on cost budget (e.g., max $5 per bottleneck task).

## Conclusion

Zerg Rush eliminates critical-path bottlenecks by parallelizing bottleneck tasks across multiple Queens. The implementation is surgical and minimal:

- **3 new enum variants** (ZergRush status, tracking fields)
- **5 new TaskDag methods** (assign_zerg, zerg_winner, is_zerg_task, bottleneck_score, bottleneck_tasks)
- **2-phase scheduling** (zerg rush first, then normal)
- **Winner detection + loser cancellation** in event handler
- **Worktree sync** after merge

The result: Faster DAG completion, better Queen utilization, at the cost of increased LLM API usage for bottleneck tasks.
