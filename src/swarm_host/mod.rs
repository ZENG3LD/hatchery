//! Swarm Host mode — AI Coordinator + smart workers with shared memory.
//!
//! Architecture:
//! - **Orchestrator** (this Rust code): spawns processes, routes events, manages SharedMemory
//! - **Coordinator** (Claude session): reads PRD, builds DAG, assigns tasks, monitors progress
//! - **Workers** (Claude sessions): execute tasks, report results, share knowledge
//!
//! Communication flow:
//! ```text
//! Coordinator --JSON commands--> Orchestrator --prompts--> Workers
//! Workers --@hatchery: commands--> Orchestrator --status--> Coordinator
//! Workers --@hatchery:knowledge--> SharedMemory --prompt injection--> Workers
//! ```

pub mod commands;
pub mod shared_memory;

use crate::prd;
use crate::progress;
use crate::safety;
use crate::types::{HatcheryConfig, SwarmResult};
use anyhow::Result;
use shared_memory::{SharedMemory, SwarmTask, Target, TaskResult, TaskStatus};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use zengeld_hub_core::{CliEvent, CliTool, PipeProcess};

/// Embedded prompts.
// Reserved for v2 (AI coordinator session)
#[allow(dead_code)]
const COORDINATOR_PROMPT: &str = include_str!("prompts/coordinator.md");
pub const WORKER_PROMPT: &str = include_str!("prompts/worker.md");

/// Worker state tracked by the orchestrator.
struct WorkerState {
    process: PipeProcess,
    parser: Box<dyn zengeld_hub_core::NdjsonParser + Send>,
    id: usize,
    current_task: Option<usize>,
    last_activity: Instant,
    output_buffer: String,
    /// Working directory for this worker (may be a worktree path).
    working_dir: std::path::PathBuf,
}

