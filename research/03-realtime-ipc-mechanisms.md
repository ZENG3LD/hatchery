# Real-Time IPC Mechanisms for CLI-Based Agent Swarms

**Research Date:** 2026-02-06
**Target Environment:** Windows 10+ with MSYS2, Rust
**Use Case:** CLI-based hatchery binary spawning multiple Claude Code agent sub-processes

---

## Executive Summary

This research evaluates real-time communication mechanisms for building a Rust-based CLI orchestrator ("hatchery") that can spawn and coordinate multiple Claude Code agent processes on Windows with MSYS2. Key findings:

1. **Claude Code CLI** fully supports programmatic invocation with `--print`, `--output-format json`, and stdin piping
2. **MCP protocol** provides the best high-level coordination mechanism (agents connect to hatchery as MCP server)
3. **Named pipes** (Windows) and **Unix domain sockets** (MSYS2 compatibility) both work, with `interprocess` crate providing cross-platform abstraction
4. **Hooks** enable passive monitoring but cannot provide bidirectional real-time coordination
5. **SQLite + file watchers** is a robust fallback for coordination state

---

## 1. CLI-Based IPC Options

### 1.1 Named Pipes (Windows Native)

**Availability:** Full native support on Windows 10+ (Build 17063+)

**Rust Crate:** `interprocess` v2.2+

The `interprocess` crate provides Windows named pipes via `interprocess::os::windows::named_pipe`, with:
- `PipeStream` and `PipeListener` for synchronous I/O
- Async variants for Tokio integration
- Server/client pattern similar to TCP sockets

**Example pattern:**
```rust
use interprocess::os::windows::named_pipe::{PipeListener, PipeMode};

// Hatchery creates named pipe server
let listener = PipeListener::create(
    r"\\.\pipe\hatchery_control",
    PipeMode::Messages
)?;

// Agent connects as client
let stream = PipeStream::connect(r"\\.\pipe\hatchery_control")?;
```

**Pros:**
- Native Windows performance
- Message-mode support (preserves message boundaries)
- Works in pure Windows environments

**Cons:**
- Windows-only (not portable to pure Unix)
- MSYS2 compatibility requires testing (Cygwin layer may introduce overhead)

