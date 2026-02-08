# Claude-Flow Swarm Orchestration Internals Dissection

**Repository**: claude-flow (12.6K stars)
**Source**: `c:\Users\VA PC\CODING\ML_TRADING\nemo\research\revolver-research\_tmp_src\claude-flow`
**Architecture**: TypeScript-based orchestration framework for Claude Code
**Version**: V3 (latest architecture)

---

## Executive Summary

Claude-flow is a **coordination-only framework** — it does NOT execute code itself. Instead, it:
1. **Initializes swarm state** via CLI commands (topology, agent counts, memory namespaces)
2. **Delegates execution** to Claude Code's built-in `Task` tool (real agent spawning)
3. **Tracks coordination** via shared memory (sql.js + HNSW vector search)
4. **Learns patterns** via hooks system and neural substrate (RuVector Intelligence)

**Key Insight**: Claude-flow is a "ledger" that records what's happening. Claude Code's Task tool is the "executor" that does the actual work.

---

## 1. Orchestrator/Coordinator Architecture

### Main Orchestration Loop

**No actual loop exists** — it's event-driven via CLI commands.

**File**: `v3/@claude-flow/cli/src/commands/swarm.ts`

```typescript
// Swarm init - creates coordination state, does NOT spawn agents
const initCommand: Command = {
  name: 'init',
  action: async (ctx: CommandContext): Promise<CommandResult> => {
    // 1. Call MCP tool to initialize swarm coordination
    const result = await callMCPTool('swarm_init', {
      topology: topology,  // hierarchical, mesh, ring, star, hybrid
      maxAgents,
      config: {
        communicationProtocol: 'message-bus',
        consensusMechanism: 'majority',
        failureHandling: 'retry',
        loadBalancing: true,
        autoScaling: true,
      },
    });

    // 2. Save swarm state locally (file-based state)
    const swarmDir = path.join(process.cwd(), '.swarm');
    fs.writeFileSync(path.join(swarmDir, 'state.json'), JSON.stringify({
      id: result.swarmId,
      topology: result.topology,
      maxAgents: result.config.maxAgents,
      status: 'ready'
    }, null, 2));

    return { success: true, data: result };
  }
};
```

**Architecture Decision**: No centralized orchestrator process. Instead:
- **CLI commands** write state to `.swarm/state.json`
- **Agents** (spawned via Task tool) read/write shared memory
- **Coordinator skills** (e.g., hierarchical-coordinator) manage delegation

**Why This Design**:
- Stateless CLI = no daemon to manage
- File-based state = survives crashes
- Claude Code Task tool = real execution power
- MCP tools = coordination primitives only

---

## 2. Agent Spawning Mechanism

### How Agents Are Created

**CRITICAL**: Claude-flow does NOT spawn agents. It only registers intent.

**File**: `v3/@claude-flow/cli/src/commands/agent.ts`

```typescript
const spawnCommand: Command = {
  name: 'spawn',
  action: async (ctx: CommandContext): Promise<CommandResult> => {
    // Call MCP tool - this only creates a RECORD
    const result = await callMCPTool('agent_spawn', {
      agentType,        // coder, researcher, tester, etc.
      id: agentName,
      config: {
        provider: 'anthropic',
        model: ctx.flags.model,
        task: ctx.flags.task,
        timeout: ctx.flags.timeout,
        autoTools: ctx.flags.autoTools,
      },
      metadata: {
        name: agentName,
        capabilities: getAgentCapabilities(agentType),
      },
    });

    // Returns metadata only - NO ACTUAL AGENT SPAWNED
    return { success: true, data: result };
  }
};
```

**MCP Tool Implementation**: `v3/@claude-flow/cli/src/mcp-tools/agent-tools.ts`

```typescript
{
  name: 'agent_spawn',
  handler: async (input) => {
    // THIS IS THE KEY - it only returns metadata, doesn't spawn
    return {
      agentId: input.id || `agent-${Date.now()}`,
      agentType: input.agentType,
      status: 'active',  // Fake status - no real agent!
      createdAt: new Date().toISOString(),
    };
  },
}
```

