//! Hatchery CLI — swarm orchestration for AI coding agents.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use hatchery::cli::{HatcheryConfig, Mode};
use hatchery::mailbox::event_log::SqliteEventLog;
use hatchery::core::types::{SwarmMessage, AgentId, MessageType, Visibility, SwarmHostId, QueenId};
use hatchery::queen::spawn_mode::SpawnMode;
use hatchery::queen::completion::CompletionConfig;
use hatchery::swarm_host::{SwarmHost, SwarmHostConfig};
use hatchery::brood_lord::{BroodLord, BroodLordConfig};
use hatchery::core::operator::NullChannel;
use hatchery::core::task_dag::{Priority, Complexity};
use hatchery::prd;

#[derive(Parser)]
#[command(name = "hatchery", version, about = "Swarm orchestration for AI coding agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Spawn a swarm from a PRD file.
    Spawn {
        /// Path to the PRD markdown file.
        prd: PathBuf,

        /// Number of worker sessions.
        #[arg(short, long, default_value = "1")]
        workers: usize,

        /// Operation mode.
        #[arg(short, long, value_enum, default_value = "queen")]
        mode: Mode,

        /// Working directory for workers.
        #[arg(long)]
        dir: Option<PathBuf>,

        /// Verification command (e.g. "cargo check --package mylib").
        #[arg(long)]
        verify: Option<String>,

        /// Maximum iterations per worker.
        #[arg(long, default_value = "100")]
        max_iterations: usize,

        /// Consecutive stall iterations before pausing.
        #[arg(long, default_value = "3")]
        stall_threshold: usize,

        /// Progress file path (auto-generated if not specified).
        #[arg(long)]
        progress: Option<PathBuf>,

        /// Show verbose output from worker sessions.
        #[arg(short, long)]
        verbose: bool,

        /// Enable git worktree isolation per worker (each worker gets its own branch).
        #[arg(long)]
        worktree: bool,

        /// Enable safe-mode: restrict dangerous commands in worker prompts.
        #[arg(long)]
        safe_mode: bool,

        /// Backend for Queen mode: claude-native (default) or api
        #[arg(long, default_value = "claude-native")]
        backend: String,

        /// API endpoint URL (required when --backend api)
        #[arg(long)]
        api_url: Option<String>,

        /// Model name for API backend (required when --backend api)
        #[arg(long)]
        api_model: Option<String>,

        /// Validation command (e.g. "cargo check", "cargo test")
        #[arg(long)]
        validator: Option<String>,

        /// Context compaction threshold (0.0-1.0, default 0.8)
        #[arg(long, default_value = "0.8")]
        compaction_threshold: f32,

        /// Path for SQLite event log (auto-generated if not specified)
        #[arg(long)]
        event_log: Option<PathBuf>,

        /// Spawn mode for Queen actors: "stream" or "per-task"
        #[arg(long, default_value = "per-task")]
        spawn_mode: String,
    },

    /// Show status of an ongoing or completed run.
    Status {
        /// Path to the PRD markdown file.
        prd: PathBuf,

        /// Path to event log (for detailed status).
        #[arg(long)]
        event_log: Option<PathBuf>,
    },

    /// Query the event log for a hatchery run.
    Events {
        /// Path to SQLite event log file.
        log_path: PathBuf,

        /// Maximum number of events to show.
        #[arg(short = 'n', long, default_value = "50")]
        limit: usize,

        /// Filter by message type (e.g. "TaskResult", "Escalation").
        #[arg(long)]
        filter: Option<String>,
    },

    /// Send a message to a running swarm (writes to event log).
    Message {
        /// Target agent (e.g. "queen:Q0", "swarmhost:SH0")
        target: String,

        /// Message text
        text: String,

        /// Event log path
        #[arg(long)]
        log_path: PathBuf,
    },
}

/// Parse a target string into an AgentId.
fn parse_target(target: &str) -> AgentId {
    if let Some(id) = target.strip_prefix("queen:") {
        AgentId::Queen(QueenId(id.to_string()))
    } else if let Some(id) = target.strip_prefix("swarmhost:") {
        AgentId::SwarmHost(SwarmHostId(id.to_string()))
    } else if target == "validator" {
        AgentId::Validator
    } else if target == "broodlord" {
        AgentId::BroodLord
    } else {
        AgentId::Queen(QueenId(target.to_string()))
    }
}

/// Format an AgentId for display.
fn format_agent_id(agent: &AgentId) -> String {
    match agent {
        AgentId::Queen(id) => format!("Queen({})", id.0),
        AgentId::SwarmHost(id) => format!("SwarmHost({})", id.0),
        AgentId::Validator => "Validator".to_string(),
        AgentId::BroodLord => "BroodLord".to_string(),
        AgentId::Operator => "Operator".to_string(),
    }
}

