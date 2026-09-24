# Coordinator Agent Patterns and Autonomous Iteration Loops

Research on swarm orchestration patterns for the Hatchery system, focusing on coordinator agent design, autonomous iteration loops (Ralph pattern), hierarchical delegation, task decomposition, and prompt engineering.

---

## Table of Contents

1. [Coordinator Agent Patterns](#1-coordinator-agent-patterns)
2. [Autonomous Iteration (Ralph Pattern)](#2-autonomous-iteration-ralph-pattern)
3. [Hierarchical Delegation](#3-hierarchical-delegation)
4. [Task Decomposition](#4-task-decomposition)
5. [Prompt Engineering for Coordinators](#5-prompt-engineering-for-coordinators)
6. [Real-World Examples](#6-real-world-examples)
7. [Implementation Recommendations](#7-implementation-recommendations)

---

## 1. Coordinator Agent Patterns

### 1.1 Core Architecture

A **coordinator agent** is a central orchestrator that receives high-level tasks, decomposes them into subtasks, delegates to specialist worker agents, and aggregates results. The coordinator maintains global state and makes routing decisions.

**Key Components:**
- **Action spaces**: Observability-increasing actions (task inspection, chat retrieval), graph-modifying operations (add/remove tasks, establish dependencies), and delegation/communication functions
- **State representation**: Task-dependency graph, worker capabilities, communication history, artifacts, and stakeholder preferences
- **Heterogeneous teams**: AI workers with specialized capabilities plus human collaborators in dynamic compositions

### 1.2 Framework Comparison

| Framework | Coordinator Pattern | State Management | Strengths |
|-----------|---------------------|------------------|-----------|
| **LangGraph** | Supervisor node with conditional routing | Explicit TypedDict state | Maximum control, clear data flow |
| **CrewAI** | Manager agent overseeing hierarchical processes | Implicit via task context | Rapid deployment, built-in memory |
| **AutoGen** | GroupChatManager with LLM-based speaker selection | Conversation message history | Dynamic agent selection, flexible orchestration |

### 1.3 LangGraph Supervisor Pattern

LangGraph uses a **state machine** where a central orchestrator node evaluates conditions and directs workflow through conditional routing logic. The supervisor node determines the next step based on evaluated state.

**Key Features:**
- Explicit state via TypedDict structures makes data flow transparent
- Conditional routing enables dynamic workflow adaptation
- Suitable for complex orchestration requiring precise control

### 1.4 CrewAI Manager Agent

CrewAI implements coordination through:
- **Task dependencies**: Sequential or hierarchical execution
- **Agent delegation**: Agents can request assistance from other agents
- **Hierarchical processes**: One manager agent oversees subordinate agents
- **Custom managers**: Users can designate specific agents as managers with tailored behavior

**Coordination Model:** Distributed coordination where agents can delegate rather than relying solely on a single coordinator node.

### 1.5 AutoGen GroupChatManager

AutoGen provides the **GroupChatManager** as an orchestrator for multi-agent conversations.

**Speaker Selection:**
- Uses LLM-based selector algorithm
- Maintains conversation history
- Queries model with prompt: "Read the following conversation. Then select the next role from [participants] to play."
- Tracks previous speaker to avoid consecutive turns by same agent

**State Management:**
- Maintains `_chat_history` list of `UserMessage` objects
- Formats message history with source attribution
- Represents images as "[Image]" in formatted history

**Termination Handling:**
- Monitors message content for completion signals
- Stops when receiving approval message from user
- Ends chat without issuing further `RequestToSpeak` messages

**Orchestration Protocol:**
- Publish-subscribe pattern for agent communication
- GroupChatManager publishes `RequestToSpeak` to selected agent's topic
- Agents respond with `GroupChatMessage` to common group chat topic

### 1.6 What Information Does a Coordinator Need?

Based on research findings, a coordinator requires:

1. **Task List with Dependencies**
   - Task IDs, descriptions, status (pending/in-progress/completed)
   - Dependency graph (directed edges representing precedence constraints)
   - Priority levels for task ordering

2. **Worker Status**
   - Available agents and their capabilities/specializations
   - Current workload and capacity
   - Agent descriptions (help coordinator make routing decisions)
   - Performance/reliability metrics from past interactions

3. **Shared Memory**
   - **Private memory**: User-specific fragments
   - **Shared memory**: Cross-user knowledge with access controls
   - **Communication history**: Messages between agents
   - **Artifacts**: Documents, code, decisions produced by workers

4. **Context and Constraints**
   - High-level goals from Lead agent
   - Resource limits (budget, time, tokens)
   - Quality gates and acceptance criteria
   - Stakeholder preferences

### 1.7 Decision-Making Strategies

Research identifies three baseline coordination strategies:

| Strategy | Characteristics | Trade-offs |
|----------|-----------------|------------|
| **Chain-of-Thought** | Higher constraint adherence (0.589±0.140) | 17× runtime overhead |
| **Upfront Assignment** | Maximizes goal completion (0.502±0.209) | Sacrifices constraint adherence |
| **Reactive Communication** | Heavy reliance on status queries | Less proactive planning |

**Key Finding:** No single strategy simultaneously optimizes goal achievement, completion time, constraint adherence, and stakeholder engagement. Coordinators must balance these trade-offs based on task requirements.

### 1.8 When to Re-assign, Escalate, or Complete

**Re-assignment Triggers:**
- Worker reports task is larger than expected
- Worker idle while dependencies remain unresolved
- Worker repeatedly fails to make progress
- Better-suited worker becomes available

**Escalation Triggers:**
- Task decomposition reveals gaps in coordinator's understanding
- Workers produce incompatible sub-plans
- Resource constraints (budget, time) at risk
- Quality gates failing despite retries

**Completion Criteria:**
- All tasks in dependency graph completed
- Quality gates passed (tests, linting, type checking)
- Stakeholder acceptance/approval received
- Explicit completion markers from workers

---

## 2. Autonomous Iteration (Ralph Pattern)

### 2.1 Core Definition

The **Ralph Loop** is a self-iterating mechanism that continuously re-executes the same prompt until verifiable completion criteria are met, rather than relying on the LLM's subjective assessment of task completion.

### 2.2 Ralph Loop vs ReAct

| Aspect | ReAct | Ralph Loop |
|--------|-------|-----------|
| **Control** | Agent decides when to stop | External stop hooks force continuation |
| **Exit Condition** | LLM self-assessment | Machine-verifiable completion signals |
| **Context** | Single session history | Cross-session persistence via files/Git |
| **Error Recovery** | Fixes errors within reasoning chain | Allows failure, restarts from file system |
| **Use Case** | Dynamic Q&A, complex limited-step tasks | Mechanical refactoring, test migration, TDD |

### 2.3 Iteration Mechanism

**Core Loop Structure:**
```bash
while :; do
  cat PROMPT.md | claude-code --continue
done
```

**Stop Hook Interception:**
- When agent attempts to exit, system intercepts unless predefined "Completion Promise" marker exists (e.g., `<promise>COMPLETE</promise>`)
- Same prompt reinjected, but agent observes changed file states and Git history from previous iterations
- Shifts state management from LLM token memory to persistent disk storage

### 2.4 State Persistence Components

**Three Essential Files:**

1. **progress.txt**
   - Appended log recording iteration attempts
   - Patterns discovered and pitfalls encountered
   - Learning history across iterations

2. **prd.json**
   - Structured task list with completion status tracking
   - Acceptance criteria for each task
   - Dependency information

3. **Git History**
   - Commits after each successful step
   - Provides diffs for objective situation assessment
   - Enables rollback on failures

**Benefits:**
- Addresses context degradation in long-running tasks
- Provides objective record of progress
- Enables recovery from crashes or errors

### 2.5 Termination Conditions

Tasks exit when meeting one of these criteria:

1. **Explicit Markers:** Agent outputs `<promise>COMPLETE</promise>`
2. **Verifiable Standards:** All tests pass, coverage exceeds threshold, lint errors eliminated
3. **Maximum Iterations:** Safety valve preventing infinite loops (typically 5-50 iterations depending on task complexity)
4. **Clear Completion Criteria:** Machine-checkable conditions like "Build succeeds" or "Type checks pass"

### 2.6 Optimal Iteration Cycle

**Poll Interval:**
- No explicit polling in Ralph Loop—agent runs continuously
- Iteration boundary occurs when agent signals completion
- System re-invokes immediately if completion criteria not met

**Max Iterations:**
- **Simple tasks:** 5-10 iterations
- **Medium complexity:** 10-25 iterations
- **Complex refactoring:** 25-50 iterations
- Always include safety valve to prevent infinite loops

**Timeout:**
- Per-iteration timeout: 2-10 minutes depending on task complexity
- Total task timeout: 30 minutes to 4 hours
- Cost control: Estimate $5-150 per task depending on complexity

### 2.7 Stall Detection and Recovery

**Stall Indicators:**
- Same file modified multiple times without progress
- Git diffs show reverted changes
- Error messages repeating across iterations
- Progress.txt shows circular reasoning

**Recovery Strategies:**
- **Inject hint:** Add specific guidance to prompt based on stall pattern
- **Simplify scope:** Break task into smaller chunks
- **Change approach:** Modify PRD to suggest alternative strategy
- **Human intervention:** Escalate to Lead agent for guidance

**Implementation Pattern:**
```bash
iteration=0
max_stalls=3
stall_count=0

while [ $iteration -lt $max_iterations ]; do
  # Run agent
  cat PROMPT.md | claude-code --continue

  # Check for progress
  current_hash=$(git rev-parse HEAD)
  if [ "$current_hash" = "$last_hash" ]; then
    ((stall_count++))
    if [ $stall_count -ge $max_stalls ]; then
      echo "Stall detected. Injecting recovery hint..."
      cat RECOVERY_HINT.md >> PROMPT.md
      stall_count=0
    fi
  else
    stall_count=0
  fi

  last_hash=$current_hash
  ((iteration++))
done
```

### 2.8 Best Practices

**Start with HITL (Human-in-the-Loop):**
- Observe and optimize prompts before AFK (Away From Keyboard) runs
- Validate first few iterations manually

**Define Scope Clearly:**
- Use structured prd.json with acceptance criteria
- Break large tasks into feature-sized chunks

**Implement Feedback Loops:**
- Run type checking, tests, and linting between iterations
- Provide results to next iteration via progress.txt

**Small Iterations:**
- Complete one feature per iteration with immediate validation
- Commit after each successful step

**Cost Control:**
- Set strict max-iterations
- Monitor token usage
- Estimate costs upfront ($5-150 per task)

**Suitable For:**
- TDD development
- Greenfield projects
- Code refactoring
- Test migration
- Mechanical transformations

**Unsuitable For:**
- Tasks requiring subjective judgment
- Strategies without clear success criteria
- Exploratory research (use different patterns)

### 2.9 Could a Coordinator Run a Similar Loop?

**Yes, with adaptations:**

1. **Check Tasks:** Read shared task list and worker status
2. **Assign:** Delegate available tasks to idle workers
3. **Wait:** Poll for completion signals or timeout
4. **Check Results:** Validate worker outputs against acceptance criteria
5. **Next:** Update task graph, re-assign failures, spawn new workers as needed

**Key Differences from Ralph Loop:**
- Coordinator manages **multiple concurrent workers**, not sequential iterations
- State includes **worker status and inter-agent messages**, not just file system
- Termination when **all tasks complete**, not when single goal met
- Recovery involves **re-assignment or escalation**, not just re-running same agent

**Implementation Sketch:**
```
while tasks_remaining():
  available_tasks = get_unblocked_tasks()
  idle_workers = get_idle_workers()

  for task, worker in match(available_tasks, idle_workers):
    assign(task, worker)

  sleep(poll_interval)  # e.g., 30 seconds

  completed = check_worker_outputs()
  for task_id, result in completed:
    if validate(result):
      mark_complete(task_id)
      unblock_dependents(task_id)
    else:
      if retries_exhausted(task_id):
        escalate_to_lead(task_id)
      else:
        reassign(task_id)

  if stall_detected():
    analyze_bottleneck()
    rebalance_workload()
```

---

## 3. Hierarchical Delegation

### 3.1 Three-Tier Architecture

**Lead (Opus) → Coordinator (Sonnet) → Workers (Sonnet)**

**Optimal Strategy:** Hierarchical fallback—start with the cheapest sufficient model, upgrade on failure.

**Simple Principle:**
> "Opus for thinking, Sonnet for doing."

### 3.2 Model Selection for Efficiency

**Token Usage Comparison:**
- **At medium effort:** Opus 4.5 matches Sonnet 4.5's best performance while burning **76% fewer tokens**
- **At high effort:** Opus beats Sonnet by 4.3 points using **48% fewer tokens**
- **Application development:** Opus uses **19.3% fewer total tokens** than Sonnet for same tasks

**Practical Example:**
- Exploration with Haiku: 8k tokens
- Planning with Sonnet: 12k tokens
- Implementation with Sonnet: 85k tokens
- Testing with Sonnet: 18k tokens
- Review with Opus: 12k tokens
- **Total cost:** $0.59 per feature

### 3.3 Minimizing Opus Token Usage

**What Opus Should Do:**
1. Receive high-level goals from user
2. Evaluate goal complexity and feasibility
3. Spawn Coordinator with structured objective
4. Review Coordinator's final summary and deliverables
5. Make strategic decisions when escalations occur

**What Opus Should NOT Do:**
- Read/write individual files (delegate to workers)
- Implement code (delegate to rust-implementer)
- Research APIs (delegate to research-agent)
- Debug specific errors (let Coordinator manage debugging loop)
- Micromanage worker task assignment (trust Coordinator)

**Token Optimization Strategies:**
- **Offload to sub-agents:** Reduces token usage in main conversation, maintains critical context
- **Context filtering:** Coordinator manages what each worker sees—don't drag entire conversation history into every task
- **Hierarchical structure:** Ensures primary agent acts as central coordinator, managing complexity while individual workers contribute to overarching goal

### 3.4 What Decisions Should Opus Make vs Delegate to Coordinator?

| Decision Type | Owner | Rationale |
|---------------|-------|-----------|
| **Strategic direction** | Opus | High-level goals require context of user's broader needs |
| **Task breakdown approach** | Coordinator | Coordinator has detailed technical understanding |
| **Worker selection** | Coordinator | Coordinator knows worker capabilities and current load |
| **Task priorities** | Opus (initial), Coordinator (ongoing) | Opus sets priorities; Coordinator adapts based on execution |
| **Quality gates** | Opus (define), Coordinator (enforce) | Opus defines standards; Coordinator checks compliance |
| **Escalations** | Opus | Coordinator escalates when stuck; Opus provides strategic guidance |
| **Resource allocation** | Opus (budget), Coordinator (distribution) | Opus sets limits; Coordinator distributes within constraints |
| **Completion approval** | Opus | Final deliverable review ensures alignment with user intent |

### 3.5 How Should Coordinator Report Back?

**Summary Format (Default):**
```
## Status: [In Progress | Completed | Blocked]

## Progress Overview
- Tasks completed: 12/20
- Workers active: 3
- Blocked tasks: 2 (waiting on API key, dependency issue)

## Key Accomplishments
- Implemented Binance connector (tests passing)
- Research completed for Bybit and OKX
- Fixed critical bug in WebSocket handler

## Issues Requiring Escalation
- Task #7: Ambiguous requirement about rate limiting
  - Blocker: Need clarification on per-endpoint vs global limits
  - Recommendation: Use per-endpoint based on API docs

## Next Steps
- Complete OKX implementation (assigned to worker-2)
- Begin integration testing (assigned to worker-3)
- Research alternative approach for blocked task (assigned to worker-1)

## Resource Usage
- Tokens: 145k / 500k budget
- Time: 1.2 hours / 4 hour estimate
- Cost: $3.20 / $10 budget
```

**Full Details (On Request):**
- Individual task status for all 20 tasks
- Worker activity logs
- Code diffs for completed tasks
- Test results and error messages

**Key Principle:** Default to concise summaries. Provide full details only when:
- Opus requests them explicitly
- Escalation requires context
- Final deliverable review

### 3.6 Benefits of Hierarchical Delegation

1. **Cost Efficiency:** Opus time is expensive; Coordinator + Workers handle 90%+ of work with cheaper models
2. **Scalability:** Coordinator can manage N workers in parallel without consuming Opus tokens
3. **Context Preservation:** Opus maintains high-level context without drowning in implementation details
4. **Specialization:** Workers focus on narrow tasks with curated context, producing better results
5. **Recovery:** Coordinator handles retries and debugging loops without involving Opus

---

## 4. Task Decomposition

### 4.1 Core Concept

The **Manager Agent** coordinates activity via a **dynamic task graph** (Directed Acyclic Graph / DAG), where:
- **Nodes** represent tasks
- **Directed edges** represent dependencies

The coordinator decomposes problems and delegates sub-tasks to specialized agents, focusing on high-level strategy while workers focus on execution.

### 4.2 Decomposition Strategies

**Fine-Grained Task Decomposition:**
- User query broken into large number of small, granular tasks
- Each task represents minimal unit of work
- Allows high degree of parallelism
- Many tasks executed simultaneously

**TDAG Framework (Task Decomposition and Agent Generation):**
- Decomposes complex task into smaller subtasks
- Each subtask managed by specifically generated subagent
- Dynamic role discovery and assignment

### 4.3 Dependency Detection Between Tasks

**DAG Representation:**
- Subtasks as nodes
- Dependencies as directed edges
- Allows parallel execution of independent subtasks
- Handles explicit prerequisites

**Dependency Management:**
- Independent tasks executed in parallel
- Tasks with dependencies scheduled sequentially
- Framework efficiently manages parallel execution while respecting dependencies
- Coordinator ensures dependent tasks executed in correct order

**Example:**
```
Task 1: Research Binance API
Task 2: Implement Binance connector (depends on Task 1)
Task 3: Research Bybit API (independent)
Task 4: Write integration tests (depends on Task 2)

Execution:
- Phase 1: Run Tasks 1 and 3 in parallel
- Phase 2: Run Task 2 after Task 1 completes
- Phase 3: Run Task 4 after Task 2 completes
```

### 4.4 Automatic vs Manual Task Splitting

**Automatic Splitting:**
- Coordinator uses LLM to decompose high-level goal
- TDAG framework dynamically generates subtasks
- Chain-of-Thought reasoning for complex decomposition

**Pros:**
- Fast iteration—no manual planning required
- Adapts to new task types automatically
- Leverages LLM's reasoning capabilities

**Cons:**
- **Compositional failures** when graph depth/branching/novelty exceed thresholds
- **Shallow pattern matching** rather than genuine hierarchical reasoning
- **Cascading errors** when worker sub-plans become incompatible

**Manual Splitting:**
- User or Lead agent provides structured task list (e.g., prd.json)
- Coordinator assigns pre-defined tasks to workers
- Tasks already broken into appropriate granularity

**Pros:**
- Predictable structure—no surprises from LLM decomposition
- Clearer dependencies and acceptance criteria
- Easier to estimate resource requirements

**Cons:**
- Requires upfront planning effort
- Less adaptive to unexpected complexities
- Manual overhead increases with task complexity

**Recommended Hybrid Approach:**
- **High-level decomposition:** Manual (Lead agent provides task breakdown)
- **Low-level decomposition:** Automatic (Coordinator refines into worker-sized chunks)

### 4.5 Handling Tasks That Are Bigger Than Expected

**Detection:**
- Worker reports estimated effort exceeds allocation
- Worker stuck for multiple iterations without progress
- Subtask complexity reveals need for additional research or dependencies

**Strategies:**

1. **Re-decompose:**
   - Coordinator breaks task into smaller subtasks
   - Assigns subtasks to multiple workers
   - Updates dependency graph accordingly

2. **Escalate:**
   - Report to Lead agent that scope underestimated
   - Request additional resources (time, budget, workers)
   - Get clarification on priorities

3. **Re-assign:**
   - Move task to more experienced/specialized worker
   - Provide additional context or tools
   - Pair workers for collaborative effort

4. **Simplify:**
   - Reduce scope to meet original estimate
   - Defer nice-to-have features
   - Focus on core functionality

**Example Workflow:**
```
1. Worker-1 claims "Implement Bybit connector"
2. Worker-1 discovers Bybit has 3 separate APIs (Spot, Futures, Options)
3. Worker-1 sends message to Coordinator: "Task larger than expected. Need decomposition."
4. Coordinator:
   - Creates Task 2a: Implement Bybit Spot
   - Creates Task 2b: Implement Bybit Futures
   - Creates Task 2c: Implement Bybit Options
   - Assigns 2a to Worker-1 (already has context)
   - Assigns 2b to Worker-2
   - Assigns 2c to Worker-3
5. Workers proceed in parallel
```

### 4.6 Critical Bottleneck

Research identifies that **task decomposition quality correlates almost linearly with overall system performance**:

> "Mapping a high-level workflow description into a task graph with governance constraints is the bottleneck that unlocks all downstream capabilities."

**Implications for Hatchery:**
- Invest heavily in decomposition prompt engineering
- Consider neuro-symbolic approaches (symbolic planners + learned abstractions)
- Treat task-graph induction as reinforcement learning problem (learn from past decompositions)
- Validate decomposition quality early—don't proceed with flawed task graphs

---

## 5. Prompt Engineering for Coordinators

### 5.1 System Prompt Template

**Core Structure:**

```markdown
# Role
You are a Coordinator Agent managing a team of specialized workers to accomplish complex tasks.

# Responsibilities
1. Decompose high-level goals into worker-sized tasks
2. Assign tasks to appropriate workers based on capabilities
3. Monitor progress and handle blockers
4. Synthesize worker outputs into cohesive results
5. Escalate to Lead when strategic decisions needed

# Context Encoding
You have access to:
- Task list with dependencies (see TASK_GRAPH below)
- Worker status (see WORKER_STATUS below)
- Shared memory (see SHARED_MEMORY below)
- Communication history (see MESSAGES below)

## TASK_GRAPH
{task_graph_json}

## WORKER_STATUS
{worker_status_json}

## SHARED_MEMORY
{shared_memory_json}

## MESSAGES
{recent_messages}

# Decision-Making Guidelines
- Prefer parallel execution when tasks are independent
- Avoid bottlenecks—don't block workers waiting for your responses
- Batch updates every {poll_interval} seconds to minimize overhead
- Escalate only when you lack information to proceed
- Validate worker outputs against acceptance criteria

# Communication Protocol
- Use `write(worker_id, message)` for targeted communication
- Use `broadcast(message)` sparingly (high cost)
- Use structured message types: task_assignment, status_request, shutdown_request
- Workers will send: task_completed, idle_notification, blocker_reported

# Termination Conditions
Complete when:
- All tasks in TASK_GRAPH marked as completed
- All quality gates passed
- No blockers remaining
- Lead agent approves deliverables

# Efficiency Targets
- Minimize messages: Batch updates, avoid unnecessary chatter
- Maximize parallelism: Assign tasks as soon as workers idle
- Optimize token usage: Summarize rather than repeat full context
```

### 5.2 Encoding Task List

**JSON Format (Recommended):**

```json
{
  "tasks": [
    {
      "id": "task-1",
      "title": "Research Binance API",
      "description": "Create 6 research files in research/ directory",
      "status": "completed",
      "assignee": "research-agent-1",
      "dependencies": [],
      "acceptance_criteria": [
        "6 files exist in research/",
        "All endpoints documented",
        "WebSocket streams covered"
      ],
      "estimated_tokens": 15000,
      "actual_tokens": 14200
    },
    {
      "id": "task-2",
      "title": "Implement Binance connector",
      "description": "Create connector implementation based on research",
      "status": "in-progress",
      "assignee": "rust-implementer-1",
      "dependencies": ["task-1"],
      "acceptance_criteria": [
        "Code compiles",
        "Tests pass",
        "Follows KuCoin pattern"
      ],
      "estimated_tokens": 25000,
      "actual_tokens": null
    },
    {
      "id": "task-3",
      "title": "Research Bybit API",
      "description": "Create 6 research files in research/ directory",
      "status": "pending",
      "assignee": null,
      "dependencies": [],
      "acceptance_criteria": [
        "6 files exist in research/",
        "All endpoints documented"
      ],
      "estimated_tokens": 15000,
      "actual_tokens": null
    }
  ]
}
```

**Benefits:**
- Structured, machine-readable format
- Easy to update programmatically
- Clear dependencies and status tracking
- Enables automated progress calculation

### 5.3 Encoding Worker Status

**JSON Format:**

```json
{
  "workers": [
    {
      "id": "research-agent-1",
      "type": "research-agent",
      "status": "idle",
      "current_task": null,
      "capabilities": ["web_search", "web_fetch", "documentation_analysis"],
      "max_tokens": 50000,
      "tokens_used": 14200,
      "tasks_completed": 1,
      "success_rate": 1.0,
      "last_activity": "2026-02-06T10:30:00Z"
    },
    {
      "id": "rust-implementer-1",
      "type": "rust-implementer",
      "status": "active",
      "current_task": "task-2",
      "capabilities": ["rust_coding", "testing", "debugging"],
      "max_tokens": 100000,
      "tokens_used": 42000,
      "tasks_completed": 0,
      "success_rate": null,
      "last_activity": "2026-02-06T10:45:00Z"
    }
  ]
}
```

**Benefits:**
- Real-time visibility into worker availability
- Load balancing based on tokens_used
- Capability-based task routing
- Performance tracking via success_rate

### 5.4 Encoding Shared Memory

**Two-Tier Structure (Collaborative Memory Pattern):**

```json
{
  "private_memory": {
    "user-1": [
      {
        "id": "mem-1",
        "content": "User prefers KuCoin pattern for new connectors",
        "timestamp": "2026-02-06T09:00:00Z",
        "contributing_agents": ["lead-agent"],
        "accessed_resources": []
      }
    ]
  },
  "shared_memory": {
    "global": [
      {
        "id": "mem-2",
        "content": "V5 connectors follow pattern: endpoints.rs, auth.rs, parser.rs, connector.rs, websocket.rs",
        "timestamp": "2026-02-06T08:00:00Z",
        "contributing_agents": ["lead-agent", "coordinator-1"],
        "accessed_resources": ["v5/exchanges/kucoin/"],
        "access_control": "public"
      },
      {
        "id": "mem-3",
        "content": "Binance rate limits: 1200 requests/minute for public endpoints, 50 requests/10 seconds for orders",
        "timestamp": "2026-02-06T10:15:00Z",
        "contributing_agents": ["research-agent-1"],
        "accessed_resources": ["research/binance/"],
        "access_control": "public"
      }
    ]
  }
}
```

**Benefits:**
- Cross-task knowledge sharing
- Prevents redundant research
- Access control for sensitive information
- Provenance tracking (who created, when, from what resources)

### 5.5 Efficient Context Encoding Strategies

**Problem:** Coordinators can be overwhelmed by large context (hundreds of tasks, dozens of workers, extensive message history).

**Solutions:**

1. **Incremental Updates:**
   - Don't resend entire task graph each iteration
   - Send only changes: `{"task-2": {"status": "completed"}}`

2. **Sliding Window:**
   - Keep only recent N messages in context
   - Archive older messages to long-term storage
   - Retrieve on-demand if needed for specific decisions

3. **Summarization:**
   - Compress completed tasks: "12 tasks completed successfully"
   - Detailed status only for active/blocked tasks

4. **Lazy Loading:**
   - Provide task IDs in prompt
   - Coordinator requests full details only when needed
   - Reduces tokens for "read-only" iterations

5. **Structured Flags:**
   - Use compact representations: `"pending": ["task-3", "task-5"]`
   - Coordinator expands details as needed

**Example Efficient Encoding:**

```markdown
# Task Summary
- Completed: 12 (see archive)
- In Progress: 3 (task-2, task-7, task-9)
- Pending: 5 (task-3, task-5, task-10, task-11, task-12)
- Blocked: 2 (task-4: waiting on API key, task-8: dependency issue)

# Active Tasks Detail
Task-2: Implement Binance connector
  - Assignee: rust-implementer-1
  - Progress: 60% (4/7 files completed)
  - Last update: 5 minutes ago

Task-7: Debug WebSocket connection
  - Assignee: rust-implementer-2
  - Progress: 30% (identified root cause)
  - Last update: 2 minutes ago

Task-9: Write integration tests
  - Assignee: rust-implementer-3
  - Progress: 80% (6/8 tests passing)
  - Last update: 1 minute ago

# Worker Availability
- Idle: research-agent-1 (ready for new task)
- Active: rust-implementer-1, rust-implementer-2, rust-implementer-3
```

### 5.6 Instructions for Efficiency

**Key Directives:**

```markdown
# Efficiency Guidelines

## Minimize Messages
- Batch task assignments: Assign multiple tasks in one message if workers available
- Don't send status requests every iteration—only when needed for decision
- Workers report completion; you don't need to poll

## Maximize Parallelism
- Assign tasks as soon as dependencies resolved and workers idle
- Don't wait for all tasks in a phase to complete before starting next phase
- Example: If Task-4 depends on Task-2 (done) but not Task-3 (in progress), assign Task-4 now

## Batch Updates
- Update shared memory once per iteration, not per task
- Aggregate worker results before reporting to Lead
- Send summary to Lead every {report_interval}, not every change

## Token Optimization
- Reference task IDs instead of repeating full descriptions
- Use structured message types (avoid verbose natural language)
- Summarize completed work; provide details only for active/blocked tasks
```

### 5.7 Google ADK Coordinator Example

From research, a practical coordinator prompt:

```python
coordinator = LlmAgent(
    name="CoordinatorAgent",
    instruction="Analyze user intent. Route billing issues to BillingSpecialist and bugs to TechSupportSpecialist.",
    sub_agents=[billing_specialist, tech_support]
)
```

**Higher-Level Example:**

```
You are a travel concierge that uses sub-agents to fulfill requests.
Delegate flight questions to FlightAgent and itinerary questions to ItineraryAgent.
```

**Key Principle:**
> "The description field of your sub-agents is effectively your API documentation for the LLM."

Coordinators rely on worker descriptions to make routing decisions. Ensure worker prompts include clear capability descriptions.

---

## 6. Real-World Examples

### 6.1 Claude Code Swarm Orchestration

**Architecture:**
- **TeammateTool**: 13 operations for spawning, managing, coordinating agents
- **Leader-Teammate Model**: One primary agent orchestrates multiple spawned agents
- **Shared Task System**: Centralized work queue with dependency management stored in `~/.claude/tasks/{team}/`
- **Inbox Messaging**: JSON files for agent-to-agent communication

**Agent Types:**
- **Subagents (Task Tool)**: Short-lived, return direct results, no team overhead
- **Teammates (Task + team_name)**: Persistent, join teams, access shared task lists

**Key Operations:**

| Operation | Purpose |
|-----------|---------|
| `spawnTeam` | Create team infrastructure, designate leader |
| `write` | Message one teammate (targeted communication) |
| `broadcast` | Send message to all teammates (expensive, use sparingly) |
| `requestShutdown` | Leader requests teammate exit |
| `approveShutdown` | Teammate confirms shutdown (required) |
| `cleanup` | Remove all team resources after agents exit |

**Orchestration Patterns:**

1. **Parallel Specialists:**
   - Multiple agents review code simultaneously
   - Each sends findings to team-lead
   - Useful for comprehensive reviews (security + performance + architecture)

2. **Sequential Pipeline:**
   - Tasks with explicit dependencies auto-unblock
   - When Task #1 completes, Task #2 auto-unblocks
   - Perfect for multi-stage workflows (research → plan → implement → test)

3. **Self-Organizing Swarm:**
   - Workers continuously poll TaskList, claiming available tasks
   - Natural load-balancing without explicit coordination
   - Each agent races for unclaimed work

4. **Research-Then-Execute:**
   - Synchronous research returns results directly
   - Feeds findings into implementation prompts

5. **Plan Approval Workflow:**
   - Agents with `plan_mode_required: true` send plans for leader approval before proceeding

**Communication Protocol:**
- Workers' text output NOT visible to team
- Must use `write` to communicate
- Messages flow through inbox JSON files
- Structured message types: `shutdown_request`, `task_completed`, `idle_notification`, `plan_approval_request`

**Agent Lifecycle:**
1. Leader calls `spawnTeam`
2. Spawn workers with appropriate roles
3. Workers claim tasks from shared queue
4. Workers execute and send results via `write`
5. Leader calls `requestShutdown` for each worker
6. Workers call `approveShutdown`
7. Leader calls `cleanup` after all agents exit

**Graceful Shutdown:**
- Workers have 5-minute heartbeat timeout
- If worker crashes: automatically marked inactive, tasks remain claimable, cleanup succeeds
- Orphaned teams cause "Cannot cleanup with active members" error

**Spawn Backends:**
- **in-process**: Invisible background tasks (fastest, no visibility)
- **tmux**: Separate terminal panes (visible output, persists beyond leader exit)
- **iterm2**: macOS iTerm2 split panes (visual debugging)

### 6.2 Cursor Agentic-CursorRules

**Problem:** Context overload when agents access entire repositories.

**Solution:** File-tree partitioning by domain boundaries.

**Implementation:**
1. Define domains in `config.yaml` (e.g., `backend/api`, `frontend/dashboard`, `shared/utils`)
2. Generate agent files like `@agent_backend_api.md` describing each domain's scope
3. Reference these files when starting AI conversations

**Benefits:**
- **Reduced context noise:** Agents work with curated, relevant file structures
- **Lower coordination overhead:** Less need to synchronize across entire repos
- **Focused diffs:** Changes stay localized to intended domains
- **Clear boundaries:** Prevents accidental cross-domain modifications

**Key Insight:**
> "Keeps each agent inside a clearly defined slice of the codebase, where conversations stay focused, diffs stay local, and coordination overhead drops because agents aren't trying to understand the entire universe at once."

**Application to Hatchery:**
- Assign each worker a domain (e.g., "binance connector", "bybit connector")
- Workers only access files within their domain
- Coordinator manages cross-domain dependencies
- Reduces token usage and prevents interference

### 6.3 Windsurf Cascade

**Coordination Approach:**
- Automatically finds and loads relevant context for tasks
- No manual file tagging required
- Deeply understands project structure
- Propagates changes across many files when refactoring

**Multi-File Coordination:**
- Defaults to agent-style philosophy
- Asks for approval before running or applying changes
- Especially effective for monorepos and multi-module projects

**Comparison to Cursor:**
- **Cursor**: Batch operations across hundreds of files, explicit agent mode
- **Windsurf**: Inline learning, explains code while assisting

### 6.4 Rust Swarm Crates

**swarms-rs:**
- Enterprise-grade multi-agent orchestration framework in Rust
- Tool systems, MCP integration, swarm orchestration
- Persistence layers for state management

**ruv-swarm-core:**
- Foundational orchestration crate
- Core traits, abstractions, coordination primitives
- Task orchestration with priority-based queues
- Message passing, fault tolerance
- 7 distinct cognitive patterns

**ruvswarm-mcp:**
- Model Context Protocol (MCP) server for swarm orchestration
- Provides Claude Code access to swarm intelligence capabilities

**ruv-swarm-agents:**
- Specialized AI agents with diverse cognitive patterns
- Built on WebAssembly with SIMD optimization

**ruv-swarm-cli:**
- CLI for managing distributed agent swarms
- Multiple topologies, orchestration strategies
- Real-time monitoring, intelligent agent management

**Application to Hatchery:**
- Consider using `ruv-swarm-core` for coordinator infrastructure
- Leverage MCP for integration with Claude Code
- Adopt cognitive diversity patterns for worker specialization

### 6.5 Collaborative Memory Framework

**Two-Tier Memory:**
1. **Private Memory** (`ℳ^private`): Isolated fragments per user
2. **Shared Memory** (`ℳ^shared`): Selectively shared across users

**Access Control:**
- **User-to-Agent Graph** (`G_UA`): Which users can invoke specific agents
- **Agent-to-Resource Graph** (`G_AR`): Which resources each agent can access
- Agents can only access memory where all contributing agents are permitted AND all accessed resources match allowances

**Coordination Pipeline:**
1. Coordinator selects relevant agents for query
2. Agents retrieve filtered memory using read policy
3. Agents access permitted external resources
4. Responses update memory via write policies
5. Aggregator synthesizes final answer from agent outputs

**Application to Hatchery:**
- Use shared memory for cross-task knowledge (API rate limits, patterns, decisions)
- Use private memory for user-specific preferences
- Implement access control to prevent workers from accessing sensitive data
- Coordinator aggregates worker outputs with memory context

---

## 7. Implementation Recommendations

### 7.1 Hatchery Architecture

Based on research findings, here's a recommended architecture:

```
┌─────────────────────────────────────────────────────────────┐
│                      LEAD (Opus 4.6)                        │
│  - Receives high-level goals from user                      │
│  - Spawns Coordinator with structured objective             │
│  - Reviews final summary and approves deliverables          │
│  - Makes strategic decisions on escalations                 │
└────────────────┬────────────────────────────────────────────┘
                 │
                 │ Spawn with structured task list (prd.json)
                 ↓
┌─────────────────────────────────────────────────────────────┐
│                  COORDINATOR (Sonnet 4.5)                   │
│  - Maintains task graph with dependencies                   │
│  - Assigns tasks to workers based on capabilities           │
│  - Monitors progress and handles blockers                   │
│  - Runs iteration loop (check → assign → wait → validate)   │
│  - Escalates to Lead when strategic decisions needed        │
│  - Reports summary to Lead every N iterations               │
└────┬─────────┬─────────┬─────────┬──────────────────────────┘
     │         │         │         │
     │ Assign  │ Assign  │ Assign  │ Assign
     ↓         ↓         ↓         ↓
┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐
│ WORKER  │ │ WORKER  │ │ WORKER  │ │ WORKER  │
│ (Sonnet)│ │ (Sonnet)│ │ (Sonnet)│ │ (Sonnet)│
│         │ │         │ │         │ │         │
│ Type:   │ │ Type:   │ │ Type:   │ │ Type:   │
│ research│ │ rust-   │ │ rust-   │ │ rust-   │
│ -agent  │ │ impl    │ │ impl    │ │ impl    │
└─────────┘ └─────────┘ └─────────┘ └─────────┘
     │         │         │         │
     └─────────┴─────────┴─────────┘
               │
       Shared Memory, Task Queue, Inbox Messages
```

### 7.2 Coordinator Iteration Loop

**Pseudocode:**

```python
def coordinator_loop(task_graph, max_iterations=50, poll_interval=30):
    iteration = 0

    while iteration < max_iterations:
        # 1. Check tasks
        available_tasks = get_unblocked_pending_tasks(task_graph)
        idle_workers = get_idle_workers()

        # 2. Assign
        for task in available_tasks:
            worker = select_best_worker(task, idle_workers)
            if worker:
                assign_task(task, worker)
                idle_workers.remove(worker)

        # 3. Wait
        time.sleep(poll_interval)

        # 4. Check results
        completed = check_worker_outputs()
        for task_id, result in completed:
            if validate(result):
                mark_complete(task_id)
                unblock_dependents(task_id)
                record_success(worker_id)
            else:
                if retries_exhausted(task_id):
                    escalate_to_lead(task_id, result.error)
                else:
                    reassign(task_id)

        # 5. Check for stalls
        if stall_detected():
            rebalance_workload()
            if stall_persists():
                escalate_to_lead("Coordinator stalled")

        # 6. Report to Lead (every N iterations)
        if iteration % report_frequency == 0:
            send_summary_to_lead()

        # 7. Check completion
        if all_tasks_complete() and all_quality_gates_passed():
            send_final_report_to_lead()
            break

        iteration += 1

    if iteration >= max_iterations:
        escalate_to_lead("Max iterations reached")
```

### 7.3 File Structure

**Shared State Files:**

```
hatchery/
├── teams/
│   └── {team_id}/
│       ├── task_graph.json          # Task list with dependencies
│       ├── worker_status.json       # Worker availability and metrics
│       ├── shared_memory.json       # Cross-task knowledge
│       ├── inboxes/
│       │   ├── coordinator.json     # Messages to coordinator
│       │   ├── worker-1.json        # Messages to worker-1
│       │   ├── worker-2.json        # Messages to worker-2
│       │   └── ...
│       └── logs/
│           ├── coordinator.log      # Coordinator activity
│           ├── worker-1.log         # Worker-1 activity
│           └── ...
└── config/
    ├── coordinator_prompt.md        # Coordinator system prompt
    ├── worker_prompts/
    │   ├── research-agent.md
    │   ├── rust-implementer.md
    │   └── rust-expert.md
    └── quality_gates.json           # Acceptance criteria
```

### 7.4 Coordinator Prompt Template (Concrete)

**File: `hatchery/config/coordinator_prompt.md`**

```markdown
# Role
You are a Coordinator Agent (Sonnet 4.5) managing a team of specialized workers to accomplish complex software development tasks.

You report to a Lead agent (Opus 4.6) who spawned you with a high-level goal and structured task list.

# Responsibilities
1. **Task Assignment:** Assign pending tasks to idle workers based on:
   - Worker capabilities (research, Rust coding, debugging)
   - Current workload (tokens used, active tasks)
   - Task dependencies (only assign if dependencies complete)

2. **Progress Monitoring:** Check worker outputs against acceptance criteria:
   - Code compiles (`cargo check`)
   - Tests pass (`cargo test`)
   - Files exist and contain expected content
   - Follows project patterns (e.g., KuCoin reference implementation)

3. **Blocker Resolution:**
   - Re-assign tasks if worker stuck
   - Provide additional context or hints
   - Escalate to Lead if strategic decision needed

4. **Reporting:** Send concise summaries to Lead every 10 iterations:
   - Tasks completed / in-progress / blocked
   - Key accomplishments
   - Issues requiring escalation
   - Resource usage (tokens, time, cost)

5. **Completion:** Verify all tasks complete, quality gates passed, deliverables ready.

# Context
You have access to:
- **Task Graph:** `task_graph.json` (tasks, dependencies, status)
- **Worker Status:** `worker_status.json` (availability, capabilities, metrics)
- **Shared Memory:** `shared_memory.json` (knowledge, decisions, patterns)
- **Inbox Messages:** `inboxes/coordinator.json` (messages from workers)

Read these files at the start of each iteration. Update `task_graph.json` and `worker_status.json` after assigning tasks or receiving results.

# Decision-Making Guidelines
- **Prefer Parallel Execution:** Assign all independent tasks immediately
- **Avoid Bottlenecks:** Don't block workers waiting for your responses
- **Minimize Token Usage:** Reference task IDs, not full descriptions
- **Escalate Decisively:** Don't waste iterations on ambiguous requirements

# Communication Protocol
- **Assign Task:** Write to `inboxes/{worker_id}.json`:
  ```json
  {
    "type": "task_assignment",
    "task_id": "task-3",
    "title": "Research Bybit API",
    "description": "Create 6 research files in src/exchanges/bybit/research/",
    "acceptance_criteria": ["6 files exist", "All endpoints documented"],
    "reference": "See src/exchanges/kucoin/research/ for pattern",
    "estimated_tokens": 15000
  }
  ```

- **Request Status:** Write to `inboxes/{worker_id}.json`:
  ```json
  {
    "type": "status_request",
    "task_id": "task-2"
  }
  ```

- **Escalate to Lead:** Write to `inboxes/lead.json`:
  ```json
  {
    "type": "escalation",
    "reason": "Task-4 blocked: Ambiguous requirement about rate limiting",
    "context": "Worker unsure if per-endpoint or global limits apply",
    "recommendation": "Use per-endpoint based on API docs"
  }
  ```

- **Workers Send:** `task_completed`, `blocker_reported`, `idle_notification`

# Iteration Cycle (Every 30 Seconds)
1. Read `task_graph.json`, `worker_status.json`, `inboxes/coordinator.json`
2. Process messages (task completions, blockers, idle notifications)
3. Validate completed tasks against acceptance criteria
4. Update task_graph.json (mark complete, unblock dependents)
5. Assign available tasks to idle workers
6. Update worker_status.json (assign tasks, track tokens)
7. Check for stalls (same task stuck for 3+ iterations)
8. Every 10 iterations: Report summary to Lead
9. Check if all tasks complete → send final report

# Termination Conditions
Complete when:
- All tasks in task_graph.json have status "completed"
- All quality gates passed (tests, linting, compilation)
- No blockers remaining
- Lead agent approves final deliverables

Signal completion by writing to `inboxes/lead.json`:
```json
{
  "type": "completion",
  "summary": "All 20 tasks completed. 3 connectors implemented (Binance, Bybit, OKX). Tests passing. Ready for review.",
  "deliverables": [
    "src/exchanges/binance/",
    "src/exchanges/bybit/",
    "src/exchanges/okx/",
    "tests/binance_integration.rs",
    "tests/bybit_integration.rs",
    "tests/okx_integration.rs"
  ],
  "resource_usage": {
    "total_tokens": 145000,
    "total_cost": "$3.20",
    "total_time": "1.2 hours"
  }
}
```

# Efficiency Targets
- **Minimize Messages:** Batch task assignments; avoid polling every iteration
- **Maximize Parallelism:** Assign tasks as soon as dependencies resolved
- **Optimize Tokens:** Use task IDs, not full context; summarize completed work
- **Target Metrics:**
  - Average iteration time: < 2 minutes
  - Worker utilization: > 80% (minimize idle time)
  - Escalation rate: < 10% of tasks
  - First-time pass rate: > 70% of tasks
```

### 7.5 Worker Prompt Template (Concrete)

**File: `hatchery/config/worker_prompts/research-agent.md`**

```markdown
# Role
You are a Research Agent (Sonnet 4.5) specializing in API documentation research.

You are part of a team coordinated by a Coordinator Agent. You receive task assignments, execute them, and report results.

# Capabilities
- **WebSearch:** Find official documentation and authoritative sources
- **WebFetch:** Retrieve and analyze specific documentation pages
- **Markdown Writing:** Compile comprehensive research reports

# Task Workflow
1. **Receive Assignment:** Read `inboxes/{your_worker_id}.json` for new tasks
2. **Execute:** Use WebSearch and WebFetch to gather information
3. **Validate:** Check your output against acceptance criteria
4. **Report:** Write completion message to `inboxes/coordinator.json`
5. **Idle:** Notify coordinator you're ready for next task

# Communication Protocol
When task completed, write to `inboxes/coordinator.json`:
```json
{
  "type": "task_completed",
  "task_id": "task-1",
  "worker_id": "{your_worker_id}",
  "status": "success",
  "deliverables": [
    "src/exchanges/binance/research/endpoints_full.md",
    "src/exchanges/binance/research/websocket_full.md",
    "src/exchanges/binance/research/authentication.md",
    "src/exchanges/binance/research/response_formats.md",
    "src/exchanges/binance/research/rate_limits.md",
    "src/exchanges/binance/research/error_handling.md"
  ],
  "tokens_used": 14200,
  "notes": "All 6 files created. Documented 50+ endpoints, 20 WebSocket streams, 3 auth methods."
}
```

When blocked, write to `inboxes/coordinator.json`:
```json
{
  "type": "blocker_reported",
  "task_id": "task-3",
  "worker_id": "{your_worker_id}",
  "reason": "Documentation URL returns 404",
  "attempted_solutions": [
    "Searched for alternative docs",
    "Checked web archive"
  ],
  "recommendation": "Need clarification on correct documentation source"
}
```

When idle, write to `inboxes/coordinator.json`:
```json
{
  "type": "idle_notification",
  "worker_id": "{your_worker_id}",
  "status": "ready",
  "available_tokens": 35800
}
```

# Quality Standards
- Comprehensive: Cover all endpoints, parameters, data structures
- Accurate: Use official documentation; cite sources
- Structured: Follow markdown format with clear headings
- Complete: Meet all acceptance criteria before reporting completion

# Output Format
Save research to specified file paths using Write tool. Always include:
- Endpoint URLs and HTTP methods
- Parameters (required vs optional, types, descriptions)
- Response formats (JSON structures, field descriptions)
- Rate limits and authentication details
- Examples and edge cases
```

### 7.6 Lead Agent Workflow

**When User Provides Goal:**

1. **Receive Goal:** User describes high-level objective (e.g., "Implement Binance, Bybit, and OKX connectors")

2. **Decompose into Task List:**
   - Use existing patterns (e.g., carousel prompts)
   - Create structured `task_graph.json`
   - Define dependencies and acceptance criteria

3. **Spawn Coordinator:**
   ```
   Task(coordinator-sonnet): "Coordinate team to complete task list.
   Read task_graph.json from hatchery/teams/{team_id}/.
   Assign tasks to workers based on capabilities.
   Follow coordinator_prompt.md.
   Report summary every 10 iterations."
   ```

4. **Monitor Progress:**
   - Receive summaries every 10 iterations
   - Review escalations and provide strategic guidance
   - Approve/reject critical decisions

5. **Final Review:**
   - Receive completion message from Coordinator
   - Review deliverables against original goal
   - Approve or request changes

**Token Usage Target:**
- Lead (Opus): < 20k tokens (5% of total)
- Coordinator (Sonnet): 50-100k tokens (25% of total)
- Workers (Sonnet): 200-300k tokens (70% of total)

### 7.7 Quality Gates

**Define in `quality_gates.json`:**

```json
{
  "gates": [
    {
      "name": "Code Compiles",
      "command": "cd zengeld-terminal/crates/connectors/crates/v5 && cargo check",
      "success_criteria": "exit code 0",
      "required": true
    },
    {
      "name": "Tests Pass",
      "command": "cargo test --package connectors-v5 -- --nocapture",
      "success_criteria": "all tests passed, real data returned",
      "required": true
    },
    {
      "name": "Follows Pattern",
      "command": "ls src/exchanges/{exchange}/ | grep -E '(endpoints|auth|parser|connector|websocket)\\.rs'",
      "success_criteria": "5 files exist",
      "required": true
    },
    {
      "name": "Documentation Complete",
      "command": "ls src/exchanges/{exchange}/research/ | wc -l",
      "success_criteria": "6 files exist",
      "required": false
    }
  ]
}
```

**Coordinator validates each completed task against relevant gates before marking as complete.**

### 7.8 Stall Detection

**Indicators:**
- Same task remains "in-progress" for > 5 iterations
- Worker repeatedly requests same information
- No new files committed to Git for > 3 iterations
- Error messages repeating across iterations

**Coordinator Response:**
1. Send targeted hint to worker based on stall pattern
2. Re-assign to different worker if hint doesn't help
3. Escalate to Lead if task reveals missing information

**Example:**
```json
{
  "type": "hint",
  "task_id": "task-2",
  "worker_id": "rust-implementer-1",
  "message": "Stall detected. You've modified connector.rs 3 times without progress. Check if you're missing import for ExchangeError. Reference kucoin/connector.rs line 8."
}
```

### 7.9 Cost Control

**Budget Allocation:**
- Define max tokens per task (e.g., 15k for research, 25k for implementation)
- Track actual usage in `worker_status.json`
- Escalate if task approaching budget without completion
- Lead agent sets overall budget (e.g., $10 for entire goal)

**Per-Worker Limits:**
- Max 100k tokens per worker per session
- Spawn new worker if limit reached
- Prevents runaway token usage from single worker

**Coordinator Budget:**
- Coordinator itself limited to 100k tokens
- Mostly spent on reading task graph and worker status
- Optimize by using incremental updates and lazy loading

---

## Sources

### Coordinator Agent Patterns
- [AI Agent Orchestration: Multi-Agent Workflow Guide](https://www.digitalapplied.com/blog/ai-agent-orchestration-workflows-guide)
- [Agent Orchestration 2026: LangGraph, CrewAI & AutoGen Guide | Iterathon](https://iterathon.tech/blog/ai-agent-orchestration-frameworks-2026)
- [CrewAI / Hierarchical Manager: Build Reflection Enabled Agentic](https://teetracker.medium.com/crewai-hierarchical-manager-build-reflection-enabled-agentic-flow-8255c8c414ec)
- [Custom Manager Agent - CrewAI](https://docs.crewai.com/how-to/custom-manager-agent)
- [AI Agents Need a Boss: Building with the Supervisor Pattern in LangGraph + MCP](https://medium.com/@ashuashu20691/ai-agents-need-a-boss-building-with-the-supervisor-pattern-in-langgraph-mcp-9d8b7443e8fb)
- [GitHub - langchain-ai/langgraph-supervisor-py](https://github.com/langchain-ai/langgraph-supervisor-py)

### AutoGen GroupChatManager
- [Conversation Patterns | AutoGen 0.2](https://microsoft.github.io/autogen/0.2/docs/tutorial/conversation-patterns/)
- [Group Chat — AutoGen](https://microsoft.github.io/autogen/stable//user-guide/core-user-guide/design-patterns/group-chat.html)
- [Grouchat of 7 Agents and Groupchat manager as Coordinator](https://github.com/microsoft/autogen/discussions/2943)
- [GroupChat - AG2](https://docs.ag2.ai/latest/docs/user-guide/advanced-concepts/groupchat/groupchat/)
- [How to Orchestrate the Conversation Between Multiple Agents in AutoGen](https://yeyu.substack.com/p/how-to-orchestrate-the-conversation)

### Autonomous Agent Iteration Loops
- [AddyOsmani.com - Self-Improving Coding Agents](https://addyosmani.com/blog/self-improving-agents/)
- [Loop agents - Agent Development Kit](https://google.github.io/adk-docs/agents/workflow-agents/loop-agents/)
- [Self-Evolving Agents - A Cookbook for Autonomous Agent Retraining | OpenAI Cookbook](https://cookbook.openai.com/examples/partners/self_evolving_agents/autonomous_agent_retraining)
- [Agent Loop - Strands Agents](https://strandsagents.com/latest/documentation/docs/user-guide/concepts/agents/agent-loop/)
- [Understanding the Agent Loop in AWS Strands Agent Framework](https://dev.to/sreeni5018/understanding-the-agent-loop-in-aws-strands-agent-framework-4hhn)
- [Agentic AI: The Agent Loop & Tools for Building Autonomous Agents](https://you.com/resources/the-agent-loop-how-ai-agents-actually-work-and-how-to-build-one)
- [04. Autonomy and Loops — ReAct Loop - AI Agent Course](https://kshvakov.github.io/ai-agent-course/04-autonomy-and-loops/)
- [From ReAct to Ralph Loop A Continuous Iteration Paradigm for AI Agents](https://www.alibabacloud.com/blog/from-react-to-ralph-loop-a-continuous-iteration-paradigm-for-ai-agents_602799)

### Hierarchical Delegation & Token Optimization
- [Token Optimization Strategies | affaan-m/everything-claude-code | DeepWiki](https://deepwiki.com/affaan-m/everything-claude-code/12.2-token-optimization-strategies)
- [The Agent Unlock: Why Opus 4.5 Changed How I Work](https://hyperdev.matsuoka.com/p/the-agent-unlock-why-opus-45-changed)
- [Claude Code How-To: Use Opus, Sonnet, or Haiku for Team Agents](https://www.geeky-gadgets.com/claude-code-task-delegation/)
- [Claude Sonnet 4.5 vs Opus for Claude Code - Performance Comparison](https://claudelog.com/faqs/claude-4-sonnet-vs-opus/)
- [Claude Opus 4.5: Agentic Engineering & Workflow Scaling](https://www.geektak.com/blog/claude-opus-45-agentic-engineering-workflow)
- [Claude Sonnet 4.5 vs Opus 4.5: A Real-World Comparison](https://www.cosmicjs.com/blog/claude-sonnet-45-vs-opus-45-a-real-world-comparison)
- [Anthropic's Claude Opus 4.6 brings 1M token context and 'agent teams'](https://venturebeat.com/technology/anthropics-claude-opus-4-6-brings-1m-token-context-and-agent-teams-to-take)
- [Claude Opus 4.5 vs Sonnet 4.5 In-Depth Comparison](https://help.apiyi.com/en/claude-opus-4-5-vs-sonnet-4-5-comparison-en.html)

### Task Decomposition
- [Advancing Agentic Systems: Dynamic Task Decomposition, Tool Integration and Evaluation](https://arxiv.org/html/2410.22457v1)
- [Orchestrating Human-AI Teams: The Manager Agent as a Unifying Research Challenge](https://arxiv.org/html/2510.02557v1)
- [Dynamic role discovery and assignment in multi-agent task decomposition](https://link.springer.com/article/10.1007/s40747-023-01071-x)
- [TDAG: A Multi-Agent Framework based on Dynamic Task Decomposition and Agent Generation](https://arxiv.org/abs/2402.10178)
- [LLM Agent Task Decomposition Strategies](https://apxml.com/courses/agentic-llm-memory-architectures/chapter-4-complex-planning-tool-integration/task-decomposition-strategies)
- [How to Create Task Decomposition](https://oneuptime.com/blog/post/2026-01-30-task-decomposition/view)
- [Task Decomposition in Agent Systems - Matoffo](https://matoffo.com/task-decomposition-in-agent-systems/)
- [Workforce — CAMEL 0.2.23 documentation](https://docs.camel-ai.org/key_modules/workforce.html)
- [Task Decomposition | AutoGen 0.2](https://microsoft.github.io/autogen/0.2/docs/topics/task_decomposition/)

### Prompt Engineering
- [LLM Agents | Prompt Engineering Guide](https://www.promptingguide.ai/research/llm-agents)
- [PromptHub Blog: Prompt Engineering for AI Agents](https://www.prompthub.us/blog/prompt-engineering-for-ai-agents)
- [Developer's guide to multi-agent patterns in ADK](https://developers.googleblog.com/developers-guide-to-multi-agent-patterns-in-adk/)
- [Prompt Engineering with Google's Agent Development Kit (ADK)](https://medium.com/@george_6906/prompt-engineering-with-googles-agent-development-kit-adk-d748ba212440)
- [prompthub/agent-prompts: A collection of mostly system level prompts for popular and open-source agents](https://app.prompthub.us/prompthub/collection/agent-prompts)
- [Example: Changing the System Prompt | Agents | Mastra Docs](https://mastra.ai/examples/agents/system-prompt)

### Real-World Examples
- [Claude Code Swarm Orchestration Skill - Complete guide](https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea)
- [What Is the Claude Code Swarm Feature?](https://www.atcyrus.com/stories/what-is-claude-code-swarm-feature)
- [Claude Code Swarms: Multi-Agent AI Coding Is Here](https://zenvanriel.nl/ai-engineer-blog/claude-code-swarms-multi-agent-orchestration/)
- [GitHub - ruvnet/claude-flow](https://github.com/ruvnet/claude-flow)
- [I Tested Oh My Claude Code The Only Agents Swarm Orchestration You Need](https://medium.com/@joe.njenga/i-tested-oh-my-claude-code-the-only-agents-swarm-orchestration-you-need-7338ad92c00f)
- [Build Agent Skills Faster with Claude Code 2.1 Release](https://medium.com/@richardhightower/build-agent-skills-faster-with-claude-code-2-1-release-6d821d5b8179)
- [Claude Swarm Mode Complete Guide](https://help.apiyi.com/en/claude-code-swarm-mode-multi-agent-guide-en.html)
- [Claude Code's Hidden Multi-Agent System](https://paddo.dev/blog/claude-code-hidden-swarm/)
- [swarm-coordination - Claude Skills](https://claude-plugins.dev/skills/@joelhooks/opencode-swarm-plugin/swarm-coordination)

### Multi-File Coordination
- [GitHub - s-smits/agentic-cursorrules](https://github.com/s-smits/agentic-cursorrules)
- [Windsurf vs Cursor: which is the better AI code editor?](https://www.builder.io/blog/windsurf-vs-cursor)
- [Agentic IDE Comparison: Cursor vs Windsurf vs Antigravity](https://www.codecademy.com/article/agentic-ide-comparison-cursor-vs-windsurf-vs-antigravity)
- [Windsurf vs Cursor: A Comparison With Examples](https://www.datacamp.com/blog/windsurf-vs-cursor)

### Rust Swarm Crates
- [swarms-rs - crates.io](https://crates.io/crates/swarms-rs)
- [ruv-swarm-core - crates.io](https://crates.io/crates/ruv-swarm-core/0.2.0)
- [ruvswarm-mcp - crates.io](https://crates.io/crates/ruvswarm-mcp/1.1.0)
- [GitHub - The-Swarm-Corporation/swarms-rs](https://github.com/The-Swarm-Corporation/swarms-rs)
- [micro_swarm - crates.io](https://crates.io/crates/micro_swarm/0.1.0)
- [GitHub - georgesheth/swarms-rust](https://github.com/georgesheth/swarms-rust)
- [ruv-swarm-agents — ML/AI/statistics in Rust](https://lib.rs/crates/ruv-swarm-agents)
- [ruv-swarm-cli — command-line utility in Rust](https://lib.rs/crates/ruv-swarm-cli)

### Shared Memory & Context Encoding
- [An Efficient Context-Dependent Memory Framework](https://aclanthology.org/2025.naacl-industry.80.pdf)
- [Collaborative Memory: Multi-User Memory Sharing in LLM Agents](https://arxiv.org/html/2505.18279v1)
- [Why Multi-Agent Systems Need Memory Engineering | MongoDB](https://medium.com/mongodb/why-multi-agent-systems-need-memory-engineering-153a81f8d5be)
- [Adding Memory to an Agent | Microsoft Learn](https://learn.microsoft.com/en-us/agent-framework/tutorials/agents/memory)
- [How to Build Multi Agent AI Systems With Context Engineering](https://www.vellum.ai/blog/multi-agent-systems-building-with-context-engineering)
- [Context engineering in agents - Docs by LangChain](https://docs.langchain.com/oss/python/langchain/context-engineering)
- [Microsoft Agent Framework: Giving Agents Contextual Memory Using AIContextProvider](https://jamiemaguire.net/index.php/2025/12/20/microsoft-agent-framework-giving-agents-contextual-memory-using-aicontextprovider/)
- [Short-term memory - Multi-agent Reference Architecture](https://microsoft.github.io/multi-agent-reference-architecture/docs/memory/Short-Term-Memory.html)
- [Memory Blocks: The Key to Agentic Context Management | Letta](https://www.letta.com/blog/memory-blocks)