**The REAL Agent Spawning** happens via Claude Code's Task tool:

**File**: `CLAUDE.md` (coordinator instructions)

```markdown
// STEP 2: Spawn ALL agents IN BACKGROUND in a SINGLE message
Task({
  prompt: "Research requirements, analyze codebase patterns",
  subagent_type: "researcher",
  run_in_background: true  // ← CRITICAL: Real agent execution
})
Task({
  prompt: "Design architecture based on research",
  subagent_type: "system-architect",
  run_in_background: true
})
```

**Architecture Decision**:
1. **CLI `agent spawn`**: Records intention, returns fake status
2. **Claude Code sees CLI output**: Knows to use Task tool
3. **Task tool spawns real agents**: Actual work happens here
4. **Agents report back**: Via shared memory or results

**Why This Design**:
- CLI can't spawn LLM instances (no API access)
- Task tool is Claude Code's native capability
- Separation of concerns: CLI = coordination, Task = execution

---

## 3. Mailbox/Messaging System

### Inter-Agent Communication

**File**: `.agents/skills/agent-hierarchical-coordinator/SKILL.md`

```yaml
hooks:
  pre: |
    # MANDATORY: Write initial status to coordination namespace
    mcp__claude-flow__memory_usage store "swarm$hierarchical$status" \
      "{\"agent\":\"hierarchical-coordinator\",\"status\":\"initializing\"}" \
      --namespace=coordination
  post: |
    # MANDATORY: Write completion status
    mcp__claude-flow__memory_usage store "swarm$hierarchical$complete" \
      "{\"status\":\"complete\",\"agents_used\":5}" \
      --namespace=coordination
```

**Memory-Based Messaging Protocol**:

**Memory Key Structure**:
```
swarm$hierarchical/*        - Coordinator's own data
swarm$worker-1/*            - Individual worker states
swarm$shared/*              - Shared coordination data
ALL use namespace: "coordination"
```

**Communication Pattern** (from hierarchical-coordinator SKILL.md):

```javascript
// 1️⃣ Agent writes initial status
mcp__claude-flow__memory_usage({
  action: "store",
  key: "swarm$hierarchical$status",
  namespace: "coordination",
  value: JSON.stringify({
    agent: "hierarchical-coordinator",
    status: "active",
    workers: [],
    tasks_assigned: []
  })
})

// 2️⃣ Update progress after delegation
mcp__claude-flow__memory_usage({
  action: "store",
  key: "swarm$hierarchical$progress",
  namespace: "coordination",
  value: JSON.stringify({
    completed: ["task1"],
    in_progress: ["task2"],
    workers_active: 3
  })
})

// 3️⃣ CHECK worker status before assigning
const workerStatus = mcp__claude-flow__memory_usage({
  action: "retrieve",
  key: "swarm$worker-1$status",
  namespace: "coordination"
})

// 4️⃣ SIGNAL completion
mcp__claude-flow__memory_usage({
  action: "store",
  key: "swarm$hierarchical$complete",
  namespace: "coordination",
  value: JSON.stringify({
    status: "complete",
    deliverables: ["final_product"]
  })
})
```

**Message Format** (JSON in memory):

```typescript
interface AgentStatus {
  agent: string;           // Agent identifier
  status: string;          // "initializing" | "active" | "complete"
  workers?: string[];      // Child workers (hierarchical)
  tasks_assigned?: any[];  // Tasks delegated
  progress?: number;       // 0-100
  deliverables?: string[]; // Output artifacts
  timestamp: number;       // Unix timestamp
}
```

**Architecture Decision**:
- **No message queue**: Just shared memory (sql.js database)
- **Pull-based**: Agents poll memory keys to check status
- **Namespace isolation**: `coordination` namespace for swarm state
- **Vector search**: For semantic discovery of related state