/// Run Swarm Host mode.
pub fn run(config: &HatcheryConfig) -> Result<SwarmResult> {
    let start = Instant::now();

    // Parse PRD into tasks
    let prd_tasks = prd::parse_prd(&config.prd_path)?;
    let _prd_content = std::fs::read_to_string(&config.prd_path)?;
    let (initial_done, total) = prd::progress(&prd_tasks);

    println!(
        "[SWARM HOST] {} tasks ({} done, {} remaining), {} workers",
        total,
        initial_done,
        total - initial_done,
        config.workers
    );

    if initial_done == total {
        println!("[SWARM HOST] All tasks already complete!");
        return Ok(SwarmResult {
            total_tasks: total,
            completed_tasks: total,
            total_iterations: 0,
            duration_secs: 0,
            workers_used: 0,
        });
    }

    // Initialize SharedMemory
    let persist_path = config
        .prd_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("shared_memory.json");
    let memory = SharedMemory::new(Some(persist_path), config.workers);

    // Build task list with dependency inference from indentation
    let swarm_tasks = build_swarm_tasks(&prd_tasks);
    memory.set_tasks(swarm_tasks);
    memory.refresh_task_readiness();

    println!(
        "[SWARM HOST] DAG built: {} ready tasks",
        memory.get_ready_tasks().len()
    );

    // L2: Worktree isolation
    let worktree_mgr = if config.worktree_isolation {
        let mgr = safety::worktree::WorktreeManager::new(&config.working_dir, None)?;
        mgr.prune()?;
        Some(mgr)
    } else {
        None
    };

    // Spawn workers (with optional worktree paths)
    let mut workers = spawn_workers(config, &worktree_mgr)?;
    println!("[SWARM HOST] Spawned {} workers", workers.len());

    // Main orchestration loop — no separate Coordinator process in v1,
    // we do coordination in Rust (more reliable than an LLM coordinator).
    // The Coordinator prompt is reserved for v2 where we can pass it context.
    let stall_timeout = Duration::from_secs(300); // 5 min default
    let poll_interval = Duration::from_millis(100);
    let status_interval = Duration::from_secs(10);
    let mut last_status = Instant::now();

    loop {
        // Check overall completion
        let (done, _) = memory.progress();
        if done == total {
            println!("\n[SWARM HOST] All tasks complete!");
            break;
        }

        // Check if all workers are dead and no tasks ready
        let mut alive = 0;
        for w in workers.values_mut() {
            if w.process.is_running() {
                alive += 1;
            }
        }
        if alive == 0 && memory.get_ready_tasks().is_empty() {
            let (done, _) = memory.progress();
            if done < total {
                println!("[SWARM HOST] All workers exited, {} tasks remain", total - done);
            }
            break;
        }

        // Refresh DAG readiness
        memory.refresh_task_readiness();

        // Assign ready tasks to idle workers
        let ready = memory.get_ready_tasks();
        for task_id in ready {
            // Find an idle worker — iterate keys to avoid double-borrow issues
            let mut idle_worker_id = None;
            for (id, w) in workers.iter_mut() {
                if w.current_task.is_none() && w.process.is_running() {
                    idle_worker_id = Some(*id);
                    break;
                }
            }
            let Some(worker_id) = idle_worker_id else { break };
            let worker = workers.get_mut(&worker_id).unwrap();
            {
                let task_text = memory.get_task_text(task_id).unwrap_or_default();
                let knowledge = memory.knowledge_summary();
                let messages = memory.drain_messages(worker.id);
                let msg_text = format_messages(&messages);
                let verify_cmd = config.verify_cmd.as_deref().unwrap_or("cargo check");

                let mut prompt = WORKER_PROMPT
                    .replace("{WORKER_ID}", &worker.id.to_string())
                    .replace("{TASK_ID}", &task_id.to_string())
                    .replace("{TASK}", &task_text)
                    .replace("{KNOWLEDGE}", &knowledge)
                    .replace("{MESSAGES}", &msg_text)
                    .replace("{VERIFY_CMD}", verify_cmd);

                // L3: Safe-mode restrictions
                let suffix = safety::safe_mode_suffix(config.safe_mode);
                if !suffix.is_empty() {
                    prompt.push_str("\n\n");
                    prompt.push_str(suffix);
                }

                // Send task to worker
                if worker.process.write(&prompt).is_ok() {
                    worker.current_task = Some(task_id);
                    worker.last_activity = Instant::now();
                    memory.assign_task(task_id, worker.id);
                    memory.update_task_status(task_id, TaskStatus::InProgress);
                    println!(
                        "[SWARM HOST] Assigned task {} to Worker {}: {}",
                        task_id,
                        worker.id,
                        truncate(&task_text, 60)
                    );
                }
            }
        }

        // Poll all workers for output
        for worker in workers.values_mut() {
            while let Some(line) = worker.process.try_recv() {
                worker.last_activity = Instant::now();
                let events = worker.parser.parse_line(&line);
                for event in events {
                    match event {
                        CliEvent::AssistantText { ref text, .. } => {
                            worker.output_buffer.push_str(text);

                            // Check for @hatchery commands
                            let cmds = commands::parse_commands(text);
                            for cmd in cmds {
                                handle_command(cmd, worker.id, &memory, config);
                            }

                            if config.verbose {
                                print!("[W{}] {}", worker.id, text);
                            }
                        }
                        CliEvent::Error { ref message } => {
                            eprintln!("[W{}] Error: {}", worker.id, message);
                        }
                        CliEvent::SessionEnd { .. } => {
                            // Worker finished its turn
                            if let Some(task_id) = worker.current_task.take() {
                                // Check if worker reported result via @hatchery:result
                                // If not, assume progress was made (heuristic)
                                let snap = memory.snapshot();
                                let task_status = snap
                                    .tasks
                                    .iter()
                                    .find(|t| t.id == task_id)
                                    .map(|t| t.status);

                                if task_status == Some(TaskStatus::InProgress) {
                                    // Worker didn't explicitly report — check verification
                                    if let Some(ref verify) = config.verify_cmd {
                                        if run_verify(verify, &config.working_dir) {
                                            memory.update_task_status(task_id, TaskStatus::Completed);
                                            // Mark PRD checkbox
                                            let fresh = prd::parse_prd(&config.prd_path).ok();
                                            if let Some(tasks) = fresh {
                                                if let Some(t) = tasks.iter().find(|t| t.id == task_id && !t.done) {
                                                    let _ = prd::mark_complete(&config.prd_path, t);
                                                }
                                            }
                                            // L1: git commit with attribution
                                            let task_text = memory.get_task_text(task_id).unwrap_or_default();
                                            let wtag = format!("W{}", worker.id);
                                            let _ = safety::git_commit_task(&worker.working_dir, "swarm", &wtag, &task_text);
                                            // L2: Merge worktree back
                                            if let Some(ref mgr) = worktree_mgr {
                                                let _ = mgr.merge(&wtag);
                                            }
                                            println!("[SWARM HOST] Worker {} completed task {} (verified)", worker.id, task_id);
                                        } else {
                                            memory.update_task_status(task_id, TaskStatus::Failed);
                                            println!("[SWARM HOST] Worker {} failed task {} (verification failed)", worker.id, task_id);
                                        }
                                    } else {
                                        // No verify — assume success
                                        memory.update_task_status(task_id, TaskStatus::Completed);
                                        let fresh = prd::parse_prd(&config.prd_path).ok();
                                        if let Some(tasks) = fresh {
                                            if let Some(t) = tasks.iter().find(|t| t.id == task_id && !t.done) {
                                                let _ = prd::mark_complete(&config.prd_path, t);
                                            }
                                        }
                                        // L1: git commit with attribution
                                        let task_text = memory.get_task_text(task_id).unwrap_or_default();
                                        let wtag = format!("W{}", worker.id);
                                        let _ = safety::git_commit_task(&worker.working_dir, "swarm", &wtag, &task_text);
                                        // L2: Merge worktree back
                                        if let Some(ref mgr) = worktree_mgr {
                                            let _ = mgr.merge(&wtag);
                                        }
                                        println!("[SWARM HOST] Worker {} completed task {}", worker.id, task_id);
                                    }
                                }
                            }
                            worker.output_buffer.clear();
                        }
                        _ => {}
                    }
                }
            }
        }

        // Stall detection
        for worker in workers.values_mut() {
            if let Some(task_id) = worker.current_task {
                if worker.last_activity.elapsed() > stall_timeout {
                    println!(
                        "[SWARM HOST] Worker {} stalled on task {} ({}s idle), reassigning",
                        worker.id,
                        task_id,
                        worker.last_activity.elapsed().as_secs()
                    );
                    let _ = worker.process.kill();
                    memory.update_task_status(task_id, TaskStatus::Ready);
                    worker.current_task = None;
                }
            }
        }

        // Status report
        if last_status.elapsed() > status_interval {
            let (done, total) = memory.progress();
            let active = workers.values().filter(|w| w.current_task.is_some()).count();
            let idle = workers.len() - active; // Approximate — avoids mut borrow for is_running
            println!(
                "[SWARM HOST] {} | {} active, {} idle | {}s elapsed",
                progress::progress_bar(done, total, 25),
                active,
                idle,
                start.elapsed().as_secs()
            );
            last_status = Instant::now();
        }

        std::thread::sleep(poll_interval);
    }

    // Cleanup workers
    for worker in workers.values_mut() {
        let _ = worker.process.kill();
    }

    // L2: Cleanup worktrees
    if let Some(ref mgr) = worktree_mgr {
        for i in 0..config.workers {
            let wtag = format!("W{}", i);
            let _ = mgr.cleanup(&wtag);
        }
    }

    let (done, total) = memory.progress();
    println!(
        "\n[SWARM HOST] Finished: {} | {} workers, {}s",
        progress::progress_bar(done, total, 30),
        config.workers,
        start.elapsed().as_secs()
    );

    Ok(SwarmResult {
        total_tasks: total,
        completed_tasks: done,
        total_iterations: 0,
        duration_secs: start.elapsed().as_secs(),
        workers_used: config.workers,
    })
}

