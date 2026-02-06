# Hatchery PRD: Brood Lord Mode

**Version:** 1.0
**Status:** Draft
**Priority:** P2

## Overview

Brood Lord mode is the pinnacle of Hatchery's swarm orchestration capabilities. It introduces a full hierarchical command structure with the Lead (human's main Claude session), an Opus Manager (strategic AI coordinator), multiple L2 Coordinators (tactical Sonnet swarm managers), and Workers (Sonnet executors). This architecture enables massive parallelism, strategic planning at the Opus level, and tactical execution at the Sonnet level, maximizing token efficiency and human oversight.

The mode is named after StarCraft's Brood Lord: a flying siege unit that attacks from high altitude (strategic level), with each attack spawning Broodlings (sub-swarms with L2 coordinators). The Opus Manager acts as the strategic brain, decomposing complex master PRDs into focused sub-PRDs, spawning L2 Coordinator sub-swarms for each, and maintaining cross-swarm knowledge sharing. The Lead retains full visibility and can inject real-time commands, reprioritizations, or escalations through the PTY link.

This is the ultimate mode for enterprise-scale development projects: building entire microservice architectures, migrating large codebases, or implementing complex multi-component systems.

## Architecture

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                          Hatchery Brood Lord                                 │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                      LEAD (Human's Session)                            │ │
│  │  - Human Claude Code session (PTY, interactive)                        │ │
│  │  - Full visibility into all sub-swarms                                 │ │
│  │  - Can send real-time commands to Opus Manager via PTY                 │ │
│  │  - Receives escalations from Opus Manager                              │ │
│  │  - Issues: replan, reprioritize, inject_task, query_status             │ │
│  └────────────────────────────┬───────────────────────────────────────────┘ │
│                               │ PTY Link (PtyWrapper)                       │
│                               ▼                                             │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │              OPUS MANAGER (Strategic Coordinator)                      │ │
│  │  - Spawned by Hatchery as PtyWrapper (interactive PTY)                 │ │
│  │  - Runs Claude Opus 4.6 (strategic intelligence)                       │ │
│  │  - Reads master PRD, decomposes into N sub-PRDs                        │ │
│  │  - Spawns L2 Coordinators, assigns sub-PRDs                            │ │
│  │  - Maintains global SharedMemory (cross-swarm knowledge)               │ │
│  │  - Monitors sub-swarm progress, escalates blockers to Lead             │ │
│  │  - Responds to Lead commands: replan, status, inject tasks             │ │
│  └────────────┬────────────────┬────────────────┬──────────────────────────┘ │
│               │                │                │ PipeProcess connections   │
│               ▼                ▼                ▼                           │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐             │
│  │ L2 Coordinator 1│  │ L2 Coordinator 2│  │ L2 Coordinator N│             │
│  │   (Sonnet)      │  │   (Sonnet)      │  │   (Sonnet)      │             │
│  │                 │  │                 │  │                 │             │
│  │ Sub-PRD: Auth   │  │ Sub-PRD: API    │  │ Sub-PRD: Tests  │             │
│  │ Workers: 4      │  │ Workers: 6      │  │ Workers: 3      │             │
│  │                 │  │                 │  │                 │             │
│  │ DAG: 10 tasks   │  │ DAG: 20 tasks   │  │ DAG: 8 tasks    │             │
│  │ Status: 3/10    │  │ Status: 12/20   │  │ Status: 8/8 ✓   │             │
│  └────────┬────────┘  └────────┬────────┘  └────────┬────────┘             │
│           │ PipeProcess         │ PipeProcess        │                      │
│    ┌──────┴──────┐       ┌──────┴──────┐     ┌──────┴──────┐               │
│    ▼      ▼      ▼       ▼      ▼      ▼     ▼      ▼      ▼               │
│  [W1.1] [W1.2] [W1.3] [W2.1] [W2.2] [W2.3] [W3.1] [W3.2] [W3.3]            │
│   (Sonnet Workers - execute tasks within sub-swarm scope)                   │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                    Global SharedMemory                                 │ │
│  │  ┌──────────────────────────────────────────────────────────────────┐  │ │
│  │  │ Cross-Swarm Knowledge:                                           │  │ │
│  │  │ - L2.1.auth.method = "bearer"                                    │  │ │
│  │  │ - L2.2.api.base_url = "https://api.example.com"                  │  │ │
│  │  │ - L2.3.test.framework = "pytest"                                 │  │ │
│  │  └──────────────────────────────────────────────────────────────────┘  │ │
│  │  ┌──────────────────────────────────────────────────────────────────┐  │ │
│  │  │ Sub-PRD Status:                                                  │  │ │
│  │  │ - L2.1 (Auth): 3/10 tasks, 4 workers, 12m elapsed                │  │ │
│  │  │ - L2.2 (API): 12/20 tasks, 6 workers, 18m elapsed                │  │ │
│  │  │ - L2.3 (Tests): 8/8 tasks COMPLETE, 3 workers                    │  │ │
│  │  └──────────────────────────────────────────────────────────────────┘  │ │
│  │  ┌──────────────────────────────────────────────────────────────────┐  │ │
│  │  │ Message Queue (inter-L2):                                        │  │ │
│  │  │ - L2.2→L2.1: "Need auth pattern from your swarm"                 │  │ │
│  │  │ - L2.1→L2.2: "Check knowledge.L2.1.auth.method"                  │  │ │
│  │  └──────────────────────────────────────────────────────────────────┘  │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
│  Communication Matrix:                                                      │
│  - Lead ↔ Opus Manager:      PTY (interactive, keystroke-level)             │
│  - Opus Manager → L2:         PipeProcess.write() (task assignments)        │
│  - L2 ← Workers:              @hatchery: commands (NDJSON output)           │
│  - L2 ↔ L2:                   Global SharedMemory messages                  │
│  - Workers ↔ Workers (L2.X):  Local SharedMemory messages                   │
│  - Lead → Any:                hatchery message <target> <text>              │
│  - Any → Lead:                @hatchery:escalate (via Opus Manager)         │
└──────────────────────────────────────────────────────────────────────────────┘

Token Efficiency:
  - Opus only does strategic work: PRD decomposition, escalation handling
  - Sonnet does all execution: L2 coordination + worker implementation
  - Lead delegates 100% to Opus Manager (no direct work)
```

## User Stories

### US-1: Lead Session Integration
**As a** Lead (human's Claude session), **I want** to spawn an Opus Manager as a PTY subprocess, **so that** I can delegate strategic planning while maintaining oversight.

#### Acceptance Criteria
- [ ] AC-1.1: Lead spawns Opus Manager via `PtyWrapper::new(CliTool::ClaudeCode, working_dir)`
- [ ] AC-1.2: Opus Manager runs in interactive PTY mode (full terminal access)
- [ ] AC-1.3: Lead sends initial master PRD to Opus Manager via `pty.write(prd_content)`
- [ ] AC-1.4: Lead receives Opus Manager output via `pty.read()` (blocking or async)
- [ ] AC-1.5: Lead can send real-time commands: `replan`, `status`, `inject_task`, `kill_swarm`
- [ ] AC-1.6: Lead command syntax: `@lead:replan reason="Requirements changed"` (sent via PTY write)
- [ ] AC-1.7: Opus Manager acknowledges Lead commands within 5 seconds
- [ ] AC-1.8: Lead displays unified dashboard showing all L2 swarm statuses
- [ ] AC-1.9: Lead logs all Opus Manager communication to `lead_opus_link.log`
- [ ] AC-1.10: Lead can gracefully shutdown Opus Manager via `@lead:shutdown`

### US-2: Opus Manager Spawning
**As a** Hatchery orchestrator, **I want** to spawn an Opus Manager session with strategic prompt, **so that** it decomposes the master PRD.

#### Acceptance Criteria
- [ ] AC-2.1: Opus Manager spawned as PtyWrapper with Opus 4.6 model
- [ ] AC-2.2: Opus Manager prompt loaded from `prompts/brood_lord_opus_manager.md`
- [ ] AC-2.3: Prompt includes master PRD, sub-PRD decomposition instructions, L2 spawn commands
- [ ] AC-2.4: Prompt instructs Opus to output JSON commands: `{"cmd": "spawn_l2", "sub_prd": "...", "workers": N}`
- [ ] AC-2.5: Opus Manager has full context window (200K tokens)
- [ ] AC-2.6: Opus Manager uses interactive mode (can respond to Lead queries)
- [ ] AC-2.7: Opus Manager session persists for entire Brood Lord run (not killed after first response)
- [ ] AC-2.8: Opus Manager working directory is same as Lead's (shared filesystem access)
- [ ] AC-2.9: Opus Manager environment includes `HATCHERY_MODE=brood_lord` and `OPUS_ROLE=manager`
- [ ] AC-2.10: Opus Manager spawn failures logged and escalated to Lead immediately

### US-3: Master PRD Decomposition
**As an** Opus Manager, **I want** to decompose the master PRD into focused sub-PRDs, **so that** each L2 swarm has a clear scope.

#### Acceptance Criteria
- [ ] AC-3.1: Opus Manager analyzes master PRD and identifies logical component boundaries
- [ ] AC-3.2: Opus Manager creates N sub-PRDs (N = 2-10, configurable via `--max-l2-swarms`)
- [ ] AC-3.3: Each sub-PRD is a standalone markdown file: `sub_prd_auth.md`, `sub_prd_api.md`, etc.
- [ ] AC-3.4: Sub-PRDs contain subset of master PRD tasks, grouped by component/feature
- [ ] AC-3.5: Sub-PRDs include context section: dependencies on other sub-PRDs, shared knowledge keys
- [ ] AC-3.6: Opus Manager outputs decomposition plan: `{"cmd": "decomposition", "sub_prds": [...]}`
- [ ] AC-3.7: Decomposition plan includes worker count per sub-PRD (based on complexity)
- [ ] AC-3.8: Decomposition logged to `opus_decomposition.json` for auditing
- [ ] AC-3.9: Lead can review decomposition before L2 spawn (optional `--manual-approve` flag)
- [ ] AC-3.10: Opus Manager validates decomposition coverage (all master PRD tasks included)

### US-4: L2 Coordinator Spawning
**As an** Opus Manager, **I want** to spawn L2 Coordinator sub-swarms, **so that** sub-PRDs are executed in parallel.

#### Acceptance Criteria
- [ ] AC-4.1: Opus Manager spawns L2s via `{"cmd": "spawn_l2", "id": 0, "sub_prd": "sub_prd_auth.md", "workers": 4}`
- [ ] AC-4.2: Orchestrator parses spawn command and creates L2 PipeProcess
- [ ] AC-4.3: L2 spawned with Sonnet model (not Opus, for cost efficiency)
- [ ] AC-4.4: L2 prompt loaded from `prompts/brood_lord_l2_coordinator.md`
- [ ] AC-4.5: L2 prompt includes: sub-PRD, worker count, global SharedMemory access, Opus Manager contact
- [ ] AC-4.6: L2 operates as Swarm Host Coordinator (DAG, task assignment, worker management)
- [ ] AC-4.7: Each L2 has unique ID (L2.0, L2.1, ...) for namespacing
- [ ] AC-4.8: L2 spawn is sequential with 2-second delay (avoid API rate limit spike)
- [ ] AC-4.9: L2 spawn failures logged and retried up to 3 times
- [ ] AC-4.10: L2 count limited to 10 (error if Opus attempts to spawn more)

### US-5: L2 Worker Spawning
**As an** L2 Coordinator, **I want** to spawn worker sub-swarms for my sub-PRD, **so that** tasks are executed.

#### Acceptance Criteria
- [ ] AC-5.1: L2 spawns workers via same mechanism as Swarm Host mode
- [ ] AC-5.2: Worker count specified in L2 spawn command (varies per sub-PRD)
- [ ] AC-5.3: Workers scoped to L2 (Worker IDs: L2.0.W0, L2.0.W1, L2.1.W0, etc.)
- [ ] AC-5.4: Workers use Sonnet model (configurable via `--worker-model`)
- [ ] AC-5.5: Worker prompt loaded from `prompts/brood_lord_worker.md`
- [ ] AC-5.6: Worker prompt includes: sub-PRD context, L2 ID, local + global SharedMemory access
- [ ] AC-5.7: Workers can read global knowledge: `@hatchery:query global.L2.*.api.*`
- [ ] AC-5.8: Workers can write to local knowledge: `@hatchery:knowledge local.auth.pattern=...`
- [ ] AC-5.9: Workers can write to global knowledge: `@hatchery:knowledge global.L2.0.auth.method=...`
- [ ] AC-5.10: Worker spawn is parallel within L2 (all L2.X workers spawn simultaneously)

### US-6: Global SharedMemory
**As an** Opus Manager, **I want** a global SharedMemory accessible by all L2s and workers, **so that** cross-swarm knowledge is shared.

#### Acceptance Criteria
- [ ] AC-6.1: Global SharedMemory structure includes: `knowledge`, `l2_status`, `messages`, `results`
- [ ] AC-6.2: Knowledge namespaced by L2: `global.L2.0.auth.method`, `global.L2.1.api.base_url`
- [ ] AC-6.3: L2 status includes: tasks completed/total, workers active/idle, elapsed time, last update
- [ ] AC-6.4: Messages support L2-to-L2 routing: `L2.0 → L2.1: "API base URL is ..."`
- [ ] AC-6.5: Results aggregated from all L2s: `{"L2.0": {"completed": 5, "failed": 1}, ...}`
- [ ] AC-6.6: Global SharedMemory persisted to `global_shared_memory.json`
- [ ] AC-6.7: Global SharedMemory updates broadcast to Opus Manager every 10 seconds
- [ ] AC-6.8: Opus Manager can query global state: `{"cmd": "query_global", "l2_id": 0}`
- [ ] AC-6.9: Global knowledge has read-all, write-own policy (L2.0 can read L2.1's knowledge but can't overwrite)
- [ ] AC-6.10: Global SharedMemory size limited to 10MB (prune oldest entries if exceeded)

### US-7: Local SharedMemory per L2
**As an** L2 Coordinator, **I want** a local SharedMemory for my sub-swarm, **so that** workers within my swarm can collaborate.

#### Acceptance Criteria
- [ ] AC-7.1: Each L2 has its own local SharedMemory: `local_l2_0_shared_memory.json`
- [ ] AC-7.2: Local SharedMemory structure same as Swarm Host mode: `knowledge`, `tasks`, `messages`, `results`
- [ ] AC-7.3: Workers write to local knowledge: `@hatchery:knowledge local.test.setup=...`
- [ ] AC-7.4: Workers read local knowledge: automatically injected in task prompts
- [ ] AC-7.5: Local messages route only within L2: `L2.0.W1 → L2.0.W2: "..."`
- [ ] AC-7.6: Local SharedMemory updates do not trigger global broadcasts (isolated)
- [ ] AC-7.7: L2 Coordinator syncs relevant local knowledge to global: `local.auth.method` → `global.L2.0.auth.method`
- [ ] AC-7.8: Sync strategy: L2 decides which knowledge is globally relevant
- [ ] AC-7.9: Local SharedMemory persisted after each update
- [ ] AC-7.10: Local SharedMemory loaded on L2 resume

### US-8: PTY Link Lead ↔ Opus Manager
**As a** Lead, **I want** bidirectional communication with Opus Manager via PTY, **so that** I can steer the swarm in real-time.

#### Acceptance Criteria
- [ ] AC-8.1: Lead sends commands by writing to PTY: `pty.write("@lead:status\n")`
- [ ] AC-8.2: Opus Manager detects `@lead:` prefix and routes to command handler
- [ ] AC-8.3: Opus Manager responds via STDOUT (Lead reads from PTY)
- [ ] AC-8.4: Supported Lead commands: `status`, `replan`, `inject_task`, `reprioritize`, `kill_swarm`, `escalate`
- [ ] AC-8.5: `@lead:status` returns summary of all L2 swarms (JSON or markdown table)
- [ ] AC-8.6: `@lead:replan reason="..."` triggers Opus to re-decompose master PRD
- [ ] AC-8.7: `@lead:inject_task l2=0 task="..."` adds task to L2.0's DAG
- [ ] AC-8.8: `@lead:reprioritize l2=1 task_id=5 priority=high` changes task priority in L2.1
- [ ] AC-8.9: `@lead:kill_swarm l2=2` gracefully shuts down L2.2 sub-swarm
- [ ] AC-8.10: `@lead:escalate issue="..."` surfaces issue to Lead for human decision

### US-9: Escalation Protocol
**As an** Opus Manager or L2 Coordinator, **I want** to escalate blockers to the Lead, **so that** human intervention can resolve them.

#### Acceptance Criteria
- [ ] AC-9.1: Workers escalate to L2: `@hatchery:escalate issue="Unclear requirement in task 5"`
- [ ] AC-9.2: L2 escalates to Opus Manager: `{"cmd": "escalate", "l2_id": 0, "issue": "..."}`
- [ ] AC-9.3: Opus Manager escalates to Lead: writes `@lead:escalate l2=0 issue="..."` to PTY
- [ ] AC-9.4: Lead sees escalation in real-time (highlighted in dashboard)
- [ ] AC-9.5: Lead can respond: `@lead:resolve l2=0 task=5 guidance="..."`
- [ ] AC-9.6: Guidance propagated: Lead → Opus → L2 → Worker
- [ ] AC-9.7: Escalation logged to `escalations.log` with timestamp and resolution
- [ ] AC-9.8: Escalation types: `blocker` (blocks progress), `question` (clarification), `error` (technical failure)
- [ ] AC-9.9: Escalation timeout: if Lead doesn't respond in 5 minutes, Opus makes best-guess decision
- [ ] AC-9.10: Escalation count tracked per L2 (metric for swarm health)

### US-10: Sub-PRD Decomposition Algorithm
**As an** Opus Manager, **I want** an intelligent algorithm to decompose PRDs, **so that** sub-PRDs are logically coherent.

#### Acceptance Criteria
- [ ] AC-10.1: Decomposition strategy: group tasks by component, feature, or architectural layer
- [ ] AC-10.2: Heuristics: tasks mentioning same file/module grouped together
- [ ] AC-10.3: Heuristics: tasks with shared dependencies grouped together
- [ ] AC-10.4: Heuristics: independent tasks distributed across sub-PRDs for parallelism
- [ ] AC-10.5: Opus Manager outputs decomposition rationale: "Grouped auth tasks in L2.0 because they share auth.rs"
- [ ] AC-10.6: Sub-PRD size balanced (avoid one huge sub-PRD and tiny others)
- [ ] AC-10.7: Sub-PRD count optimized: aim for N = sqrt(total_tasks) L2 swarms
- [ ] AC-10.8: Cross-swarm dependencies minimized (tasks in different L2s should be independent)
- [ ] AC-10.9: If cross-swarm dependency unavoidable, document in sub-PRD: "Depends on L2.1 completing task X"
- [ ] AC-10.10: Decomposition validates all master PRD tasks covered (no orphaned tasks)

### US-11: Worker Assignment within L2
**As an** L2 Coordinator, **I want** to assign tasks to workers within my sub-swarm, **so that** work is distributed efficiently.

#### Acceptance Criteria
- [ ] AC-11.1: L2 uses same assignment algorithm as Swarm Host mode (DAG-based, ready tasks)
- [ ] AC-11.2: Worker capacity: each worker handles 1 task at a time (no parallel tasks per worker)
- [ ] AC-11.3: Assignment balances load: assign to least-busy worker
- [ ] AC-11.4: Assignment prioritizes critical path tasks (longest dependency chain)
- [ ] AC-11.5: Assignment logged: `[L2.0] Assigned task 3 to Worker L2.0.W1`
- [ ] AC-11.6: Assignment failures (no idle workers) handled via retry with backoff
- [ ] AC-11.7: Workers report completion via `@hatchery:result task_id=3 status=success`
- [ ] AC-11.8: L2 updates local SharedMemory and syncs to global SharedMemory
- [ ] AC-11.9: L2 notifies Opus Manager of major milestones (e.g., 50% complete)
- [ ] AC-11.10: L2 can request more workers from Opus: `{"cmd": "request_workers", "count": 2}`

### US-12: Cross-Swarm Communication
**As an** L2 Coordinator, **I want** to send messages to other L2s, **so that** we can coordinate on shared dependencies.

#### Acceptance Criteria
- [ ] AC-12.1: L2 sends message via `{"cmd": "send_message", "to": "L2.1", "text": "..."}` (routed through Opus Manager)
- [ ] AC-12.2: Opus Manager relays message to target L2 via PipeProcess.write()
- [ ] AC-12.3: Target L2 receives message and includes in next worker prompt (if relevant)
- [ ] AC-12.4: Messages logged to global SharedMemory for audit trail
- [ ] AC-12.5: Message delivery guaranteed within 10 seconds (or timeout)
- [ ] AC-12.6: Broadcast messages supported: `{"cmd": "send_message", "to": "all", "text": "..."}`
- [ ] AC-12.7: Message replies tracked: `{"cmd": "reply_message", "msg_id": 123, "text": "..."}`
- [ ] AC-12.8: Message queue per L2 (FIFO, max 50 messages)
- [ ] AC-12.9: Expired messages pruned after 10 minutes
- [ ] AC-12.10: Message routing logged: `[Opus] Routed message L2.0 → L2.1`

### US-13: Lead Command Interface
**As a** Lead, **I want** a command interface to control the Brood Lord swarm, **so that** I can intervene when needed.

#### Acceptance Criteria
- [ ] AC-13.1: `@lead:status` returns JSON summary of all L2s and workers
- [ ] AC-13.2: `@lead:status l2=0` returns detailed status of L2.0
- [ ] AC-13.3: `@lead:replan` triggers full master PRD re-decomposition
- [ ] AC-13.4: `@lead:inject_task l2=X task="..."` adds task to L2.X DAG
- [ ] AC-13.5: `@lead:reprioritize l2=X task_id=Y priority=high|low` changes task priority
- [ ] AC-13.6: `@lead:kill_swarm l2=X` gracefully shuts down L2.X
- [ ] AC-13.7: `@lead:pause` pauses all L2 swarms (workers finish current task, then idle)
- [ ] AC-13.8: `@lead:resume` resumes paused swarms
- [ ] AC-13.9: `@lead:message target=L2.X text="..."` sends message to L2 or worker
- [ ] AC-13.10: All Lead commands logged to `lead_commands.log` with timestamps

### US-14: Opus Manager Monitoring
**As an** Opus Manager, **I want** to monitor all L2 swarms, **so that** I can detect issues and rebalance work.

#### Acceptance Criteria
- [ ] AC-14.1: Opus Manager polls L2 status every 10 seconds
- [ ] AC-14.2: Metrics tracked: tasks completed/total, workers active/idle/stalled, elapsed time
- [ ] AC-14.3: Opus Manager detects slow L2s (progress rate < 1 task/10 minutes)
- [ ] AC-14.4: On slow L2 detection, Opus can spawn additional workers: `{"cmd": "add_workers", "l2_id": 0, "count": 2}`
- [ ] AC-14.5: Opus Manager detects stalled L2s (no progress for 10 minutes)
- [ ] AC-14.6: On stall, Opus escalates to Lead or attempts to reassign tasks
- [ ] AC-14.7: Opus Manager aggregates token usage across all L2s
- [ ] AC-14.8: Opus Manager estimates total cost and time remaining
- [ ] AC-14.9: Opus Manager logs monitoring data to `opus_monitoring.json`
- [ ] AC-14.10: Opus Manager can kill underperforming L2s and redistribute tasks

### US-15: Status Aggregation Across Hierarchy
**As a** Lead, **I want** to see aggregated status across the entire hierarchy, **so that** I understand overall progress.

#### Acceptance Criteria
- [ ] AC-15.1: Status hierarchy: Lead → Opus → L2s → Workers
- [ ] AC-15.2: Status includes: total tasks (all L2s), completed, failed, in-progress, blocked
- [ ] AC-15.3: Status includes per-L2 breakdown: `L2.0: 5/10, L2.1: 12/20, L2.2: 8/8 COMPLETE`
- [ ] AC-15.4: Status includes per-worker breakdown (within each L2)
- [ ] AC-15.5: Status includes token usage and cost per L2 and total
- [ ] AC-15.6: Status includes estimated completion time (based on current progress rate)
- [ ] AC-15.7: Status includes escalation count (unresolved blockers)
- [ ] AC-15.8: Status displayed in terminal dashboard (refreshes every 1 second)
- [ ] AC-15.9: Status exported to JSON: `hatchery status > brood_lord_status.json`
- [ ] AC-15.10: Status includes health indicators: green (on track), yellow (slow), red (stalled)

### US-16: Graceful Shutdown
**As a** user, **I want** graceful shutdown on Ctrl+C, **so that** all swarms save state.

#### Acceptance Criteria
- [ ] AC-16.1: Lead detects SIGINT (Ctrl+C) and sends `@lead:shutdown` to Opus Manager
- [ ] AC-16.2: Opus Manager sends shutdown command to all L2s: `{"cmd": "shutdown"}`
- [ ] AC-16.3: Each L2 sends shutdown to its workers
- [ ] AC-16.4: Workers finish current tool call (max 5 seconds), then exit
- [ ] AC-16.5: L2s wait for all workers to exit (max 10 seconds), save local SharedMemory, then exit
- [ ] AC-16.6: Opus Manager waits for all L2s to exit (max 20 seconds), saves global SharedMemory, then exits
- [ ] AC-16.7: After timeout at any level, forcefully kill remaining processes
- [ ] AC-16.8: Lead generates final summary report with partial results
- [ ] AC-16.9: Second Ctrl+C triggers immediate force kill (SIGKILL all)
- [ ] AC-16.10: Exit code 130 (standard for SIGINT)

### US-17: Resume from Global State
**As a** user, **I want** to resume a Brood Lord session from saved state, **so that** I can recover from crashes.

#### Acceptance Criteria
- [ ] AC-17.1: Support `hatchery brood_lord --resume global_shared_memory.json`
- [ ] AC-17.2: Load global SharedMemory and L2 local SharedMemories
- [ ] AC-17.3: Opus Manager resumes with same sub-PRD decomposition
- [ ] AC-17.4: Spawn same number of L2s with same worker counts
- [ ] AC-17.5: Each L2 resumes from its local SharedMemory (skip completed tasks)
- [ ] AC-17.6: Resume logged: "Resuming from {timestamp}, L2.0: 5/10, L2.1: 12/20, ..."
- [ ] AC-17.7: Resume resets in-progress tasks to ready (workers re-execute)
- [ ] AC-17.8: Resume preserves global and local knowledge
- [ ] AC-17.9: Resume appends to existing logs (no overwrite)
- [ ] AC-17.10: Resume mode visible in dashboard: "RESUMED" badge

### US-18: Opus Prompt Engineering
**As a** Hatchery developer, **I want** a sophisticated Opus Manager prompt, **so that** strategic planning is effective.

#### Acceptance Criteria
- [ ] AC-18.1: Prompt defines role: "You are the Opus Manager, strategic coordinator for a massive AI swarm"
- [ ] AC-18.2: Prompt includes master PRD and decomposition guidelines
- [ ] AC-18.3: Prompt includes JSON command reference: `spawn_l2`, `query_global`, `escalate`, `replan`
- [ ] AC-18.4: Prompt instructs Opus to minimize cross-swarm dependencies
- [ ] AC-18.5: Prompt instructs Opus to balance sub-PRD sizes
- [ ] AC-18.6: Prompt instructs Opus to monitor L2 progress and intervene if stalled
- [ ] AC-18.7: Prompt includes examples of good decomposition strategies
- [ ] AC-18.8: Prompt warns against spawning too many L2s (overhead > benefit)
- [ ] AC-18.9: Prompt instructs Opus to escalate blockers to Lead
- [ ] AC-18.10: Prompt template supports variables: `{MASTER_PRD}`, `{MAX_L2_SWARMS}`, `{LEAD_NAME}`

### US-19: L2 Coordinator Prompt Engineering
**As a** Hatchery developer, **I want** a well-crafted L2 Coordinator prompt, **so that** tactical execution is efficient.

#### Acceptance Criteria
- [ ] AC-19.1: Prompt defines role: "You are L2.{id}, tactical coordinator for sub-PRD: {name}"
- [ ] AC-19.2: Prompt includes sub-PRD and worker count
- [ ] AC-19.3: Prompt includes access to local + global SharedMemory
- [ ] AC-19.4: Prompt instructs L2 to build DAG and assign tasks (Swarm Host behavior)
- [ ] AC-19.5: Prompt instructs L2 to sync important local knowledge to global
- [ ] AC-19.6: Prompt instructs L2 to send messages to other L2s if dependencies exist
- [ ] AC-19.7: Prompt instructs L2 to report progress to Opus Manager every 60 seconds
- [ ] AC-19.8: Prompt instructs L2 to escalate blockers via `{"cmd": "escalate", "issue": "..."}`
- [ ] AC-19.9: Prompt includes examples of good task assignment strategies
- [ ] AC-19.10: Prompt template supports variables: `{L2_ID}`, `{SUB_PRD}`, `{WORKERS}`, `{GLOBAL_KNOWLEDGE}`

### US-20: Worker Prompt Engineering
**As a** Hatchery developer, **I want** a well-crafted Worker prompt, **so that** task execution is high-quality.

#### Acceptance Criteria
- [ ] AC-20.1: Prompt defines role: "You are Worker L2.{l2_id}.W{worker_id}, executing tasks for sub-PRD: {name}"
- [ ] AC-20.2: Prompt includes task text, verification command, relevant knowledge
- [ ] AC-20.3: Prompt instructs worker to check both local and global knowledge before starting
- [ ] AC-20.4: Prompt instructs worker to write discoveries to local knowledge
- [ ] AC-20.5: Prompt instructs worker to write globally-relevant discoveries to global knowledge
- [ ] AC-20.6: Prompt instructs worker to report result via `@hatchery:result task_id=X status=Y`
- [ ] AC-20.7: Prompt instructs worker to escalate if blocked via `@hatchery:escalate issue="..."`
- [ ] AC-20.8: Prompt includes examples of good task implementations
- [ ] AC-20.9: Prompt warns against marking tasks complete without verification
- [ ] AC-20.10: Prompt template supports variables: `{WORKER_ID}`, `{TASK}`, `{LOCAL_KNOWLEDGE}`, `{GLOBAL_KNOWLEDGE}`

### US-21: CLI Interface
**As a** user, **I want** a clean CLI interface to launch Brood Lord mode, **so that** I can start hierarchical swarms.

#### Acceptance Criteria
- [ ] AC-21.1: Command syntax: `hatchery brood_lord <master_prd.md> [OPTIONS]`
- [ ] AC-21.2: Support `--max-l2-swarms N` flag (default: auto-detect, max: 10)
- [ ] AC-21.3: Support `--workers-per-l2 N` flag (default: 4, range: 1-16)
- [ ] AC-21.4: Support `--opus-model MODEL` flag (default: "claude-opus-4.6")
- [ ] AC-21.5: Support `--l2-model MODEL` flag (default: "claude-sonnet-4.5")
- [ ] AC-21.6: Support `--worker-model MODEL` flag (default: same as L2)
- [ ] AC-21.7: Support `--manual-approve` flag (Lead reviews decomposition before L2 spawn)
- [ ] AC-21.8: Support `--no-global-persist` flag (disable global SharedMemory persistence)
- [ ] AC-21.9: Support `--resume <state_file>` flag
- [ ] AC-21.10: Validate master PRD exists and is readable before spawning Opus Manager

### US-22: Error Recovery
**As an** orchestrator, **I want** robust error recovery at all hierarchy levels, **so that** swarms continue after failures.

#### Acceptance Criteria
- [ ] AC-22.1: Worker crashes: L2 detects and reassigns task to another worker
- [ ] AC-22.2: L2 crashes: Opus Manager detects and respawns L2 with resume from local state
- [ ] AC-22.3: Opus Manager crashes: Lead detects and escalates to human
- [ ] AC-22.4: API rate limits: pause affected entity (worker/L2), resume after reset
- [ ] AC-22.5: SharedMemory corruption: load from backup, log warning
- [ ] AC-22.6: Network errors: retry with exponential backoff (max 3 retries)
- [ ] AC-22.7: Parsing errors: log and ignore (don't crash entity)
- [ ] AC-22.8: All errors logged to entity-specific log file
- [ ] AC-22.9: Critical errors escalated through hierarchy: Worker → L2 → Opus → Lead
- [ ] AC-22.10: Lead can manually trigger error recovery: `@lead:recover l2=0`

### US-23: Token Budget Management
**As an** Opus Manager, **I want** to track and enforce token budgets, **so that** costs stay within limits.

#### Acceptance Criteria
- [ ] AC-23.1: Global token budget configurable via `--max-tokens N` flag
- [ ] AC-23.2: Opus Manager tracks token usage per L2 (aggregated from workers)
- [ ] AC-23.3: Opus Manager warns when 80% of budget consumed
- [ ] AC-23.4: Opus Manager pauses swarms when 100% of budget consumed
- [ ] AC-23.5: Budget allocation strategy: divide equally among L2s initially
- [ ] AC-23.6: Budget rebalancing: reallocate from completed L2s to active ones
- [ ] AC-23.7: Opus Manager escalates budget exhaustion to Lead
- [ ] AC-23.8: Lead can increase budget: `@lead:increase_budget tokens=50000`
- [ ] AC-23.9: Token usage logged to `token_usage.json` (per entity, timestamped)
- [ ] AC-23.10: Budget warnings visible in dashboard

### US-24: Cost Estimation
**As a** Lead, **I want** real-time cost estimation, **so that** I can monitor spending.

#### Acceptance Criteria
- [ ] AC-24.1: Opus Manager calculates cost based on token usage and model pricing
- [ ] AC-24.2: Pricing table: Opus input/output, Sonnet input/output (configurable)
- [ ] AC-24.3: Cost aggregated per L2 and total
- [ ] AC-24.4: Cost estimate includes projected cost to completion (based on progress rate)
- [ ] AC-24.5: Cost displayed in dashboard: "Total: $12.34 | Projected: $25.00"
- [ ] AC-24.6: Cost logged to `cost_tracking.json` with timestamps
- [ ] AC-24.7: Lead can set cost limit: `@lead:set_cost_limit usd=50.00`
- [ ] AC-24.8: Opus Manager pauses swarms if cost limit reached
- [ ] AC-24.9: Cost warnings at 50%, 80%, 100% of limit
- [ ] AC-24.10: Cost summary included in final report

### US-25: Dashboard Visualization
**As a** Lead, **I want** a rich terminal dashboard, **so that** I can visualize the entire swarm hierarchy.

#### Acceptance Criteria
- [ ] AC-25.1: Dashboard shows hierarchy: Lead → Opus → L2s → Workers
- [ ] AC-25.2: Dashboard updates every 1 second (non-blocking)
- [ ] AC-25.3: L2 panels show: name, tasks (completed/total), workers (active/idle), status
- [ ] AC-25.4: Worker panels show: ID, current task, elapsed time, status
- [ ] AC-25.5: Global stats: total tasks, completion %, cost, ETA
- [ ] AC-25.6: Recent messages (last 5) in scrolling log
- [ ] AC-25.7: Escalations highlighted in red
- [ ] AC-25.8: Color coding: green (healthy), yellow (slow), red (stalled/error)
- [ ] AC-25.9: Dashboard fits within 120-column terminal width
- [ ] AC-25.10: Dashboard saved to `dashboard_snapshot.txt` on Ctrl+C

## Technical Design

### Core Data Structures

```rust
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use serde::{Deserialize, Serialize};
use zengeld_hub_core::{PtyWrapper, PipeProcess, CliTool, CliEvent};

/// Brood Lord orchestrator state.
pub struct BroodLordOrchestrator {
    pub opus_manager: PtyWrapper,
    pub l2_coordinators: HashMap<L2Id, L2Coordinator>,
    pub global_shared_memory: Arc<RwLock<GlobalSharedMemory>>,
    pub config: BroodLordConfig,
}

type L2Id = usize;
type WorkerId = String; // "L2.0.W1"

/// L2 Coordinator (sub-swarm).
pub struct L2Coordinator {
    pub id: L2Id,
    pub process: PipeProcess,
    pub workers: HashMap<WorkerId, PipeProcess>,
    pub local_shared_memory: Arc<RwLock<LocalSharedMemory>>,
    pub sub_prd: SubPrd,
}

/// Global SharedMemory (cross-swarm).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSharedMemory {
    pub version: u32,
    pub knowledge: HashMap<String, KnowledgeEntry>, // Namespaced: "global.L2.0.auth.method"
    pub l2_status: HashMap<L2Id, L2Status>,
    pub messages: Vec<CrossSwarmMessage>,
    pub results: HashMap<L2Id, HashMap<TaskId, TaskResult>>,
    pub metadata: GlobalMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2Status {
    pub l2_id: L2Id,
    pub name: String,
    pub tasks_completed: usize,
    pub tasks_total: usize,
    pub workers_active: usize,
    pub workers_idle: usize,
    pub elapsed_seconds: u64,
    pub last_update: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossSwarmMessage {
    pub from: L2Id,
    pub to: MessageTarget,
    pub text: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageTarget {
    L2(L2Id),
    AllL2s,
    OpusManager,
}

/// Local SharedMemory (per L2 sub-swarm).
pub type LocalSharedMemory = crate::swarm_host::SharedMemoryState; // Reuse Swarm Host structure

/// Sub-PRD (decomposed from master PRD).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubPrd {
    pub id: L2Id,
    pub name: String,
    pub content: String, // Markdown content
    pub tasks: Vec<Task>,
    pub dependencies: Vec<L2Id>, // Other L2s this sub-PRD depends on
    pub worker_count: usize,
}

/// Master PRD decomposition result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decomposition {
    pub sub_prds: Vec<SubPrd>,
    pub rationale: String, // Opus explanation of decomposition strategy
    pub cross_swarm_deps: Vec<(L2Id, L2Id)>, // Directed edges: (L2.0, L2.1) means L2.1 depends on L2.0
}

/// Brood Lord configuration.
#[derive(Debug, Clone)]
pub struct BroodLordConfig {
    pub master_prd_path: PathBuf,
    pub max_l2_swarms: usize,
    pub workers_per_l2: usize,
    pub opus_model: String,
    pub l2_model: String,
    pub worker_model: String,
    pub manual_approve: bool,
    pub working_dir: PathBuf,
    pub max_tokens: Option<u64>,
    pub max_cost_usd: Option<f64>,
}

impl Default for BroodLordConfig {
    fn default() -> Self {
        Self {
            master_prd_path: PathBuf::from("master_prd.md"),
            max_l2_swarms: 10,
            workers_per_l2: 4,
            opus_model: "claude-opus-4.6".to_string(),
            l2_model: "claude-sonnet-4.5".to_string(),
            worker_model: "claude-sonnet-4.5".to_string(),
            manual_approve: false,
            working_dir: std::env::current_dir().unwrap(),
            max_tokens: None,
            max_cost_usd: None,
        }
    }
}
```

### Opus Manager Command Protocol

```json
// Decomposition result
{"cmd": "decomposition", "sub_prds": [{"id": 0, "name": "Auth", "content": "...", "tasks": [...], "worker_count": 4}, ...], "rationale": "..."}

// Spawn L2 Coordinator
{"cmd": "spawn_l2", "l2_id": 0, "sub_prd": {...}, "workers": 4}

// Query global state
{"cmd": "query_global", "l2_id": 0}

// Add workers to L2
{"cmd": "add_workers", "l2_id": 0, "count": 2}

// Kill L2
{"cmd": "kill_l2", "l2_id": 2}

// Escalate to Lead
{"cmd": "escalate", "l2_id": 0, "issue": "Worker L2.0.W1 stuck on task 5"}

// Replan (trigger re-decomposition)
{"cmd": "replan", "reason": "Requirements changed"}

// Report progress
{"cmd": "progress", "l2_status": [{"l2_id": 0, "completed": 5, "total": 10}, ...], "total_cost_usd": 12.34}
```

### Lead Command Protocol

```
// Status query
@lead:status
@lead:status l2=0

// Replan
@lead:replan reason="New requirements"

// Inject task
@lead:inject_task l2=0 task="Add unit test for auth module"

// Reprioritize
@lead:reprioritize l2=1 task_id=5 priority=high

// Kill swarm
@lead:kill_swarm l2=2

// Pause/Resume
@lead:pause
@lead:resume

// Message
@lead:message target=L2.0 text="Check global knowledge for API base URL"

// Resolve escalation
@lead:resolve l2=0 task=5 guidance="Use bearer token from docs/auth.md"

// Budget management
@lead:increase_budget tokens=50000
@lead:set_cost_limit usd=100.00

// Shutdown
@lead:shutdown
```

### PTY Link Implementation

```rust
impl BroodLordOrchestrator {
    pub fn spawn_opus_manager(&mut self) -> Result<(), Error> {
        let master_prd = std::fs::read_to_string(&self.config.master_prd_path)?;

        let prompt = format!(
            "You are the Opus Manager for a Brood Lord swarm.\n\nMaster PRD:\n{}\n\nDecompose into sub-PRDs and output JSON commands.",
            master_prd
        );

        self.opus_manager = PtyWrapper::new(
            CliTool::ClaudeCode,
            &self.config.working_dir,
        )?;

        self.opus_manager.write(&prompt)?;

        Ok(())
    }

    pub fn send_lead_command(&mut self, command: &str) -> Result<(), Error> {
        self.opus_manager.write(&format!("{}\n", command))?;
        Ok(())
    }

    pub async fn listen_opus_output(&mut self) -> Result<(), Error> {
        loop {
            if let Some(line) = self.opus_manager.try_read() {
                // Parse JSON commands from Opus
                if let Ok(cmd) = serde_json::from_str::<OpusCommand>(&line) {
                    self.handle_opus_command(cmd).await?;
                }
            }
        }
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
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = "0.3"
anyhow = "1"
petgraph = "0.6"
parking_lot = "0.12"
ratatui = "0.28" # Terminal UI dashboard
crossterm = "0.28" # Terminal control
```

## Verification

### US-1-2: Lead ↔ Opus PTY Link
```bash
# Manual test: Lead sends command, Opus responds
hatchery brood_lord master_prd.md --manual-approve
# In Lead session: @lead:status
# Expected: Opus Manager responds with JSON status
```

### US-3-4: PRD Decomposition and L2 Spawning
```bash
cat > master_prd.md <<EOF
# Master PRD
## Auth Module
- [ ] Implement OAuth2 client
- [ ] Add token refresh logic

## API Module
- [ ] Implement REST client
- [ ] Add error handling

## Tests
- [ ] Write auth tests
- [ ] Write API tests
EOF

hatchery brood_lord master_prd.md --max-l2-swarms 3
# Expected: 3 L2s spawned (Auth, API, Tests), logged to stdout
```

### US-6-7: Global and Local SharedMemory
```bash
# Run Brood Lord, check state files after completion
hatchery brood_lord master_prd.md
ls -la global_shared_memory.json local_l2_*_shared_memory.json
# Expected: 1 global + N local SharedMemory files exist

cat global_shared_memory.json | jq '.knowledge'
# Expected: Contains knowledge from multiple L2s, namespaced by L2 ID
```

### US-8-9: Escalation Protocol
```bash
# Create PRD with ambiguous task
cat > master_prd.md <<EOF
- [ ] Implement the thing (deliberately vague)
EOF

RUST_LOG=debug hatchery brood_lord master_prd.md
# Expected: Worker escalates to L2, L2 escalates to Opus, Opus escalates to Lead
# Check lead_opus_link.log for "@lead:escalate" messages
```

### US-15: Status Aggregation
```bash
# Run Brood Lord, query status mid-execution
hatchery brood_lord master_prd.md &
PID=$!
sleep 30
# In another terminal: send status command to Opus Manager PTY (TBD: implement CLI command)
# Expected: JSON output showing all L2s and workers

kill $PID
```

### US-16-17: Shutdown and Resume
```bash
# Start Brood Lord, Ctrl+C mid-execution
hatchery brood_lord master_prd.md
# ^C after 60s
ls -la global_shared_memory.json
# Expected: State saved

# Resume
hatchery brood_lord --resume global_shared_memory.json
# Expected: Loads state, continues from last checkpoint
```

### US-25: Dashboard
```bash
# Run Brood Lord with dashboard
hatchery brood_lord master_prd.md
# Expected: Live terminal dashboard showing hierarchy, updates every 1 second
```

## Open Questions

1. **Opus Cost**: Running Opus Manager continuously may be expensive. Should it sleep between polling cycles?
   - **Proposal**: Opus polls every 30s, sleeps between polls (not continuous inference).

2. **L2 Auto-Scaling**: Should Opus automatically add/remove workers from L2s based on load?
   - **Proposal**: v1 static worker counts, v2 add auto-scaling.

3. **Cross-Swarm DAG**: How to handle task dependencies across L2s (e.g., L2.1 task depends on L2.0 task)?
   - **Proposal**: v1 document as comment in sub-PRD, manual coordination via messages. v2 add global DAG.

4. **Opus Model Selection**: Should Opus Manager always use Opus 4.6, or allow Sonnet for simple PRDs?
   - **Proposal**: Auto-detect: if master PRD < 50 tasks, use Sonnet. If >= 50, use Opus.

5. **Lead Interface**: Should Lead be a human Claude session, or a separate TUI application?
   - **Proposal**: v1 human Claude session (requires PTY integration). v2 add standalone TUI.

6. **Knowledge Namespacing**: Should global knowledge be flat or hierarchical?
   - **Proposal**: Hierarchical with dots: `global.L2.0.module.auth.method`.

7. **Message Delivery Guarantees**: Should messages be guaranteed-delivery (ACK required)?
   - **Proposal**: v1 best-effort (no ACK), v2 add ACK mechanism.

8. **Sub-PRD Format**: Should sub-PRDs be full markdown files or JSON objects?
   - **Proposal**: Full markdown files (human-readable, can be edited manually).

9. **Opus Manager Redundancy**: Should there be a backup Opus Manager in case of crash?
   - **Proposal**: v1 no redundancy (crash escalates to Lead), v2 add hot standby.

10. **Token Allocation**: Should token budgets be enforced strictly (hard stop) or soft limits (warnings)?
    - **Proposal**: Soft limits with warnings, Lead can override and increase budget.