**Why This Design**:
- Simple: No broker/queue to manage
- Persistent: SQLite survives crashes
- Fast: HNSW index = 150x-12,500x speedup
- Observable: All state visible in `.claude-flow/memory/`

---

## 4. Task Distribution Mechanism

### Task Orchestration

**File**: `.agents/skills/agent-orchestrator-task/SKILL.md`

```yaml
name: task-orchestrator
description: Central coordination agent for task decomposition
capabilities:
  - task_decomposition
  - execution_planning
  - dependency_management
  - result_aggregation
```

**Task Decomposition Pattern**:

```
Phase 1: Task Analysis
  ├─ Parse incoming task requirements
  ├─ Identify key deliverables and constraints
  └─ Estimate resource requirements

Phase 2: Task Breakdown
  ├─ Break down into work packages
  ├─ Define dependencies and sequencing
  └─ Assign priority levels and deadlines

Phase 3: Agent Assignment
  ├─ Determine required agent types
  ├─ Filter agents by capability match
  ├─ Score agents by performance history
  ├─ Consider current workload
  └─ Select optimal agent
```

**Execution Strategies** (from swarm.ts):

```typescript
function getAgentPlan(strategy: string) {
  const plans = {
    specialized: [
      { role: 'Coordinator', type: 'coordinator', count: 1 },
      { role: 'Researcher', type: 'researcher', count: 1 },
      { role: 'Architect', type: 'architect', count: 1 },
      { role: 'Coder', type: 'coder', count: 2 },
      { role: 'Tester', type: 'tester', count: 1 },
      { role: 'Reviewer', type: 'reviewer', count: 1 }
    ],
    development: [
      { role: 'Coordinator', type: 'coordinator', count: 1 },
      { role: 'Architect', type: 'architect', count: 1 },
      { role: 'Coder', type: 'coder', count: 3 },
      { role: 'Tester', type: 'tester', count: 2 },
      { role: 'Reviewer', type: 'reviewer', count: 1 }
    ],
    research: [
      { role: 'Coordinator', type: 'coordinator', count: 1 },
      { role: 'Researcher', type: 'researcher', count: 4 },
      { role: 'Analyst', type: 'analyst', count: 2 }
    ]
  };
  return plans[strategy] || plans.development;
}
```

**Task Assignment Algorithm** (from hierarchical-coordinator):

```python
def assign_task(task, available_agents):
    # 1. Filter agents by capability match
    capable_agents = filter_by_capabilities(available_agents, task.required_capabilities)

    # 2. Score agents by performance history (from memory)
    scored_agents = score_by_performance(capable_agents, task.type)

    # 3. Consider current workload (from status in memory)
    balanced_agents = consider_workload(scored_agents)

    # 4. Select optimal agent
    return select_best_agent(balanced_agents)
```

**Task State Tracking**:

```typescript
// From swarm.ts - dynamic status from filesystem
function getSwarmStatus(swarmId?: string) {
  const tasksDir = path.join('.swarm', 'tasks');
  let completedTasks = 0;
  let inProgressTasks = 0;
  let pendingTasks = 0;

  // Read task files
  const taskFiles = fs.readdirSync(tasksDir).filter(f => f.endsWith('.json'));
  for (const file of taskFiles) {
    const task = JSON.parse(fs.readFileSync(path.join(tasksDir, file)));
    if (task.status === 'completed') completedTasks++;
    else if (task.status === 'in_progress') inProgressTasks++;
    else pendingTasks++;
  }

  const totalTasks = completedTasks + inProgressTasks + pendingTasks;
  const progress = totalTasks > 0 ? Math.round((completedTasks / totalTasks) * 100) : 0;

  return {
    tasks: { total: totalTasks, completed: completedTasks, inProgress: inProgressTasks },
    progress
  };
}
```

**Architecture Decision**:
- **File-based task queue**: `.swarm/tasks/*.json`
- **Status polling**: Read files to check progress
- **Strategy templates**: Pre-defined agent plans
- **Dynamic routing**: Based on capabilities + load

