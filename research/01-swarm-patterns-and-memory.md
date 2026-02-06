# Multi-Agent Swarm Orchestration Patterns & Shared Memory for Coding Agents

**Research Date**: 2026-02-06
**Focus**: Coordination patterns, shared memory, and orchestration strategies for CLI-based coding agent swarms

---

## Executive Summary

This document analyzes six major multi-agent frameworks (Claude Code Agent Teams, CrewAI, AutoGen, LangGraph, Microsoft Magentic-One, OpenAI Swarm), five shared memory approaches (filesystem, Redis, SQLite, blackboard, event sourcing), and four coordinator patterns (orchestrator, choreography, hierarchical, blackboard). Key finding: **for CLI-based coding agents, hierarchical orchestrator patterns with filesystem-based shared state offer the best balance of simplicity, observability, and reliability.**

---

## 1. Multi-Agent Frameworks Analysis

### 1.1 Claude Code Agent Teams (Anthropic, 2026)

**Status**: Experimental, production-ready alongside Opus 4.6

#### Architecture
- **Lead agent** (main coordinator) + **Teammates** (independent Claude Code instances)
- **Shared task list** with file-locking for atomic task claiming
- **Mailbox system** for direct agent-to-agent messaging
- **Automatic message delivery** (no polling required)
- Storage: `~/.claude/teams/{team-name}/config.json` and `~/.claude/tasks/{team-name}/`

#### Coordination Mechanism
- Lead creates team, spawns teammates with spawn prompts
- Teammates self-claim tasks from shared list (with dependency management)
- Direct messaging via `message` (1:1) or `broadcast` (1:N)
- Idle notifications automatically sent to lead when teammates finish

#### Communication Patterns
- Each teammate has **own context window** (no shared history)
- Teammates load project context (CLAUDE.md, MCP servers, skills) on spawn
- Messages delivered automatically via mailbox system
- Task status changes propagate through shared task list

#### Strengths for Coding Tasks
- ✅ **Native filesystem integration** (teammates are full Claude Code sessions)
- ✅ **Strong isolation** (separate context windows prevent context pollution)
- ✅ **Built-in task coordination** (shared task list with dependencies)
- ✅ **Observable** (in-process or split-pane display modes)
- ✅ **Directly interactive** (can message teammates individually)
- ✅ **File-locking prevents race conditions** on task claiming

