# Hatchery PRD: Queen Mode

**Version:** 1.0
**Status:** Draft
**Priority:** P0

## Overview

Queen mode is the foundational swarm orchestration pattern for Hatchery — a pure Rust rewrite of the Ralph autonomous iteration script. It spawns N independent PipeProcess workers that execute PRD checkboxes in parallel, with no shared state and no coordinator. Each worker operates completely independently, iterating through assigned tasks until completion or failure.

The mode is named after StarCraft's Queen: a unit that injects larvae into Hatcheries, providing minimal "dumb" production support. Workers iterate PRD checkboxes independently with no coordination overhead.

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                       Hatchery Queen                             │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │                    Main Orchestrator                       │ │
│  │  - Reads PRD markdown                                      │ │
│  │  - Divides checkboxes among N workers                      │ │
│  │  - Spawns PipeProcess workers                              │ │
│  │  - Monitors worker output (NDJSON)                         │ │
│  │  - Displays unified progress bar                           │ │
│  │  - Collects results + generates summary                    │ │
│  └────────────────────────────────────────────────────────────┘ │
│                             │                                    │
│        ┌────────────────────┼────────────────────┐               │
│        ▼                    ▼                    ▼               │
│  ┌──────────┐         ┌──────────┐         ┌──────────┐         │
│  │ Worker 1 │         │ Worker 2 │   ...   │ Worker N │         │
│  │          │         │          │         │          │         │
│  │ Tasks:   │         │ Tasks:   │         │ Tasks:   │         │
│  │ [0,N,2N] │         │ [1,N+1]  │         │ [2,N+2]  │         │
│  │          │         │          │         │          │         │
│  │ State:   │         │ State:   │         │ State:   │         │
│  │ - Index  │         │ - Index  │         │ - Index  │         │
│  │ - Iter#  │         │ - Iter#  │         │ - Iter#  │         │
│  │ - Stalls │         │ - Stalls │         │ - Stalls │         │
│  │ - Log    │         │ - Log    │         │ - Log    │         │
│  └──────────┘         └──────────┘         └──────────┘         │
│        │                    │                    │               │
│        └────────────────────┴────────────────────┘               │
│                             │                                    │
│                             ▼                                    │
│              ┌────────────────────────────┐                      │
│              │   PipeProcess (hub-core)   │                      │
│              │   - claude --output-format │                      │
│              │     stream-json            │                      │
│              │   - NDJSON stdout          │                      │
│              └────────────────────────────┘                      │
└──────────────────────────────────────────────────────────────────┘

Each worker has its own:
  - progress_{worker_id}.md (iteration log, errors, learnings)
  - task assignments (static, round-robin from PRD)
  - PipeProcess instance (independent Claude session)