/// Build SwarmTasks from parsed PRD tasks, inferring dependencies from indentation.
fn build_swarm_tasks(prd_tasks: &[crate::types::Task]) -> Vec<SwarmTask> {
    // Simple heuristic: tasks are independent (no indentation inference yet).
    // TODO: Parse indentation levels for parent-child dependencies.
    prd_tasks
        .iter()
        .map(|t| SwarmTask {
            id: t.id,
            text: t.description.clone(),
            status: if t.done {
                TaskStatus::Completed
            } else {
                TaskStatus::Ready
            },
            dependencies: vec![],
            assigned_to: None,
            started_at: None,
            completed_at: None,
            prd_line: t.line_number,
        })
        .collect()
}

/// Spawn N worker PipeProcesses, optionally in isolated worktrees.
fn spawn_workers(
    config: &HatcheryConfig,
    worktree_mgr: &Option<safety::worktree::WorktreeManager>,
) -> Result<HashMap<usize, WorkerState>> {
    let mut workers = HashMap::new();

    for i in 0..config.workers {
        // Stagger spawning to avoid API rate limit spike
        if i > 0 {
            std::thread::sleep(Duration::from_secs(1));
        }

        // L2: Create worktree for this worker
        let wtag = format!("W{}", i);
        let worker_dir = if let Some(ref mgr) = worktree_mgr {
            match mgr.create(&wtag) {
                Ok(path) => path,
                Err(e) => {
                    eprintln!("[SWARM HOST] Failed to create worktree for W{}: {}", i, e);
                    config.working_dir.clone()
                }
            }
        } else {
            config.working_dir.clone()
        };

        let initial_prompt = format!(
            "You are Worker {}. You will receive tasks from the Hatchery swarm coordinator. \
             Wait for task assignment. When you receive a task, implement it and report results \
             using @hatchery:result command.",
            i
        );

        match PipeProcess::new(CliTool::ClaudeCode, &worker_dir, &initial_prompt) {
            Ok(process) => {
                let parser = zengeld_hub_core::create_ndjson_parser(CliTool::ClaudeCode);
                workers.insert(
                    i,
                    WorkerState {
                        process,
                        parser,
                        id: i,
                        current_task: None,
                        last_activity: Instant::now(),
                        output_buffer: String::new(),
                        working_dir: worker_dir,
                    },
                );
                println!("[SWARM HOST] Worker {} spawned", i);
            }
            Err(e) => {
                eprintln!("[SWARM HOST] Failed to spawn Worker {}: {}", i, e);
            }
        }
    }

    if workers.is_empty() {
        anyhow::bail!("Failed to spawn any workers");
    }

    Ok(workers)
}

