# rusqlite + tokio Integration Patterns for SwarmMailbox

Research for Hatchery V2 SwarmMailbox implementation with SQLite event log.

## 1. rusqlite Crate Overview

### Version & Features
- **Latest Version**: 0.38.0 (as of January 2025)
- **Core Features**:
  - Comprehensive transaction support via `Transaction` struct
  - Parameterized queries with `params!()` (positional) and `named_params!()` (named)
  - Safe Rust API over SQLite C library
  - 100% documented across Linux, macOS, Windows

### WAL Mode Setup

SQLite's Write-Ahead Logging (WAL) mode is ideal for concurrent read/single-writer scenarios.

**Enable WAL mode:**
```rust
use rusqlite::Connection;

let conn = Connection::open("swarm.db")?;
conn.execute("PRAGMA journal_mode=WAL", [])?;
```

**WAL Benefits for SwarmMailbox:**
- **Concurrent reads**: Multiple agents can read their inbox simultaneously
- **Single writer**: SwarmHost writes without blocking readers
- **Fast writes**: Only write content once (vs twice in rollback journal)
- **Sequential I/O**: Better disk performance
- **Persistent setting**: WAL mode persists across connections

**WAL creates additional files:**
- `swarm.db-wal`: Write-ahead log file
- `swarm.db-shm`: Shared memory index for fast lookups

**Auto-checkpoint**: SQLite automatically checkpoints when WAL reaches 1000 pages or last connection closes.

### Schema Creation

```rust
conn.execute(
    "CREATE TABLE IF NOT EXISTS events (
        id TEXT PRIMARY KEY,
        timestamp TEXT NOT NULL,
        from_agent TEXT NOT NULL,
        to_agent TEXT NOT NULL,
        msg_type TEXT NOT NULL,
        payload TEXT NOT NULL,
        correlation_id TEXT,
        visibility TEXT NOT NULL
    )",
    [],
)?;

conn.execute("CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp)", [])?;
conn.execute("CREATE INDEX IF NOT EXISTS idx_events_to ON events(to_agent)", [])?;
conn.execute("CREATE INDEX IF NOT EXISTS idx_events_correlation ON events(correlation_id)", [])?;
```

### Parameterized Queries

**Positional parameters:**
```rust
conn.execute(
    "INSERT INTO events (id, timestamp, from_agent, to_agent, msg_type, payload, correlation_id, visibility)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    params![id, timestamp, from_agent, to_agent, msg_type, payload, correlation_id, visibility],
)?;
```

**Named parameters:**
```rust
use rusqlite::named_params;

conn.execute(
    "INSERT INTO events (id, timestamp, from_agent, to_agent, msg_type, payload, correlation_id, visibility)
     VALUES (:id, :timestamp, :from, :to, :type, :payload, :corr, :vis)",
    named_params! {
        ":id": id,
        ":timestamp": timestamp,
        ":from": from_agent,
        ":to": to_agent,
        ":type": msg_type,
        ":payload": payload,
        ":corr": correlation_id,
        ":vis": visibility,
    },
)?;
```

### Transaction Support

```rust
let tx = conn.transaction()?;
tx.execute("INSERT INTO events ...", params![...])?;
tx.execute("INSERT INTO events ...", params![...])?;
tx.commit()?;
```

## 2. rusqlite + tokio Integration Patterns

### Option A: tokio-rusqlite (Recommended for Simplicity)

**Crate**: `tokio-rusqlite` v0.5.1

**Pros:**
- Clean async API with `.await`
- Connection can be cloned cheaply
- Uses mpsc channel + background thread internally
- No manual spawn_blocking calls

**Cons:**
- One background thread per Connection
- Additional dependency

**Example:**
```rust
use tokio_rusqlite::Connection;

let conn = Connection::open("swarm.db").await?;

// Clone is cheap - share across agents
let conn_clone = conn.clone();

// All operations are async
conn.call(|conn| {
    conn.execute("PRAGMA journal_mode=WAL", [])?;
    conn.execute("CREATE TABLE IF NOT EXISTS events (...)", [])?;
    Ok(())
}).await?;

// Insert message
conn.call(move |conn| {
    conn.execute(
        "INSERT INTO events (id, timestamp, from_agent, to_agent, msg_type, payload, correlation_id, visibility)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, timestamp, from_agent, to_agent, msg_type, payload, correlation_id, visibility],
    )?;
    Ok(())
}).await?;

// Read inbox
let messages = conn.call(move |conn| {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, from_agent, msg_type, payload, correlation_id, visibility
         FROM events WHERE to_agent = ?1 ORDER BY timestamp"
    )?;

    let rows = stmt.query_map([agent_id], |row| {
        Ok(SwarmMessage {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            from: row.get(2)?,
            msg_type: row.get(3)?,
            payload: row.get(4)?,
            correlation_id: row.get(5)?,
            visibility: row.get(6)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
}).await?;
```