No shared memory. No coordination. Pure parallel execution.
```

## User Stories

### US-1: PRD Markdown Parsing
**As a** Hatchery user, **I want** to provide a PRD markdown file with `[ ]` checkboxes, **so that** workers can autonomously execute tasks.

#### Acceptance Criteria
- [ ] AC-1.1: Parser extracts all `[ ]` (uncompleted) checkboxes from markdown file
- [ ] AC-1.2: Parser preserves checkbox line numbers for accurate updates
- [ ] AC-1.3: Parser extracts task text from checkbox lines (content after `[ ]` or `[x]`)
- [ ] AC-1.4: Parser handles nested checkboxes (indented with spaces/tabs)
- [ ] AC-1.5: Parser ignores `[x]` (completed) checkboxes
- [ ] AC-1.6: Parser supports markdown code blocks containing `[ ]` without treating them as tasks
- [ ] AC-1.7: Parser returns structured TaskList with indices, text, and completion status
- [ ] AC-1.8: Parser handles empty PRD files gracefully (return empty task list)
- [ ] AC-1.9: Parser handles malformed checkbox syntax (`[]`, `[ x]`, `[X]`) robustly
- [ ] AC-1.10: Parser supports UTF-8 markdown files with emoji/unicode in task text

### US-2: Task Division Algorithm
**As a** Hatchery orchestrator, **I want** to divide PRD tasks evenly among N workers, **so that** workload is balanced.

#### Acceptance Criteria
- [ ] AC-2.1: Use round-robin assignment (worker 0 gets [0, N, 2N, ...], worker 1 gets [1, N+1, 2N+1, ...])
- [ ] AC-2.2: Handle case where N workers > total tasks (some workers get zero tasks)
- [ ] AC-2.3: Each worker receives a static task assignment at startup (no dynamic rebalancing)
- [ ] AC-2.4: Worker task assignment is logged to worker's progress file
- [ ] AC-2.5: Assignment algorithm is deterministic (same PRD + N always produces same division)
- [ ] AC-2.6: Support --workers CLI flag to set N (default: 4)
- [ ] AC-2.7: Validate N is between 1 and 32 (error if outside range)
- [ ] AC-2.8: Assignment respects task dependencies (if task B depends on A, assign both to same worker)
- [ ] AC-2.9: Dependency detection uses checkbox indentation (nested = depends on parent)
- [ ] AC-2.10: Workers with zero assigned tasks exit immediately with success status

### US-3: PipeProcess Worker Spawning
**As a** Hatchery orchestrator, **I want** to spawn N independent Claude sessions via PipeProcess, **so that** workers execute tasks in parallel.

#### Acceptance Criteria
- [ ] AC-3.1: Each worker spawned with `PipeProcess::new(CliTool::ClaudeCode, working_dir, initial_prompt)`
- [ ] AC-3.2: Initial prompt includes full PRD content + assigned task indices
- [ ] AC-3.3: Initial prompt instructs Claude to implement ONE task, mark it `[x]`, output verification command
- [ ] AC-3.4: Workers spawn with `--output-format stream-json --dangerously-skip-permissions --verbose`
- [ ] AC-3.5: Each worker has unique working directory or isolated state (avoid file conflicts)
- [ ] AC-3.6: Worker spawn failures are logged and counted toward total failures
- [ ] AC-3.7: If any worker fails to spawn, Hatchery continues with remaining workers
- [ ] AC-3.8: Worker process IDs are tracked for kill/cleanup on Ctrl+C
- [ ] AC-3.9: Workers inherit environment variables from parent (ANTHROPIC_API_KEY, etc.)
- [ ] AC-3.10: Worker spawn uses tokio async runtime for non-blocking I/O

### US-4: NDJSON Output Parsing
**As a** worker monitor, **I want** to parse NDJSON output from PipeProcess, **so that** I can detect task completion, errors, and tool calls.

#### Acceptance Criteria
- [ ] AC-4.1: Use hub-core's `create_ndjson_parser(CliTool::ClaudeCode)` for parsing
- [ ] AC-4.2: Detect `CliEvent::SessionStart` and log worker initialization
- [ ] AC-4.3: Detect `CliEvent::AssistantText` and append to worker's output log
- [ ] AC-4.4: Detect `CliEvent::ToolCallStart` for verification commands (e.g., `cargo check`)
- [ ] AC-4.5: Detect `CliEvent::ToolCallResult` to determine success/failure of verification
- [ ] AC-4.6: Detect `CliEvent::TurnComplete` to mark end of iteration
- [ ] AC-4.7: Detect `CliEvent::SessionEnd` and finalize worker status
- [ ] AC-4.8: Detect `CliEvent::Error` and log to worker's error file
- [ ] AC-4.9: Handle malformed NDJSON gracefully (log warning, continue processing)
- [ ] AC-4.10: Buffer partial lines until complete NDJSON object received

### US-5: Task Iteration Loop
**As a** worker, **I want** to iterate through assigned tasks sequentially, **so that** I complete my workload.

#### Acceptance Criteria
- [ ] AC-5.1: Worker starts with first assigned task index
- [ ] AC-5.2: On successful verification, mark task `[x]` in PRD file
- [ ] AC-5.3: On successful verification, move to next task index
- [ ] AC-5.4: On failed verification, log error and retry same task (up to 3 retries)
- [ ] AC-5.5: After 3 failed retries, mark task `[!]` (failed) and skip to next task
- [ ] AC-5.6: Worker exits when all assigned tasks are completed or failed
- [ ] AC-5.7: Each iteration is logged to `progress_{worker_id}.md` with timestamp
- [ ] AC-5.8: Iteration log includes: task text, verification command, result, duration
- [ ] AC-5.9: Worker tracks total iterations and max iteration limit (default: 100)
- [ ] AC-5.10: Worker exits with error if max iterations reached without completing all tasks

### US-6: Verification Command Execution
**As a** worker, **I want** to run verification commands after task completion, **so that** I know implementation succeeded.

#### Acceptance Criteria
- [ ] AC-6.1: Default verification command is `cargo check` (configurable via `--verify` CLI flag)
- [ ] AC-6.2: Verification command extracted from Claude's tool calls (ToolCallStart with name="Bash")
- [ ] AC-6.3: If Claude doesn't call verification, use default command
- [ ] AC-6.4: Verification success = exit code 0
- [ ] AC-6.5: Verification failure = exit code != 0
- [ ] AC-6.6: Capture stdout/stderr from verification command to worker log
- [ ] AC-6.7: Verification timeout after 120 seconds (configurable via `--verify-timeout`)
- [ ] AC-6.8: Support multiple verification commands per task (e.g., `cargo check && cargo test`)
- [ ] AC-6.9: Verification runs in worker's working directory
- [ ] AC-6.10: Verification command failures trigger retry logic (see US-5)

### US-7: Git Commit on Success
**As a** worker, **I want** to create git commits after successful task completion, **so that** progress is versioned.

#### Acceptance Criteria
- [ ] AC-7.1: After successful verification, run `git add -A` in working directory
- [ ] AC-7.2: Run `git commit -m "Queen W{id}: {task_text}"` with worker ID and task text
- [ ] AC-7.3: Commit message includes iteration number and timestamp
- [ ] AC-7.4: Commit creation failures are logged but do not fail the task
- [ ] AC-7.5: Support `--no-git` flag to disable git commits
- [ ] AC-7.6: Detect if working directory is not a git repo (skip commits, log warning)
- [ ] AC-7.7: Handle git conflicts gracefully (log error, continue without commit)
- [ ] AC-7.8: Each commit has Co-Authored-By trailer: `Co-Authored-By: Hatchery Queen <worker_{id}@hatchery>`
- [ ] AC-7.9: Support `--git-branch` flag to commit to specific branch
- [ ] AC-7.10: Log git commit SHA to worker's progress file

### US-8: Stall Detection
**As a** worker monitor, **I want** to detect when workers are stalled (no progress for 2+ iterations), **so that** I can retry or escalate.

#### Acceptance Criteria
- [ ] AC-8.1: Track last successful task completion timestamp per worker
- [ ] AC-8.2: If 2 consecutive iterations fail verification, mark worker as "stalled"
- [ ] AC-8.3: On stall detection, log warning to worker's progress file
- [ ] AC-8.4: On stall, retry task with additional context: previous error messages + suggestion to try different approach
- [ ] AC-8.5: Maximum 3 stall retries per task before marking task `[!]` failed
- [ ] AC-8.6: Stall detection respects iteration timeout (iteration taking >10 minutes counts as stall)
- [ ] AC-8.7: Stalled workers continue to next task after max retries
- [ ] AC-8.8: Global stall counter tracks total stalls across all workers
- [ ] AC-8.9: If all workers stall simultaneously, pause and prompt user for intervention (optional)
- [ ] AC-8.10: Stall metrics included in final summary report

### US-9: Progress Log Compaction
**As a** worker, **I want** to compress progress logs when they exceed 20KB, **so that** disk space is conserved.

#### Acceptance Criteria
- [ ] AC-9.1: Check `progress_{worker_id}.md` file size after each iteration
- [ ] AC-9.2: If file size > 20KB, trigger compaction
- [ ] AC-9.3: Compaction keeps: last 5 iterations, all errors, all task completions
- [ ] AC-9.4: Compaction discards: verbose tool output, redundant success messages
- [ ] AC-9.5: Compaction preserves chronological order
- [ ] AC-9.6: Compacted sections marked with `<!-- COMPACTED: {original_size} bytes -->`
- [ ] AC-9.7: Compaction is atomic (write to temp file, rename on success)
- [ ] AC-9.8: Compaction failures logged but do not halt worker
- [ ] AC-9.9: Support `--no-compact` flag to disable compaction
- [ ] AC-9.10: Compaction algorithm is deterministic (same input → same output)

### US-10: Progress Bar Display
**As a** user, **I want** to see a unified progress bar showing overall task completion, **so that** I can monitor swarm progress.

#### Acceptance Criteria
- [ ] AC-10.1: Progress bar shows: `[####------] 40% (12/30 tasks) | W1: 3/8 | W2: 5/7 | W3: 4/8 | W4: 0/7`
- [ ] AC-10.2: Progress bar updates every 500ms (non-blocking)
- [ ] AC-10.3: Per-worker status shows: completed tasks / total assigned tasks
- [ ] AC-10.4: Progress bar color codes worker status: green (active), yellow (stalled), red (error)
- [ ] AC-10.5: Progress bar displays elapsed time: `Elapsed: 12m 34s`
- [ ] AC-10.6: Progress bar shows estimated time remaining based on completion rate
- [ ] AC-10.7: Progress bar fits within 80-column terminal width
- [ ] AC-10.8: Progress bar uses unicode box-drawing characters (fallback to ASCII if unsupported)
- [ ] AC-10.9: Progress bar includes spinner for active workers
- [ ] AC-10.10: Progress bar hides on completion and shows final summary

### US-11: CLI Interface
**As a** user, **I want** a clean CLI interface to launch Queen mode, **so that** I can easily start swarms.

#### Acceptance Criteria
- [ ] AC-11.1: Command syntax: `hatchery queen <prd.md> [OPTIONS]`
- [ ] AC-11.2: Support `--workers N` flag (default: 4, range: 1-32)
- [ ] AC-11.3: Support `--verify "command"` flag (default: "cargo check")
- [ ] AC-11.4: Support `--verify-timeout SECONDS` flag (default: 120)
- [ ] AC-11.5: Support `--max-iterations N` flag (default: 100)
- [ ] AC-11.6: Support `--no-git` flag to disable git commits
- [ ] AC-11.7: Support `--no-compact` flag to disable log compaction
- [ ] AC-11.8: Support `--working-dir PATH` flag to set working directory
- [ ] AC-11.9: Support `--help` and `--version` flags
- [ ] AC-11.10: Validate PRD file exists and is readable before spawning workers

### US-12: Error Handling
**As a** Hatchery orchestrator, **I want** robust error handling for worker failures, **so that** one failure doesn't crash the entire swarm.

#### Acceptance Criteria
- [ ] AC-12.1: Worker crashes are detected via `PipeProcess::is_running()` polling
- [ ] AC-12.2: Crashed workers are logged with exit code and last output
- [ ] AC-12.3: Crashed workers' tasks are marked `[!]` (failed) in PRD
- [ ] AC-12.4: Remaining workers continue execution after one worker crashes
- [ ] AC-12.5: Main orchestrator exits with code 1 if any worker crashed
- [ ] AC-12.6: Main orchestrator exits with code 0 if all workers completed successfully
- [ ] AC-12.7: Ctrl+C sends SIGTERM to all workers and waits for graceful shutdown (max 5s)
- [ ] AC-12.8: After 5s timeout, SIGKILL all remaining workers
- [ ] AC-12.9: API errors (rate limits, auth failures) logged to worker progress file
- [ ] AC-12.10: File I/O errors (PRD read, progress write) are retried up to 3 times

### US-13: Summary Report Generation
**As a** user, **I want** a summary report after swarm completion, **so that** I can review results.

#### Acceptance Criteria
- [ ] AC-13.1: Summary shows: total tasks, completed, failed, duration, cost estimate
- [ ] AC-13.2: Per-worker breakdown: tasks completed, iterations, stalls, errors
- [ ] AC-13.3: Summary lists all failed tasks with error messages
- [ ] AC-13.4: Summary includes git commit count and SHAs
- [ ] AC-13.5: Summary saved to `queen_summary_{timestamp}.md`
- [ ] AC-13.6: Summary written to stdout as well as file
- [ ] AC-13.7: Summary includes token usage (input/output) aggregated from all workers
- [ ] AC-13.8: Summary calculates cost estimate based on token usage and Claude pricing
- [ ] AC-13.9: Summary shows completion percentage and success rate
- [ ] AC-13.10: Summary includes average iteration time per task

### US-14: Worker Context Management
**As a** worker, **I want** to maintain iteration context across turns, **so that** Claude learns from previous failures.

#### Acceptance Criteria
- [ ] AC-14.1: First prompt includes: PRD, assigned tasks, verification command
- [ ] AC-14.2: Subsequent prompts include: previous errors, stall count, suggestions
- [ ] AC-14.3: Context includes last 3 iterations' results (success/failure, error messages)
- [ ] AC-14.4: Context includes compacted progress log (last 5 iterations)
- [ ] AC-14.5: Context trimmed to fit within Claude's context window (200K tokens)
- [ ] AC-14.6: Context trimming prioritizes: current task > recent errors > old successes
- [ ] AC-14.7: Context includes PRD section relevant to current task (±5 checkboxes)
- [ ] AC-14.8: Context includes suggestion to read relevant files if task mentions specific files
- [ ] AC-14.9: Context updated after each ToolCallResult event
- [ ] AC-14.10: Context persisted to worker state file for resume capability (future)

### US-15: PRD File Updates
**As a** worker, **I want** to update the PRD file with completion status, **so that** progress is visible.

#### Acceptance Criteria
- [ ] AC-15.1: On successful verification, update `[ ]` → `[x]` for completed task
- [ ] AC-15.2: On max retries exceeded, update `[ ]` → `[!]` for failed task
- [ ] AC-15.3: PRD updates are atomic (use file locking or temp file + rename)
- [ ] AC-15.4: PRD updates preserve original formatting (indentation, spacing)
- [ ] AC-15.5: PRD updates are append-only (no lines deleted, order preserved)
- [ ] AC-15.6: Concurrent PRD updates from multiple workers handled via file locking
- [ ] AC-15.7: File lock timeout after 5 seconds (retry up to 3 times)
- [ ] AC-15.8: Failed PRD updates logged but do not fail task completion
- [ ] AC-15.9: PRD file backed up before first update to `{prd}.queen.backup`
- [ ] AC-15.10: Support `--dry-run` flag that skips PRD updates (log only)

### US-16: Worker Prompt Engineering
**As a** Hatchery developer, **I want** optimized prompts for workers, **so that** Claude produces high-quality implementations.

#### Acceptance Criteria
- [ ] AC-16.1: Initial prompt includes system message defining worker role and constraints
- [ ] AC-16.2: Prompt instructs Claude to implement ONE task only, then stop
- [ ] AC-16.3: Prompt requires Claude to run verification command after implementation
- [ ] AC-16.4: Prompt includes examples of good task implementations (if available)
- [ ] AC-16.5: Prompt warns against skipping verification or marking tasks complete prematurely
- [ ] AC-16.6: Prompt includes PRD context (full file or relevant section)
- [ ] AC-16.7: Prompt includes task assignment (which checkboxes worker is responsible for)
- [ ] AC-16.8: Prompt includes iteration history (previous attempts, errors)
- [ ] AC-16.9: Prompt template loaded from `prompts/queen_worker.md` (customizable)
- [ ] AC-16.10: Prompt supports variable substitution: `{PRD}`, `{TASK_INDEX}`, `{VERIFY_CMD}`, `{CONTEXT}`

### US-17: Configuration File Support
**As a** power user, **I want** to define Queen configuration in a TOML file, **so that** I can reuse settings.

#### Acceptance Criteria
- [ ] AC-17.1: Support `hatchery queen --config queen.toml`
- [ ] AC-17.2: Config file schema: `workers`, `verify_command`, `verify_timeout`, `max_iterations`, `git_enabled`, `compaction_enabled`
- [ ] AC-17.3: Config file supports `[worker_overrides]` section for per-worker settings
- [ ] AC-17.4: CLI flags override config file values
- [ ] AC-17.5: Config file supports environment variable expansion: `verify_command = "${CARGO_BIN} check"`
- [ ] AC-17.6: Config validation on load (error if invalid values)
- [ ] AC-17.7: Config file can specify PRD path: `prd = "docs/implementation.md"`
- [ ] AC-17.8: Config file can specify working directory: `working_dir = "../zengeld-terminal"`
- [ ] AC-17.9: Generate example config with `hatchery queen --print-config > queen.toml`
- [ ] AC-17.10: Config file supports comments (`# comment`)

### US-18: Logging and Observability
**As a** developer, **I want** detailed logging from Hatchery, **so that** I can debug worker issues.

#### Acceptance Criteria
- [ ] AC-18.1: Use `tracing` crate for structured logging
- [ ] AC-18.2: Log levels: TRACE (NDJSON events), DEBUG (worker state), INFO (progress), WARN (stalls), ERROR (failures)
- [ ] AC-18.3: Default log level: INFO (configurable via `RUST_LOG` env var)
- [ ] AC-18.4: Log to stdout with colored output (disable colors if not TTY)
- [ ] AC-18.5: Log to file `hatchery_{timestamp}.log` (optional, via `--log-file` flag)
- [ ] AC-18.6: Worker logs include worker ID prefix: `[W1] Completed task 0`
- [ ] AC-18.7: Orchestrator logs include timestamp and level
- [ ] AC-18.8: Logs include span context for tracing worker execution
- [ ] AC-18.9: Log API rate limit warnings before they occur (based on token usage)
- [ ] AC-18.10: Support `--quiet` flag to suppress INFO logs (show WARN and ERROR only)

### US-19: Graceful Shutdown
**As a** user, **I want** graceful shutdown on Ctrl+C, **so that** workers finish current tasks before exiting.

#### Acceptance Criteria
- [ ] AC-19.1: Register signal handler for SIGINT (Ctrl+C) and SIGTERM
- [ ] AC-19.2: On signal, set global shutdown flag and send stop signal to all workers
- [ ] AC-19.3: Workers finish current iteration before exiting (max 5 seconds)
- [ ] AC-19.4: After 5s timeout, forcefully kill workers with SIGKILL
- [ ] AC-19.5: Save partial progress to PRD file before exit
- [ ] AC-19.6: Save worker state to resume files (future feature)
- [ ] AC-19.7: Display shutdown message: "Shutting down gracefully... (Ctrl+C again to force)"
- [ ] AC-19.8: Second Ctrl+C immediately kills all workers
- [ ] AC-19.9: Generate summary report even on interrupted shutdown
- [ ] AC-19.10: Exit with code 130 (standard for SIGINT)

### US-20: Worker Resume Capability
**As a** user, **I want** to resume failed workers from saved state, **so that** I can recover from crashes.

#### Acceptance Criteria
- [ ] AC-20.1: Workers save state to `worker_{id}.state.json` after each iteration
- [ ] AC-20.2: State includes: current task index, iteration count, stall count, context
- [ ] AC-20.3: Support `hatchery queen --resume <state_dir>`
- [ ] AC-20.4: On resume, workers load state and continue from last task
- [ ] AC-20.5: Resume validates state version (error if incompatible)
- [ ] AC-20.6: Resume skips already-completed tasks (marked `[x]` in PRD)
- [ ] AC-20.7: Resume resets stall counters (fresh start)
- [ ] AC-20.8: Resume appends to existing progress logs
- [ ] AC-20.9: Resume generates new summary report (includes previous session data)
- [ ] AC-20.10: Resume mode logged in summary: "Resumed from iteration X"

## Technical Design

### Core Data Structures

```rust
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use zengeld_hub_core::{PipeProcess, CliTool, CliEvent};

/// Parsed PRD with task list.
#[derive(Debug, Clone)]
pub struct Prd {
    /// Path to original PRD file.
    pub path: PathBuf,
    /// All tasks extracted from PRD.
    pub tasks: Vec<Task>,
    /// Full markdown content.
    pub content: String,
}

/// A single task from the PRD.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Line number in PRD file (1-indexed).
    pub line_number: usize,
    /// Task text (content after `[ ]`).
    pub text: String,
    /// Completion status.
    pub status: TaskStatus,
    /// Indentation level (for dependency detection).
    pub indent_level: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Not started.
    Pending,
    /// In progress.
    InProgress,
    /// Completed successfully.
    Completed,
    /// Failed after retries.
    Failed,
}

/// Queen worker configuration.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub id: usize,
    pub working_dir: PathBuf,
    pub assigned_tasks: Vec<usize>, // Indices into Prd.tasks
    pub verify_command: String,
    pub verify_timeout: u64,
    pub max_iterations: usize,
    pub git_enabled: bool,
    pub compaction_enabled: bool,
}

/// Worker runtime state.
pub struct Worker {
    pub config: WorkerConfig,
    pub process: PipeProcess,
    pub current_task_index: usize,
    pub iteration_count: usize,
    pub stall_count: usize,
    pub context: WorkerContext,
    pub progress_log: PathBuf,
}

/// Worker context (accumulates across iterations).
#[derive(Debug, Clone, Default)]
pub struct WorkerContext {
    /// Last 3 iteration results.
    pub recent_iterations: Vec<IterationResult>,
    /// Last successful task completion time.
    pub last_success: Option<std::time::Instant>,
    /// Accumulated error messages.
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IterationResult {
    pub task_index: usize,
    pub success: bool,
    pub verification_output: String,
    pub duration: std::time::Duration,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Orchestrator configuration.
#[derive(Debug, Clone)]
pub struct QueenConfig {
    pub prd_path: PathBuf,
    pub workers: usize,
    pub verify_command: String,
    pub verify_timeout: u64,
    pub max_iterations: usize,
    pub working_dir: PathBuf,
    pub git_enabled: bool,
    pub compaction_enabled: bool,
    pub dry_run: bool,
}

impl Default for QueenConfig {
    fn default() -> Self {
        Self {
            prd_path: PathBuf::from("PRD.md"),
            workers: 4,
            verify_command: "cargo check".to_string(),
            verify_timeout: 120,
            max_iterations: 100,
            working_dir: std::env::current_dir().unwrap(),
            git_enabled: true,
            compaction_enabled: true,
            dry_run: false,
        }
    }
}
```

### PRD Parser Algorithm

```rust
use regex::Regex;

pub fn parse_prd(path: &Path) -> Result<Prd, Error> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();

    let checkbox_re = Regex::new(r"^(\s*)-\s*\[([ x!])\]\s+(.+)$")?;
    let mut tasks = Vec::new();

    for (line_num, line) in lines.iter().enumerate() {
        if let Some(caps) = checkbox_re.captures(line) {
            let indent = caps.get(1).map_or(0, |m| m.as_str().len());
            let status_char = caps.get(2).unwrap().as_str();
            let text = caps.get(3).unwrap().as_str().to_string();

            let status = match status_char {
                " " => TaskStatus::Pending,
                "x" => TaskStatus::Completed,
                "!" => TaskStatus::Failed,
                _ => continue,
            };

            tasks.push(Task {
                line_number: line_num + 1,
                text,
                status,
                indent_level: indent / 2, // Assuming 2-space indents
            });
        }
    }

    Ok(Prd { path: path.to_path_buf(), tasks, content })
}
```

### Task Assignment Algorithm

```rust
/// Divide tasks using round-robin, respecting dependencies.
pub fn assign_tasks(prd: &Prd, num_workers: usize) -> Vec<Vec<usize>> {
    let mut assignments: Vec<Vec<usize>> = vec![Vec::new(); num_workers];

    let pending_tasks: Vec<usize> = prd.tasks.iter().enumerate()
        .filter(|(_, t)| t.status == TaskStatus::Pending)
        .map(|(i, _)| i)
        .collect();

    // Simple round-robin (no dependency analysis for Queen v1)
    for (idx, task_index) in pending_tasks.iter().enumerate() {
        let worker_id = idx % num_workers;
        assignments[worker_id].push(*task_index);
    }

    assignments
}
```

### Worker Event Loop

```rust
impl Worker {
    pub async fn run(&mut self, prd: &Prd) -> Result<WorkerSummary, Error> {
        let mut parser = create_ndjson_parser(CliTool::ClaudeCode);

        for task_idx in &self.config.assigned_tasks {
            let task = &prd.tasks[*task_idx];
            self.current_task_index = *task_idx;

            // Iteration loop for this task
            let mut retries = 0;
            loop {
                self.iteration_count += 1;

                // Send task prompt
                let prompt = self.build_prompt(task, prd);
                self.process.write(&prompt)?;

                // Parse NDJSON output
                let mut verification_success = false;
                loop {
                    if let Some(line) = self.process.try_recv() {
                        for event in parser.parse_line(&line) {
                            match event {
                                CliEvent::ToolCallResult { is_error, output, .. } => {
                                    verification_success = !is_error;
                                    self.log_verification(output);
                                }
                                CliEvent::SessionEnd { is_error, .. } => {
                                    if is_error {
                                        retries += 1;
                                        self.stall_count += 1;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    if !self.process.is_running() {
                        break;
                    }
                }

                if verification_success {
                    self.mark_task_complete(prd, *task_idx)?;
                    self.git_commit(task)?;
                    break; // Move to next task
                } else if retries >= 3 {
                    self.mark_task_failed(prd, *task_idx)?;
                    break; // Skip to next task
                } else {
                    // Retry with additional context
                    self.update_context_with_failure();
                }
            }

            self.compact_progress_log_if_needed()?;
        }

        Ok(self.generate_summary())
    }

    fn build_prompt(&self, task: &Task, prd: &Prd) -> String {
        format!(
            "You are Worker {}. Implement this task:\n\n{}\n\nPRD context:\n{}\n\nRun this verification: {}\n\nPrevious attempts:\n{}",
            self.config.id,
            task.text,
            self.extract_prd_context(prd, self.current_task_index),
            self.config.verify_command,
            self.format_context()
        )
    }
}
```

## Dependencies

```toml
[dependencies]
zengeld-hub-core = { path = "../zengeld-hub/crates/core" }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive"] }
regex = "1"
chrono = "0.4"
tracing = "0.1"
tracing-subscriber = "0.3"
indicatif = "0.17" # Progress bars
anyhow = "1"
toml = "0.8"
parking_lot = "0.12" # File locking
```

## Verification

### US-1: PRD Parsing
```bash
# Create test PRD
cat > test.md <<EOF
- [ ] Task 1
- [x] Task 2 (completed)
- [ ] Task 3
  - [ ] Subtask 3.1
- [!] Task 4 (failed)
EOF

# Test parser
cargo run --bin hatchery -- queen test.md --dry-run --workers 2
# Expected: Parses 3 pending tasks (1, 3, 3.1), assigns [1, 3.1] to W0, [3] to W1
```

### US-2-5: Worker Spawning and Execution
```bash
# Create minimal PRD
cat > mini.md <<EOF
- [ ] Add function `pub fn hello() -> String { "world".to_string() }` to src/lib.rs
- [ ] Add test for hello() function
EOF

# Run with 2 workers
cargo run --bin hatchery -- queen mini.md --workers 2 --verify "cargo test"
# Expected: 2 workers spawn, W0 implements task 0, W1 implements task 1, both succeed
```

### US-6-7: Verification and Git Commits
```bash
# Run with git enabled
cargo run --bin hatchery -- queen mini.md --workers 1
# Expected: Task completion triggers cargo check, then git commit with message "Queen W0: Add function..."

git log -1 --oneline
# Expected: Shows commit with "Queen W0:" prefix
```

### US-8: Stall Detection
```bash
# Create impossible task
cat > stall.md <<EOF
- [ ] Implement a function that solves the halting problem
EOF

# Run with low retry limit
cargo run --bin hatchery -- queen stall.md --workers 1
# Expected: 3 verification failures → stall detected → task marked [!] → worker exits
```

### US-9: Log Compaction
```bash
# Create verbose PRD with many tasks
cargo run --bin hatchery -- queen large.md --workers 1
ls -lh progress_0.md
# Expected: File size stays under 20KB due to compaction, contains "COMPACTED" markers
```

### US-10: Progress Bar
```bash
# Run with 4 workers on 20 tasks
cargo run --bin hatchery -- queen large.md --workers 4
# Expected: Live progress bar showing per-worker status, updates every 500ms, green/yellow/red colors
```

### US-13: Summary Report
```bash
cargo run --bin hatchery -- queen test.md --workers 2
cat queen_summary_*.md
# Expected: Summary with total tasks, completed, failed, per-worker breakdown, token usage, cost estimate
```

### US-17: Config File
```bash
# Generate example config
hatchery queen --print-config > queen.toml

# Edit config, then run
hatchery queen --config queen.toml
# Expected: Uses config values, CLI flags override config
```

### US-19: Graceful Shutdown
```bash
# Run long task, press Ctrl+C after 5 seconds
cargo run --bin hatchery -- queen large.md --workers 4
# ^C after 5s
# Expected: "Shutting down gracefully..." message, workers finish current iteration, summary generated, exit code 130
```

### US-20: Resume
```bash
# Run task, kill mid-execution
cargo run --bin hatchery -- queen test.md --workers 2
# ^C
ls worker_*.state.json
# Expected: State files exist

# Resume
hatchery queen --resume .
# Expected: Workers load state, continue from last task, append to progress logs
```

## Open Questions

1. **Dependency Resolution**: Should Queen v1 support explicit task dependencies (e.g., via YAML frontmatter), or rely solely on indentation heuristics?
   - **Proposal**: v1 uses indentation only, v2 adds explicit dependency syntax.

2. **Worker Crash Recovery**: Should crashed workers be auto-restarted, or tasks reassigned to other workers?
   - **Proposal**: v1 marks tasks as failed, v2 adds task reassignment.

3. **Progress File Format**: Should progress logs be markdown or structured JSON?
   - **Proposal**: Markdown for human readability, with optional `--json-logs` flag for machine parsing.

4. **Verification Command Detection**: Should we parse Claude's intent (e.g., "I will run cargo check") or only look at actual Bash tool calls?
   - **Proposal**: Only trust actual tool calls, ignore natural language mentions.

5. **Token Budget**: Should workers have per-worker token budgets, or shared pool?
   - **Proposal**: v1 no explicit budgets (rely on API rate limits), v2 adds budget management.

6. **PRD Format**: Should we support alternative formats (YAML, JSON) or markdown-only?
   - **Proposal**: v1 markdown only, v2 adds YAML/JSON parsers.

7. **Model Selection**: Should workers use configurable models (Haiku, Sonnet, Opus), or hardcoded Sonnet?
   - **Proposal**: v1 hardcoded Sonnet, v2 adds `--model` flag.

8. **Rate Limit Handling**: Should workers pause on rate limits, or exit immediately?
   - **Proposal**: Workers pause and retry with exponential backoff (max 3 retries).

9. **PRD Updates Conflict Resolution**: If two workers update the same line simultaneously, which wins?
   - **Proposal**: File locking ensures atomic updates, last write wins (tasks should be disjoint anyway).

10. **Iteration Context Size**: Should context grow unbounded, or have max size (e.g., 50K tokens)?
    - **Proposal**: Max 50K tokens, trim oldest non-error content first.
