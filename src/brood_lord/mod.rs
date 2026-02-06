//! Brood Lord mode — swarm of swarms.
//!
//! Three-tier hierarchy:
//! 1. **Opus Manager** — decomposes master PRD into sub-PRDs, spawns L2 sub-swarms
//! 2. **L2 Coordinators** — each runs a SwarmHost for its sub-PRD
//! 3. **Workers** — execute tasks within each L2 sub-swarm
//!
//! The orchestrator (this Rust code) manages all processes, global shared memory,
//! and message routing. Opus Manager is a PipeProcess that outputs JSON commands.
//! Each L2 is essentially `swarm_host::run()` operating on a subset of tasks.

pub mod global_memory;
pub mod types;

use crate::prd;
use crate::progress;
use crate::swarm_host;
use crate::types::{HatcheryConfig, SwarmResult};
use anyhow::Result;
use global_memory::GlobalMemory;
use types::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use zengeld_hub_core::{CliEvent, CliTool, PipeProcess};

/// Embedded prompts.
const OPUS_MANAGER_PROMPT: &str = include_str!("prompts/opus_manager.md");
#[allow(dead_code)]
const L2_COORDINATOR_PROMPT: &str = include_str!("prompts/l2_coordinator.md");
#[allow(dead_code)]
const WORKER_PROMPT: &str = include_str!("prompts/worker.md");

/// State of an L2 sub-swarm (managed by the orchestrator).
#[allow(dead_code)]
struct L2SwarmState {
    id: L2Id,
    name: String,
    sub_prd_path: PathBuf,
    /// The L2 runs as a SwarmHost internally — we track its config here.
    /// In v1, each L2 is a separate PipeProcess running Claude, which internally
    /// operates as a SwarmHost coordinator.
    local_memory: swarm_host::shared_memory::SharedMemory,
    workers: HashMap<usize, WorkerProcess>,
    start_time: Instant,
    tasks_total: usize,
}

struct WorkerProcess {
    process: PipeProcess,
    parser: Box<dyn zengeld_hub_core::NdjsonParser + Send>,
    id: usize,
    current_task: Option<usize>,
    last_activity: Instant,
}

