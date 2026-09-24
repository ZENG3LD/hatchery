# Search & Storage Strategies Evaluation for Swarm Orchestration

**Analysis Date:** 2026-02-08
**Source:** 18 CLI coding agent projects from revolver-research
**Focus:** Extracting patterns for multi-agent swarm coordination

---

## Executive Summary

After analyzing 18 production coding agents, clear patterns emerge:

**Key Finding:** Most agents use **dual-plane architecture**:
1. **Durable storage plane** (SQLite/JSON/PostgreSQL) for sessions/messages
2. **Search/retrieval plane** (vector DB / FTS / structural indexing)

**Best for Swarms:**
- **LangGraph's separation** of checkpointer (execution state) vs store (long-term memory)
- **Aider's RepoMap** (symbol graph + PageRank) for structural code navigation
- **Continue's content-addressed indexing** with branch-aware reuse
- **OpenClaw's delta-threshold sync** for live index updates
- **Cline's atomic writes + file watcher** for multi-instance safety

**Avoid:**
- Pure JSON-per-file without concurrency control (fragile for swarms)
- Tight coupling between session storage and search index
- Single global vector collection (doesn't scale across agents)

---

## Analysis by Category

### 1. File Search Strategies

#### EXCELLENT IDEAS

**1.1 Ripgrep + Fuzzy Hybrid (Cline, OpenCode, Crush)**
- **Pattern:** `rg --files` for listing + `fzf` for fuzzy ranking + `rg --json` for content search
- **Why excellent:** Fast, deterministic, no indexing delay, multiroot support
- **Swarm application:** Each agent can search without shared index state
- **Evidence:** Cline (`src/services/search/file-search.ts`), Crush (`internal/agent/tools/rg.go`)

**1.2 Tree-sitter Symbol Graph + PageRank (Aider)**
- **Pattern:** Extract definitions/references → build dependency graph → PageRank with personalization
- **Why excellent:** Structure-aware, no vector DB needed, explicit ranking logic
- **Swarm application:** Shared symbol graph as coordination substrate
- **Evidence:** Aider (`aider/repomap.py`)

**1.3 Content-Addressed Caching (Continue)**
- **Pattern:** `sha256(file_content) → global_cache` + branch-aware tag catalog
- **Why excellent:** Avoids re-indexing same content across branches/agents
- **Swarm application:** Shared cache reduces duplicate work
- **Evidence:** Continue (`core/indexing/refreshIndex.ts`)

#### GOOD IDEAS

**1.4 Multi-backend Retriever Abstraction (CAMEL, MetaGPT)**
- **Pattern:** Unified API over FAISS/Qdrant/Chroma/BM25/Elasticsearch
- **Why good:** Allows swapping backends without rewriting agent logic
- **Caveat:** Operational complexity managing multiple backends
- **Evidence:** CAMEL (`camel/retrievers/`), MetaGPT (`metagpt/rag/`)

**1.5 AST/Map-based Retrieval (Plandex)**
- **Pattern:** Tree-sitter file maps → LLM selects relevant files → explicit load
- **Why good:** No embeddings, explicit selection, token-aware
- **Caveat:** Depends on LLM quality for file selection
- **Evidence:** Plandex (`app/server/syntax/file_map/map.go`)

#### BAD IDEAS

**1.6 JSON LIKE Substring Search (Goose)**
- **Pattern:** SQL `json_each` + `LIKE '%keyword%'` over message JSON
- **Why bad:** Slow on large histories, no ranking, substring-only
- **Avoid:** For multi-agent search across histories
- **Evidence:** Goose (`crates/goose/src/session/chat_history_search.rs`)

**1.7 Full Directory Scan Per Search (Open Interpreter)**
- **Pattern:** Walk filesystem on every search without index
- **Why bad:** High latency, no caching, doesn't scale
- **Avoid:** Use indexed search or at least in-memory cache
- **Evidence:** Open Interpreter (`interpreter/core/computer/files/`)

---

### 2. State Persistence

#### EXCELLENT IDEAS

**2.1 Atomic Write Pattern (Cline, OpenCode)**
- **Pattern:** Write to temp file → rename (atomic)
- **Why excellent:** Prevents corruption on crashes, multi-instance safe
- **Swarm application:** Critical for shared state files between agents
- **Evidence:** Cline (`src/core/storage/disk.ts` `atomicWriteFile`)

**2.2 Checkpointer vs Store Separation (LangGraph)**
- **Pattern:**
  - Checkpointer: versioned execution state (thread_id-scoped)
  - Store: long-term memory (namespace hierarchy)
- **Why excellent:** Clear separation of concerns, independent scaling
- **Swarm application:** Each agent has checkpointer; shared store for coordination
- **Evidence:** LangGraph (`libs/checkpoint/`, `libs/checkpoint/langgraph/store/`)

**2.3 SQLite + WAL Mode (Crush, Goose, Continue)**
- **Pattern:** SQLite with WAL for concurrent reads during writes
- **Why excellent:** ACID semantics, local durable storage, concurrent safe
- **Swarm application:** Shared SQLite DB for coordination metadata
- **Evidence:** Crush (`internal/db/connect.go`), Continue (`core/indexing/refreshIndex.ts`)

**2.4 File-per-Entity + Event Bus (OpenCode)**
- **Pattern:** JSON files for durability + in-memory event bus for live updates
- **Why excellent:** Simple durability model + real-time coordination
- **Swarm application:** Each agent writes own files, subscribes to global bus
- **Evidence:** OpenCode (`packages/opencode/src/storage/`, `src/bus/`)

#### GOOD IDEAS

**2.5 Backend-Agnostic File Storage (AutoGPT)**
- **Pattern:** Abstract `FileStorage` with LOCAL/S3/GCS backends
- **Why good:** Flexible deployment (local dev, cloud prod)
- **Caveat:** Adds operational complexity
- **Evidence:** AutoGPT (`classic/forge/forge/file_storage/`)

**2.6 Versioned File History (Crush)**
- **Pattern:** `files` table with `(path, session_id, version)` unique constraint
- **Why good:** Audit trail, rollback capability
- **Caveat:** Storage growth over time
- **Evidence:** Crush (`internal/db/sql/files.sql`)

**2.7 Delta-Threshold Live Sync (OpenClaw)**
- **Pattern:** Accumulate delta bytes/messages → sync on threshold
- **Why good:** Reduces I/O, amortizes sync cost
- **Caveat:** Slight staleness between thresholds
- **Evidence:** OpenClaw (`src/memory/manager.ts` `updateSessionDelta`)

#### BAD IDEAS

**2.8 In-Memory Only State (Open Interpreter base)**
- **Pattern:** `self.messages` list as canonical state, no auto-persist
- **Why bad:** Lost on crash, no durability guarantees
- **Avoid:** For production swarms
- **Evidence:** Open Interpreter (`interpreter/core/core.py`)

**2.9 Process-Local Locks Only (OpenCode)**
- **Pattern:** In-memory reader-writer lock, no filesystem advisory locks
- **Why bad:** Multi-process writes can corrupt
- **Avoid:** For distributed swarms
- **Evidence:** OpenCode (`packages/opencode/src/util/lock.ts`)

---

### 3. Vector/Semantic Search

#### EXCELLENT IDEAS

**3.1 Hybrid Retrieval: FTS + Vector + Reranker (Continue, OpenClaw)**
- **Pattern:** SQLite FTS5 + LanceDB/sqlite_vec + optional rerank model
- **Why excellent:** Fast keyword fallback, semantic boost, quality ranking
- **Swarm application:** Shared FTS table + per-agent vector collections
- **Evidence:** Continue (`core/context/retrieval/`), OpenClaw (`src/memory/manager-search.ts`)

**3.2 Embedding Cache with Provider/Model Key (OpenClaw)**
- **Pattern:** `embedding_cache` table keyed by `(provider, model, providerKey, hash)`
- **Why excellent:** Avoids duplicate embeddings across agents/sessions
- **Swarm application:** Shared cache reduces API costs
- **Evidence:** OpenClaw (`src/memory/embeddings*.ts`)

**3.3 Namespace Hierarchy Store (LangGraph)**
- **Pattern:** Store items in hierarchical namespaces `tuple[str, ...]`
- **Why excellent:** Natural multi-tenant/multi-agent partitioning
- **Swarm application:** Each agent/swarm gets namespace prefix
- **Evidence:** LangGraph (`libs/checkpoint/langgraph/store/base/`)

**3.4 Workspace-Scoped Vector Collections (Roo Code)**
- **Pattern:** Collection name = `ws-<sha256(workspacePath)>`
- **Why excellent:** Isolates indexes, prevents cross-contamination
- **Swarm application:** Per-project collections, shared within project agents
- **Evidence:** Roo Code (`src/services/code-index/vector-store/qdrant-client.ts`)

#### GOOD IDEAS

**3.5 Short-Term + Long-Term + Entity Memory Split (CrewAI)**
- **Pattern:** STM (vector RAG), LTM (SQLite chronological), Entity (vector RAG)
- **Why good:** Mirrors human memory types
- **Caveat:** Coordination complexity across 3 stores
- **Evidence:** CrewAI (`lib/crewai/src/crewai/memory/`)

**3.6 Auto-Switching RAG on Token Pressure (CAMEL RepoAgent)**
- **Pattern:** Start with full context → switch to RAG when exceeds token budget
- **Why good:** Efficiency without premature optimization
- **Caveat:** Abrupt quality change at threshold
- **Evidence:** CAMEL (`camel/agents/repo_agent.py`)

**3.7 Pluggable Memory Backend (AutoGen)**
- **Pattern:** `Memory` contract + ChromaDB/Redis/Mem0/Canvas backends
- **Why good:** Swappable without agent code change
- **Caveat:** Each backend has different operational semantics
- **Evidence:** AutoGen (`python/packages/autogen-ext/src/autogen_ext/memory/`)

#### BAD IDEAS

**3.8 Single Global Vector Collection (early implementations)**
- **Pattern:** All content in one Chroma/Qdrant collection
- **Why bad:** No isolation, scaling limits, namespace collisions
- **Avoid:** Use workspace/agent-scoped collections
- **Not explicitly found but implied by later fixes**

**3.9 Embedding Dimension Mismatch → Silent Fail**
- **Pattern:** Query with different dimension than index → poor results
- **Why bad:** Silent degradation, hard to debug
- **Mitigation:** Explicit dimension check + collection recreation (CrewAI, Roo Code)
- **Evidence:** CrewAI (`lib/crewai/src/crewai/knowledge/storage/`)

---

### 4. Shared Memory / Inter-Agent State

#### EXCELLENT IDEAS

**4.1 Event Bus + SSE Streams (OpenCode, OpenClaw)**
- **Pattern:** In-memory event bus + SSE `/event` endpoint for subscribers
- **Why excellent:** Real-time coordination, loose coupling, multi-instance
- **Swarm application:** Agents publish task updates, orchestrator subscribes
- **Evidence:** OpenCode (`packages/opencode/src/bus/`), OpenClaw (`src/sessions/transcript-events.ts`)

**4.2 Dedicated taskHistory File + Watcher (Cline)**
- **Pattern:** Separate `state/taskHistory.json` + `chokidar` watcher for cross-instance sync
- **Why excellent:** Explicit shared state, external edit support
- **Swarm application:** Shared task registry with live sync
- **Evidence:** Cline (`src/core/storage/StateManager.ts`)

**4.3 PostgreSQL for Metadata + Filesystem for Payloads (Plandex)**
- **Pattern:** SQL for quotas/locks/control; files for context bodies
- **Why excellent:** Best of both worlds (ACID + large blobs)
- **Swarm application:** Centralized coordination via SQL, distributed payloads
- **Evidence:** Plandex (`app/server/db/`)

**4.4 Redis Message History (AutoGen)**
- **Pattern:** Redis as remote KV store for agent memory
- **Why excellent:** Multi-process, network-distributed, fast
- **Swarm application:** Shared memory bus for agent coordination
- **Evidence:** AutoGen (`python/packages/autogen-ext/src/autogen_ext/memory/redis/`)

#### GOOD IDEAS

**4.5 Conversation Summaries in SQL (Crush, Goose)**
- **Pattern:** `summary_message_id` in session → use as context anchor
- **Why good:** Compression without vector DB
- **Caveat:** Summary quality depends on model
- **Evidence:** Crush (`internal/db/sql/sessions.sql`), Goose (`crates/goose/src/session/`)

**4.6 Flow State Persistence (CrewAI)**
- **Pattern:** SQLite `flow_states` table for pause/resume workflows
- **Why good:** Durable workflow state across restarts
- **Caveat:** SQLite contention on high write loads
- **Evidence:** CrewAI (`lib/crewai/src/crewai/flow/persistence/sqlite.py`)

**4.7 Shared Context Items (AutoGPT)**
- **Pattern:** Explicit `open_file/open_folder` in context manager
- **Why good:** Transparent, token-aware
- **Caveat:** Manual management, no auto-relevance
- **Evidence:** AutoGPT (`classic/forge/forge/components/context/`)

#### IRRELEVANT

**4.8 Canvas Memory (AutoGen)**
- **Pattern:** Single text canvas with get/update/patch tools
- **Why irrelevant:** Single-document focus, not suited for multi-agent coordination
- **Evidence:** AutoGen (`python/packages/autogen-ext/src/autogen_ext/memory/canvas/`)

---

### 5. Task State Storage

#### EXCELLENT IDEAS

**5.1 TODO File + Checkbox Format (Ralph pattern from CLAUDE.md)**
- **Pattern:** Markdown checklist in repo, agent marks done with `[x]`
- **Why excellent:** Human-readable, git-trackable, simple coordination
- **Swarm application:** Shared TODO.md per task, agents update atomically
- **Evidence:** `CLAUDE.md` automation patterns (not in external repos)

**5.2 Task DAG in SQL (OpenHands, Crush)**
- **Pattern:** `tasks` table with parent_id/dependencies + status fields
- **Why excellent:** Query dependencies, detect cycles, transactional updates
- **Swarm application:** Shared task graph, agents claim/update status
- **Evidence:** Crush (`internal/db/sql/sessions.sql`)

**5.3 Versioned Extension State (Goose)**
- **Pattern:** Extension state keys like `todo.v0`, `todo.v1` for schema evolution
- **Why excellent:** Forward compatibility, no migration hell
- **Swarm application:** Agents can evolve state schemas independently
- **Evidence:** Goose (`crates/goose/src/session/extension_data.rs`)

#### GOOD IDEAS

**5.4 Artifact Tracking via Write Hooks (AutoGPT)**
- **Pattern:** `workspace.on_write_file` → create/update artifact in DB
- **Why good:** Auto-sync file writes with metadata
- **Caveat:** Coupling between storage and DB
- **Evidence:** AutoGPT (`classic/original_autogpt/autogpt/app/agent_protocol_server.py`)

**5.5 Session Diff Tracking (OpenCode)**
- **Pattern:** `Storage.write(["session_diff", sessionID], diffs)`
- **Why good:** Audit trail for reasoning about changes
- **Caveat:** Storage growth
- **Evidence:** OpenCode (`packages/opencode/src/session/`)

#### BAD IDEAS

**5.6 In-Memory Task Queue Only (early Open Interpreter)**
- **Pattern:** `unsent_messages` queue without durability
- **Why bad:** Lost on crash
- **Avoid:** For production swarms
- **Evidence:** Open Interpreter (`interpreter/core/async_core.py`)

---

### 6. Codebase Indexing

#### EXCELLENT IDEAS

**6.1 Incremental Indexing with Hash Cache (Continue, Roo Code)**
- **Pattern:** File hash comparison → skip unchanged → only reindex modified
- **Why excellent:** Fast incremental updates, low latency
- **Swarm application:** Shared index, each agent triggers incremental update
- **Evidence:** Continue (`core/indexing/refreshIndex.ts`), Roo Code (`src/services/code-index/processors/scanner.ts`)

**6.2 Index Completeness Marker (OpenClaw, Roo Code)**
- **Pattern:** Metadata point in vector DB: `indexing_complete = true/false`
- **Why excellent:** Agents know when index is stale
- **Swarm application:** Coordinator checks completeness before assigning tasks
- **Evidence:** OpenClaw (`src/memory/manager.ts`)

**6.3 File Watcher + Delta Sync (OpenClaw, Cline)**
- **Pattern:** `chokidar` FS watcher → mark dirty → debounced sync
- **Why excellent:** Live updates without polling
- **Swarm application:** Shared index stays fresh as agents edit files
- **Evidence:** OpenClaw (`src/memory/manager.ts`), Cline (`src/core/storage/StateManager.ts`)

**6.4 Snapshot Index + Temp Swap (OpenClaw)**
- **Pattern:** Reindex to temp DB → swap index files on completion
- **Why excellent:** Non-blocking reindex, rollback on failure
- **Swarm application:** Zero-downtime index updates
- **Evidence:** OpenClaw (implied in manager lifecycle)

#### GOOD IDEAS

**6.5 Language-Specific Parsers (Continue, Aider)**
- **Pattern:** Tree-sitter parsers per language for chunk boundaries
- **Why good:** Semantic chunks, not arbitrary line splits
- **Caveat:** Parser maintenance burden
- **Evidence:** Continue (`core/indexing/`), Aider (`aider/repomap.py`)

**6.6 BM25 Retriever (CAMEL, MetaGPT)**
- **Pattern:** Lexical BM25 index as alternative/complement to vector
- **Why good:** Fast, no embeddings, good for exact term matching
- **Caveat:** Requires separate index maintenance
- **Evidence:** CAMEL (`camel/retrievers/bm25_retriever.py`)

#### BAD IDEAS

**6.7 Full Reindex on Every File Change**
- **Pattern:** No incremental logic, rebuild entire index
- **Why bad:** High latency, wasted work
- **Avoid:** Use delta sync + hash cache
- **Not explicitly found (all modern agents use incremental)**

---

## Recommendations for Swarm Orchestration

### Core Architecture

**Adopt Dual-Plane Model:**
```
Execution State Plane (per agent):
- SQLite checkpointer with WAL mode
- thread_id-scoped versioned states
- Atomic file writes for session/message persistence

Shared Knowledge Plane:
- Namespace hierarchy store (LangGraph pattern)
- Hybrid retrieval: SQLite FTS5 + pgvector/Qdrant
- Content-addressed embedding cache
```

### File Search Strategy

**Primary:** Ripgrep + tree-sitter symbol graph
**Fallback:** Fuzzy file search via fzf
**Rationale:** No indexing latency, works immediately, deterministic

### State Persistence

**Mandatory:**
- Atomic writes (temp + rename)
- SQLite + WAL for shared metadata
- File watcher for live sync

**Per-Agent:**
- JSON per task/session
- Local checkpointer DB

**Shared:**
- PostgreSQL for coordination (locks, quotas, task DAG)
- Redis for live message passing (optional)

### Vector Search

**Architecture:**
- Workspace-scoped collections (not global)
- Embedding cache with (provider, model, key, hash) as key
- Hybrid: FTS first, vector boost, optional rerank

**Backends:**
- Local: sqlite_vec (lightweight) or LanceDB (feature-rich)
- Distributed: Qdrant (for multi-node swarms)

### Inter-Agent Coordination

**Real-time:**
- Event bus + SSE streams for status updates
- Redis pub/sub for distributed swarms

**Durable:**
- Shared `taskHistory.json` with file watcher
- PostgreSQL task DAG with status fields

**Task State:**
- Markdown TODO files with checkboxes (human-readable)
- SQL task table for programmatic queries

### Codebase Indexing

**Initial:**
1. Full scan with tree-sitter chunking
2. Batch embeddings with caching
3. Upsert to vector store
4. Mark `indexing_complete = true`

**Incremental:**
1. File watcher triggers on changes
2. Hash comparison → skip unchanged
3. Delete old points for modified files
4. Upsert new points
5. Content-addressed cache reuse

**Safety:**
- Reindex to temp DB, swap on completion
- Rollback on errors
- Delta-threshold sync (debounced)

---

## Anti-Patterns to Avoid

### For Swarms

**1. Single Global Collection**
- Problem: No isolation, scaling limits
- Fix: Workspace-scoped or namespace-prefixed collections

**2. No Concurrency Control**
- Problem: Race conditions, corruption
- Fix: Atomic writes + WAL + process locks

**3. Synchronous Blocking Indexing**
- Problem: Agents wait for reindex
- Fix: Async indexing + progress events

**4. Tight Coupling: Storage ↔ Search**
- Problem: Can't swap backends
- Fix: Interface abstraction (like AutoGen Memory contract)

**5. No Indexing Completeness Signal**
- Problem: Agents don't know when index is stale
- Fix: Metadata marker in store

**6. Dimension Mismatch Silent Fail**
- Problem: Poor results, hard to debug
- Fix: Explicit check on init, recreate if needed

**7. Full Reindex on Minor Changes**
- Problem: Wasted work, high latency
- Fix: Incremental updates + hash cache

---

## Implementation Priority for Hatchery

### Phase 1: Minimal Viable Coordination

1. **SQLite shared DB** (Crush pattern) for:
   - Task registry
   - Agent status
   - Coordination locks

2. **Ripgrep-based search** (no vector DB):
   - `rg --files` + fzf for file search
   - `rg --json` for content search

3. **Atomic JSON files** (OpenCode pattern) for:
   - Per-agent session state
   - Task results

4. **File watcher** (Cline pattern) for:
   - Shared `taskHistory.json` sync

### Phase 2: Add Semantic Search

5. **Tree-sitter RepoMap** (Aider pattern):
   - Symbol graph per workspace
   - PageRank for relevance

6. **Local vector store** (Continue pattern):
   - sqlite_vec or LanceDB
   - Content-addressed cache

7. **Hybrid retrieval**:
   - FTS5 + vector + optional rerank

### Phase 3: Distributed Swarms

8. **PostgreSQL** (Plandex pattern):
   - Centralized metadata
   - ACID guarantees

9. **Redis pub/sub**:
   - Live agent coordination
   - Message passing

10. **S3/GCS backend** (AutoGPT pattern):
    - Distributed payload storage

---

## Conclusion

The 18 analyzed agents reveal a **convergence toward hybrid architectures**:
- **Durable local state** (SQLite/JSON) for agent autonomy
- **Shared coordination plane** (SQL/Redis/event bus) for swarm coordination
- **Structural + semantic search** (tree-sitter + vector) for code understanding

**Key insight:** Modern agents **don't rely solely on vector search**. The best combine:
- Fast structural indexing (tree-sitter, ripgrep)
- Semantic boost (embeddings + vector DB)
- Explicit coordination (SQL task DAG, event bus)

For Hatchery swarm orchestration, start with **LangGraph's dual-plane architecture**:
- Checkpointer for agent execution state
- Store for shared knowledge
- Add Aider's RepoMap for code navigation
- Use Continue's incremental indexing for scale
- Adopt Cline's atomic writes for safety

This gives a **robust, scalable foundation** for multi-agent swarms without premature complexity.

---

## Sources

All evidence from local research files:
- `c:\Users\VA PC\CODING\ML_TRADING\nemo\research\revolver-research\search-and-storage\projects-v1\*.md`

18 projects analyzed:
1. Aider
2. AutoGen
3. AutoGPT
4. CAMEL
5. Cline
6. Continue
7. CrewAI
8. Crush
9. Goose
10. LangGraph
11. MetaGPT
12. Open Interpreter
13. OpenClaw
14. OpenCode
15. OpenHands
16. Plandex
17. Roo Code
18. SWE-agent