**Why This Design**:
- Observable: Tasks visible as files
- Crash-safe: File writes are atomic
- Simple: No complex queue infrastructure
- Flexible: Easy to add custom strategies

---

## 5. Memory/Context Sharing

### Shared State Management

**Backend**: sql.js (WASM SQLite) + HNSW vector index

**File**: `v3/@claude-flow/cli/src/mcp-tools/memory-tools.ts`

```typescript
export const memoryTools: MCPTool[] = [
  {
    name: 'memory_store',
    description: 'Store a value in memory with vector embedding',
    handler: async (input) => {
      // 1. Store in sql.js SQLite database
      const result = await storeEntry({
        key: input.key,
        value: input.value,
        namespace: input.namespace || 'default',
        generateEmbeddingFlag: true,  // Auto-generate vector embedding
        tags: input.tags,
        ttl: input.ttl,
      });

      // 2. Returns embedding metadata
      return {
        success: true,
        hasEmbedding: !!result.embedding,
        embeddingDimensions: result.embedding?.dimensions,
        backend: 'sql.js + HNSW',
      };
    },
  },
  {
    name: 'memory_search',
    description: 'Semantic vector search using HNSW (150x-12,500x faster)',
    handler: async (input) => {
      // 1. Generate query embedding
      // 2. HNSW search for similar vectors
      const result = await searchEntries({
        query: input.query,
        namespace: input.namespace,
        limit: input.limit || 10,
        threshold: input.threshold || 0.3,  // Cosine similarity threshold
      });

      // 3. Return ranked results
      return {
        results: result.results.map(r => ({
          key: r.key,
          value: r.content,
          similarity: r.score  // 0-1 cosine similarity
        })),
        searchTime: `${duration.toFixed(2)}ms`,
        backend: 'HNSW + sql.js',
      };
    },
  }
];
```

**Memory Schema** (sql.js):

```sql
CREATE TABLE entries (
  id TEXT PRIMARY KEY,
  key TEXT NOT NULL,
  namespace TEXT NOT NULL,
  content TEXT NOT NULL,
  embedding BLOB,           -- Vector embedding (binary)
  embedding_dimensions INT, -- 384 for default model
  tags TEXT,                -- JSON array
  createdAt TEXT,
  updatedAt TEXT,
  accessCount INT DEFAULT 0,
  ttl INT,                  -- Expiration timestamp
  UNIQUE(namespace, key)
);

CREATE INDEX idx_namespace ON entries(namespace);
CREATE INDEX idx_tags ON entries(tags);
CREATE INDEX idx_ttl ON entries(ttl);
```

**HNSW Index** (Hierarchical Navigable Small World):
- **Built on embeddings**: 384-dimensional vectors
- **Speedup**: 150x-12,500x faster than linear scan
- **Algorithm**: Graph-based approximate nearest neighbor search
- **Tradeoff**: 95%+ recall, <100ms latency

**Context Sharing Patterns**:

```javascript
// Pattern 1: Shared Coordination State
await memory_store({
  key: "swarm$shared$hierarchy",
  namespace: "coordination",
  value: JSON.stringify({
    queen: "hierarchical-coordinator",
    workers: ["worker1", "worker2"],
    command_chain: {}
  })
});

// Pattern 2: Agent-to-Agent Handoff
await memory_store({
  key: "swarm$shared$design-doc",
  namespace: "coordination",
  value: JSON.stringify({
    created_by: "architect-agent",
    for_agent: "coder-agent",
    architecture: { /* ... */ }
  })
});

// Pattern 3: Pattern Learning
await memory_store({
  key: "pattern-auth-jwt",
  namespace: "patterns",
  value: "JWT with refresh tokens. Store refresh in httpOnly cookie, access in memory."
});

// Later: Semantic search for similar patterns
const results = await memory_search({
  query: "authentication session management",
  namespace: "patterns",
  limit: 5
});
// Returns: pattern-auth-jwt with 0.87 similarity
```

