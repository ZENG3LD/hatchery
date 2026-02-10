//! Hatchery CLI — swarm orchestration for AI coding agents.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use std::io::{BufRead, Write as IoWrite};

use hatchery::cli::HatcheryConfig;
use hatchery::nydus::mailbox::event_log::SqliteEventLog;
use hatchery::core::types::{SwarmMessage, AgentId, MessageType, Visibility, NydusId, QueenId};
use hatchery::queen::completion::CompletionConfig;
use hatchery::nydus::{Nydus, NydusConfig};
use hatchery::core::task_dag::{Priority, Complexity};
use hatchery::core::dag_generator;
use hatchery::nydus::ipc::protocol::{IpcRequest, IpcResponse};
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

        /// Disable git worktree isolation per worker (enabled by default).
        #[arg(long)]
        no_worktree: bool,

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

        /// Use LLM to decompose PRD into tasks with dependencies.
        #[arg(long)]
        llm_decompose: bool,

        /// Keep swarm alive after DAG completion, wait for operator commands.
        #[arg(long)]
        keep_alive: bool,
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

    /// Interact with shared memory.
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },

    /// Send or read messages.
    Mailbox {
        #[command(subcommand)]
        action: MailboxAction,
    },

    /// Run validation on current changes.
    Validate {
        /// Validation command override.
        #[arg(long)]
        cmd: Option<String>,
    },

    /// Query status of one or all queens.
    QueenStatus {
        /// Queen ID to query (e.g., "Q0"). If not specified, shows all queens.
        #[arg(long)]
        queen: Option<String>,
    },

    /// Inject a new task into the running swarm.
    Inject {
        /// Task description/prompt.
        prompt: String,

        /// Target queen ID (e.g., "Q0"). If not specified, scheduler assigns to next idle queen.
        #[arg(long)]
        queen: Option<String>,

        /// Priority (0-255, higher = more important). Default: 128.
        #[arg(long)]
        priority: Option<u8>,

        /// Custom task ID. If not specified, auto-generated as "injected-{uuid}".
        #[arg(long)]
        task_id: Option<String>,
    },

    /// Health check — ping the running Nydus.
    Ping,

    /// Gracefully shutdown the running swarm.
    Shutdown,

    /// Get overall swarm status (tasks, queens, uptime).
    SwarmStatus,
}

#[derive(Subcommand)]
enum MemoryAction {
    /// Read entries from shared memory.
    Read {
        /// Key to read (exact match).
        #[arg(long)]
        key: Option<String>,

        /// Pattern to filter keys (substring match).
        #[arg(long)]
        pattern: Option<String>,

        /// Output format: "json" (default) or "text".
        #[arg(long, default_value = "json")]
        format: String,
    },

    /// Write a key-value entry to shared memory.
    Write {
        /// Key name.
        #[arg(long)]
        key: String,

        /// Value (interpreted as JSON; plain strings auto-quoted).
        #[arg(long)]
        value: String,

        /// TTL in seconds (0 = no expiration).
        #[arg(long)]
        ttl: Option<u64>,
    },

    /// List all keys in shared memory.
    List,

    /// Show memory metadata.
    Info,
}

#[derive(Subcommand)]
enum MailboxAction {
    /// Send a message to another agent.
    Send {
        /// Target agent (e.g. "queen:Q0", "swarmhost:SH0").
        #[arg(long)]
        to: String,

        /// Message text.
        #[arg(long)]
        message: String,
    },

    /// Read messages.
    Read {
        /// Filter by sender.
        #[arg(long)]
        from: Option<String>,

        /// Maximum messages to show.
        #[arg(short = 'n', long, default_value = "20")]
        limit: usize,
    },
}

/// Parse a target string into an AgentId.
fn parse_target(target: &str) -> AgentId {
    if let Some(id) = target.strip_prefix("queen:") {
        AgentId::Queen(QueenId(id.to_string()))
    } else if let Some(id) = target.strip_prefix("swarmhost:") {
        AgentId::Nydus(NydusId(id.to_string()))
    } else if target == "validator" {
        AgentId::Validator
    } else {
        AgentId::Queen(QueenId(target.to_string()))
    }
}