### Option B: Arc<Mutex<Connection>> + spawn_blocking

**Pros:**
- No additional dependencies
- Full control over connection lifecycle
- Direct use of rusqlite API

**Cons:**
- Manual spawn_blocking for every operation
- Mutex contention (though minimal for read-heavy workload)
- More boilerplate

**Example:**
```rust
use std::sync::Arc;
use tokio::sync::Mutex;
use rusqlite::Connection;

let conn = Arc::new(Mutex::new(Connection::open("swarm.db")?));

// Setup (once)
let conn_clone = Arc::clone(&conn);
tokio::task::spawn_blocking(move || {
    let conn = conn_clone.blocking_lock();
    conn.execute("PRAGMA journal_mode=WAL", [])?;
    conn.execute("CREATE TABLE IF NOT EXISTS events (...)", [])?;
    Ok::<_, rusqlite::Error>(())
}).await??;

// Insert message
let conn_clone = Arc::clone(&conn);
tokio::task::spawn_blocking(move || {
    let conn = conn_clone.blocking_lock();
    conn.execute(
        "INSERT INTO events (...) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![...],
    )?;
    Ok::<_, rusqlite::Error>(())
}).await??;
```

**Why spawn_blocking?**
- rusqlite's `Connection` contains `RefCell`, which is not `Sync`
- Cannot use with `tokio::spawn` (requires `Send + Sync`)
- `spawn_blocking` runs on a separate thread pool designed for blocking operations
- Thread pool reuses workers efficiently (no thread per call)

### Option C: deadpool-sqlite or r2d2-sqlite

**Pros:**
- Connection pooling for multiple concurrent operations
- Mature pool management (deadpool-sqlite v0.12.1)

**Cons:**
- Overkill for single-writer scenario
- Additional complexity
- Not needed for low throughput (~100 msgs/sec)

**When to use:** High-concurrency services with many simultaneous DB operations. Not suitable for SwarmMailbox's access pattern.

## 3. Comparison: tokio-rusqlite vs spawn_blocking

| Feature | tokio-rusqlite | Arc<Mutex> + spawn_blocking |
|---------|----------------|------------------------------|
| Dependencies | +1 crate | None |
| API ergonomics | Excellent (clean async) | Verbose (manual blocking) |
| Connection sharing | Cheap clone | Arc clone |
| Thread overhead | 1 thread per Connection | Shared blocking pool |
| Boilerplate | Minimal | High |
| Control | Less (abstracted) | Full |

**For SwarmMailbox use case (single writer, ~100 msgs/sec):**
- **tokio-rusqlite** is the simplest and cleanest solution
- One Connection per SwarmMailbox is fine (low memory overhead)
- Background thread is acceptable for this throughput

## 4. tokio::sync::mpsc for Message Routing

### Bounded vs Unbounded Channels

**Bounded Channel** (`mpsc::channel(capacity)`):
- **Pros:**
  - Backpressure: Sender blocks when channel is full
  - Memory safety: Prevents unbounded queue growth
  - Suitable for most async task communication
- **Cons:**
  - Can deadlock if not careful
  - Requires capacity tuning

**Unbounded Channel** (`mpsc::unbounded_channel()`):
- **Pros:**
  - Never blocks sender (always succeeds immediately)
  - Works from sync and async contexts
  - Simple API
- **Cons:**
  - No backpressure: Can blow up memory if producers outrun consumers
  - Risk of OOM in pathological cases

### Recommendation for SwarmMailbox

**Use bounded channels with capacity 100-1000:**
```rust
use tokio::sync::mpsc;
use std::collections::HashMap;

struct SwarmMailbox {
    db: Connection, // tokio-rusqlite Connection
    routing_table: HashMap<AgentId, mpsc::Sender<SwarmMessage>>,
}

impl SwarmMailbox {
    pub fn register_agent(&mut self, agent_id: AgentId) -> mpsc::Receiver<SwarmMessage> {
        let (tx, rx) = mpsc::channel(100); // Bounded with 100 message capacity
        self.routing_table.insert(agent_id, tx);
        rx
    }

    pub async fn send_message(&self, msg: SwarmMessage) -> Result<(), Box<dyn Error>> {
        // Write to SQLite event log
        self.db.call(move |conn| {
            conn.execute(
                "INSERT INTO events (id, timestamp, from_agent, to_agent, msg_type, payload, correlation_id, visibility)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    msg.id,
                    msg.timestamp,
                    msg.from,
                    msg.to,
                    msg.msg_type,
                    serde_json::to_string(&msg.payload)?,
                    msg.correlation_id,
                    serde_json::to_string(&msg.visibility)?,
                ],
            )?;
            Ok(())
        }).await?;

        // Route to in-memory channel if agent is online
        if let Some(tx) = self.routing_table.get(&msg.to) {
            tx.send(msg).await?;
        }

        Ok(())
    }
}
```