/// Handle a parsed @hatchery command from a worker.
fn handle_command(
    cmd: commands::HatcheryCommand,
    worker_id: usize,
    memory: &SharedMemory,
    config: &HatcheryConfig,
) {
    match cmd {
        commands::HatcheryCommand::Knowledge { key, value } => {
            println!("[W{}] Knowledge: {} = {}", worker_id, key, value);
            memory.set_knowledge(key, value, worker_id);
        }
        commands::HatcheryCommand::Result {
            task_id,
            status,
            message,
        } => {
            let task_status = match status.as_str() {
                "success" => TaskStatus::Completed,
                "failed" => TaskStatus::Failed,
                _ => TaskStatus::Failed,
            };
            println!(
                "[W{}] Result: task {} = {}{}",
                worker_id,
                task_id,
                status,
                message.as_ref().map(|m| format!(" ({})", m)).unwrap_or_default()
            );
            memory.set_result(TaskResult {
                task_id,
                worker_id,
                status: task_status,
                verification_output: String::new(),
                error_message: message,
                duration_secs: 0,
                git_commit_sha: None,
            });

            // Mark PRD checkbox on success
            if task_status == TaskStatus::Completed {
                if let Ok(tasks) = prd::parse_prd(&config.prd_path) {
                    if let Some(t) = tasks.iter().find(|t| t.id == task_id && !t.done) {
                        let _ = prd::mark_complete(&config.prd_path, t);
                    }
                }
            }
        }
        commands::HatcheryCommand::Message { to, text } => {
            let target = match to.to_lowercase().as_str() {
                "coordinator" | "coord" => Target::Coordinator,
                "all" | "broadcast" => Target::All,
                s => {
                    // Parse "W2" or "2"
                    let id_str = s.trim_start_matches(|c: char| c.is_alphabetic());
                    if let Ok(id) = id_str.parse::<usize>() {
                        Target::Worker(id)
                    } else {
                        Target::Coordinator // fallback
                    }
                }
            };
            println!("[W{}→{}] {}", worker_id, to, truncate(&text, 60));
            memory.send_message(worker_id, target, text);
        }
        commands::HatcheryCommand::Query { pattern } => {
            let results = memory.get_knowledge(&pattern);
            println!(
                "[W{}] Query: {} ({} results)",
                worker_id,
                pattern,
                results.len()
            );
            // Results will be injected in the next prompt to this worker
        }
    }
}

fn format_messages(messages: &[shared_memory::Message]) -> String {
    if messages.is_empty() {
        return "(none)".to_string();
    }
    messages
        .iter()
        .map(|m| format!("From W{}: {}", m.from, m.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn run_verify(cmd: &str, working_dir: &std::path::Path) -> bool {
    let output = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", cmd])
            .current_dir(working_dir)
            .output()
    } else {
        std::process::Command::new("sh")
            .args(["-c", cmd])
            .current_dir(working_dir)
            .output()
    };
    output.map(|o| o.status.success()).unwrap_or(false)
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        let mut end = max.saturating_sub(3);
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}