/// Format a MessageType for display.
fn format_msg_type(msg_type: &MessageType) -> String {
    match msg_type {
        MessageType::TaskAssignment => "TaskAssignment".to_string(),
        MessageType::TaskResult => "TaskResult".to_string(),
        MessageType::TaskProgress => "TaskProgress".to_string(),
        MessageType::StatusRequest => "StatusRequest".to_string(),
        MessageType::StatusReport => "StatusReport".to_string(),
        MessageType::Knowledge => "Knowledge".to_string(),
        MessageType::KnowledgeQuery => "KnowledgeQuery".to_string(),
        MessageType::Escalation => "Escalation".to_string(),
        MessageType::Shutdown => "Shutdown".to_string(),
        MessageType::Custom(s) => format!("Custom({})", s),
    }
}

/// Format payload for display (truncate if too long).
fn format_payload(payload: &serde_json::Value) -> String {
    let s = payload.to_string();
    if s.len() > 80 {
        format!("{}...", &s[..77])
    } else {
        s
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Spawn {
            prd,
            workers,
            mode,
            dir,
            verify,
            max_iterations,
            stall_threshold,
            progress,
            verbose,
            worktree,
            safe_mode,
            backend,
            api_url,
            api_model,
            validator,
            compaction_threshold,
            event_log,
            spawn_mode,
        } => {
            let working_dir = dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

            let config = HatcheryConfig {
                prd_path: prd,
                workers,
                mode,
                working_dir: working_dir.clone(),
                verify_cmd: verify.clone(),
                max_iterations,
                stall_threshold,
                progress_path: progress,
                verbose,
                worktree_isolation: worktree,
                safe_mode,
                backend,
                api_url,
                api_model,
                validator_cmd: validator,
                compaction_threshold,
                event_log_path: event_log,
                spawn_mode: spawn_mode.clone(),
            };

            // Parse PRD
            let prd_tasks = prd::parse_prd(&config.prd_path)?;
            let (done, total) = prd::progress(&prd_tasks);
            println!("[HATCHERY] PRD: {}/{} tasks complete", done, total);

            match config.mode {
                Mode::Queen | Mode::SwarmHost => {
                    // Create SwarmHostConfig
                    let swarm_config = SwarmHostConfig {
                        max_queens: config.workers,
                        verify_cmd: config.verify_cmd.clone(),
                        working_dir: config.working_dir.clone(),
                        git_isolation: config.worktree_isolation,
                        autosave_interval: Duration::from_secs(60),
                        max_iterations: config.max_iterations,
                    };

                    // Create SwarmHost
                    let mut swarm = SwarmHost::new(
                        SwarmHostId("SH0".to_string()),
                        swarm_config,
                    )?;

                    // Parse spawn mode
                    let spawn_mode: SpawnMode = config.spawn_mode.parse()
                        .unwrap_or(SpawnMode::PerTask);

                    let completion_config = CompletionConfig::default();

                    // Register Queen actors
                    for i in 0..config.workers {
                        let queen_id = QueenId(format!("Q{}", i));
                        swarm.register_queen_actor(
                            queen_id,
                            "sonnet".to_string(),
                            spawn_mode,
                            completion_config.clone(),
                        )?;
                    }

                    // Add PRD tasks to DAG (only uncompleted ones)
                    for task in &prd_tasks {
                        if !task.done {
                            swarm.add_task(
                                &format!("prd-{}", task.id),
                                &task.description,
                                vec![],  // no dependencies for now
                                Priority::Normal,
                                Complexity::Medium,
                                None,
                            );
                        }
                    }

                    // Run event-driven loop
                    let start = Instant::now();
                    println!("[HATCHERY] Starting event-driven loop (spawn mode: {})", spawn_mode);

                    swarm.run().await?;

                    // Shutdown
                    swarm.shutdown().await?;

                    let elapsed = start.elapsed().as_secs();
                    let progress = swarm.progress();
                    println!("\n[HATCHERY] Result: {}/{} tasks complete in {}s",
                        progress.completed, progress.total_tasks, elapsed);
                }

                Mode::BroodLord => {
                    // Create SwarmHostConfig
                    let swarm_config = SwarmHostConfig {
                        max_queens: config.workers,
                        verify_cmd: config.verify_cmd.clone(),
                        working_dir: config.working_dir.clone(),
                        git_isolation: config.worktree_isolation,
                        autosave_interval: Duration::from_secs(60),
                        max_iterations: config.max_iterations,
                    };

                    // Create BroodLord
                    let bl_config = BroodLordConfig {
                        max_swarm_hosts: 4,
                        default_swarm_config: swarm_config.clone(),
                        max_total_iterations: config.max_iterations,
                    };
                    let mut lord = BroodLord::new(bl_config, Box::new(NullChannel::new()));

                    // Create one SwarmHost with all tasks
                    let mut swarm = SwarmHost::new(SwarmHostId("SH0".to_string()), swarm_config)?;

                    // Parse spawn mode
                    let spawn_mode: SpawnMode = config.spawn_mode.parse()
                        .unwrap_or(SpawnMode::PerTask);

                    let completion_config = CompletionConfig::default();

                    // Register Queen actors
                    for i in 0..config.workers {
                        let queen_id = QueenId(format!("Q{}", i));
                        swarm.register_queen_actor(
                            queen_id,
                            "sonnet".to_string(),
                            spawn_mode,
                            completion_config.clone(),
                        )?;
                    }

                    // Add PRD tasks to DAG
                    for task in &prd_tasks {
                        if !task.done {
                            swarm.add_task(
                                &format!("prd-{}", task.id),
                                &task.description,
                                vec![],
                                Priority::Normal,
                                Complexity::Medium,
                                None,
                            );
                        }
                    }

                    // Add swarm to BroodLord
                    lord.add_swarm("main", swarm, 100)?;

                    // Run tick loop
                    let start = Instant::now();
                    loop {
                        let tick_result = lord.tick().await?;
                        let progress = lord.global_progress();
                        println!("[HATCHERY] Tick {}: {}/{} tasks across {} swarms (active={} completed={})",
                            tick_result.iteration,
                            progress.completed_tasks, progress.total_tasks,
                            progress.total_swarms, progress.active_swarms, progress.completed_swarms);

                        if lord.is_complete() {
                            break;
                        }
                        if tick_result.iteration >= config.max_iterations {
                            println!("[HATCHERY] Max iterations reached");
                            break;
                        }
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }

                    // Shutdown
                    lord.shutdown().await?;

                    let elapsed = start.elapsed().as_secs();
                    let progress = lord.global_progress();
                    println!("\n[HATCHERY] Result: {}/{} tasks complete in {}s",
                        progress.completed_tasks, progress.total_tasks, elapsed);
                }
            }
        }

        Commands::Status { prd, event_log } => {
            let tasks = hatchery::prd::parse_prd(&prd)?;
            let (done, total) = hatchery::prd::progress(&tasks);

            println!("PRD: {}", prd.display());
            println!("{}", hatchery::progress::progress_bar(done, total, 40));
            println!();

            for task in &tasks {
                let marker = if task.done { "✓" } else { "○" };
                println!("  {} Task {}: {}", marker, task.id, task.description);
            }

            // If event log specified, show latest events
            if let Some(log_path) = event_log {
                if log_path.exists() {
                    println!("\n--- Latest Events ---");
                    match SqliteEventLog::new(&log_path) {
                        Ok(log) => {
                            let total = log.count();
                            println!("Total events: {}", total);

                            // Get last 10 events
                            match log.replay(None) {
                                Ok(events) => {
                                    let recent: Vec<_> = events.iter().rev().take(10).collect();
                                    for msg in recent.iter().rev() {
                                        println!("[{}] {} → {}: {} | {}",
                                            msg.timestamp.format("%Y-%m-%d %H:%M:%S"),
                                            format_agent_id(&msg.from),
                                            format_agent_id(&msg.to),
                                            format_msg_type(&msg.msg_type),
                                            format_payload(&msg.payload));
                                    }
                                }
                                Err(e) => eprintln!("Error reading events: {}", e),
                            }
                        }
                        Err(e) => eprintln!("Error opening event log: {}", e),
                    }
                }
            }
        }

        Commands::Events { log_path, limit, filter } => {
            let log = SqliteEventLog::new(&log_path)?;
            let total = log.count();

            println!("Event log: {}", log_path.display());
            println!("Total events: {}", total);
            println!();

            let events = log.replay(None)?;

            // Apply filter if specified
            let filtered: Vec<_> = if let Some(filter_type) = filter {
                events.into_iter()
                    .filter(|msg| {
                        let msg_type_str = format_msg_type(&msg.msg_type);
                        msg_type_str.contains(&filter_type)
                    })
                    .collect()
            } else {
                events
            };

            // Take last N events
            let to_show: Vec<_> = filtered.iter().rev().take(limit).collect();

            for msg in to_show.iter().rev() {
                println!("[{}] {} → {}: {} | {}",
                    msg.timestamp.format("%Y-%m-%d %H:%M:%S"),
                    format_agent_id(&msg.from),
                    format_agent_id(&msg.to),
                    format_msg_type(&msg.msg_type),
                    format_payload(&msg.payload));
            }
        }

        Commands::Message { target, text, log_path } => {
            let log = SqliteEventLog::new(&log_path)?;

            let msg = SwarmMessage {
                id: uuid::Uuid::new_v4().to_string(),
                from: AgentId::Operator,
                to: parse_target(&target),
                msg_type: MessageType::Custom(text.clone()),
                payload: serde_json::json!({"text": text}),
                timestamp: chrono::Utc::now(),
                correlation_id: None,
                visibility: Visibility {
                    agent_visible: true,
                    coordinator_visible: true,
                    user_visible: true
                },
            };

            log.log(&msg);
            println!("Message sent to {} at {}",
                format_agent_id(&msg.to),
                msg.timestamp.format("%Y-%m-%d %H:%M:%S"));
        }
    }

    Ok(())
}