**Why bounded?**
- Provides natural backpressure if an agent is slow to consume messages
- Capacity of 100-1000 is ample for AI agent communication patterns
- Prevents memory leaks from runaway message production

**Pattern: One receiver per agent**
- Each agent owns their `mpsc::Receiver<SwarmMessage>`
- SwarmMailbox holds `mpsc::Sender<SwarmMessage>` in routing table
- Sender can be cloned to multiple SwarmHost threads if needed

## 5. Proposed SQLite Schema

```sql
-- Event log for all swarm messages
CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY,              -- UUID v4
    timestamp TEXT NOT NULL,          -- ISO 8601 format (e.g., "2026-02-08T12:34:56.789Z")
    from_agent TEXT NOT NULL,         -- Agent ID (e.g., "queen-1", "swarm-host")
    to_agent TEXT NOT NULL,           -- Target agent ID or "broadcast"
    msg_type TEXT NOT NULL,           -- Message type enum (e.g., "TaskAssignment", "TaskComplete")
    payload TEXT NOT NULL,            -- JSON serialized payload
    correlation_id TEXT,              -- Optional: link related messages (e.g., request-response)
    visibility TEXT NOT NULL          -- JSON array of agent IDs who can see this message
);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
CREATE INDEX IF NOT EXISTS idx_events_to ON events(to_agent);
CREATE INDEX IF NOT EXISTS idx_events_from ON events(from_agent);
CREATE INDEX IF NOT EXISTS idx_events_correlation ON events(correlation_id);
CREATE INDEX IF NOT EXISTS idx_events_type ON events(msg_type);

-- Optional: Metadata table for swarm state
CREATE TABLE IF NOT EXISTS swarm_metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

**Schema Notes:**
- **TEXT for timestamps**: SQLite doesn't have native datetime type; use ISO 8601 strings for portability
- **JSON fields**: Store `payload` and `visibility` as JSON strings; parse in Rust with `serde_json`
- **Indexes**: Optimize common queries (inbox lookup, time-range queries, correlation tracking)
- **visibility**: JSON array like `["queen-1", "queen-2", "swarm-host"]` for access control

## 6. Recommendation: Simplest Approach

Given that SwarmMailbox is for AI swarm orchestration (not high-performance database):

### **Use tokio-rusqlite with bounded mpsc channels**

**Justification:**
1. **Simplicity**: Clean async API, no manual spawn_blocking
2. **Correct for use case**: Single writer (SwarmHost), multiple readers (Queens), low throughput
3. **WAL mode**: Enables concurrent reads without blocking writes
4. **Bounded channels**: Provides backpressure and memory safety
5. **No over-engineering**: Connection pooling is unnecessary for ~100 msgs/sec

**Implementation Plan:**
1. Use `tokio-rusqlite::Connection` with WAL mode
2. Clone connection cheaply for shared access across SwarmHost and Queens
3. Use `HashMap<AgentId, mpsc::Sender<SwarmMessage>>` for routing
4. Bounded channels with capacity 100-1000
5. SQLite event log for durability and audit trail
6. In-memory channels for real-time delivery

**Trade-offs Accepted:**
- One background thread per Connection (acceptable for this scale)
- No connection pooling (not needed)
- Bounded channel capacity requires tuning (default 100 is safe)

## 7. Code Example: Complete SwarmMailbox

```rust
use tokio_rusqlite::Connection;
use tokio::sync::mpsc;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use rusqlite::params;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmMessage {
    pub id: String,
    pub timestamp: String,
    pub from: String,
    pub to: String,
    pub msg_type: String,
    pub payload: serde_json::Value,
    pub correlation_id: Option<String>,
    pub visibility: Vec<String>,
}

pub struct SwarmMailbox {
    db: Connection,
    routing_table: HashMap<String, mpsc::Sender<SwarmMessage>>,
}