**Architecture Decision**:
- **Vector embeddings**: Enable semantic search (not just keyword)
- **Namespaces**: Isolate coordination, patterns, tasks, etc.
- **HNSW index**: Trade accuracy for speed (acceptable for agent coordination)
- **sql.js**: WASM = no native deps, cross-platform

**Why This Design**:
- **Auto-learning**: Agents find past solutions via semantic search
- **Fast**: HNSW index = <100ms searches even with 10K+ entries
- **Persistent**: SQLite = survives crashes
- **Portable**: WASM = no Python/C++ dependencies

---

## 6. Git Coordination Strategy

**NONE.** Claude-flow does NOT manage git operations.

From CLAUDE.md:

```markdown
### Claude Code Handles ALL EXECUTION:
- Git operations
- File operations (Read, Write, Edit)
- Bash commands

### CLI Tools Handle Coordination (via Bash):
- Swarm init
- Memory store/search
- Hooks
```

**Why No Git Integration**:
- Claude Code already has git tools
- Coordination framework should be tool-agnostic
- Git is execution, not coordination

**However**, skills can reference git workflows:

From `agent-github-modes/SKILL.md`:
```yaml
name: github-modes
capabilities:
  - pr_management
  - issue_tracking
  - ci_cd_integration
```

But these are **skill definitions** for Claude Code agents to follow, not git automation built into claude-flow.

---

## 7. Agent Skills / SKILL.md Format

**Total Skills**: 132 SKILL.md files found

**Skill Structure**:

```yaml
---
name: agent-hierarchical-coordinator
description: Agent skill for hierarchical-coordinator
---

---
name: hierarchical-coordinator
type: coordinator
color: "#FF6B35"
description: Queen-led hierarchical swarm coordination
capabilities:
  - swarm_coordination
  - task_decomposition
  - agent_supervision
priority: critical
hooks:
  pre: |
    echo "Initializing swarm"
    mcp__claude-flow__swarm_init hierarchical --maxAgents=10
    mcp__claude-flow__memory_usage store "status" "{\"status\":\"init\"}"
  post: |
    echo "Coordination complete"
    mcp__claude-flow__memory_usage store "complete" "{\"status\":\"done\"}"
---

# Skill Documentation (Markdown)
Instructions for how the agent should behave...
```

**Key Skills for Swarm Orchestration**:

1. **agent-hierarchical-coordinator** (`skills/agent-hierarchical-coordinator/SKILL.md`)
   - Queen-led delegation model
   - Spawns specialized workers
   - Monitors performance
   - Resolves conflicts
   - **Memory Protocol**: MANDATORY status writes every step

2. **agent-coordinator-swarm-init** (`skills/agent-coordinator-swarm-init/SKILL.md`)
   - Topology selection (hierarchical, mesh, star, ring)
   - Resource allocation
   - Memory namespace setup
   - **ENFORCES** memory write requirements

3. **agent-orchestrator-task** (`skills/agent-orchestrator-task/SKILL.md`)
   - Task decomposition
   - Dependency graphs
   - Result synthesis
   - Progress tracking via TodoWrite

4. **agent-mesh-coordinator** (`skills/agent-mesh-coordinator/SKILL.md`)
   - Peer-to-peer coordination
   - Gossip-based consensus
   - No central authority

5. **agent-adaptive-coordinator** (`skills/agent-adaptive-coordinator/SKILL.md`)
   - Dynamic topology switching
   - Load-based routing
   - Auto-scaling decisions

**Skill Invocation**: Via AGENTS.md instructions

```markdown
When the user asks for swarm initialization:
1. Use $agent-coordinator-swarm-init skill
2. Follow MANDATORY memory protocol
3. Spawn agents via Claude Code Task tool
```

**Architecture Decision**:
- **Skills = prompts**: Just instructions for Claude Code agents
- **No code execution**: Skills don't run, agents follow them
- **YAML frontmatter**: Metadata for routing/discovery
- **Hooks**: Pre/post actions (bash commands)

