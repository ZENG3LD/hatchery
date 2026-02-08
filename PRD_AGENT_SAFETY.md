# PRD: Agent Safety & Git Isolation for Hatchery

## Overview

Add three layers of agent safety to Hatchery: improved git attribution (L1), git worktree isolation (L2), and safe-mode bash restrictions (L3). L1 is always-on, L2 and L3 are opt-in CLI flags.

## Architecture

```
HatcheryConfig
├── git_attribution: bool     (default: true)     ← L1
├── worktree_isolation: bool  (default: false)     ← L2, --worktree
└── safe_mode: bool           (default: false)     ← L3, --safe-mode
```

New module: `src/safety/` with submodules:
```
src/safety/
├── mod.rs          # Re-exports, SafetyConfig struct
├── worktree.rs     # Git worktree lifecycle (create/merge/cleanup)
└── policy.rs       # Safe-mode rules, prompt injection for restrictions
```

---

## Level 1: Git Attribution (always-on)

### What changes

- [ ] Add `Co-Authored-By: Hatchery-W{id} <hatchery@nemo>` to every auto-commit
- [ ] Queen: update `git_commit()` to include worker ID and mode tag in commit message format: `feat(queen/W{id}): {task}`
- [ ] SwarmHost: after worker completes task and orchestrator does verification, auto-commit with `feat(swarm/W{id}): {task}`
- [ ] BroodLord: auto-commit with `feat(brood/L2.{l2_id}.W{id}): {task}`
- [ ] All commits get `Co-Authored-By: Hatchery-W{id} <hatchery@nemo>` trailer

### Integration points

Queen `git_commit()` already exists — extend it with worker_id parameter.
SwarmHost/BroodLord: add `git_commit_task()` helper in `safety/mod.rs` that all modes call after successful task+verify.

---

## Level 2: Git Worktree Isolation (`--worktree`)

### Concept

Each worker gets its own `git worktree` on a separate branch. Workers edit files in their isolated worktree. When a task completes, the orchestrator merges the worker's branch back into the main branch.

### What changes

- [ ] `safety/worktree.rs`: `WorktreeManager` struct with `create()`, `merge()`, `cleanup()`, `prune()`
- [ ] `WorktreeManager::create(worker_id, base_branch)` → creates `git worktree add {path} -b hatchery/W{id}` from base branch
- [ ] Worktree path: `{working_dir}/.hatchery/worktrees/W{id}/`
- [ ] `WorktreeManager::merge(worker_id)` → `git merge --no-ff hatchery/W{id}` into base branch, handles conflicts
- [ ] `WorktreeManager::cleanup(worker_id)` → `git worktree remove`, `git branch -d`
- [ ] `WorktreeManager::prune()` → `git worktree prune` at startup to clean orphans
- [ ] On merge conflict: mark task as Failed, reset merge, let another worker retry

### Config changes

- [ ] Add `worktree_isolation: bool` to `HatcheryConfig`
- [ ] Add `--worktree` CLI flag to `Commands::Spawn`
- [ ] When enabled, each worker's `working_dir` becomes the worktree path instead of the shared working dir

### Queen integration

- [ ] Before spawning workers: call `WorktreeManager::prune()`
- [ ] Per worker: create worktree, set `PipeProcess` working_dir to worktree path
- [ ] After task success + verify: merge worktree branch, then cleanup
- [ ] On worker exit: cleanup worktree regardless

### SwarmHost integration

- [ ] Before spawning workers: prune stale worktrees
- [ ] In `spawn_workers()`: create worktree per worker, pass worktree path to `PipeProcess::new()`
- [ ] In completion handler (SessionEnd with verified success): merge + cleanup
- [ ] On stall/kill: cleanup worktree without merge

### BroodLord integration

- [ ] Same pattern per L2 worker — worktree path = `.hatchery/worktrees/L2.{l2_id}.W{id}/`
- [ ] Merge happens at L2 level, not global

### Tests

- [ ] Unit test: `WorktreeManager::create` creates worktree dir and branch
- [ ] Unit test: `WorktreeManager::cleanup` removes worktree and branch
- [ ] Integration: create → make changes → merge → verify changes in main

---

## Level 3: Safe Mode (`--safe-mode`)

### Concept

When enabled, worker prompts are augmented with explicit restrictions on dangerous operations. This is prompt-level enforcement (not process-level sandboxing).

### What changes

- [ ] `safety/policy.rs`: `SafetyPolicy` struct with `restrictions_prompt()` method
- [ ] The restrictions prompt is injected into every worker prompt when safe_mode is enabled
- [ ] Restrictions text defines: allowed commands, forbidden commands, file scope rules

### Restrictions content

- [ ] Create `src/safety/prompts/safe_mode.md` with:
  - Allowed bash: `cargo`, `git status`, `git diff`, `git add`, `git commit`, `cat`, `ls`, `find`, `grep`, `echo`, `mkdir`, `cp`, `mv`, `touch`, `rustc`, `rustfmt`, `clippy`
  - Forbidden bash: `rm -rf`, `git push`, `git push --force`, `git reset --hard`, `git clean`, `curl | sh`, `wget | sh`, `sudo`, `chmod 777`, `kill -9`, any pipe to `sh`/`bash`, `dd`, `mkfs`
  - File scope: "Only modify files directly related to your task. Do not delete files you didn't create."
  - Network: "Do not make network requests unless the task explicitly requires it."

### Config changes

- [ ] Add `safe_mode: bool` to `HatcheryConfig`
- [ ] Add `--safe-mode` CLI flag to `Commands::Spawn`

### Prompt integration

- [ ] `safety::policy::safe_mode_prompt()` returns the restriction text (include_str from safe_mode.md)
- [ ] Queen: append to `build_iteration_prompt()` when safe_mode enabled
- [ ] SwarmHost: append to worker prompt in task assignment loop when safe_mode enabled
- [ ] BroodLord: same as SwarmHost, append to worker prompt

### Tests

- [ ] Unit test: `safe_mode_prompt()` returns non-empty string containing "FORBIDDEN"
- [ ] Unit test: prompt contains all restricted commands

---

## CLI Changes Summary

```
hatchery spawn PRD.md --workers 4 --mode swarm-host \
    --worktree \          # Level 2: git worktree isolation
    --safe-mode \         # Level 3: bash restrictions
    --verify "cargo check"
```

Both `--worktree` and `--safe-mode` are independent flags, can be combined or used separately.

---

## Implementation Order

1. Create `src/safety/mod.rs`, `policy.rs`, `worktree.rs` module structure
2. L1: Git attribution — modify commits in all three modes
3. L3: Safe mode — write prompt, wire into all modes (simplest)
4. L2: Worktree isolation — implement WorktreeManager, integrate into all modes
5. Tests
6. cargo check + cargo test
7. Commit