impl SwarmMailbox {
    pub async fn new(db_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let conn = Connection::open(db_path).await?;

        // Enable WAL mode and create schema
        conn.call(|conn| {
            conn.execute("PRAGMA journal_mode=WAL", [])?;
            conn.execute(
                "CREATE TABLE IF NOT EXISTS events (
                    id TEXT PRIMARY KEY,
                    timestamp TEXT NOT NULL,
                    from_agent TEXT NOT NULL,
                    to_agent TEXT NOT NULL,
                    msg_type TEXT NOT NULL,
                    payload TEXT NOT NULL,
                    correlation_id TEXT,
                    visibility TEXT NOT NULL
                )",
                [],
            )?;
            conn.execute("CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp)", [])?;
            conn.execute("CREATE INDEX IF NOT EXISTS idx_events_to ON events(to_agent)", [])?;
            conn.execute("CREATE INDEX IF NOT EXISTS idx_events_correlation ON events(correlation_id)", [])?;
            Ok::<_, rusqlite::Error>(())
        }).await?;

        Ok(Self {
            db: conn,
            routing_table: HashMap::new(),
        })
    }

    pub fn register_agent(&mut self, agent_id: String) -> mpsc::Receiver<SwarmMessage> {
        let (tx, rx) = mpsc::channel(100);
        self.routing_table.insert(agent_id, tx);
        rx
    }

    pub async fn send_message(&self, msg: SwarmMessage) -> Result<(), Box<dyn std::error::Error>> {
        // Persist to SQLite
        let msg_clone = msg.clone();
        self.db.call(move |conn| {
            conn.execute(
                "INSERT INTO events (id, timestamp, from_agent, to_agent, msg_type, payload, correlation_id, visibility)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    msg_clone.id,
                    msg_clone.timestamp,
                    msg_clone.from,
                    msg_clone.to,
                    msg_clone.msg_type,
                    serde_json::to_string(&msg_clone.payload)?,
                    msg_clone.correlation_id,
                    serde_json::to_string(&msg_clone.visibility)?,
                ],
            )?;
            Ok::<_, Box<dyn std::error::Error>>(())
        }).await?;

        // Route to in-memory channel
        if let Some(tx) = self.routing_table.get(&msg.to) {
            let _ = tx.send(msg).await; // Ignore error if agent is dead
        }

        Ok(())
    }

    pub async fn read_inbox(&self, agent_id: String) -> Result<Vec<SwarmMessage>, Box<dyn std::error::Error>> {
        self.db.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, timestamp, from_agent, msg_type, payload, correlation_id, visibility
                 FROM events WHERE to_agent = ?1 ORDER BY timestamp"
            )?;

            let rows = stmt.query_map([agent_id], |row| {
                let payload_str: String = row.get(4)?;
                let visibility_str: String = row.get(6)?;

                Ok(SwarmMessage {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    from: row.get(2)?,
                    to: String::new(), // Not stored in SELECT
                    msg_type: row.get(3)?,
                    payload: serde_json::from_str(&payload_str).unwrap(),
                    correlation_id: row.get(5)?,
                    visibility: serde_json::from_str(&visibility_str).unwrap(),
                })
            })?;

            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        }).await
    }
}
```

## Sources

- [tokio-rusqlite crates.io](https://crates.io/crates/tokio-rusqlite)
- [tokio_rusqlite documentation](https://docs.rs/tokio-rusqlite/latest/tokio_rusqlite/)
- [rusqlite documentation](https://docs.rs/rusqlite/latest/rusqlite/)
- [GitHub: tokio-rusqlite](https://github.com/programatik29/tokio-rusqlite)
- [rusqlite Issue #1013: How to use with tokio::spawn](https://github.com/rusqlite/rusqlite/issues/1013)
- [tokio::task::spawn_blocking documentation](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html)
- [Tokio Discussion #3251: Do tokio reuse spawn_blocking threads?](https://github.com/tokio-rs/tokio/discussions/3251)
- [deadpool-sqlite documentation](https://docs.rs/deadpool-sqlite)
- [GitHub: deadpool](https://github.com/bikeshedder/deadpool)
- [tokio::sync::mpsc documentation](https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html)
- [Tokio Tutorial: Channels](https://tokio.rs/tokio/tutorial/channels)
- [SQLite: Write-Ahead Logging](https://sqlite.org/wal.html)
- [SQLite: Pragma Statements](https://www.sqlite.org/pragma.html)
- [Simon Willison's TIL: Enabling WAL mode for SQLite](https://til.simonwillison.net/sqlite/enabling-wal-mode)
- [Fly.io Blog: How SQLite Scales Read Concurrency](https://fly.io/blog/sqlite-internals-wal/)
- [Medium: Mastering Tokio - Building mpsc Channels](https://medium.com/@CodeWithPurpose/mastering-tokio-building-mpsc-channels-for-maximum-throughput-afb15ca64260)