**Why This Design**:
- Flexible: Easy to add new skills (just markdown)
- LLM-native: Skills are prompts, not code
- Observable: Skills are readable files
- Composable: Skills reference other skills

---

## 8. Prompt System Architecture

### System Prompts

**Main Coordinator Prompt**: `CLAUDE.md` (1033 lines)

Key sections:

```markdown
## 🚨 AUTOMATIC SWARM ORCHESTRATION

When starting work on complex tasks, Claude Code MUST automatically:
1. Initialize the swarm using CLI tools via Bash
2. Spawn concurrent agents using Claude Code's Task tool
3. Coordinate via hooks and memory

### 🚨 CRITICAL: CLI + Task Tool in SAME Message
1. Call CLI tools via Bash to initialize coordination
2. IMMEDIATELY call Task tool to spawn REAL working agents
3. Both CLI and Task calls must be in the SAME response
```

**Agent Routing Table**:

```markdown
| Code | Task | Agents |
|------|------|--------|
| 1 | Bug Fix | coordinator, researcher, coder, tester |
| 3 | Feature | coordinator, architect, coder, tester, reviewer |
| 5 | Refactor | coordinator, architect, coder, reviewer |
| 7 | Performance | coordinator, perf-engineer, coder |
| 9 | Security | coordinator, security-architect, auditor |

Codes 1-9: hierarchical/specialized (anti-drift)
```

**Auto-Learning Protocol**:

```markdown
### Before Starting Any Task
1. Search memory for relevant patterns
2. Check if similar task was done before
3. Load learned optimizations

### After Completing Any Task Successfully
1. Store successful pattern for future reference
2. Train neural patterns on the successful approach
3. Record task completion with metrics
4. Trigger optimization worker if performance-related
```

**Anti-Drift Configuration**:

```markdown
## Anti-Drift Config (PREFERRED)
npx @claude-flow/cli@latest swarm init \
  --topology hierarchical \
  --max-agents 8 \
  --strategy specialized

Anti-Drift Guidelines:
- hierarchical: Coordinator catches divergence
- max-agents 6-8: Smaller team = less drift
- specialized: Clear roles, no overlap
- consensus: raft (leader maintains state)
```

**Codex-Specific Prompts**: `AGENTS.md` (635 lines)

```markdown
## 📢 TL;DR - READ THIS FIRST

1. claude-flow = LEDGER (tracks state, stores memory, coordinates)
2. Codex = EXECUTOR (writes code, runs commands, creates files)
3. NEVER stop after calling claude-flow - IMMEDIATELY continue working
4. If you need something BUILT/EXECUTED, YOU do it, not claude-flow
```

**Prompt Architecture Decision**:
- **Separation**: CLAUDE.md for Claude Code, AGENTS.md for Codex
- **Imperative tone**: MUST, ALWAYS, NEVER (clear boundaries)
- **Concrete examples**: Show exact CLI commands, not abstractions
- **Anti-patterns**: Explicitly forbid wrong behaviors

**Why This Design**:
- **LLM limitations**: Need explicit instructions to prevent drift
- **Tool confusion**: LLMs conflate CLI tools with execution
- **Async complexity**: Must teach "spawn and wait" pattern
- **Learning**: Prompt includes memory protocol for self-improvement

---

## Key Findings & Architecture Decisions

### 1. **Coordination-Only Philosophy**

Claude-flow is NOT an execution engine. It's a **state tracker** that:
- Writes swarm state to files (`.swarm/state.json`)
- Provides MCP tools for memory/coordination primitives
- Defines skills (prompts) for Claude Code agents to follow

**Real execution** happens via:
- Claude Code's `Task` tool (spawns real LLM agents)
- Claude Code's file operations (Read, Write, Edit)
- Claude Code's Bash tool (runs commands)