/// Format an AgentId for display.
fn format_agent_id(agent: &AgentId) -> String {
    match agent {
        AgentId::Queen(id) => format!("Queen({})", id.0),
        AgentId::Nydus(id) => format!("Nydus({})", id.0),
        AgentId::Validator => "Validator".to_string(),
        AgentId::Operator => "Operator".to_string(),
        AgentId::Infestor(id) => format!("Infestor({})", id.0),
    }
}

/// Format agent ID from wire format (e.g., "queen:Q0" -> "Queen(Q0)")
fn format_agent_id_from_wire(wire: &str) -> String {
    if let Some(qid) = wire.strip_prefix("queen:") {
        format!("Queen({})", qid)
    } else if let Some(nid) = wire.strip_prefix("nydus:") {
        format!("Nydus({})", nid)
    } else if wire == "validator" {
        "Validator".to_string()
    } else if wire == "operator" {
        "Operator".to_string()
    } else {
        wire.to_string()
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
        MessageType::MemoryRef => "MemoryRef".to_string(),
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

/// Resolve IPC port from env var or port file.
fn resolve_ipc_port() -> Result<u16> {
    // Try HATCHERY_PORT env var first
    if let Ok(port_str) = std::env::var("HATCHERY_PORT") {
        return port_str.parse::<u16>().map_err(|e| anyhow::anyhow!("Invalid HATCHERY_PORT: {}", e));
    }

    // Try reading from .hatchery/*.port file
    let working_dir = std::env::var("HATCHERY_WORKING_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default());

    let hatchery_dir = working_dir.join(".hatchery");
    if hatchery_dir.exists() {
        // Find any .port file
        if let Ok(entries) = std::fs::read_dir(&hatchery_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("port") {
                    if let Ok(contents) = std::fs::read_to_string(&path) {
                        if let Ok(port) = contents.trim().parse::<u16>() {
                            return Ok(port);
                        }
                    }
                }
            }
        }
    }

    Err(anyhow::anyhow!(
        "Cannot find IPC port. Set HATCHERY_PORT env var or ensure Nydus is running."
    ))
}

/// Send an IPC request and get response (synchronous TCP).
fn ipc_call(request: IpcRequest) -> Result<IpcResponse> {
    let port = resolve_ipc_port()?;
    let addr = format!("127.0.0.1:{}", port);

    let mut stream = std::net::TcpStream::connect(&addr)
        .map_err(|e| anyhow::anyhow!("Cannot connect to Nydus at {}: {}", addr, e))?;

    // Send request
    let json = serde_json::to_string(&request)?;
    stream.write_all(json.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    // Shutdown write half to signal we're done sending
    stream.shutdown(std::net::Shutdown::Write)?;

    // Read response
    let mut reader = std::io::BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;

    let response: IpcResponse = serde_json::from_str(line.trim())
        .map_err(|e| anyhow::anyhow!("Invalid response: {}", e))?;

    Ok(response)
}

/// Populate Nydus DAG with tasks from PRD.
///
/// If `llm_decompose` is true, uses Claude CLI to analyze the PRD and generate
/// tasks with dependency relationships. Falls back to checkbox parsing on any error.
async fn populate_dag(
    nydus: &mut Nydus,
    prd_path: &std::path::Path,
    working_dir: &std::path::Path,
    llm_decompose: bool,
) -> Result<()> {
    let prd_content = std::fs::read_to_string(prd_path)
        .map_err(|e| anyhow::anyhow!("Failed to read PRD {}: {}", prd_path.display(), e))?;

    if llm_decompose {
        eprintln!("[HATCHERY] LLM decomposition enabled, calling Claude CLI...");
        match dag_generator::decompose_prd(&prd_content, working_dir).await {
            Ok(tasks) => {
                eprintln!("[HATCHERY] LLM generated {} tasks with dependencies", tasks.len());
                for gt in &tasks {
                    let deps: Vec<String> = gt.dependencies
                        .iter()
                        .map(|d| format!("llm-{}", d))
                        .collect();
                    nydus.add_task(
                        &format!("llm-{}", gt.id),
                        &gt.description,
                        deps,
                        gt.priority,
                        gt.complexity,
                        gt.skill_hint.clone(),
                    );
                }
                let stats = nydus.progress();
                let ready = stats.total_tasks - stats.completed - stats.in_progress - stats.blocked - stats.failed;
                eprintln!("[HATCHERY] DAG: {} total, {} ready, {} blocked",
                    stats.total_tasks, ready, stats.blocked);
                return Ok(());
            }
            Err(e) => {
                eprintln!("[HATCHERY] LLM decomposition failed: {}. Falling back to checkbox parsing.", e);
            }
        }
    }

    // Fallback: regex-based checkbox parsing (original behavior)
    let prd_tasks = prd::parse_prd_content(&prd_content)?;
    let (done, total) = prd::progress(&prd_tasks);
    eprintln!("[HATCHERY] PRD: {}/{} tasks (checkbox mode, {}/{} done)", total - done, total, done, total);

    for task in &prd_tasks {
        if !task.done {
            nydus.add_task(
                &format!("prd-{}", task.id),
                &task.description,
                task.dependencies.clone(),
                Priority::Normal,
                Complexity::Medium,
                task.skill_hint.clone(),
            );
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Spawn {
            prd,
            workers,
            dir,
            verify,
            max_iterations,
            stall_threshold,
            progress,
            verbose,
            no_worktree,
            safe_mode,
            backend,
            api_url,
            api_model,
            validator,
            compaction_threshold,
            event_log,
            llm_decompose,
            keep_alive,
        } => {
            let working_dir = dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

            let config = HatcheryConfig {
                prd_path: prd,
                workers,
                working_dir: working_dir.clone(),
                verify_cmd: verify.clone(),
                max_iterations,
                stall_threshold,
                progress_path: progress,
                verbose,
                worktree_isolation: !no_worktree,
                safe_mode,
                backend,
                api_url,
                api_model,
                validator_cmd: validator,
                compaction_threshold,
                event_log_path: event_log,
            };

            // Create NydusConfig
            let nydus_config = NydusConfig {
                min_queens: 3,
                max_queens: 8,
                verify_cmd: config.verify_cmd.clone(),
                working_dir: config.working_dir.clone(),
                git_isolation: config.worktree_isolation,
                autosave_interval: Duration::from_secs(60),
                max_iterations: config.max_iterations,
                setting_sources: None,
                keep_alive,
                prd_path: Some(config.prd_path.clone()),
                zerg_rush_enabled: true,
                zerg_rush_min_bottleneck: 2,
                zerg_rush_max_queens: 5,
            };

            // Create Nydus
            let mut nydus = Nydus::new(
                NydusId("SH0".to_string()),
                nydus_config,
            )?;

            let completion_config = CompletionConfig::default();

            // Register initial Queen actors (min_queens = 3)
            for i in 0..3 {
                let queen_id = QueenId(format!("Q{}", i));
                nydus.register_queen_actor(
                    queen_id,
                    "sonnet".to_string(),
                    completion_config.clone(),
                )?;
            }

            // After Queens registration, automatically register Infestor when git isolation is enabled
            if config.worktree_isolation {
                if let Err(e) = nydus.register_infestor("sonnet") {
                    eprintln!("[HATCHERY] Warning: Failed to register Infestor: {}", e);
                } else {
                    eprintln!("[HATCHERY] Infestor automatically registered (git isolation enabled)");
                }
            }

            // Add tasks to DAG (LLM decomposition or fallback to checkboxes)
            populate_dag(&mut nydus, &config.prd_path, &config.working_dir, llm_decompose).await?;

            // Run event-driven loop
            let start = Instant::now();
            println!("[HATCHERY] Starting event-driven loop (stream mode)");

            nydus.run().await?;

            // Shutdown
            nydus.shutdown().await?;

            let elapsed = start.elapsed().as_secs();
            let progress = nydus.progress();
            let total_cost = progress.total_queen_cost_usd + progress.total_infestor_cost_usd;
            println!("\n[HATCHERY] Result: {}/{} tasks complete in {}s (${:.2} queens + ${:.2} infestor = ${:.2} total)",
                progress.completed, progress.total_tasks, elapsed,
                progress.total_queen_cost_usd, progress.total_infestor_cost_usd, total_cost);
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

        Commands::Ping => {
            match ipc_call(IpcRequest::Ping) {
                Ok(IpcResponse::Ok { data }) => println!("{}", data),
                Ok(IpcResponse::Error { message }) => {
                    eprintln!("Error: {}", message);
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Connection failed: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Memory { action } => {
            match action {
                MemoryAction::Read { key, pattern, format: fmt } => {
                    let req = IpcRequest::MemoryRead { key, pattern };
                    match ipc_call(req)? {
                        IpcResponse::Ok { data } => {
                            if fmt == "text" {
                                if let Some(arr) = data.as_array() {
                                    for entry in arr {
                                        println!("{}: {}",
                                            entry["key"].as_str().unwrap_or("?"),
                                            entry["value"]);
                                    }
                                }
                            } else {
                                println!("{}", serde_json::to_string_pretty(&data)?);
                            }
                        }
                        IpcResponse::Error { message } => {
                            eprintln!("Error: {}", message);
                            std::process::exit(1);
                        }
                    }
                }
                MemoryAction::Write { key, value, ttl } => {
                    // Parse value as JSON, or wrap as string
                    let json_value: serde_json::Value = serde_json::from_str(&value)
                        .unwrap_or_else(|_| serde_json::Value::String(value));
                    let req = IpcRequest::MemoryWrite {
                        key,
                        value: json_value,
                        ttl_secs: ttl,
                        queen_id: None,
                    };
                    match ipc_call(req)? {
                        IpcResponse::Ok { data } => println!("{}", data),
                        IpcResponse::Error { message } => {
                            eprintln!("Error: {}", message);
                            std::process::exit(1);
                        }
                    }
                }
                MemoryAction::List => {
                    match ipc_call(IpcRequest::MemoryList)? {
                        IpcResponse::Ok { data } => {
                            if let Some(entries) = data["entries"].as_array() {
                                for entry in entries {
                                    println!("  {} (by {}, {})",
                                        entry["key"].as_str().unwrap_or("?"),
                                        entry["author"].as_str().unwrap_or("?"),
                                        entry["timestamp"].as_str().unwrap_or("?"));
                                }
                                println!("\n{} entries, version {}",
                                    data["count"], data["version"]);
                            }
                        }
                        IpcResponse::Error { message } => {
                            eprintln!("Error: {}", message);
                            std::process::exit(1);
                        }
                    }
                }
                MemoryAction::Info => {
                    match ipc_call(IpcRequest::MemoryInfo)? {
                        IpcResponse::Ok { data } => {
                            println!("{}", serde_json::to_string_pretty(&data)?);
                        }
                        IpcResponse::Error { message } => {
                            eprintln!("Error: {}", message);
                            std::process::exit(1);
                        }
                    }
                }
            }
        }

        Commands::Mailbox { action } => {
            match action {
                MailboxAction::Send { to, message } => {
                    let req = IpcRequest::MailboxSend {
                        to,
                        message,
                        msg_type: None,
                        queen_id: None,
                    };
                    match ipc_call(req)? {
                        IpcResponse::Ok { data } => println!("{}", data),
                        IpcResponse::Error { message } => {
                            eprintln!("Error: {}", message);
                            std::process::exit(1);
                        }
                    }
                }
                MailboxAction::Read { from, limit } => {
                    let req = IpcRequest::MailboxRead {
                        from,
                        limit: Some(limit),
                    };
                    match ipc_call(req)? {
                        IpcResponse::Ok { data } => {
                            if let Some(arr) = data.as_array() {
                                for msg in arr {
                                    let timestamp = msg["timestamp"].as_str().unwrap_or("?");
                                    let from_str = msg["from"].as_str().unwrap_or("?");
                                    let to_str = msg["to"].as_str().unwrap_or("?");
                                    let msg_type = msg["msg_type"].as_str().unwrap_or("?");
                                    let payload = format_payload(&msg["payload"]);

                                    // Convert wire format to human-readable format
                                    let from_display = format_agent_id_from_wire(from_str);
                                    let to_display = format_agent_id_from_wire(to_str);

                                    println!("[{}] {} -> {}: {} | {}",
                                        timestamp, from_display, to_display, msg_type, payload);
                                }
                                if arr.is_empty() {
                                    println!("No messages.");
                                }
                            }
                        }
                        IpcResponse::Error { message } => {
                            eprintln!("Error: {}", message);
                            std::process::exit(1);
                        }
                    }
                }
            }
        }

        Commands::Validate { cmd } => {
            let req = IpcRequest::Validate { command: cmd };
            match ipc_call(req)? {
                IpcResponse::Ok { data } => {
                    let passed = data["passed"].as_bool().unwrap_or(false);
                    if passed {
                        println!("PASSED");
                    } else {
                        println!("FAILED");
                        if let Some(feedback) = data["feedback"].as_str() {
                            eprintln!("{}", feedback);
                        }
                        std::process::exit(1);
                    }
                }
                IpcResponse::Error { message } => {
                    eprintln!("Error: {}", message);
                    std::process::exit(1);
                }
            }
        }

        Commands::QueenStatus { queen } => {
            let req = IpcRequest::QueenStatus {
                queen_id: queen.clone(),
            };
            match ipc_call(req)? {
                IpcResponse::Ok { data } => {
                    if let Some(queens) = data["queens"].as_array() {
                        if queens.is_empty() {
                            println!("No queens found.");
                        } else {
                            for queen_data in queens {
                                let id = queen_data["id"].as_str().unwrap_or("?");
                                let status = queen_data["status"].as_str().unwrap_or("?");
                                let spawn_mode = queen_data["spawn_mode"].as_str().unwrap_or("?");
                                let is_alive = queen_data["is_alive"].as_bool().unwrap_or(false);

                                let status_detail = if let Some(task_id) = queen_data["task_id"].as_str() {
                                    if let Some(progress) = queen_data["progress"].as_f64() {
                                        format!("{} (task: {}, progress: {:.0}%)", status, task_id, progress * 100.0)
                                    } else {
                                        format!("{} (task: {})", status, task_id)
                                    }
                                } else {
                                    status.to_string()
                                };

                                let alive_str = if is_alive { "alive" } else { "dead" };
                                println!("{}: {} | {} | {}", id, status_detail, spawn_mode, alive_str);
                            }
                        }
                    } else {
                        eprintln!("Invalid response format");
                        std::process::exit(1);
                    }
                }
                IpcResponse::Error { message } => {
                    eprintln!("Error: {}", message);
                    std::process::exit(1);
                }
            }
        }

        Commands::Inject { prompt, queen, priority, task_id } => {
            let req = IpcRequest::InjectTask {
                queen_id: queen,
                prompt,
                priority,
                task_id,
            };
            match ipc_call(req)? {
                IpcResponse::Ok { data } => {
                    let task_id = data["task_id"].as_str().unwrap_or("?");
                    let status = data["status"].as_str().unwrap_or("?");

                    if let Some(assigned_to) = data["assigned_to"].as_str() {
                        println!("Task {} injected and assigned to {} (status: {})", task_id, assigned_to, status);
                    } else {
                        println!("Task {} injected and queued in DAG (status: {})", task_id, status);
                    }
                }
                IpcResponse::Error { message } => {
                    eprintln!("Error: {}", message);
                    std::process::exit(1);
                }
            }
        }

        Commands::Shutdown => {
            match ipc_call(IpcRequest::Shutdown)? {
                IpcResponse::Ok { data } => {
                    println!("{}", data);
                }
                IpcResponse::Error { message } => {
                    eprintln!("Error: {}", message);
                    std::process::exit(1);
                }
            }
        }

        Commands::SwarmStatus => {
            match ipc_call(IpcRequest::SwarmStatus)? {
                IpcResponse::Ok { data } => {
                    let queen_cost = data["total_queen_cost_usd"].as_f64().unwrap_or(0.0);
                    let infestor_cost = data["total_infestor_cost_usd"].as_f64().unwrap_or(0.0);
                    let total_cost = queen_cost + infestor_cost;

                    println!("=== Swarm Status ===");
                    println!("Total tasks:    {}", data["total_tasks"].as_u64().unwrap_or(0));
                    println!("Completed:      {}", data["completed"].as_u64().unwrap_or(0));
                    println!("Failed:         {}", data["failed"].as_u64().unwrap_or(0));
                    println!("In progress:    {}", data["in_progress"].as_u64().unwrap_or(0));
                    println!("Queens alive:   {}", data["queens_alive"].as_u64().unwrap_or(0));
                    println!("Queens idle:    {}", data["queens_idle"].as_u64().unwrap_or(0));
                    println!("Queen cost:     ${:.2}", queen_cost);
                    println!("Infestor cost:  ${:.2}", infestor_cost);
                    println!("Total cost:     ${:.2}", total_cost);
                    println!("Uptime:         {}s", data["uptime_secs"].as_u64().unwrap_or(0));
                    println!("Keep-alive:     {}", data["keep_alive"].as_bool().unwrap_or(false));
                }
                IpcResponse::Error { message } => {
                    eprintln!("Error: {}", message);
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}