**Sources:**
- [interprocess crate](https://crates.io/crates/interprocess)
- [interprocess Windows named_pipe docs](https://docs.rs/interprocess/latest/x86_64-pc-windows-msvc/interprocess/os/windows/named_pipe/index.html)

---

### 1.2 Unix Domain Sockets (MSYS2 Compatible)

**Availability:** Windows 10 Build 17063+ with AF_UNIX support, MSYS2 runtime supports AF_LOCAL/AF_UNIX

**Critical Limitation:** MSYS2/Cygwin Unix sockets **cannot** communicate with native Windows AF_UNIX sockets. You must choose one environment.

**Rust Crate:** `interprocess::local_socket` (cross-platform abstraction)

The `local_socket` module uses Unix domain sockets on Unix and named pipes on Windows, providing a unified API.

**Example pattern:**
```rust
use interprocess::local_socket::{LocalSocketListener, LocalSocketStream};

// Server (hatchery)
let listener = LocalSocketListener::bind("/tmp/hatchery.sock")?;

// Client (agent)
let stream = LocalSocketStream::connect("/tmp/hatchery.sock")?;
```

**Pros:**
- Cross-platform API (same code works on Linux/macOS/Windows)
- MSYS2 native support (no Cygwin translation overhead)
- Pathname-based addressing (easy to discover)

**Cons:**
- Windows pathname sockets require special handling (abstract addresses not fully supported)
- MSYS2 ↔ Native Windows interop is broken (must stay in one environment)

**Decision Point:** If all processes run in MSYS2 environment (likely for Claude Code CLI), this is the best option.

**Sources:**
- [AF_UNIX comes to Windows](https://devblogs.microsoft.com/commandline/af_unix-comes-to-windows/)
- [MSYS2 Network Stack and Socket Support](https://deepwiki.com/msys2/msys2-runtime/2.4-network-stack-and-socket-support)
- [Unix Sockets on Windows](https://batsov.com/articles/2022/01/20/unix-sockets-are-now-supported-on-windows/)

---

### 1.3 Shared Files + File Watchers (notify crate)

**Availability:** Universal (all platforms)

**Rust Crate:** `notify` v7+ (MSRV 1.85)

The `notify` crate provides cross-platform filesystem event monitoring:
- Linux/Android: inotify
- macOS: FSEvents or kqueue
- Windows: ReadDirectoryChangesW
- Fallback: polling

**Example pattern:**
```rust
use notify::{Watcher, RecursiveMode, recommended_watcher};

let mut watcher = recommended_watcher(|res: Result<Event, _>| {
    match res {
        Ok(event) => println!("Event: {:?}", event),
        Err(e) => println!("Error: {:?}", e),
    }
})?;

watcher.watch(Path::new("./coordination"), RecursiveMode::Recursive)?;
```

**Coordination Pattern:**
- Hatchery writes `tasks/task-<id>.json` (new work)
- Agents watch `tasks/` directory
- Agents write `results/result-<id>.json`
- Hatchery watches `results/` directory

**Pros:**
- Simple, no special permissions
- Easy to debug (just read files)
- Crash-resistant (state persists)

**Cons:**
- Latency (file system polling, even with native watchers)
- Race conditions (need atomic write patterns)
- Cleanup required (orphaned files)

**Sources:**
- [notify crate](https://crates.io/crates/notify)
- [notify GitHub](https://github.com/notify-rs/notify)
- [notify documentation](https://docs.rs/notify/)

---

### 1.4 SQLite as Shared Coordination Database

**Availability:** Universal (all platforms)

**Rust Crate:** `rusqlite` v0.32+

SQLite provides ACID transactions with WAL mode for concurrent readers + single writer.

**Critical Pattern:** Use `BEGIN IMMEDIATE` for all write transactions to avoid deadlocks.

**Example schema:**
```sql
CREATE TABLE tasks (
    id INTEGER PRIMARY KEY,
    agent_id TEXT,
    status TEXT, -- 'pending', 'claimed', 'done'
    payload TEXT,
    created_at INTEGER
);

CREATE TABLE messages (
    id INTEGER PRIMARY KEY,
    from_agent TEXT,
    to_agent TEXT,
    content TEXT,
    read INTEGER DEFAULT 0
);
```

**Coordination Pattern:**
```rust
use rusqlite::{Connection, params};

// Agent claims work
conn.execute(
    "UPDATE tasks SET status = 'claimed', agent_id = ?1
     WHERE id = ?2 AND status = 'pending'",
    params![agent_id, task_id]
)?;

// Check if we got it (optimistic locking)
let claimed: bool = conn.query_row(
    "SELECT agent_id = ?1 FROM tasks WHERE id = ?2",
    params![agent_id, task_id],
    |row| row.get(0)
)?;
```

**Pros:**
- ACID guarantees (no race conditions)
- SQL query flexibility
- WAL mode allows concurrent readers
- Single file (easy backup)

**Cons:**
- SQLITE_BUSY errors require retry logic
- Single writer bottleneck
- Not true real-time (polling required)

**Sources:**
- [SQLite File Locking and Concurrency](https://sqlite.org/lockingv3.html)
- [rusqlite unlock_notify discussion](https://github.com/rusqlite/rusqlite/discussions/1406)
- [Parallel read and write in SQLite](https://www.skoumal.com/en/parallel-read-and-write-in-sqlite/)

---

### 1.5 stdin/stdout Piping Between Processes

**Availability:** Universal (POSIX + Windows)

**Rust Crate:** `std::process::Command`, `tokio::process::Command`

**Pattern:**
```rust
use std::process::{Command, Stdio};
use std::io::Write;

let mut child = Command::new("claude")
    .arg("--print")
    .arg("--output-format").arg("json")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .spawn()?;

let mut stdin = child.stdin.take().unwrap();
stdin.write_all(b"Your prompt here\n")?;
drop(stdin); // Close stdin to signal EOF

let output = child.wait_with_output()?;
let response: serde_json::Value = serde_json::from_slice(&output.stdout)?;
```

**Pros:**
- Simple, no IPC infrastructure needed
- Works everywhere
- Natural for request/response patterns

**Cons:**
- One-way communication (need separate pipe for bidirectional)
- Process management overhead (spawn/kill)
- No broadcast (1:1 only)

**Sources:**
- [std::process::Command](https://doc.rust-lang.org/std/process/struct.Command.html)
- [tokio::process::Command](https://docs.rs/tokio/latest/tokio/process/struct.Command.html)
- [rust-subprocess crate](https://github.com/hniksic/rust-subprocess)

---

### 1.6 TCP Localhost Sockets

**Availability:** Universal

**Rust Crate:** `std::net::TcpListener`, `tokio::net::TcpListener`

**Pattern:**
```rust
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

let listener = TcpListener::bind("127.0.0.1:0").await?;
let addr = listener.local_addr()?;

// Spawn agents with --env HATCHERY_PORT=$PORT
// Agents connect to 127.0.0.1:$PORT

while let Ok((mut socket, _)) = listener.accept().await {
    tokio::spawn(async move {
        let mut buf = vec![0; 1024];
        loop {
            let n = socket.read(&mut buf).await?;
            if n == 0 { break; }
            socket.write_all(&buf[0..n]).await?;
        }
        Ok::<_, std::io::Error>(())
    });
}
```

**Pros:**
- Universal, no special setup
- Well-understood debugging tools (netstat, tcpdump)
- Built-in buffering

**Cons:**
- Firewall prompts (Windows Defender)
- Overhead vs Unix sockets/named pipes
- Port collision risk

**Decision:** Use only if other options fail.

---

## 2. Claude Code CLI Integration

### 2.1 Programmatic Invocation

**Primary Flag:** `--print` / `-p` runs non-interactive mode and exits after response.

**Output Format:** `--output-format json` returns structured JSON response.

**Example invocation:**
```bash
claude --print --output-format json "Explain Rust ownership"
```

**Response format:**
```json
{
  "status": "success",
  "response": "...",
  "tool_calls": [...],
  "usage": {...}
}
```

**Stdin piping:**
```bash
cat file.txt | claude --print "Summarize this file"
```

**Sources:**
- [CLI reference](https://code.claude.com/docs/en/cli-reference)
- [Running Claude Code from Windows CLI](https://dstreefkerk.github.io/2026-01-running-claude-code-from-windows-cli/)
- [Claude Code CLI Cheatsheet](https://shipyard.build/blog/claude-code-cheat-sheet/)

---

### 2.2 Agent-Specific Flags

**Custom System Prompts:**
- `--append-system-prompt "text"` - adds instructions while keeping defaults
- `--system-prompt "text"` - replaces entire system prompt

**Agents and Subagents:**
- `--agent <name>` - use custom agent from `.claude/agents/`
- `--agents '{"name": {...}}'` - define subagents via JSON

**Environment:**
- `$CLAUDE_PROJECT_DIR` - project root path (available in hooks)
- `$CLAUDE_CODE_REMOTE` - set to "true" in web sessions

**Sources:**
- [CLI reference - system prompt flags](https://code.claude.com/docs/en/cli-reference#system-prompt-flags)

---

### 2.3 Structured Input/Output for Coordination

**JSON Schema Output:**
```bash
claude --print --json-schema '{"type":"object","properties":{"answer":{"type":"string"}}}' "What is 2+2?"
```

Forces Claude to return JSON matching the schema.

**Stream JSON:**
```bash
claude --print --output-format stream-json --include-partial-messages "Long task"
```

Returns JSONL stream for progressive responses.

**Verdict:** `--print --output-format json` is sufficient for most coordination needs.

**Sources:**
- [CLI reference - JSON flags](https://code.claude.com/docs/en/cli-reference)

---

## 3. MCP Protocol for Agent Coordination

### 3.1 Architecture Overview

**Model Context Protocol (MCP)** is Anthropic's standard for connecting Claude to external tools and data sources. A Rust binary can implement an MCP server that agents connect to.

**Key Insight:** Instead of the hatchery calling Claude, **agents call the hatchery** via MCP tools.

**Architecture:**
```
┌─────────────┐
│  Hatchery   │ ← Rust binary (MCP server via stdio/SSE)
│ MCP Server  │
└─────────────┘
      ↑ ↑ ↑
      │ │ └─────── Agent 3 (claude --mcp-config hatchery.json)
      │ └───────── Agent 2
      └─────────── Agent 1
```

Each agent connects to hatchery's MCP server, gaining access to coordination tools:
- `claim_task` - pull work from queue
- `report_result` - submit completed work
- `broadcast_message` - send to all agents
- `query_state` - check coordination state

**Sources:**
- [MCP Build Server Guide](https://modelcontextprotocol.io/docs/develop/build-server)
- [Building MCP Servers in Rust](https://mcpcat.io/guides/building-mcp-server-rust/)

---

### 3.2 Rust MCP Server Implementation

**Crate:** `rmcp` (official Rust SDK)

**Dependencies:**
```toml
[dependencies]
rmcp = { version = "0.3", features = ["server", "transport-io", "macros"] }
tokio = { version = "1.46", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

**Server structure:**
```rust
use rmcp::{ServerHandler, ServiceExt, tool, tool_router, model::*};

pub struct Hatchery {
    tool_router: ToolRouter<Hatchery>,
    // Shared state (Arc<Mutex<...>>)
}

#[tool_router]
impl Hatchery {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Claim a pending task from the queue")]
    async fn claim_task(&self, agent_id: String) -> String {
        // Atomically claim work
    }

    #[tool(description = "Report task completion")]
    async fn report_result(&self, task_id: String, result: String) -> String {
        // Store result, mark task done
    }
}

#[tool_handler]
impl ServerHandler for Hatchery {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let service = Hatchery::new().serve(transport).await?;
    service.waiting().await?;
    Ok(())
}
```

**Critical:** Never write to stdout (reserved for JSON-RPC). Log to stderr:
```rust
use tracing_subscriber;

tracing_subscriber::fmt()
    .with_max_level(tracing::Level::DEBUG)
    .with_writer(std::io::stderr)
    .with_ansi(false)
    .init();
```

**Sources:**
- [rmcp crate](https://lib.rs/crates/skill-mcp)
- [Building MCP Servers in Rust](https://mcpcat.io/guides/building-mcp-server-rust/)
- [rust-analyzer MCP server](https://crates.io/crates/rust-analyzer-mcp)

---

### 3.3 Agent Configuration

**Claude Desktop config** (`claude_desktop_config.json`):
```json
{
  "mcpServers": {
    "hatchery": {
      "command": "C:\\path\\to\\hatchery.exe"
    }
  }
}
```

**CLI invocation** (agents can use same config):
```bash
claude --mcp-config ./hatchery-config.json --print "Claim next task and execute it"
```

**Agent workflow:**
1. Agent starts, connects to hatchery MCP server
2. Calls `claim_task` tool
3. Executes work
4. Calls `report_result` tool
5. Loops

**Verdict:** **MCP is the best high-level coordination mechanism** for agent swarms. It's designed exactly for this use case.

**Sources:**
- [MCP Server documentation](https://modelcontextprotocol.io/docs/develop/build-server)

---

## 4. Rust Crates for IPC Coordination

### 4.1 Cross-Process Channels

**tokio::sync::mpsc** - async channels, **same process only** (not IPC)

**crossbeam::channel** - sync channels, **same process only**

**ipc-channel** - **true multi-process IPC**, drop-in replacement for Rust channels

**Implementation:**
```rust
use ipc_channel::ipc::{IpcSender, IpcReceiver, channel};

// Create channel
let (tx, rx): (IpcSender<String>, IpcReceiver<String>) = channel()?;

// Serialize sender to pass to child process
let serialized = serde_json::to_string(&tx)?;

// Child process deserializes and uses it
let tx_child: IpcSender<String> = serde_json::from_str(&serialized)?;
tx_child.send("Hello from child".to_string())?;
```

**Platform Implementation:**
- Unix: file descriptor passing over Unix sockets
- macOS: Mach ports
- Windows: named pipes

**Limitations:**
- Unbounded channels only (send never blocks)
- Consumes OS resources (sockets, file descriptors)
- Serde-based serialization (overhead)

**Verdict:** Good for simple parent → child communication, but not ideal for swarm coordination (star topology, not mesh).

**Sources:**
- [ipc-channel GitHub](https://github.com/servo/ipc-channel)
- [ipc-channel crate](https://crates.io/crates/ipc-channel)
- [Rust Channel Comparison](https://codeandbitters.com/rust-channel-comparison/)

---

### 4.2 Agent Orchestration Frameworks

**swarms-rs** - Enterprise-grade multi-agent orchestration framework

**Key features:**
- Sequential and concurrent workflows
- MCP integration for external tools
- Hierarchical (queen/workers) or mesh patterns

**Example:**
```rust
use swarms_rs::*;

let swarm = SwarmBuilder::new()
    .add_agent(Agent::new("researcher"))
    .add_agent(Agent::new("implementer"))
    .pattern(Pattern::Sequential)
    .build();

swarm.execute_task("Research and implement feature X").await?;
```

**Verdict:** Potentially useful if building from scratch, but our use case (coordinating Claude Code CLI processes) doesn't fit their architecture.

**Sources:**
- [swarms-rs GitHub](https://github.com/The-Swarm-Corporation/swarms-rs)
- [Building Production-Grade Agentic Applications with Swarms Rust](https://medium.com/@kyeg/building-production-grade-agentic-applications-with-swarms-rust-a-comprehensive-tutorial-bb567c02340f)

---

## 5. The Ralph Pattern Analysis

### 5.1 Current Implementation

**File:** `ralph-rust/ralph-rust.sh`

**Architecture:**
- Bash script loop (max iterations)
- Reads PRD files with `[ ]` checkbox criteria
- Calls `claude --dangerously-skip-permissions -p` with PRD + progress file
- Checks if criteria were completed (checkbox → `[x]`)
- Waits for cargo operations if stalled

**Key mechanisms:**
- File-based progress tracking (`tasks/progress-*.txt`)
- Polling for cargo processes (`ps aux | grep cargo`)
- Compaction of progress files when >20KB
- Git log for commit tracking

**Strengths:**
- Simple, no dependencies
- Resume-friendly (progress files persist)
- Easy to debug

**Weaknesses:**
- Sequential (one iteration at a time)
- Bash-only (not portable to pure Windows)
- No real-time coordination (wait for full iteration)

**Sources:** `ralph-rust/ralph-rust.sh` (local file)

---

### 5.2 Rust Binary Replacement Strategy

**Proposed architecture:**
```
┌─────────────────┐
│   Hatchery      │
│   (Rust CLI)    │
│                 │
│ ┌─────────────┐ │
│ │ MCP Server  │ │ ← Coordination tools
│ └─────────────┘ │
│ ┌─────────────┐ │
│ │ Task Queue  │ │ ← PRD criteria as tasks
│ └─────────────┘ │
│ ┌─────────────┐ │
│ │ Progress DB │ │ ← SQLite state
│ └─────────────┘ │
└─────────────────┘
        ↓
    Spawns N agents
        ↓
┌─────────────────┐
│  Claude --mcp   │ ← Agent 1
└─────────────────┘
┌─────────────────┐
│  Claude --mcp   │ ← Agent 2
└─────────────────┘
```

**Workflow:**
1. Hatchery parses PRD, creates task per criterion
2. Spawns N claude processes with `--mcp-config hatchery.json`
3. Agents call `claim_task`, execute work, call `report_result`
4. Hatchery updates PRD checkboxes
5. Repeat until all criteria done

**Benefits over bash script:**
- Parallel execution (N agents)
- Real-time coordination (MCP tools)
- Portable (Windows native)
- Better error handling
- Progress restoration (SQLite)

---

## 6. Hooks as Communication Channel

### 6.1 Hook Types and Trigger Points

**Available hooks:**
- `SessionStart` - when agent starts
- `PreToolUse` - before tool execution (can block)
- `PostToolUse` - after successful tool execution
- `PostToolUseFailure` - after tool failure
- `PermissionRequest` - when permission dialog would show
- `Stop` - when agent finishes (can prevent stop)
- `SubagentStop` - when subagent finishes
- `UserPromptSubmit` - before processing user prompt (can block)

**Configuration:**
```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "C:\\path\\to\\notify-hatchery.exe",
            "async": true
          }
        ]
      }
    ]
  }
}
```

**Sources:**
- [Hooks reference](https://code.claude.com/docs/en/hooks)
- [Automate workflows with hooks](https://www.eesel.ai/blog/hooks-in-claude-code)

---

### 6.2 Hook Input/Output Protocol

**Input (stdin JSON):**
```json
{
  "session_id": "abc123",
  "hook_event_name": "PostToolUse",
  "tool_name": "Write",
  "tool_input": {
    "file_path": "/path/to/file.txt",
    "content": "..."
  },
  "tool_response": {
    "success": true
  },
  "cwd": "/project/path",
  "permission_mode": "default"
}
```

**Output (stdout JSON, exit 0):**
```json
{
  "hookSpecificOutput": {
    "hookEventName": "PostToolUse",
    "additionalContext": "File written, tests passing"
  }
}
```

**Exit codes:**
- `0` - success, parse JSON
- `2` - blocking error (hook blocks action)
- Other - non-blocking error

**Sources:** [Hooks reference - Input and Output](https://code.claude.com/docs/en/hooks#hook-input-and-output)

---

### 6.3 Hooks for Agent Monitoring

**Use case:** Hatchery monitors agent progress via hooks

**Pattern:**
1. Agents configured with `PostToolUse` hook
2. Hook calls hatchery endpoint (HTTP, named pipe, or writes file)
3. Hatchery updates dashboard

**Example hook script:**
```bash
#!/bin/bash
# notify-hatchery.sh

INPUT=$(cat)
TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name')
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id')

# Send to hatchery via TCP
echo "{\"agent\": \"$SESSION_ID\", \"tool\": \"$TOOL_NAME\"}" | nc 127.0.0.1 9999

exit 0
```

**Limitations:**
- **One-way only** (hooks can't receive responses)
- Hooks run per-agent (can't coordinate between agents)
- Async hooks can't block actions

**Verdict:** Hooks are useful for **monitoring**, not **coordination**.

**Sources:**
- [Hooks reference](https://code.claude.com/docs/en/hooks)
- [Claude Code power user customization](https://claude.com/blog/how-to-configure-hooks)

---

### 6.4 Environment Variables Available to Hooks

**Special variables:**
- `$CLAUDE_PROJECT_DIR` - project root (absolute path)
- `$CLAUDE_PLUGIN_ROOT` - plugin directory (for plugin hooks)
- `$CLAUDE_CODE_REMOTE` - "true" if web session, unset if CLI
- `$CLAUDE_ENV_FILE` - (SessionStart only) file path for persisting env vars

**Example (persisting env for Bash commands):**
```bash
#!/bin/bash
# SessionStart hook

if [ -n "$CLAUDE_ENV_FILE" ]; then
  echo 'export HATCHERY_ADDR=127.0.0.1:9999' >> "$CLAUDE_ENV_FILE"
  echo 'export AGENT_ID='$SESSION_ID >> "$CLAUDE_ENV_FILE"
fi
```

**Use case:** Pass hatchery connection info to agents via environment.

**Sources:** [Hooks reference - Persist environment variables](https://code.claude.com/docs/en/hooks#persist-environment-variables)

---

## 7. Claude Code Swarm Patterns

### 7.1 TeammateTool Coordination

**Native Claude Code feature** for multi-agent teams (requires Claude Code internals, not accessible to external binaries).

**Key operations:**
- `spawnTeam` - create shared workspace
- `write(agent, message)` - send message to specific teammate
- `requestShutdown(agent)` - graceful termination
- `approvePlan` / `rejectPlan` - gating mechanism

**Message flow:**
- Teammates communicate via inbox JSON files
- Located at `~/.claude/teams/{name}/inboxes/{agent}`
- Message types: text, `shutdown_request`, `task_completed`, `plan_approval_request`

**Verdict:** Not usable for external coordination (internal Claude Code API only).

**Sources:** [Claude Code Swarm Orchestration Skill](https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea)

---

### 7.2 Task System Patterns

**Pattern 1: Parallel Specialists**
- Spawn multiple agents with different system prompts
- Each reviews from different angle (security, performance, style)
- Results aggregate via shared inbox

**Pattern 2: Sequential Pipeline**
- Task dependencies (`addBlockedBy: ["1"]`)
- Stage 2 waits for stage 1 completion
- Linear workflow

**Pattern 3: Self-Organizing Swarm**
- Workers poll `TaskList()`
- Claim unclaimed pending tasks
- Complete and loop
- Natural load balancing

**Applicability to hatchery:** Pattern 3 (self-organizing swarm) maps directly to MCP-based coordination with `claim_task` tool.

---

## 8. Recommendations

### 8.1 Recommended Architecture

**Tier 1: MCP-Based Coordination (Best)**

```
Hatchery (Rust)
├── MCP Server (stdio transport)
│   ├── claim_task(agent_id) → Task
│   ├── report_result(task_id, result)
│   ├── get_agent_status(agent_id) → Status
│   └── broadcast(message)
├── SQLite State Database
│   ├── tasks (id, status, payload)
│   ├── agents (id, status, last_seen)
│   └── results (task_id, agent_id, output)
└── Agent Spawner
    └── Spawns: claude --mcp-config hatchery.json --print "$prompt"
```

**Agents:**
- Connect to hatchery via MCP
- Use `claim_task` to pull work
- Execute task (Read, Write, Bash tools)
- Report via `report_result`

**Benefits:**
- Real-time bidirectional communication
- Standardized protocol (MCP)
- Clean separation of concerns
- Crash recovery (SQLite state)

**Sources:** All MCP sources listed in Section 3

---

### 8.2 Fallback: Named Pipes + File State

If MCP proves too complex:

```
Hatchery (Rust)
├── Named Pipe Server (\\.\pipe\hatchery)
│   └── JSON-RPC protocol
├── SQLite State Database
└── Agent Spawner
```

**Agents:**
- Connect to `\\.\pipe\hatchery`
- Send JSON-RPC requests:
  ```json
  {"jsonrpc":"2.0","method":"claim_task","params":{"agent":"A1"},"id":1}
  ```
- Receive responses:
  ```json
  {"jsonrpc":"2.0","result":{"task_id":"T123","work":"..."},"id":1}
  ```

**Implementation:**
```rust
use interprocess::os::windows::named_pipe::PipeListener;

let listener = PipeListener::create(r"\\.\pipe\hatchery", PipeMode::Bytes)?;
for stream in listener.incoming() {
    let mut stream = stream?;
    tokio::spawn(async move {
        // Handle JSON-RPC requests
    });
}
```

**Benefits:**
- Lower-level control
- No MCP SDK dependency
- Simple protocol

**Drawbacks:**
- Need to implement JSON-RPC
- No standardized error handling
- Agent needs custom client code

---

### 8.3 Minimum Viable Implementation

**For initial prototype:**

1. **Hatchery** (Rust):
   - Parse PRD into task list
   - Write `tasks/task-<id>.json` files
   - Watch `results/` with `notify` crate
   - Update PRD checkboxes

2. **Agents** (bash wrapper):
   ```bash
   while true; do
     TASK=$(ls tasks/*.json | head -1)
     if [ -z "$TASK" ]; then sleep 1; continue; fi
     mv "$TASK" "claimed-$(basename $TASK)"

     RESULT=$(claude --print "$(cat claimed-$(basename $TASK))")

     echo "$RESULT" > "results/result-$(basename $TASK)"
     rm "claimed-$(basename $TASK)"
   done
   ```

3. **Benefits:**
   - No IPC infrastructure
   - File system is coordination layer
   - Easy to debug

4. **Upgrade path:**
   - Replace file polling with MCP tools
   - Keep SQLite state for persistence

---

## 9. Implementation Roadmap

### Phase 1: File-Based Prototype (Week 1)

**Goal:** Prove the concept with minimal dependencies

**Tasks:**
1. Rust binary parses PRD → JSON task files
2. Bash wrapper script (or Rust subprocess) polls `tasks/`
3. Claude CLI executes tasks
4. Results written to `results/`
5. Hatchery updates PRD checkboxes

**Deliverable:** Working swarm with file-based coordination

---

### Phase 2: MCP Server Implementation (Week 2-3)

**Goal:** Replace file polling with real-time MCP

**Tasks:**
1. Implement `rmcp`-based MCP server in hatchery
2. Define tools: `claim_task`, `report_result`, `get_status`
3. Test with Claude Desktop first (easier debugging)
4. Integrate with hatchery CLI spawner

**Deliverable:** MCP-based coordination with real-time updates

---

### Phase 3: Advanced Features (Week 4+)

**Goal:** Production-ready orchestration

**Tasks:**
1. SQLite persistence (crash recovery)
2. Hooks for monitoring (`PostToolUse` → hatchery notification)
3. Agent health checks (heartbeat mechanism)
4. Web dashboard (Axum + HTMX)
5. Conflict resolution (multiple agents claiming same task)

**Deliverable:** Enterprise-grade agent swarm orchestrator

---

## 10. Decision Matrix

| Mechanism | Latency | Reliability | Cross-Platform | Complexity | Verdict |
|-----------|---------|-------------|----------------|------------|---------|
| **MCP Protocol** | Low | High | Yes | Medium | **RECOMMENDED** |
| Named Pipes | Low | High | Windows-only | Low | Fallback |
| Unix Sockets | Low | High | MSYS2-only | Low | Fallback |
| File + notify | Medium | Medium | Yes | Low | Prototype |
| SQLite | Medium | High | Yes | Low | State layer |
| TCP Localhost | Low | High | Yes | Medium | Not needed |
| stdin/stdout | Low | Medium | Yes | Low | Single agent only |
| Hooks | High | Medium | Yes | Low | Monitoring only |

---

## 11. Code Samples

### Sample 1: MCP Server Tool (Rust)

```rust
use rmcp::{tool, tool_router, ServerHandler, model::*};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct ClaimTaskParams {
    agent_id: String,
}

#[derive(Serialize, Deserialize)]
struct TaskResult {
    task_id: String,
    description: String,
}

#[tool_router]
impl Hatchery {
    #[tool(description = "Claim next available task from the queue")]
    async fn claim_task(
        &self,
        Parameters(params): Parameters<ClaimTaskParams>
    ) -> Result<String, Error> {
        let task = self.queue.lock().await.claim(&params.agent_id)?;
        Ok(serde_json::to_string(&task)?)
    }
}
```

---

### Sample 2: Agent Invocation (Rust)

```rust
use std::process::{Command, Stdio};
use std::io::Write;

let mut child = Command::new("claude")
    .arg("--mcp-config").arg("hatchery.json")
    .arg("--print")
    .arg("Claim a task and execute it")
    .stdout(Stdio::piped())
    .spawn()?;

let output = child.wait_with_output()?;
let response: serde_json::Value = serde_json::from_slice(&output.stdout)?;
```

---

### Sample 3: File Watcher (Rust)

```rust
use notify::{Watcher, RecursiveMode, recommended_watcher};
use std::path::Path;

let mut watcher = recommended_watcher(move |res: Result<Event, _>| {
    match res {
        Ok(event) if event.kind.is_create() => {
            println!("New result file: {:?}", event.paths);
            // Process result
        }
        Err(e) => eprintln!("Watch error: {:?}", e),
        _ => {}
    }
})?;

watcher.watch(Path::new("./results"), RecursiveMode::NonRecursive)?;
```

---

## 12. Open Questions

1. **MSYS2 Performance:** Does MSYS2's Cygwin layer add significant overhead to named pipes? (Need benchmarking)

2. **Claude CLI Concurrency:** Can multiple `claude` processes share the same MCP server, or does each need a separate connection? (Test with Claude Desktop multi-window)

3. **Hook Overhead:** Do hooks add significant latency to tool execution? (Profile with `--debug`)

4. **Agent Crash Recovery:** If an agent crashes mid-task, how does hatchery detect and reassign? (SQLite + heartbeat + timeout)

5. **Windows Defender:** Do TCP sockets or named pipes trigger Windows Defender prompts? (Test on clean Windows 11)

---

## 13. Sources

### Core Documentation
- [Claude Code CLI Reference](https://code.claude.com/docs/en/cli-reference)
- [MCP Build Server Guide](https://modelcontextprotocol.io/docs/develop/build-server)
- [Hooks Reference](https://code.claude.com/docs/en/hooks)

### Rust Crates
- [interprocess crate](https://crates.io/crates/interprocess)
- [notify crate](https://crates.io/crates/notify)
- [rusqlite crate](https://crates.io/crates/rusqlite)
- [ipc-channel crate](https://github.com/servo/ipc-channel)
- [rmcp (Rust MCP SDK)](https://lib.rs/crates/skill-mcp)

### Technical Deep Dives
- [AF_UNIX comes to Windows](https://devblogs.microsoft.com/commandline/af_unix-comes-to-windows/)
- [Building MCP Servers in Rust](https://mcpcat.io/guides/building-mcp-server-rust/)
- [SQLite File Locking and Concurrency](https://sqlite.org/lockingv3.html)
- [Claude Code Swarm Orchestration](https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea)

### Agent Orchestration
- [swarms-rs GitHub](https://github.com/The-Swarm-Corporation/swarms-rs)
- [Rust Channel Comparison](https://codeandbitters.com/rust-channel-comparison/)

---

## Conclusion

**Primary Recommendation:** Build hatchery as an **MCP server** using `rmcp` crate with stdio transport. Agents connect via `claude --mcp-config hatchery.json` and use coordination tools (`claim_task`, `report_result`).

**Fallback Option:** Named pipes (Windows) or Unix sockets (MSYS2) with custom JSON-RPC protocol.

**Prototype Path:** File-based coordination with `notify` crate for immediate validation.

The MCP approach provides the cleanest separation of concerns, standardized protocol, and best real-time coordination capabilities for CLI-based agent swarms.