**Why**: CLI tools can't spawn LLM instances or execute code. They can only track intent.

### 2. **Memory-Based Messaging**

No traditional message queue. Instead:
- **Shared memory**: sql.js database with HNSW vector index
- **Pull-based**: Agents poll memory keys (`swarm$worker-1$status`)
- **Namespace isolation**: `coordination`, `patterns`, `tasks`
- **Semantic search**: Find related state via vector similarity

**Why**: Simple, persistent, fast (150x-12,500x with HNSW), observable.

### 3. **File-Based State**

All coordination state in files:
- `.swarm/state.json` - Swarm config
- `.swarm/agents/*.json` - Agent metadata
- `.swarm/tasks/*.json` - Task queue
- `.claude-flow/memory/*.db` - sql.js database

**Why**: Survives crashes, observable, no daemon to manage.

### 4. **Skill = Prompt**

Skills are NOT code. They're:
- **Markdown files** with YAML frontmatter
- **Prompts** for Claude Code agents to follow
- **Hooks** (bash commands) for pre/post actions

**Why**: LLM-native, flexible, easy to add/modify, composable.

### 5. **Anti-Drift via Constraints**

Prevent agent divergence through:
- **Hierarchical topology**: Central coordinator catches drift
- **Small teams**: 6-8 agents max (less coordination overhead)
- **Specialized roles**: Clear boundaries, no overlap
- **MANDATORY memory writes**: Every agent MUST write status to shared memory

**Why**: LLMs naturally drift without strong constraints. Hierarchical + small teams = tight control.

### 6. **Auto-Learning via Memory**

Agents learn from past successes:
1. **Before task**: Search memory for similar patterns
2. **During task**: Record decisions in memory
3. **After task**: Store successful approaches in `patterns` namespace
4. **Neural training**: Background workers train on patterns (RuVector SONA)

**Why**: Turn one-shot LLM interactions into cumulative learning system.

### 7. **3-Tier Model Routing**

Optimize cost/latency:
- **Tier 1**: Agent Booster (WASM) - <1ms, $0 - simple transforms (var→const)
- **Tier 2**: Haiku - ~500ms, $0.0002 - simple tasks, low complexity
- **Tier 3**: Sonnet/Opus - 2-5s, $0.003-0.015 - complex reasoning

**Why**: 75% cost reduction, 352x faster for Tier 1 tasks.

---

## Code Snippets Summary

### Swarm Initialization (CLI → File State)
```typescript
// v3/@claude-flow/cli/src/commands/swarm.ts
fs.writeFileSync('.swarm/state.json', JSON.stringify({
  id: swarmId,
  topology: 'hierarchical',
  maxAgents: 8,
  status: 'ready'
}, null, 2));
```

### Memory-Based Messaging
```javascript
// Agent writes status
memory_store({
  key: "swarm$worker-1$status",
  namespace: "coordination",
  value: JSON.stringify({ status: "working", progress: 45 })
});

// Coordinator polls status
const status = memory_retrieve({
  key: "swarm$worker-1$status",
  namespace: "coordination"
});
```

### HNSW Semantic Search
```typescript
// v3/@claude-flow/cli/src/mcp-tools/memory-tools.ts
const result = await searchEntries({
  query: "authentication patterns",
  namespace: "patterns",
  limit: 10,
  threshold: 0.7  // Cosine similarity
});
// Returns: [{key: "pattern-jwt", similarity: 0.87}, ...]
```

### Skill Definition
```yaml
# .agents/skills/agent-hierarchical-coordinator/SKILL.md
---
name: hierarchical-coordinator
capabilities:
  - swarm_coordination
  - task_decomposition
hooks:
  pre: |
    mcp__claude-flow__memory_usage store "swarm$status" '{"status":"init"}'
---
# Instructions
You are the Queen coordinator...
```

### Task Tool (Real Execution)
```javascript
// CLAUDE.md
Task({
  prompt: "Implement authentication module",
  subagent_type: "coder",
  run_in_background: true  // Parallel execution
})
```