/// Run Brood Lord mode.
pub fn run(config: &HatcheryConfig) -> Result<SwarmResult> {
    let start = Instant::now();

    // Read master PRD
    let prd_tasks = prd::parse_prd(&config.prd_path)?;
    let master_prd = std::fs::read_to_string(&config.prd_path)?;
    let (initial_done, total) = prd::progress(&prd_tasks);

    println!(
        "[BROOD LORD] Master PRD: {} tasks ({} done, {} remaining)",
        total, initial_done, total - initial_done
    );

    if initial_done == total {
        println!("[BROOD LORD] All tasks already complete!");
        return Ok(SwarmResult {
            total_tasks: total,
            completed_tasks: total,
            total_iterations: 0,
            duration_secs: 0,
            workers_used: 0,
        });
    }

    // Init global shared memory
    let global_persist = config
        .prd_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("global_shared_memory.json");
    let global = GlobalMemory::new(Some(global_persist), "opus");

    // Phase 1: Spawn Opus Manager and get decomposition
    println!("[BROOD LORD] Spawning Opus Manager...");
    let decomposition = get_decomposition(config, &master_prd, &global)?;
    let sub_prd_count = decomposition.sub_prds.len();

    println!(
        "[BROOD LORD] Decomposition: {} sub-PRDs ({})",
        sub_prd_count, decomposition.rationale
    );
    for sp in &decomposition.sub_prds {
        println!(
            "[BROOD LORD]   L2.{}: {} ({} workers)",
            sp.id, sp.name, sp.worker_count
        );
    }
    global.set_l2_count(sub_prd_count);

    // Phase 2: Write sub-PRDs to disk and spawn L2 sub-swarms
    let work_dir = config
        .prd_path
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let mut l2_swarms: HashMap<L2Id, L2SwarmState> = HashMap::new();
    let mut total_workers = 0;

    for sub_prd in &decomposition.sub_prds {
        // Write sub-PRD to file
        let sub_prd_path = work_dir.join(format!("sub_prd_l2_{}.md", sub_prd.id));
        std::fs::write(&sub_prd_path, &sub_prd.content)?;

        // Create local SharedMemory for this L2
        let local_persist = work_dir.join(format!("local_l2_{}_shared_memory.json", sub_prd.id));
        let local_memory =
            swarm_host::shared_memory::SharedMemory::new(Some(local_persist), sub_prd.worker_count);

        // Parse sub-PRD tasks and load into local memory
        let sub_tasks = prd::parse_prd(&sub_prd_path)?;
        let swarm_tasks: Vec<_> = sub_tasks
            .iter()
            .map(|t| swarm_host::shared_memory::SwarmTask {
                id: t.id,
                text: t.description.clone(),
                status: if t.done {
                    swarm_host::shared_memory::TaskStatus::Completed
                } else {
                    swarm_host::shared_memory::TaskStatus::Ready
                },
                dependencies: vec![],
                assigned_to: None,
                started_at: None,
                completed_at: None,
                prd_line: t.line_number,
            })
            .collect();
        let tasks_total = swarm_tasks.len();
        local_memory.set_tasks(swarm_tasks);

        // Spawn workers for this L2
        let worker_count = sub_prd.worker_count.min(config.workers.max(1));
        let mut workers = HashMap::new();

        for w in 0..worker_count {
            if w > 0 || sub_prd.id > 0 {
                std::thread::sleep(Duration::from_secs(1));
            }

            let init = format!(
                "You are Worker L2.{}.W{}. Wait for task assignment.",
                sub_prd.id, w
            );

            match PipeProcess::new(CliTool::ClaudeCode, &config.working_dir, &init) {
                Ok(process) => {
                    let parser = zengeld_hub_core::create_ndjson_parser(CliTool::ClaudeCode);
                    workers.insert(w, WorkerProcess {
                        process,
                        parser,
                        id: w,
                        current_task: None,
                        last_activity: Instant::now(),
                    });
                    total_workers += 1;
                }
                Err(e) => {
                    eprintln!("[BROOD LORD] Failed to spawn L2.{}.W{}: {}", sub_prd.id, w, e);
                }
            }
        }

        println!(
            "[BROOD LORD] L2.{} '{}': {} workers, {} tasks",
            sub_prd.id,
            sub_prd.name,
            workers.len(),
            tasks_total
        );

        global.update_l2_status(L2Status {
            l2_id: sub_prd.id,
            name: sub_prd.name.clone(),
            tasks_completed: 0,
            tasks_total,
            workers_active: workers.len(),
            workers_total: workers.len(),
            elapsed_secs: 0,
            last_update: chrono::Utc::now(),
            state: L2State::Running,
        });

        l2_swarms.insert(sub_prd.id, L2SwarmState {
            id: sub_prd.id,
            name: sub_prd.name.clone(),
            sub_prd_path,
            local_memory,
            workers,
            start_time: Instant::now(),
            tasks_total,
        });
    }

    println!(
        "\n[BROOD LORD] Swarm active: {} L2s, {} total workers\n",
        l2_swarms.len(),
        total_workers
    );

    // Phase 3: Main orchestration loop
    // Each L2 operates like a SwarmHost — assign tasks, poll workers, handle commands.
    let poll_interval = Duration::from_millis(100);
    let status_interval = Duration::from_secs(10);
    let stall_timeout = Duration::from_secs(300);
    let mut last_status = Instant::now();
    let worker_prompt = swarm_host::WORKER_PROMPT;

    loop {
        // Check global completion
        let (_done, _total) = global.aggregate_progress();
        if global.all_complete() {
            println!("\n[BROOD LORD] All L2 sub-swarms complete!");
            break;
        }

        // Check if all L2s are either complete or dead
        let mut any_alive = false;
        for swarm in l2_swarms.values_mut() {
            for w in swarm.workers.values_mut() {
                if w.process.is_running() {
                    any_alive = true;
                    break;
                }
            }
            if any_alive { break; }
        }
        if !any_alive {
            println!("[BROOD LORD] All workers exited");
            break;
        }

        // Per-L2 orchestration (same pattern as swarm_host::run)
        for swarm in l2_swarms.values_mut() {
            let mem = &swarm.local_memory;
            mem.refresh_task_readiness();

            // Assign ready tasks
            let ready = mem.get_ready_tasks();
            for task_id in ready {
                let mut idle_wid = None;
                for (id, w) in swarm.workers.iter_mut() {
                    if w.current_task.is_none() && w.process.is_running() {
                        idle_wid = Some(*id);
                        break;
                    }
                }
                let Some(wid) = idle_wid else { break };
                let worker = swarm.workers.get_mut(&wid).unwrap();

                let task_text = mem.get_task_text(task_id).unwrap_or_default();
                let local_knowledge = mem.knowledge_summary();
                let global_knowledge = global.knowledge_summary();
                let messages = mem.drain_messages(worker.id);
                let msg_text = if messages.is_empty() {
                    "(none)".to_string()
                } else {
                    messages.iter().map(|m| format!("From W{}: {}", m.from, m.text)).collect::<Vec<_>>().join("\n")
                };
                let verify_cmd = config.verify_cmd.as_deref().unwrap_or("cargo check");

                let prompt = worker_prompt
                    .replace("{WORKER_ID}", &format!("L2.{}.W{}", swarm.id, worker.id))
                    .replace("{TASK_ID}", &task_id.to_string())
                    .replace("{TASK}", &task_text)
                    .replace("{KNOWLEDGE}", &format!("Local:\n{}\n\nGlobal:\n{}", local_knowledge, global_knowledge))
                    .replace("{MESSAGES}", &msg_text)
                    .replace("{VERIFY_CMD}", verify_cmd);

                if worker.process.write(&prompt).is_ok() {
                    worker.current_task = Some(task_id);
                    worker.last_activity = Instant::now();
                    mem.assign_task(task_id, worker.id);
                    mem.update_task_status(task_id, swarm_host::shared_memory::TaskStatus::InProgress);
                    if config.verbose {
                        println!(
                            "[L2.{}.W{}] Assigned task {}: {}",
                            swarm.id, worker.id, task_id,
                            truncate(&task_text, 50)
                        );
                    }
                }
            }

            // Poll workers
            for worker in swarm.workers.values_mut() {
                while let Some(line) = worker.process.try_recv() {
                    worker.last_activity = Instant::now();
                    let events = worker.parser.parse_line(&line);
                    for event in events {
                        match event {
                            CliEvent::AssistantText { ref text, .. } => {
                                // Parse @hatchery commands
                                let cmds = swarm_host::commands::parse_commands(text);
                                for cmd in cmds {
                                    handle_worker_command(
                                        cmd, swarm.id, worker.id, mem, &global, config,
                                    );
                                }
                                if config.verbose {
                                    print!("[L2.{}.W{}] {}", swarm.id, worker.id, text);
                                }
                            }
                            CliEvent::SessionEnd { .. } => {
                                if let Some(task_id) = worker.current_task.take() {
                                    // Check if result was reported
                                    let snap = mem.snapshot();
                                    let status = snap.tasks.iter()
                                        .find(|t| t.id == task_id)
                                        .map(|t| t.status);
                                    if status == Some(swarm_host::shared_memory::TaskStatus::InProgress) {
                                        // Not explicitly reported — verify and mark
                                        if let Some(ref verify) = config.verify_cmd {
                                            if run_verify(verify, &config.working_dir) {
                                                mem.update_task_status(task_id, swarm_host::shared_memory::TaskStatus::Completed);
                                                mark_sub_prd_checkbox(&swarm.sub_prd_path, task_id);
                                                println!("[L2.{}.W{}] Completed task {} (verified)", swarm.id, worker.id, task_id);
                                            } else {
                                                mem.update_task_status(task_id, swarm_host::shared_memory::TaskStatus::Failed);
                                            }
                                        } else {
                                            mem.update_task_status(task_id, swarm_host::shared_memory::TaskStatus::Completed);
                                            mark_sub_prd_checkbox(&swarm.sub_prd_path, task_id);
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Stall detection
            for worker in swarm.workers.values_mut() {
                if let Some(task_id) = worker.current_task {
                    if worker.last_activity.elapsed() > stall_timeout {
                        println!(
                            "[BROOD LORD] L2.{}.W{} stalled on task {}, reassigning",
                            swarm.id, worker.id, task_id
                        );
                        let _ = worker.process.kill();
                        mem.update_task_status(task_id, swarm_host::shared_memory::TaskStatus::Ready);
                        worker.current_task = None;
                    }
                }
            }

            // Update global L2 status
            let (l2_done, l2_total) = mem.progress();
            let active_workers = swarm.workers.values().filter(|w| w.current_task.is_some()).count();
            global.update_l2_status(L2Status {
                l2_id: swarm.id,
                name: swarm.name.clone(),
                tasks_completed: l2_done,
                tasks_total: l2_total,
                workers_active: active_workers,
                workers_total: swarm.workers.len(),
                elapsed_secs: swarm.start_time.elapsed().as_secs(),
                last_update: chrono::Utc::now(),
                state: if l2_done == l2_total {
                    L2State::Completed
                } else {
                    L2State::Running
                },
            });
        }

        // Global status report
        if last_status.elapsed() > status_interval {
            let (done, total) = global.aggregate_progress();
            println!(
                "[BROOD LORD] {} | {}s elapsed",
                progress::progress_bar(done, total, 30),
                start.elapsed().as_secs()
            );
            for status in global.all_l2_status() {
                let state_icon = match status.state {
                    L2State::Completed => "✓",
                    L2State::Running => "▶",
                    L2State::Stalled => "⚠",
                    L2State::Failed => "✗",
                    L2State::Pending => "○",
                };
                println!(
                    "  {} L2.{} {}: {}/{} tasks, {} workers active",
                    state_icon, status.l2_id, status.name,
                    status.tasks_completed, status.tasks_total,
                    status.workers_active
                );
            }
            last_status = Instant::now();
        }

        std::thread::sleep(poll_interval);
    }

    // Cleanup
    for swarm in l2_swarms.values_mut() {
        for worker in swarm.workers.values_mut() {
            let _ = worker.process.kill();
        }
    }

    let (done, total) = global.aggregate_progress();
    println!(
        "\n[BROOD LORD] Finished: {} | {} L2s, {} workers, {}s",
        progress::progress_bar(done, total, 30),
        l2_swarms.len(),
        total_workers,
        start.elapsed().as_secs()
    );

    Ok(SwarmResult {
        total_tasks: total,
        completed_tasks: done,
        total_iterations: 0,
        duration_secs: start.elapsed().as_secs(),
        workers_used: total_workers,
    })
}

/// Phase 1: Spawn Opus Manager to decompose master PRD.
/// Returns the decomposition plan.
fn get_decomposition(
    config: &HatcheryConfig,
    master_prd: &str,
    global: &GlobalMemory,
) -> Result<Decomposition> {
    let prompt = OPUS_MANAGER_PROMPT
        .replace("{MASTER_PRD}", master_prd)
        .replace("{MAX_L2_SWARMS}", &config.workers.to_string())
        .replace("{GLOBAL_KNOWLEDGE}", &global.knowledge_summary())
        .replace("{L2_STATUS}", "(no L2s spawned yet)");

    println!("[BROOD LORD] Sending master PRD to Opus Manager for decomposition...");

    let mut process = PipeProcess::new(CliTool::ClaudeCode, &config.working_dir, &prompt)?;
    let mut parser = zengeld_hub_core::create_ndjson_parser(CliTool::ClaudeCode);
    let mut output = String::new();

    let timeout = Instant::now();
    let max_wait = Duration::from_secs(300); // 5 min for decomposition

    loop {
        if timeout.elapsed() > max_wait {
            let _ = process.kill();
            anyhow::bail!("Opus Manager timed out during decomposition");
        }

        if let Some(line) = process.try_recv() {
            let events = parser.parse_line(&line);
            for event in events {
                if let CliEvent::AssistantText { text, .. } = event {
                    output.push_str(&text);
                    if config.verbose {
                        print!("[OPUS] {}", text);
                    }
                }
            }
        }

        if !process.is_running() {
            // Drain remaining
            while let Some(line) = process.try_recv() {
                let events = parser.parse_line(&line);
                for event in events {
                    if let CliEvent::AssistantText { text, .. } = event {
                        output.push_str(&text);
                    }
                }
            }
            break;
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    // Parse decomposition from output — look for JSON with "cmd": "decomposition"
    parse_decomposition(&output, master_prd)
}

/// Parse decomposition from Opus output.
/// Falls back to a simple section-based split if JSON parsing fails.
fn parse_decomposition(output: &str, master_prd: &str) -> Result<Decomposition> {
    // Try to find JSON decomposition command
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('{') && trimmed.contains("decomposition") {
            if let Ok(cmd) = serde_json::from_str::<OpusCommand>(trimmed) {
                if let OpusCommand::Decomposition { sub_prds, rationale } = cmd {
                    return Ok(Decomposition { sub_prds, rationale });
                }
            }
        }
    }

    // Fallback: simple section-based decomposition from the master PRD
    println!("[BROOD LORD] Opus didn't return JSON decomposition, falling back to auto-split");
    auto_decompose(master_prd)
}

/// Automatic decomposition: split master PRD by top-level ## headings.
fn auto_decompose(master_prd: &str) -> Result<Decomposition> {
    let mut sub_prds = Vec::new();
    let mut current_name = String::new();
    let mut current_content = String::new();
    let mut id = 0;

    for line in master_prd.lines() {
        if line.starts_with("## ") && !line.starts_with("### ") {
            // Save previous section if it has tasks
            if !current_content.is_empty() && current_content.contains("[ ]") {
                sub_prds.push(SubPrd {
                    id,
                    name: current_name.clone(),
                    content: current_content.clone(),
                    cross_deps: vec![],
                    worker_count: 2, // Default
                });
                id += 1;
            }
            current_name = line.trim_start_matches("## ").trim().to_string();
            current_content = format!("# {}\n\n", current_name);
        } else {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    // Save last section
    if !current_content.is_empty() && current_content.contains("[ ]") {
        sub_prds.push(SubPrd {
            id,
            name: current_name,
            content: current_content,
            cross_deps: vec![],
            worker_count: 2,
        });
    }

    // If no sections found, treat entire PRD as single sub-PRD
    if sub_prds.is_empty() {
        sub_prds.push(SubPrd {
            id: 0,
            name: "Main".into(),
            content: master_prd.to_string(),
            cross_deps: vec![],
            worker_count: 4,
        });
    }

    Ok(Decomposition {
        sub_prds,
        rationale: "Auto-decomposed by ## sections".into(),
    })
}

fn handle_worker_command(
    cmd: swarm_host::commands::HatcheryCommand,
    l2_id: usize,
    worker_id: usize,
    local: &swarm_host::shared_memory::SharedMemory,
    global: &GlobalMemory,
    config: &HatcheryConfig,
) {
    match cmd {
        swarm_host::commands::HatcheryCommand::Knowledge { key, value } => {
            if key.starts_with("global.") {
                // Write to global memory
                global.set_knowledge(key, value);
            } else {
                // Write to local memory
                local.set_knowledge(key, value, worker_id);
            }
        }
        swarm_host::commands::HatcheryCommand::Result { task_id, status, message } => {
            let task_status = match status.as_str() {
                "success" => swarm_host::shared_memory::TaskStatus::Completed,
                _ => swarm_host::shared_memory::TaskStatus::Failed,
            };
            local.set_result(swarm_host::shared_memory::TaskResult {
                task_id,
                worker_id,
                status: task_status,
                verification_output: String::new(),
                error_message: message,
                duration_secs: 0,
                git_commit_sha: None,
            });
            println!("[L2.{}.W{}] Result: task {} = {}", l2_id, worker_id, task_id, status);
        }
        swarm_host::commands::HatcheryCommand::Message { to, text } => {
            let target = match to.to_lowercase().as_str() {
                "coordinator" | "coord" => swarm_host::shared_memory::Target::Coordinator,
                "all" => swarm_host::shared_memory::Target::All,
                s => {
                    let id_str = s.trim_start_matches(|c: char| c.is_alphabetic());
                    if let Ok(id) = id_str.parse::<usize>() {
                        swarm_host::shared_memory::Target::Worker(id)
                    } else {
                        swarm_host::shared_memory::Target::Coordinator
                    }
                }
            };
            local.send_message(worker_id, target, text);
        }
        swarm_host::commands::HatcheryCommand::Query { pattern } => {
            let _ = local.get_knowledge(&pattern);
            // Results injected in next prompt
        }
    }
    let _ = (l2_id, config); // Will be used for escalation in v2
}

fn mark_sub_prd_checkbox(sub_prd_path: &std::path::Path, task_id: usize) {
    if let Ok(tasks) = prd::parse_prd(sub_prd_path) {
        if let Some(t) = tasks.iter().find(|t| t.id == task_id && !t.done) {
            let _ = prd::mark_complete(sub_prd_path, t);
        }
    }
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
    if s.len() <= max { s } else {
        let mut end = max.saturating_sub(3);
        while end > 0 && !s.is_char_boundary(end) { end -= 1; }
        &s[..end]
    }
}