#### Weaknesses
- ❌ **No session resumption** with in-process teammates
- ❌ **High token costs** (each teammate = separate API instance)
- ❌ **No nested teams** (teammates can't spawn sub-teams)
- ❌ **Coordination overhead** for small tasks
- ❌ **Shutdown can be slow** (teammates finish current work before stopping)

#### Best Use Cases
- Research and review (parallel investigation)
- New modules/features (independent file ownership)
- Debugging with competing hypotheses
- Cross-layer coordination (frontend/backend/tests)

#### CLI-Subprocess Suitability: ⭐⭐⭐⭐⭐
Perfect fit. Teammates are **actual subprocesses** running Claude Code. Communication via mailbox + shared filesystem. Proven at scale (16 agents, 2000 sessions, built a C compiler).

---

### 1.2 CrewAI

**Status**: Production-ready, open-source

#### Architecture
- **Crews** (autonomous agent teams) + **Flows** (deterministic workflow control)
- Agents have roles, goals, backstory, tools
- Manager/worker hierarchical pattern available
- Centralized orchestrator with distributed execution

#### Coordination Mechanism
- **Hierarchical**: Manager agent coordinates workers
- **Pipeline**: Sequential handoffs between agents
- **Ensemble**: Parallel execution with result voting
- **Swarm**: Emergent coordination (less common)

#### Communication Patterns
- Orchestrator reads agent capabilities, decides execution path dynamically
- Agents pass context via task outputs
- State management through Flows (event-driven, persistent)

#### Strengths for Coding Tasks
- ✅ **5.76x faster** than alternatives for certain coding tasks (per research)
- ✅ **Modular design** (add/remove agents without retraining)
- ✅ **100+ built-in tools** (web search, API calls, etc.)
- ✅ **Enterprise-ready** (PwC case: 10% → 70% code-gen accuracy)

#### Weaknesses
- ❌ **Python-centric** (not ideal for polyglot CLI environments)
- ❌ **Flows add complexity** (may be overkill for simple coordination)
- ❌ **Less transparent** than lightweight frameworks

#### Best Use Cases
- Complex multi-step workflows with state persistence
- Enterprise codegen pipelines
- Workflows requiring 100+ tools integration

#### CLI-Subprocess Suitability: ⭐⭐⭐
Doable but heavyweight. Requires Python subprocess management. Better for library-based integration than CLI coordination.

---

### 1.3 Microsoft AutoGen

**Status**: Production framework (v0.4+ event-driven architecture)

#### Architecture
- **Asynchronous, event-driven** agent communication
- Agents are software entities maintaining own state
- **Nested structures** (agents review/critique each other's outputs)
- **Group Chat Manager** for multi-agent conversations

#### Coordination Mechanisms
- **Sequential patterns**: Predetermined task order (assembly line)
- **Concurrent patterns**: Parallel subtask processing
- **Group chat patterns**: Dynamic conversational collaboration
- **Handoff patterns**: Smooth transitions between specialists
- **Mixture of Agents (MoA)**: Feed-forward neural network structure (workers → orchestrator)

#### Agent Types
- **AssistantAgent**: LLM-powered code writer (no execution capability)
- **UserProxyAgent**: Executes code, solicits human input
- **Manager Agent**: Coordinates tasks of other agents

#### Communication Patterns
- Asynchronous messages (event-driven + request/response)
- Registered auto-reply functions (hierarchical chat, nested conversations)
- LLM-based function calling (agents decide when to invoke functions)

#### Strengths for Coding Tasks
- ✅ **Strong code execution** (UserProxyAgent auto-detects executable blocks)
- ✅ **Flexible conversation patterns** (static or dynamic topologies)
- ✅ **Iterative improvements** (agents critique each other)
- ✅ **Human-in-the-loop** configurability

#### Weaknesses
- ❌ **Complex setup** for simple tasks
- ❌ **Python-based** (like CrewAI)
- ❌ **Opaque coordination** in dynamic topologies
- ❌ **Error amplification** in independent multi-agent setups (17.2x per research)

#### Best Use Cases
- Code generation with automated testing (AssistantAgent → UserProxyAgent loop)
- Multi-round debugging with human oversight
- Complex workflows with iterative refinement

#### CLI-Subprocess Suitability: ⭐⭐⭐
Similar to CrewAI. Python-based, so requires wrapper. Better for embedded agents than CLI orchestration.

---

### 1.4 LangGraph

**Status**: Production-ready, state-aware framework

#### Architecture
- **Graph-based** state machines (directed graphs)
- **Explicit state schemas** (TypedDict + Annotated types)
- **Reducer-driven state** (prevents data loss in multi-agent systems)
- **Robust checkpointing** (persistent memory, safe parallel execution)

#### Coordination Patterns
- **Supervisor pattern**: Supervisor coordinates specialized agents (each with own scratchpad)
- **Orchestrator-worker pattern**: Dynamic worker creation via Send API, shared output state
- **Scatter-gather**: Tasks distributed, results consolidated
- **Pipeline parallelism**: Sequential stages handled concurrently

#### State Management
- TypedDict schemas define workflow state
- Conditional edges evaluate state to determine next execution path
- Modular subgraphs enable flexible control flow
- Checkpointing allows workflow resumption after failures

#### Strengths for Coding Tasks
- ✅ **Explicit state management** (no hidden context changes)
- ✅ **Parallel execution** with safe state updates
- ✅ **Flexible control flow** (single/multi-agent, hierarchical, sequential)
- ✅ **Checkpointing** for long-running tasks

#### Weaknesses
- ❌ **Graph complexity** for simple workflows
- ❌ **Python-centric** (like CrewAI, AutoGen)
- ❌ **Steeper learning curve** than lightweight frameworks

#### Best Use Cases
- Complex multi-step workflows requiring state persistence
- Parallel coding tasks with shared state coordination
- Workflows needing checkpointing/resumption

#### CLI-Subprocess Suitability: ⭐⭐
Poor fit for CLI subprocesses. Designed for Python library integration. State management is Pythonic (TypedDict), not filesystem-friendly.

---

### 1.5 Microsoft Magentic-One

**Status**: Research system (built on AutoGen)

#### Architecture
- **Orchestrator agent** (lead) + **4 specialized agents**:
  - **WebSurfer**: Browser-based tasks, website navigation
  - **FileSurfer**: File operations, document reading
  - **Coder**: Code writing and analysis
  - **ComputerTerminal**: Code execution, system operations

#### Orchestrator Workflow
- **Outer loop**: Manages Task Ledger (facts, guesses, plan)
- **Inner loop**: Manages Progress Ledger (current progress, task assignments)
- Self-reflects on progress, checks task completion
- Updates Task Ledger if no progress for N steps

#### Key Features
- **Modular design**: Add/remove agents without prompt tuning
- **Model-agnostic**: Works with GPT-4o, other LLMs
- **LLM flexibility**: Different models for different agents (cost/capability tradeoffs)

#### Strengths for Coding Tasks
- ✅ **Comprehensive toolset** (browser, files, code, terminal)
- ✅ **Self-reflection** (orchestrator checks progress, adapts plan)
- ✅ **Generalist system** (handles diverse coding tasks)

#### Weaknesses
- ❌ **Research prototype** (not production-ready)
- ❌ **Complex orchestration** (two-loop system)
- ❌ **Opaque** compared to simpler frameworks

#### Best Use Cases
- End-to-end coding tasks (research → implement → test)
- Tasks requiring browser automation + file ops + code execution
- Complex multi-domain problems

#### CLI-Subprocess Suitability: ⭐⭐⭐⭐
Good potential. Agents are specialized tools. Could map to CLI subprocesses (browser, file, coder, terminal). Orchestrator pattern fits coordinator role.

---

### 1.6 OpenAI Swarm (Superseded by OpenAI Agents SDK)

**Status**: Educational framework (replaced by Agents SDK for production)

#### Architecture
- **Lightweight, stateless** agents
- **Two primitives**: Agents + Handoffs
- **Client-side execution** (no hosted state)
- `client.run()` loop handles completion → tool execution → agent switching

#### Coordination Mechanisms
- **Agents encapsulate** instructions + tools
- **Handoffs**: Functions return another Agent to transfer control
- **Context variables**: Dynamic instructions based on passed context
- **Result objects**: Pass value + new agent + updated context downstream

#### Communication Patterns
- Stateless between calls (like Chat Completions)
- Functions automatically convert to JSON schemas
- Streaming support with custom delimiters for agent transitions

#### Strengths for Coding Tasks
- ✅ **Extreme simplicity** (two primitives only)
- ✅ **Transparent** (full control over agent behavior)
- ✅ **Testable** (client-side, no hidden state)
- ✅ **Sequential chains** (assembly line) and **conditional handoffs** (decision trees)

#### Weaknesses
- ❌ **No state persistence** (ephemeral execution)
- ❌ **No shared memory** (agents share info via messages only)
- ❌ **Replaced by Agents SDK** (use SDK for production)

#### Best Use Cases
- Learning multi-agent patterns
- Prototyping simple agent coordination
- Triage systems (routing to specialized handlers)

#### CLI-Subprocess Suitability: ⭐⭐
Poor fit. Designed for in-process, stateless execution. No built-in subprocess management. Better as inspiration for handoff patterns.

---

## 2. Shared Memory Approaches

### 2.1 Filesystem-Based (JSON/Markdown)

#### How It Works
- Agents read/write shared files in structured workspace
- Common patterns:
  - **Step results**: `step1_results.json`, `step2_results.json`
  - **Shared state**: `state.json` with atomic writes
  - **AGENTS.md**: Project-wide agent instructions
  - **Task files**: `tasks/*.json` for work queue

#### Pros
- ✅ **Language-agnostic** (any agent can parse JSON/markdown)
- ✅ **Observable** (humans can inspect files directly)
- ✅ **Simple** (no external dependencies)
- ✅ **Persistent** (survives crashes, enables resumption)
- ✅ **Version control friendly** (can track state changes in Git)

#### Cons
- ❌ **File locking complexity** (race conditions on concurrent writes)
- ❌ **No atomic multi-file updates** (need transaction mechanism)
- ❌ **Polling required** for change detection (unless using file watchers)
- ❌ **Not ideal for high-frequency updates**

#### Best For
- CLI-based agent swarms
- Long-running workflows with intermittent updates
- Systems requiring observability and debuggability
- Polyglot environments (Rust, Python, Node agents)

#### Example: Claude Code Agent Teams
Uses `~/.claude/tasks/{team-name}/` for shared task list. Atomic task claiming via file locks.

#### Example: AgentFS (Turso)
Provides:
- **POSIX-like filesystem** for files/directories
- **Key-value store** for agent state
- **Toolcall audit trail** for debugging
- **Copy-on-write isolation** (system-wide, cannot bypass)

#### CLI-Subprocess Suitability: ⭐⭐⭐⭐⭐
Perfect. Native filesystem access. Language-agnostic. Simple coordination via shared files.

---

### 2.2 Redis (In-Memory)

#### How It Works
- Agents connect to Redis server for shared state
- Data structures:
  - **Hashes/JSON**: Object storage (agent state, results)
  - **Streams**: Event sourcing (append-only log)
  - **Pub/sub**: Real-time messaging between agents
  - **Vectors**: Semantic search for RAG

#### Pros
- ✅ **Sub-millisecond performance** (in-memory)
- ✅ **Atomic operations** (prevents race conditions)
- ✅ **Real-time messaging** (pub/sub for instant coordination)
- ✅ **Multi-model** (vectors, JSON, streams in one DB)
- ✅ **Distributed** (agents can run on different machines)

#### Cons
- ❌ **External dependency** (requires Redis server)
- ❌ **Ephemeral by default** (loses data on crash unless persisted)
- ❌ **Complexity** (setup, connection management, error handling)
- ❌ **Opaque** (can't inspect state with standard tools)

#### Best For
- High-frequency coordination (real-time task queues)
- Distributed agent systems (agents on different hosts)
- Systems needing pub/sub messaging
- Enterprise workflows with existing Redis infrastructure

#### Example: LTMC (Long-Term Memory and Context)
Uses **4-tier memory**:
1. SQLite (temporal storage)
2. FAISS (semantic vector search)
3. Redis (real-time caching/orchestration)
4. Neo4j (graph relationships)

#### Example: Redis-Powered Multi-Agent Workflow
Orchestrates 8 specialized coding agents using Redis as "central nervous system" for atomic operations and pub/sub.

#### CLI-Subprocess Suitability: ⭐⭐⭐
Doable but adds complexity. Each subprocess needs Redis client. Better for distributed systems than local CLI coordination.

---

### 2.3 SQLite (Persistent Queryable State)

#### How It Works
- Agents read/write to shared SQLite database
- Common patterns:
  - **Entity tables**: Agents, tasks, results
  - **Relationship tables**: Agent dependencies, task assignments
  - **Event log**: Append-only record of agent actions
  - **Memory store**: Structured entity extraction (à la Memori)

#### Pros
- ✅ **Persistent** (survives crashes)
- ✅ **Queryable** (SQL for complex state retrieval)
- ✅ **ACID transactions** (atomic multi-row updates)
- ✅ **Portable** (single file, no server)
- ✅ **Observable** (can query with sqlite3 CLI)

#### Cons
- ❌ **File locking** on concurrent writes (SQLite limits)
- ❌ **Not ideal for high concurrency** (serialized writes)
- ❌ **Schema migrations** can be tricky
- ❌ **Requires DB library** in each agent

#### Best For
- Medium-concurrency workflows (5-10 agents)
- Systems needing queryable history
- Workflows with complex relational data
- Agents requiring structured memory (Memori use case)

#### Example: Memori (GibsonAI)
SQL-native memory engine for AI agents:
- Structured entity extraction
- Relationship mapping
- SQL-based retrieval
- Transparent, portable, queryable memory

#### Example: LTMC
Uses SQLite as temporal storage tier (bottom of 4-tier memory system).

#### CLI-Subprocess Suitability: ⭐⭐⭐⭐
Good fit. Single file, language-agnostic clients (Rust: rusqlite, Python: sqlite3). Handles moderate concurrency. Queryable for debugging.

---

### 2.4 Blackboard Pattern (Shared Knowledge Base)

#### How It Works
- **Central blackboard** (knowledge repository) that all agents read/write
- Agents ("knowledge sources") post partial solutions when constraints match blackboard state
- **Control component** decides which agent acts next (priority-based or opportunistic)
- Agents communicate **solely through blackboard** (no direct contact)

#### Pros
- ✅ **Asynchronous coordination** (agents don't block each other)
- ✅ **Incremental progress** (agents build on each other's work)
- ✅ **No direct coupling** (agents only know blackboard interface)
- ✅ **Consistency** (single source of truth for all messages)
- ✅ **Reduced prompt length** (no need for per-agent memory modules)

#### Cons
- ❌ **Single point of failure** (blackboard crashes = system halts)
- ❌ **Contention** on blackboard writes (can bottleneck)
- ❌ **Complex control logic** (deciding which agent acts next)
- ❌ **No private agent state** (everything on blackboard)

#### Best For
- Collaborative problem-solving (multiple specialists)
- Systems where partial solutions build on each other
- Workflows needing consistent shared context
- LLM-based multi-agent systems (reduces token usage per agent)

#### Example: LbMAS (LLM-based Multi-Agent Blackboard System)
- Agents' messages stored on blackboard
- Memory modules unnecessary (blackboard is memory)
- Enables more discussion turns under token constraint
- Agents decide independently what to write

#### Example: Agent Blackboard (GitHub: claudioed/agent-blackboard)
Multi-agent coordination for software engineering with 9 specialized agents:
- Documentation, API Design, Backend Architecture
- Java/Go Development, DDD, Observability

#### CLI-Subprocess Suitability: ⭐⭐⭐⭐
Good fit. Blackboard can be filesystem (shared JSON) or SQLite. Agents are subprocesses posting to blackboard. Control component can be main coordinator script.

---

### 2.5 Event Sourcing (Append-Only Log)

#### How It Works
- All agent actions recorded as **immutable events** in append-only log
- Current state derived by replaying events
- Agents subscribe to event streams, react to new events
- **Persistent subscriptions** with competing consumers (load balancing)

#### Pros
- ✅ **Full auditability** (complete history of agent actions)
- ✅ **Replayable** (recover from failures by replaying events)
- ✅ **Time-travel debugging** (inspect state at any point in past)
- ✅ **Decoupled agents** (agents react to events, not direct calls)
- ✅ **Sophisticated consumers** (multiple agents react to same event)

#### Cons
- ❌ **Complexity** (event schema design, replay logic)
- ❌ **Storage overhead** (never deletes events)
- ❌ **Eventual consistency** (state lags behind events)
- ❌ **Requires event store** (KurrentDB, Kafka, etc.)

#### Best For
- Systems requiring full audit trail (compliance, debugging)
- Long-running workflows with complex state evolution
- Distributed agents reacting to shared event stream
- Multi-agent systems needing replay for recovery

#### Example: KurrentDB + Multi-Agent Coordination
- Events recorded in append-only store
- Agents subscribe to specific streams/categories
- Server-maintained competing consumers + load balancing
- Agents automatically routed to events on append

#### Example: Shared Persistent State (Event Sourcing Pattern)
Agents pass context via event log:
- Event = mechanism for agent-to-agent context passing
- Single source of truth ensures consistent operation
- Resilience through replayable events

#### CLI-Subprocess Suitability: ⭐⭐⭐
Moderate fit. Requires event store (complexity). Better for distributed systems. Can use filesystem-based append-only log (simpler) for CLI subprocesses.

---

## 3. Coordinator Patterns

### 3.1 Orchestrator (Central Coordinator)

#### How It Works
- **Central orchestrator** interprets task, decomposes into subtasks
- Orchestrator calls agents explicitly, monitors results
- Orchestrator decides next step (call another agent or rollback)
- Command-driven communication (orchestrator → agents)

#### Pros
- ✅ **Predictable** (clear control flow)
- ✅ **Observable** (single point to monitor)
- ✅ **Easy to debug** (centralized logic)
- ✅ **Error containment** (orchestrator handles failures)
- ✅ **Best success rate** (4.4x error amplification vs. 17.2x for independent agents)

#### Cons
- ❌ **Single point of failure** (orchestrator crashes = system halts)
- ❌ **Bottleneck** (all work passes through orchestrator)
- ❌ **Less flexible** (can't adapt to emergent conditions)

#### Best For
- **Complex multi-domain workflows** (reasoning transparency critical)
- **Quality assurance** (orchestrator validates agent outputs)
- **Coding tasks requiring traceability** (who did what, when)

#### Examples
- **Claude Code Agent Teams**: Lead agent as orchestrator
- **Magentic-One**: Orchestrator with 4 specialized agents
- **CrewAI Hierarchical**: Manager agent coordinates workers

#### CLI-Subprocess Suitability: ⭐⭐⭐⭐⭐
Perfect. Main coordinator script spawns agent subprocesses, monitors outputs, decides next steps. Proven pattern.

---

### 3.2 Choreography (Event-Driven Decentralized)

#### How It Works
- **No central controller**
- Agents publish events to message bus (don't know who's listening)
- Other agents subscribe to events, perform actions independently
- Event-driven communication (agents → events → agents)

#### Pros
- ✅ **Scalable** (no central bottleneck)
- ✅ **Resilient** (no single point of failure)
- ✅ **Flexible** (agents adapt independently to events)
- ✅ **Loose coupling** (agents don't depend on each other directly)

#### Cons
- ❌ **Complex error management** (no central controller to handle failures)
- ❌ **Hard to trace** (no single execution path)
- ❌ **Testing difficulty** (emergent behavior)
- ❌ **Risk of cascading failures** (event loops, infinite reactions)

#### Best For
- **Scalable, real-time systems** (more important than traceability)
- **Microservices-style architectures** (independent services)
- **Systems with unpredictable workflows** (agents react to conditions)

#### Examples
- **Event-driven multi-agent systems** (Confluent blog patterns)
- **Pub/sub architectures** (agents subscribe to Redis/Kafka topics)

#### CLI-Subprocess Suitability: ⭐⭐
Poor fit for CLI. Requires message bus (Redis, Kafka). Adds complexity. Better for distributed systems than local coordination.

---

### 3.3 Hierarchical (Lead → Sub-Leads → Workers)

#### How It Works
- **Tree structure**: Top-level leader, mid-tier planners, bottom-tier workers
- **Strategy layer** (leader): Decides priorities
- **Planning layer** (planners): Re-expresses priorities, orders subtasks
- **Execution layer** (workers): Generate code, call APIs, run inference

#### Pros
- ✅ **Scalable** (divide-and-conquer)
- ✅ **Clear responsibilities** (each layer has defined role)
- ✅ **Parallel execution** (workers operate independently)
- ✅ **Modular** (update/test agents independently)
- ✅ **Error containment** (centralized orchestrator: 4.4x amplification vs. 17.2x)

#### Cons
- ❌ **Overhead** (multiple coordination layers)
- ❌ **Latency** (messages pass through layers)
- ❌ **Complexity** (managing tree structure)

#### Best For
- **Large-scale coding projects** (many independent subtasks)
- **Complex task decomposition** (strategy → planning → execution)
- **Systems with 10+ agents** (need hierarchical organization)

#### Examples
- **Hierarchical Multi-Agent Systems (HMAS)**
- **AutoGen Manager Agent** with worker agents
- **CrewAI Hierarchical Crews**

#### CLI-Subprocess Suitability: ⭐⭐⭐⭐
Good fit. Main coordinator (leader) spawns sub-coordinators (planners), which spawn workers. Maps to subprocess tree. Requires careful process management.

---

### 3.4 Blackboard (Reactive Shared Knowledge)

#### How It Works
- **Blackboard** (shared knowledge base) + **Knowledge sources** (agents)
- Agents react to blackboard changes when constraints match
- **Control component** decides which agent acts next
- Agents post partial solutions, others build on them

#### Pros
- ✅ **Flexible** (agents self-organize based on blackboard state)
- ✅ **Incremental progress** (partial solutions accumulate)
- ✅ **Asynchronous** (agents don't block each other)
- ✅ **Works for uncertain problems** (no predetermined solution path)

#### Cons
- ❌ **Opaque coordination** (hard to predict execution order)
- ❌ **Control complexity** (deciding which agent acts)
- ❌ **Single point of failure** (blackboard)

#### Best For
- **Exploratory coding tasks** (no clear solution path)
- **Collaborative problem-solving** (multiple specialists)
- **Systems with uncertain workflows** (agents react opportunistically)

#### Examples
- **LbMAS** (LLM-based blackboard system)
- **Agent Blackboard** (9 coding agents: docs, API design, backend, etc.)

#### CLI-Subprocess Suitability: ⭐⭐⭐⭐
Good fit. Blackboard = shared filesystem or SQLite. Agents = subprocesses polling blackboard, posting updates. Control component = main script deciding agent activation order.

---

## 4. What Works Best for Coding Tasks

### 4.1 Framework Recommendations

| Use Case | Best Framework | Why |
|----------|---------------|-----|
| **CLI-based agent swarms** | Claude Code Agent Teams | Native subprocess support, filesystem integration, proven at scale |
| **Python-embedded agents** | AutoGen or CrewAI | Rich Python ecosystems, modular designs |
| **Stateful workflows** | LangGraph | Explicit state management, checkpointing |
| **Generalist coding tasks** | Magentic-One | Comprehensive toolset (browser, files, coder, terminal) |
| **Learning/prototyping** | OpenAI Swarm | Simple, transparent, easy to understand |

### 4.2 Shared Memory Recommendations

| Environment | Best Memory Approach | Why |
|-------------|---------------------|-----|
| **CLI subprocesses** | Filesystem (JSON/Markdown) | Language-agnostic, observable, simple |
| **Local agents (5-10)** | SQLite | Queryable, persistent, ACID transactions |
| **High-frequency coordination** | Redis | Sub-millisecond, atomic, real-time |
| **Audit-critical systems** | Event Sourcing | Full history, replayable, time-travel debugging |
| **Collaborative problem-solving** | Blackboard | Incremental progress, asynchronous, flexible |

### 4.3 Coordinator Pattern Recommendations

| Workflow Type | Best Pattern | Why |
|---------------|-------------|-----|
| **Deterministic workflows** | Orchestrator | Predictable, traceable, error containment (4.4x amplification) |
| **Scalable real-time systems** | Choreography | No bottleneck, resilient, loose coupling |
| **Large projects (10+ agents)** | Hierarchical | Divide-and-conquer, parallel execution, modular |
| **Exploratory/uncertain tasks** | Blackboard | Flexible, incremental, self-organizing |

---

## 5. Recommended Architecture for CLI-Based Coding Agent Swarm

### 5.1 Architecture Summary

**Pattern**: Hierarchical Orchestrator with Filesystem-Based Shared State

**Components**:
1. **Main Coordinator** (Rust CLI)
   - Spawns agent subprocesses (Claude Code instances)
   - Manages shared task list (`tasks/*.json`)
   - Monitors agent progress, handles failures

2. **Agent Subprocesses** (Claude Code sessions)
   - Independent context windows
   - Read/write shared filesystem (`state.json`, `results/*.json`)
   - Post status updates to mailbox (`mailbox/{agent-id}.json`)

3. **Shared Filesystem State**
   - `tasks/`: Task queue with dependencies
   - `state.json`: Global workflow state
   - `results/`: Agent outputs (step1.json, step2.json, etc.)
   - `mailbox/`: Agent-to-agent messages
   - `AGENTS.md`: Project-wide agent instructions

4. **Control Flow**
   - Orchestrator → Task decomposition → Task assignment
   - Agents → Task claiming (atomic file locks) → Execution → Result posting
   - Orchestrator → Result synthesis → Next task generation

### 5.2 Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| **Coordinator pattern** | Orchestrator | Best error containment (4.4x vs. 17.2x), traceable, proven for coding |
| **Memory approach** | Filesystem (JSON) | Language-agnostic, observable, no external deps, CLI-native |
| **Task claiming** | File locks (atomic) | Prevents race conditions (proven by Claude Teams) |
| **Agent communication** | Mailbox (JSON files) | Simple, asynchronous, persistent |
| **Context isolation** | Separate processes | Prevents context pollution, parallel execution |
| **Observability** | Filesystem + logs | Human-inspectable state, standard tools (ls, cat, jq) |

### 5.3 Why This Works for Coding Agents

1. **Native CLI integration**: Agents are actual subprocesses (Claude Code), not API wrappers
2. **Language-agnostic**: JSON/markdown works with any agent implementation (Rust, Python, Node)
3. **Observable**: State visible with standard tools (cat state.json | jq)
4. **Persistent**: Survives crashes, enables resumption (read tasks/*.json on restart)
5. **Simple**: No external dependencies (Redis, Kafka, etc.)
6. **Proven**: Claude Code Agent Teams built a 100k-line C compiler with this pattern
7. **Debuggable**: Inspect files, replay task execution, time-travel debugging

### 5.4 Scaling Considerations

| Agent Count | Memory Approach | Coordinator Pattern |
|-------------|----------------|-------------------|
| **1-5 agents** | Filesystem (JSON) | Simple orchestrator |
| **5-10 agents** | Filesystem or SQLite | Orchestrator with task dependencies |
| **10-20 agents** | SQLite + Redis (optional) | Hierarchical (1 lead + 2-3 sub-leads) |
| **20+ agents** | Redis + SQLite + Event sourcing | Hierarchical (3+ layers) + blackboard for exploratory tasks |

---

## 6. Implementation Checklist

### Phase 1: Basic Orchestrator (MVP)
- [ ] Main coordinator spawns 1 agent subprocess
- [ ] Agent reads task from `tasks/task1.json`
- [ ] Agent writes result to `results/task1.json`
- [ ] Coordinator reads result, terminates agent
- [ ] Basic error handling (agent crashes)

### Phase 2: Multi-Agent Coordination
- [ ] Coordinator spawns N agents (configurable)
- [ ] Shared task queue (`tasks/*.json`)
- [ ] Atomic task claiming (file locks)
- [ ] Task dependencies (blocked until deps complete)
- [ ] Agent mailbox (`mailbox/{agent-id}.json`)

### Phase 3: Advanced Features
- [ ] Hierarchical coordination (sub-coordinators)
- [ ] Plan approval workflow (agent submits plan → coordinator approves)
- [ ] Session resumption (read state.json on restart)
- [ ] Observability dashboard (TUI showing agent status)
- [ ] SQLite upgrade (for queryable history)

### Phase 4: Production Hardening
- [ ] Redis integration (optional, for high-frequency tasks)
- [ ] Event sourcing (audit trail)
- [ ] Failure recovery (retry logic, checkpointing)
- [ ] Load balancing (distribute tasks across agents)
- [ ] Metrics/monitoring (task completion rates, error rates)

---

## 7. Key Learnings & Takeaways

### 7.1 Universal Patterns

1. **Orchestrator beats choreography for coding** (4.4x vs. 17.2x error amplification)
2. **Filesystem-based state is underrated** (simple, observable, persistent)
3. **Separate context windows are critical** (prevents context pollution, enables parallel work)
4. **Task claiming needs atomicity** (file locks, Redis atomic ops, DB transactions)
5. **Agent communication should be asynchronous** (mailbox, pub/sub, event streams)

### 7.2 Framework Insights

- **Claude Code Agent Teams**: Gold standard for CLI-based coding agents (proven at scale)
- **CrewAI/AutoGen**: Better for Python-embedded workflows than CLI subprocesses
- **LangGraph**: Overkill for simple tasks, necessary for complex stateful workflows
- **Magentic-One**: Inspiring architecture (4 specialists + orchestrator), but research-stage
- **OpenAI Swarm**: Excellent for learning, superseded by Agents SDK for production

### 7.3 Memory Insights

- **Filesystem**: Best default for CLI subprocesses (observable, simple, persistent)
- **SQLite**: Upgrade path when queryability matters (5-10 agents)
- **Redis**: Only for high-frequency coordination or distributed agents
- **Event Sourcing**: Only for audit-critical systems (adds complexity)
- **Blackboard**: Good for exploratory tasks, but control component is tricky

### 7.4 Coordinator Insights

- **Orchestrator**: Default choice for coding tasks (traceable, error containment)
- **Hierarchical**: Scale up to 20+ agents (divide-and-conquer)
- **Blackboard**: Niche use case (exploratory, uncertain workflows)
- **Choreography**: Avoid for coding (hard to trace, cascading failures)

---

## 8. Next Steps

1. **Prototype**: Implement Phase 1 (basic orchestrator + 1 agent)
2. **Test**: Run simple coding task (research API → implement connector)
3. **Iterate**: Add Phase 2 features (multi-agent, task queue, mailbox)
4. **Scale**: Test with 5-10 agents on connector carousel pipeline
5. **Harden**: Add Phase 3/4 features as needed (resumption, SQLite, observability)

---

## Sources

### Claude Code Agent Teams
- [Orchestrate teams of Claude Code sessions - Claude Code Docs](https://code.claude.com/docs/en/agent-teams)
- [Anthropic releases Opus 4.6 with new 'agent teams' | TechCrunch](https://techcrunch.com/2026/02/05/anthropic-releases-opus-4-6-with-new-agent-teams/)
- [Claude Code's Hidden Multi-Agent System](https://paddo.dev/blog/claude-code-hidden-swarm/)

### CrewAI
- [GitHub - crewAIInc/crewAI](https://github.com/crewAIInc/crewAI)
- [Introduction - CrewAI](https://docs.crewai.com/en/introduction)
- [Mastering AI Agent Orchestration- Comparing CrewAI, LangGraph, and OpenAI Swarm](https://medium.com/@arulprasathpackirisamy/mastering-ai-agent-orchestration-comparing-crewai-langgraph-and-openai-swarm-8164739555ff)

### AutoGen
- [Multi-agent Conversation Framework | AutoGen 0.2](https://microsoft.github.io/autogen/0.2/docs/Use-Cases/agent_chat/)
- [Design Patterns for AI Agents: Using Autogen for Effective Multi-Agent Collaboration](https://medium.com/@LakshmiNarayana_U/design-patterns-for-ai-agents-using-autogen-for-effective-multi-agent-collaboration-5f1067a7c63b)
- [Deep Dive into AutoGen Multi-Agent Patterns 2025](https://sparkco.ai/blog/deep-dive-into-autogen-multi-agent-patterns-2025)

### LangGraph
- [Build multi-agent systems with LangGraph and Amazon Bedrock](https://aws.amazon.com/blogs/machine-learning/build-multi-agent-systems-with-langgraph-and-amazon-bedrock/)
- [LangGraph Multi-Agent Orchestration: Complete Framework Guide + Architecture Analysis 2025](https://latenode.com/blog/ai-frameworks-technical-infrastructure/langgraph-multi-agent-orchestration/langgraph-multi-agent-orchestration-complete-framework-guide-architecture-analysis-2025)
- [Mastering LangGraph State Management in 2025](https://sparkco.ai/blog/mastering-langgraph-state-management-in-2025)

### Microsoft Magentic-One
- [Magentic-One: A Generalist Multi-Agent System for Solving Complex Tasks - Microsoft Research](https://www.microsoft.com/en-us/research/articles/magentic-one-a-generalist-multi-agent-system-for-solving-complex-tasks/)
- [Microsoft Introduces Magentic-One, a Generalist Multi-Agent System - InfoQ](https://www.infoq.com/news/2024/11/microsoft-magentic-one/)
- [Magentic-One — AutoGen](https://microsoft.github.io/autogen/stable//user-guide/agentchat-user-guide/magentic-one.html)

### OpenAI Swarm
- [GitHub - openai/swarm](https://github.com/openai/swarm)
- [OpenAI Swarm Framework Guide for Reliable Multi-Agents](https://galileo.ai/blog/openai-swarm-framework-multi-agents)
- [Deep Dive into OpenAI Swarm Agent Patterns](https://sparkco.ai/blog/deep-dive-into-openai-swarm-agent-patterns)

### Blackboard Pattern
- [Exploring Advanced LLM Multi-Agent Systems Based on Blackboard Architecture](https://arxiv.org/html/2507.01701v1)
- [Building Intelligent Multi-Agent Systems with MCPs and the Blackboard Pattern](https://medium.com/@dp2580/building-intelligent-multi-agent-systems-with-mcps-and-the-blackboard-pattern-to-build-systems-a454705d5672)
- [GitHub - claudioed/agent-blackboard](https://github.com/claudioed/agent-blackboard)

### Filesystem-Based State
- [Filesystem-Based Agent State - Awesome Agentic Patterns](https://agentic-patterns.com/patterns/filesystem-based-agent-state/)
- [GitHub - tursodatabase/agentfs](https://github.com/tursodatabase/agentfs)
- [Improve your AI code output with AGENTS.md](https://www.builder.io/blog/agents-md)

### Event Sourcing
- [Four Design Patterns for Event-Driven, Multi-Agent Systems](https://www.confluent.io/blog/event-driven-multi-agent-systems/)
- [Multi Agent Systems | Shared Persistent State](https://medium.com/@aiforhuman/multi-agent-systems-shared-persistent-state-bd33a1b5030f)
- [Event-Driven Agent Coordination with KurrentDB](https://www.kurrent.io/blog/event-driven-agent-coordination-with-kurrentdb/)

### Redis & SQLite
- [LTMC: Redis-Powered Multi-Agent Memory & Orchestration Platform](https://dev.to/oldnordic/ltmc-redis-powered-multi-agent-memory-orchestration-platform-4n6o)
- [AI agent orchestration for production systems](https://redis.io/blog/ai-agent-orchestration/)
- [GibsonAI Releases Memori: An Open-Source SQL-Native Memory Engine for AI Agents](https://www.marktechpost.com/2025/09/08/gibsonai-releases-memori-an-open-source-sql-native-memory-engine-for-ai-agents/)

### Orchestrator vs Choreography
- [Saga orchestration patterns - AWS Prescriptive Guidance](https://docs.aws.amazon.com/prescriptive-guidance/latest/agentic-ai-patterns/saga-orchestration-patterns.html)
- [Choosing the right orchestration pattern for multi agent systems](https://www.kore.ai/blog/choosing-the-right-orchestration-pattern-for-multi-agent-systems)
- [Orchestration vs Choreography | Camunda](https://camunda.com/blog/2023/02/orchestration-vs-choreography/)

### Hierarchical Multi-Agent Systems
- [Hierarchical Multi-Agent Systems: Concepts and Operational Considerations](https://overcoffee.medium.com/hierarchical-multi-agent-systems-concepts-and-operational-considerations-e06fff0bea8c)
- [Hierarchical Agent Teams](https://langchain-ai.github.io/langgraph/tutorials/multi_agent/hierarchical_agent_teams/)
- [What are Hierarchical AI Agents? | IBM](https://www.ibm.com/think/topics/hierarchical-ai-agents)

### Coding Workflows
- [AddyOsmani.com - My LLM coding workflow going into 2026](https://addyosmani.com/blog/ai-coding-workflow/)
- [Agentic Coding: How I 10x'd My Development Workflow](https://medium.com/@dataenthusiast.io/agentic-coding-how-i-10xd-my-development-workflow-e6f4fd65b7f0)
- [The Three Developer Loops: A New Framework for AI-Assisted Coding](https://itrevolution.com/articles/the-three-developer-loops-a-new-framework-for-ai-assisted-coding/)

### Agent Communication Protocols
- [Top 5 Open Protocols for Building Multi-Agent AI Systems 2026](https://onereach.ai/blog/power-of-multi-agent-ai-open-protocols/)
- [A Survey of Agent Interoperability Protocols](https://arxiv.org/html/2505.02279v1)
- [Multi-Agent Communication with Google's A2A in 2026](https://research.aimultiple.com/agent2agent/)
- [Agent Client Protocol (ACP) Explained](https://codestandup.com/posts/2025/agent-client-protocol-acp-explained/)