---

## Comparison to Other Swarm Frameworks

| Feature | Claude-Flow | LangGraph | CrewAI | AutoGen |
|---------|-------------|-----------|--------|---------|
| **Execution** | Claude Code Task tool | Python code | Python code | Python code |
| **Messaging** | Memory (sql.js) | State graph | Message queue | Multi-agent chat |
| **State** | Files (.swarm/) | Graph state | Database | In-memory |
| **Learning** | HNSW + RuVector | None | None | None |
| **Skills** | Markdown prompts | Python functions | Python classes | Python agents |
| **Topology** | 6 types (hierarchical, mesh, etc.) | Graph-based | Sequential/hierarchical | Flexible |
| **Anti-Drift** | Hierarchical + memory protocol | None | None | None |

**Claude-Flow's Unique Advantage**:
- **Auto-learning**: Accumulates knowledge via vector memory
- **Coordination-only**: Doesn't reinvent execution (uses Claude Code)
- **Observable**: All state in files/memory database
- **LLM-native**: Skills are prompts, not code

---

## Lessons for Hatchery

### 1. **Separation of Concerns**
- **Hatchery = Coordinator**: Track state, manage memory, define skills
- **Claude Code = Executor**: Spawn agents, run code, edit files
- **Don't build execution**: Use existing tools (Task tool)

### 2. **Memory-Based Messaging**
- Use shared memory (SQLite + HNSW) instead of message queues
- Namespaces for isolation (`coordination`, `patterns`, `tasks`)
- Vector search for semantic discovery

### 3. **File-Based State**
- All coordination state in `.hatchery/` directory
- Survives crashes, observable, no daemon
- Use git for attribution/versioning

### 4. **Skills as Prompts**
- Don't hardcode agent behavior
- Use markdown files with YAML frontmatter
- Hooks for pre/post actions (bash commands)

### 5. **Anti-Drift Protocol**
- Hierarchical topology for small teams (6-8 agents)
- MANDATORY memory writes (every agent writes status)
- Specialized roles (no overlap)
- Raft consensus (leader maintains authoritative state)

### 6. **Auto-Learning**
- Search memory before every task
- Store patterns after successful tasks
- Train neural substrate on patterns
- Background workers for optimization

### 7. **3-Tier Routing**
- Simple tasks → Tier 1 (WASM transforms, no LLM)
- Medium tasks → Tier 2 (Haiku)
- Complex tasks → Tier 3 (Sonnet/Opus)
- 75% cost reduction potential

---

## File Paths Reference

### Key Source Files
- **Swarm orchestration**: `v3/@claude-flow/cli/src/commands/swarm.ts`
- **Agent spawning**: `v3/@claude-flow/cli/src/commands/agent.ts`
- **Memory tools**: `v3/@claude-flow/cli/src/mcp-tools/memory-tools.ts`
- **MCP swarm tools**: `v3/@claude-flow/cli/src/mcp-tools/swarm-tools.ts`
- **Memory backend**: `v3/@claude-flow/cli/src/memory/memory-initializer.ts`

### Coordinator Prompts
- **Claude Code instructions**: `CLAUDE.md` (1033 lines)
- **Codex instructions**: `AGENTS.md` (635 lines)
- **Local config**: `CLAUDE.local.md`

### Skills
- **Hierarchical coordinator**: `.agents/skills/agent-hierarchical-coordinator/SKILL.md`
- **Swarm init**: `.agents/skills/agent-coordinator-swarm-init/SKILL.md`
- **Task orchestrator**: `.agents/skills/agent-orchestrator-task/SKILL.md`
- **Total skills**: 132 SKILL.md files

### State Files
- **Swarm state**: `.swarm/state.json`
- **Agent metadata**: `.swarm/agents/*.json`
- **Task queue**: `.swarm/tasks/*.json`
- **Memory database**: `.claude-flow/memory/store.db` (sql.js)

---

**End of Dissection**
