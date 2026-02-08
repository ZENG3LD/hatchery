# Anthropic C Compiler Swarm: Technical Deep Dive

## 1. Prompts & Prompting Strategy

### "Pick Next Most Obvious Problem" Strategy

**Core Philosophy**: No orchestration layer or master agent directing traffic. Each Claude instance independently identifies and selects work based on current project state.

**Official Quote**: "I haven't yet implemented any other method for communication between agents, nor do I enforce any process for managing high-level goals. I don't use an orchestration agent... In most cases, Claude picks up the 'next most obvious' problem."

**Implementation**:
```python
# Agent's autonomous decision loop (conceptual)
def agent_main_loop():
    while True:
        # 1. Refresh understanding of current state
        state = analyze_project_state()

        # 2. Identify most obvious next problem
        # - Failing tests (if any exist)
        # - Unclaimed tasks in current_tasks/
        # - Open source projects not yet compiling
        # - Documentation gaps
        # - Code quality issues
        next_task = pick_most_obvious_problem(state)

        # 3. Claim task via file lock
        claim_task(next_task)

        # 4. Work on task
        solve_problem(next_task)

        # 5. Release and continue
        release_task(next_task)
```

**Selection Criteria** (inferred from project behavior):
1. **Failing tests** - Highest priority when test suite has failures
2. **Unlocked tasks** - Available work in `current_tasks/` directory
3. **Open source project compilation** - After 99% test pass rate, agents picked different projects (SQLite, Redis, etc.)
4. **Specialization roles** - Later in project, agents took on meta-tasks (deduplication, optimization, documentation)

**Quote on Stuck Behavior**: "When stuck on a bug, Claude will often maintain a running doc of failed approaches and remaining tasks."

### README-Based Onboarding

**Problem**: Fresh container = zero context. Each agent starts with no conversation history.

**Official Quote**: "Each agent is dropped into a fresh container with no context and will spend significant time orienting itself, especially on large projects. To help Claude help itself, extensive READMEs and progress files should be updated frequently with the current status."

**Documentation Requirements**:
1. **README.md** - Project overview, architecture, build instructions
2. **Progress files** - Current status, completed work, active issues
3. **Failed approaches log** - Debugging history to avoid repeating failures
4. **Task list** - Available work items (implicitly via `current_tasks/` directory)

**Documentation Update Cycle**:
```
Agent starts → Reads README → Works on task → Updates progress docs → Commits
                ↑                                                         ↓
                └─────────── Next agent starts fresh ←──────────────────┘
```

**Code Comments as Orientation**:
Well-commented code serves as additional orientation material. Agents read source files to understand:
- Module responsibilities
- API contracts
- Edge cases and known issues
- TODOs and planned work

### Plan Approval Mode

**NOT USED in the compiler project.** Plan approval mode is a feature of the Claude Code Agent Teams system released in February 2026.

**How It Works (in Agent Teams)**:
1. Teammate operates in read-only planning mode
2. Teammate analyzes problem and creates implementation plan
3. Teammate sends plan approval request to lead
4. Lead reviews plan and either:
   - **Approves**: Teammate exits plan mode and implements
   - **Rejects with feedback**: Teammate revises plan and resubmits

**Quote from Agent Teams docs**: "For complex or risky tasks, you can require teammates to plan before implementing. The teammate works in read-only plan mode until the lead approves their approach."

**Why Not Used in Compiler**:
- No lead/teammate hierarchy in compiler project
- Flat peer-to-peer architecture
- Each agent autonomous and self-directed
- No approval gates - agents commit directly to repository

### Delegate Mode Prompts

**NOT USED in the compiler project.** Delegate mode is exclusive to Agent Teams feature.

**How It Works (in Agent Teams)**:
- Activated via `Shift+Tab` in team lead session
- Restricts lead to coordination-only tools:
  - Spawning teammates
  - Messaging
  - Shutting down teammates
  - Managing tasks
- Prevents lead from implementing code directly
- Forces pure orchestration role

**Quote from Agent Teams docs**: "Delegate mode prevents [the lead from implementing] by restricting the lead to coordination-only tools... useful when you want the lead to focus entirely on orchestration."

**Compiler Project Alternative**:
Instead of explicit delegate mode, the compiler project achieved role separation through:
- Docker isolation (agents can't interfere with each other)
- File-based task locks (mutual exclusion without coordinator)
- Emergent specialization (agents naturally gravitated to different roles late in project)

### Prompt Examples (Compiler Project)

**NOT DISCLOSED.** Nicholas Carlini and Anthropic have not published the actual prompts used for the compiler agents.

**Inferred Prompt Structure** (based on observed behavior):
```markdown
# System Prompt (Hypothetical)

You are an autonomous C compiler development agent working in parallel with 15 other agents.

## Your Environment
- Fresh Docker container
- Git repository cloned to /workspace
- Upstream repo at /upstream
- No conversation history from previous sessions

## Your Objectives
1. Read README.md and progress/ files to understand current state
2. Identify the most obvious next problem to solve
3. Claim a task by creating a lock file in current_tasks/
4. Work on the task autonomously
5. Pull, merge, and push changes to upstream
6. Update progress documentation
7. Release task lock when complete

## Task Selection Priority
1. Failing compiler tests (highest priority)
2. Unclaimed tasks in current_tasks/
3. Open source projects not yet compiling
4. Code quality improvements (deduplication, optimization)
5. Documentation gaps

## Conflict Resolution
When you encounter merge conflicts:
- Analyze both versions
- Preserve correctness and test compatibility
- Resolve conflicts autonomously
- Do not ask for human intervention

## Testing Requirements
- Run full test suite before pushing
- Log test results to agent_logs/
- Use ERROR keyword on same line as failure for greppability
- Maintain high test pass rate (target: 99%+)

## Communication
- Communicate via git commits, README updates, and progress files
- Do not expect direct messages from other agents
- Read git history to understand recent changes

## When Stuck
- Maintain a running doc of failed approaches
- Document remaining tasks and blockers
- Try alternative strategies before asking for help
- Consider working on a different task if truly blocked
```

**Autonomous Loop Prompt** (for continuous operation):
Official blog shows this loop wrapper:
```bash
while true; do
  COMMIT=$(git rev-parse --short=6 HEAD)
  LOGFILE="agent_logs/agent_${COMMIT}.log"
  claude --dangerously-skip-permissions \
   -p "$(cat AGENT_PROMPT.md)" \
   --model claude-opus-X-Y &> "$LOGFILE"
done
```

**Prompt Source**: `AGENT_PROMPT.md` file (contents not disclosed)

## 2. Memory & Context Management

### 1M Token Context Per Agent

**Model Capability**: Claude Opus 4.6 supports 1 million token context window.

**Compiler Project Usage**: Each agent has access to full 1M token context for:
- Entire codebase reading (100,000 lines of Rust fits in ~400-500K tokens)
- Test suite output
- Git history and recent commits
- README and documentation files
- Compilation error logs

**No Persistent Memory Across Containers**: Each Docker container starts fresh, so context window resets between sessions. Agents rely on:
- Files in git repository (persistent storage)
- README/progress docs (explicit knowledge transfer)
- Code comments (in-band documentation)

### 75% Compaction Threshold

**Claude Code Feature**: Context compaction triggers at approximately 75% context utilization, leaving 25% (~50K tokens for 200K window) free for active reasoning.

**Quote from research**: "Claude Code now triggers compaction at around 75% utilization—leaving 25% (roughly 50K tokens) free for active reasoning."

**Why 75% Threshold**:
- Provides buffer for task completion before compaction
- Prevents mid-task context corruption
- Reserves ~20% for compaction process itself
- Earlier thresholds (e.g., 90%) caused context corruption when compaction triggered mid-task

**Compiler Project Applicability**: **UNCERTAIN** whether compiler project used compaction.
- Project predates or coincides with compaction feature announcement
- Each agent runs in fresh container (context resets anyway)
- Short-lived sessions may not hit 75% threshold often

### Context Editing - 84% Reduction

**Capability**: Context editing achieves 84% token reduction while preserving important information.

**Quote from research**: "Context editing achieves an 84% reduction in token consumption, which represents a significant improvement in memory efficiency."

**How It Works** (general principle, not compiler-specific):
1. **Identify compactable content**: Verbose tool outputs, repetitive information, resolved discussions
2. **Extract key information**: Decisions, important findings, current state
3. **Replace verbose blocks**: Compact summaries instead of full transcripts
4. **Preserve references**: Keep pointers to detailed logs if needed later

**Example Compaction**:
```
Before (10,000 tokens):
[Full compiler test output with 500 test results, stack traces, warnings...]

After (1,600 tokens):
Test Summary: 485/500 passing (97%).
Failures: parse_complex_typedef, codegen_variadic_functions, ...
Key errors: Type inference issue in line 234, register allocation bug in backend.
Full logs: agent_logs/agent_a3f2b1.log
```

**Compiler Project Usage**: **NOT DISCLOSED** whether context editing was actively used. Likely candidates for compaction:
- Test suite outputs (verbose)
- Compilation error logs
- Git diff outputs
- Resolved debugging sessions

### /clear + Task DAG Persistence

**NOT APPLICABLE to compiler project.** The `/clear` command and Task DAG persistence are features of Claude Code interactive sessions, not the autonomous Docker-based agent loop.

**What /clear Does** (in interactive Claude Code):
- Clears conversation history
- Reduces context size
- Maintains connection to current project
- Preserves filesystem state

**Task DAG Persistence** (in Agent Teams):
- Tasks stored in `~/.claude/tasks/{team-name}/N.json`
- Dependency graph (`blockedBy`, `blocks` arrays) persists across sessions
- Teammates can resume work on task list after restart

**Compiler Project Alternative**:
Instead of `/clear` and task DAG:
- **Fresh containers** achieve same effect as `/clear` (zero context)
- **current_tasks/ directory** serves as persistent task list
- **Git repository** is the persistence layer for all state
- **README/progress files** explicitly document task dependencies

## 3. Task Distribution & Scheduling

### current_tasks/ File Locks

**Synchronization Mechanism**: File-based optimistic locking via git's atomic operations.

**Official Quote**: "To prevent two agents from trying to solve the same problem at the same time, the harness uses a simple synchronization algorithm: Claude takes a 'lock' on a task by writing a text file to `current_tasks/` (for example, one agent might lock `current_tasks/parse_if_statement.txt`, while another locks `current_tasks/codegen_function_definition.txt`)."

**Lock File Format**:
```
current_tasks/
├── parse_if_statement.txt        # Agent A working on this
├── codegen_function_definition.txt  # Agent B working on this
├── optimize_register_allocation.txt # Agent C working on this
└── fix_preprocessor_macros.txt   # Agent D working on this
```

**Lock File Contents**: Likely minimal or empty. The filename itself identifies the task. Possible contents:
- Agent ID or container name
- Timestamp of claim
- Brief description of approach
- Empty (lock is purely presence-based)

**Lock Acquisition Protocol**:
```bash
# Agent attempts to claim task
echo "Agent-7 claimed at $(date)" > current_tasks/my_task.txt
git add current_tasks/my_task.txt
git commit -m "Claim task: my_task"
git push

# If push succeeds → lock acquired
# If push fails → another agent claimed it, pick different task
```

### Optimistic Locking

**How It Works**: Git's atomic push operation enforces mutual exclusion.

**Race Condition Scenario**:
```
Timeline:
T0: Agent A decides to claim "parse_if_statement"
T1: Agent B decides to claim "parse_if_statement"
T2: Agent A: git push (succeeds - first to push)
T3: Agent B: git push (FAILS - conflict with A's push)
T4: Agent B: git pull (sees A's lock file)
T5: Agent B: picks different task
```

**Quote**: "If two agents try to claim the same task, git's synchronization forces the second agent to pick a different one."

**Advantages**:
- No central lock server required
- Git provides atomicity guarantees
- Distributed and decentralized
- Survives individual agent crashes
- Human-readable (just files in directory)

**Disadvantages**:
- Lock contention causes wasted work (agents pick tasks that may already be claimed)
- No priority system (first-come-first-served)
- No deadlock detection
- Stale locks if agent crashes before releasing (manual cleanup needed)

### Heartbeat Mechanism

**NOT IMPLEMENTED in the compiler project** based on available information.

**Typical Heartbeat System** (for comparison):
- Agent writes timestamp to heartbeat file every N seconds
- Coordinator checks heartbeat files for freshness
- Stale heartbeat (no update in M seconds) → agent considered dead
- Stale locks released, tasks requeued

**Compiler Project Alternative**:
- No automated heartbeat or liveness detection
- Container crashes likely leave stale locks
- Human operator (Nicholas Carlini) probably manually cleaned up stale locks
- Simple restart loop means crashed agents automatically restart and pick new tasks

**Quote from Claude Code (unrelated to compiler)**: Heartbeat exists in Cowork VM: "VM running out of memory and stopping responding to heartbeat pings (2-second interval), and after 3 consecutive failures (6 seconds), the VM is force-restarted."

### Task DAG JSON Format

**NOT USED in the compiler project.** Task DAG is exclusive to Agent Teams feature.

**Agent Teams Task Format**:
```json
{
  "id": "1",
  "subject": "Implement type inference for generics",
  "description": "Add support for generic type parameter inference...",
  "status": "in_progress",        // pending | in_progress | completed
  "owner": "agent-3",              // Which agent claimed it
  "activeForm": "Implementing...", // Present progressive verb phrase
  "blockedBy": [],                 // Task IDs that must complete first
  "blocks": ["3", "5"],            // Task IDs waiting on this one
  "createdAt": 1706000000000,      // Unix timestamp
  "updatedAt": 1706000001000       // Unix timestamp
}
```

**Stored in**: `~/.claude/tasks/{team-name}/1.json`, `2.json`, etc.

**Auto-Unblocking**:
When task #1 completes:
```json
// Task 3 was blocked by task 1
// Before:
{"id": "3", "blockedBy": ["1"], "status": "pending"}

// After task 1 completes:
{"id": "3", "blockedBy": [], "status": "pending"}  // Now claimable
```

### blockedBy/blocks Arrays

**NOT USED in compiler project.**

**How Dependency Tracking Works** (Agent Teams):
```json
// Task 1: Design API
{"id": "1", "subject": "Design API", "blockedBy": [], "blocks": ["2", "3"]}

// Task 2: Implement API (depends on task 1)
{"id": "2", "subject": "Implement API", "blockedBy": ["1"], "blocks": ["4"]}

// Task 3: Write API docs (depends on task 1)
{"id": "3", "subject": "Write docs", "blockedBy": ["1"], "blocks": []}

// Task 4: Integration tests (depends on task 2)
{"id": "4", "subject": "Integration tests", "blockedBy": ["2"], "blocks": []}
```

**Dependency Graph**:
```
    ┌──────┐
    │  1   │ Design API
    └──┬───┘
       │
   ┌───┴────┐
   ▼        ▼
┌──────┐ ┌──────┐
│  2   │ │  3   │ Implement / Docs
└──┬───┘ └──────┘
   │
   ▼
┌──────┐
│  4   │ Tests
└──────┘
```

**Claiming Rules**:
- Agent can only claim task if `blockedBy: []` (no pending dependencies)
- Attempting to claim blocked task → error or automatic rejection
- When task completes → all tasks in its `blocks` array have it removed from their `blockedBy`

**Compiler Project Alternative**:
Instead of formal DAG:
- **Implicit dependencies**: Test failures naturally encode dependencies (can't test codegen until parser works)
- **Natural ordering**: Agents pick "most obvious" problem, which often respects dependencies
- **README coordination**: Progress docs inform agents what's safe to work on

## 4. Validation & Quality Control

### GCC Torture Tests

**What They Are**: Comprehensive edge case test suite designed to break compilers.

**Official Description**: "The GCC torture test suite is a notorious collection of edge cases designed to break compilers."

**Test Coverage**:
- Obscure C language features
- Edge cases in type system
- Undefined behavior scenarios
- Optimization stress tests
- Platform-specific quirks
- ABI compatibility requirements

**Compiler Project Results**:
- **Pass Rate**: 99.1% (18,234 of 18,397 tests)
- **Significance**: Demonstrates robust C language support beyond toy implementations

### 99% Pass Rate Achievement

**Journey to 99%**:
1. **Initial Phase**: Agents work on independent test failures in parallel
   - Quote: "When suites had hundreds of independent failures," agents picked different failing tests
   - Natural load balancing through test diversity
   - Near-linear speedup with 16 agents

2. **Plateau Phase**: After 99% pass rate
   - Remaining failures likely corner cases requiring deep investigation
   - Agents shifted to compiling real-world projects
   - Quote: "After 99% pass rate, each worked on getting a different small open-source project (e.g., SQLite, Redis, libjpeg, MQuickJS, Lua) to compile."

3. **Validation Strategy**: Real-world compilation as final validation
   - If SQLite compiles and runs correctly → compiler handles production C code
   - More meaningful than synthetic tests
   - Uncovers real-world edge cases tests might miss

**Why 99% Not 100%**:
- Remaining 1% likely extremely obscure edge cases
- Cost-benefit analysis: 99% covers production usage
- GCC torture tests include deliberate compiler stress cases beyond normal code
- Diminishing returns on final 1%

### Compiled Linux 6.9 + Real Projects

**Linux 6.9 Kernel**:
- **Architectures**: x86, ARM, RISC-V
- **Bootability**: Successfully boots and runs
- **Complexity**: Massive codebase (~30M lines including drivers)
- **Significance**: Ultimate validation - Linux kernel uses every obscure C feature

**Quote**: "100,000-line compiler that can build Linux 6.9 on x86, ARM, and RISC-V."

**Real-World Projects Compiled**:
1. **QEMU** - Full system emulator (extremely complex)
2. **FFmpeg** - Multimedia framework (heavy optimization, SIMD)
3. **SQLite** - Database engine (careful C with extensive testing)
4. **PostgreSQL** - Enterprise database (large codebase, complex build)
5. **Redis** - In-memory database (performance-critical C)
6. **Doom** - Classic game (final validation, runs successfully)

**Quote**: "It successfully compiles major real-world projects including FFmpeg, Redis, PostgreSQL, QEMU, and even runs the iconic video game Doom."

**Validation Strategy**:
- Compilation success → syntactic correctness
- Program execution → semantic correctness
- Test suite pass → functional correctness
- Doom runs → end-to-end validation (human-observable success)

### Quality Hooks

**Test Harness Design**: Structured output for machine parsing.

**Official Quote**: "Test harnesses avoided 'thousands of useless bytes' of output, logging instead to files with structured format: 'if there are errors, Claude should write ERROR and put the reason on the same line so grep will find it.'"

**Structured Logging Example**:
```
# Bad (verbose, hard to parse):
Running test_parse_function...
Parsing input file...
Reading tokens...
Building AST...
Type checking...
FAILURE: Type mismatch in function argument
  Expected: int
  Got: char*
  Location: line 45, column 12
  Stack trace:
    ...10 lines of stack trace...

# Good (greppable, concise):
ERROR test_parse_function: Type mismatch int vs char* at line 45
```

**Quality Control Mechanisms**:

**1. Continuous Integration (Inferred)**:
Quote: "Implemented CI pipeline preventing new commits from breaking existing code."
- Each commit triggers test suite
- Regression detection catches breaking changes
- Prevents backsliding on test pass rate

**2. Automated Subsampling**:
Quote: "Automated subsampling with `--fast` flag running '1% or 10% random sample' per agent."
- Full test suite too slow for every iteration
- Random sampling provides fast feedback
- Probabilistic coverage catches most regressions
- Full suite runs periodically or before final push

**3. High-Fidelity Testing**:
Quote: "Improving the testing harness required finding high-quality compiler test suites, writing verifiers and build scripts for open-source software packages."
- GCC torture tests (comprehensive edge cases)
- Real project compilation (practical validation)
- Custom verifiers (correctness checking beyond compilation)
- Build scripts (automation for reproducibility)

**Quality Gates** (Inferred):
```python
# Conceptual quality gate system
def pre_push_quality_gate(commit):
    # Gate 1: Fast smoke test
    if not run_test_subset(sample=0.01):  # 1% of tests
        return REJECT("Fast tests failed")

    # Gate 2: Regression check
    if test_pass_rate < previous_pass_rate:
        return REJECT("Regression detected")

    # Gate 3: No ERROR in logs
    if grep_logs("ERROR"):
        return REJECT("Error markers found")

    # Gate 4: Periodic full validation
    if commit_count % 10 == 0:
        if not run_full_test_suite():
            return REJECT("Full suite failed")

    return APPROVE("Quality gates passed")
```

**Hook Integration with Agent Teams** (Not used in compiler, but relevant):
```python
# TeammateIdle hook (example)
def check_build_artifact(input_data, tool_use_id, context):
    """Don't let agent go idle without producing build artifact"""
    if not os.path.exists("build/compiler"):
        return {
            "hookSpecificOutput": {
                "hookEventName": "TeammateIdle",
                "permissionDecision": "deny",
                "permissionDecisionReason": "Build artifact missing"
            }
        }
    return {}

# TaskCompleted hook (example)
def enforce_test_pass(input_data, tool_use_id, context):
    """Don't mark task complete unless tests pass"""
    task_id = input_data["task_id"]
    result = run_tests_for_task(task_id)

    if result.pass_rate < 0.99:
        return {
            "hookSpecificOutput": {
                "hookEventName": "TaskCompleted",
                "permissionDecision": "deny",
                "permissionDecisionReason": f"Test pass rate {result.pass_rate} below 99%"
            }
        }
    return {}
```

## 5. Mailbox / Inbox Implementation

### ~/.claude/teams/{name}/inboxes/{agent}.json

**NOT USED in the compiler project.** This is exclusive to Agent Teams feature.

**Directory Structure** (Agent Teams):
```
~/.claude/teams/compiler-team/
├── config.json              # Team metadata
└── inboxes/
    ├── team-lead.json       # Messages for lead
    ├── worker-1.json        # Messages for worker 1
    ├── worker-2.json        # Messages for worker 2
    └── ...
```

**Team Config** (`config.json`):
```json
{
  "name": "compiler-team",
  "leadAgentId": "team-lead@compiler-team",
  "members": [
    {
      "agentId": "worker-1@compiler-team",
      "name": "worker-1",
      "agentType": "general-purpose",
      "model": "opus-4-6",
      "color": "#D94A4A",
      "backendType": "in-process",
      "tmuxPaneId": "in-process",
      "cwd": "/workspace/compiler"
    }
  ]
}
```

**Inbox File Example** (`worker-1.json`):
```json
[
  {
    "from": "team-lead",
    "text": "Focus on parsing improvements",
    "timestamp": "2026-02-08T10:30:00Z",
    "read": false
  },
  {
    "type": "task_completed",
    "from": "worker-2",
    "taskId": "5",
    "timestamp": "2026-02-08T11:00:00Z",
    "read": true
  }
]
```

**Message Delivery**:
- Lead/agents write to `{recipient}.json` file
- Recipient polls inbox for new messages (where `read: false`)
- Automatic delivery - no manual polling by user

**Quote from docs**: "When teammates send messages, they're delivered automatically to recipients. The lead doesn't need to poll for updates."

### Message Types Schema

**NOT USED in compiler project.** Schema below is for Agent Teams feature.

**1. Regular Text Message**:
```json
{
  "from": "agent-name",
  "text": "Message content here",
  "timestamp": "2026-02-08T10:30:00Z",
  "read": false
}
```

**2. Shutdown Request**:
```json
{
  "type": "shutdown_request",
  "requestId": "shutdown-abc123",
  "from": "team-lead",
  "reason": "Task completed, shutting down team",
  "timestamp": "2026-02-08T15:00:00Z",
  "read": false
}
```

**3. Shutdown Response** (Approve):
```json
{
  "type": "shutdown_response",
  "requestId": "shutdown-abc123",
  "from": "worker-1",
  "approved": true,
  "timestamp": "2026-02-08T15:01:00Z"
}
```

**4. Shutdown Response** (Reject):
```json
{
  "type": "shutdown_response",
  "requestId": "shutdown-abc123",
  "from": "worker-1",
  "approved": false,
  "reason": "Still working on critical task",
  "timestamp": "2026-02-08T15:01:00Z"
}
```

**5. Task Completed Notification**:
```json
{
  "type": "task_completed",
  "from": "worker-2",
  "taskId": "5",
  "subject": "Implement type inference",
  "timestamp": "2026-02-08T11:00:00Z",
  "read": false
}
```

**6. Plan Approval Request**:
```json
{
  "type": "plan_approval_request",
  "requestId": "plan-xyz789",
  "from": "architect",
  "planContent": "## Implementation Plan\n1. Refactor parser\n2. Add AST validation\n3. ...",
  "timestamp": "2026-02-08T09:00:00Z",
  "read": false
}
```

**7. Plan Approval Response** (Approve):
```json
{
  "type": "plan_approval_response",
  "requestId": "plan-xyz789",
  "from": "team-lead",
  "approved": true,
  "timestamp": "2026-02-08T09:15:00Z"
}
```

**8. Plan Approval Response** (Reject):
```json
{
  "type": "plan_approval_response",
  "requestId": "plan-xyz789",
  "from": "team-lead",
  "approved": false,
  "feedback": "Plan needs more detail on error handling",
  "timestamp": "2026-02-08T09:15:00Z"
}
```

**9. Join Request**:
```json
{
  "type": "join_request",
  "requestId": "join-def456",
  "proposedName": "helper-agent",
  "timestamp": "2026-02-08T08:00:00Z",
  "read": false
}
```

**10. Join Response** (Approve):
```json
{
  "type": "join_response",
  "requestId": "join-def456",
  "from": "team-lead",
  "approved": true,
  "agentId": "helper-agent@compiler-team",
  "timestamp": "2026-02-08T08:05:00Z"
}
```

**11. Join Response** (Reject):
```json
{
  "type": "join_response",
  "requestId": "join-def456",
  "from": "team-lead",
  "approved": false,
  "reason": "Team full - max 16 agents",
  "timestamp": "2026-02-08T08:05:00Z"
}
```

**12. Idle Notification**:
```json
{
  "type": "idle_notification",
  "from": "worker-3",
  "completedTaskId": "7",
  "timestamp": "2026-02-08T14:30:00Z",
  "read": false
}
```

**13. Broadcast Message**:
```json
{
  "from": "team-lead",
  "text": "Status check - report progress on current tasks",
  "broadcast": true,
  "timestamp": "2026-02-08T12:00:00Z",
  "read": false
}
```

### TeammateIdle/TaskCompleted Hooks

**Hooks Overview**: Exit-code-based control flow for agent lifecycle events.

**Quote from documentation**: "Use hooks to enforce rules when teammates finish work or tasks complete."

**TeammateIdle Hook**:

**Purpose**: Runs when a teammate is about to go idle (finished current work, no tasks claimed).

**Exit Codes**:
- **0**: Allow idle (teammate can stop and wait)
- **2**: Deny idle with feedback (teammate must continue working)

**Example Use Case**: Enforce build artifact exists before going idle.

```python
#!/usr/bin/env python3
# Hook: .claude/hooks/teammate_idle.py

import os
import sys
import json

def main():
    # Input provided via stdin
    input_data = json.loads(sys.stdin.read())

    teammate_name = input_data.get("teammate_name")
    team_name = input_data.get("team_name")

    # Check if build artifact exists
    artifact_path = f"build/{teammate_name}/compiler"
    if not os.path.exists(artifact_path):
        # Deny idle - send feedback
        print(f"ERROR: Build artifact missing at {artifact_path}", file=sys.stderr)
        sys.exit(2)  # Exit code 2 = deny with feedback

    # Allow idle
    sys.exit(0)  # Exit code 0 = approve

if __name__ == "__main__":
    main()
```

**Quote**: "TeammateIdle hooks use exit codes only, not JSON decision control, and can check conditions before allowing a teammate to go idle."

**Limitations**:
- Does NOT support prompt-based hooks
- Does NOT support agent-based hooks
- Exit code only (no structured output)

**TaskCompleted Hook**:

**Purpose**: Runs when a task is being marked as completed.

**Triggers**:
- Agent explicitly marks task complete via `TaskUpdate` tool
- Agent team teammate finishes turn with in-progress tasks

**Exit Codes**:
- **0**: Allow completion (task marked as done)
- **2**: Deny completion with feedback (task remains in-progress)

**Example Use Case**: Enforce tests pass before task completion.

```python
#!/usr/bin/env python3
# Hook: .claude/hooks/task_completed.py

import sys
import json
import subprocess

def main():
    input_data = json.loads(sys.stdin.read())

    task_id = input_data["task_id"]
    task_subject = input_data["task_subject"]

    # Run tests for this task
    result = subprocess.run(
        ["./scripts/run_tests.sh", task_id],
        capture_output=True,
        text=True
    )

    if result.returncode != 0:
        # Tests failed - deny completion
        print(f"ERROR: Tests failed for task {task_id}", file=sys.stderr)
        print(result.stderr, file=sys.stderr)
        sys.exit(2)  # Deny completion

    # Tests passed - allow completion
    sys.exit(0)

if __name__ == "__main__":
    main()
```

**Quote**: "When a TaskCompleted hook exits with code 2, the task is not marked as completed and the stderr message is fed back to the model as feedback."

**Input Fields**:
- `task_id` (required)
- `task_subject` (required)
- `task_description` (optional)
- `teammate_name` (optional)
- `team_name` (optional)
- Common fields: `cwd`, `tool_use_id`, `model`, etc.

**Limitations**:
- Does NOT support matchers (fires on ALL task completions)
- No filtering by task type or agent

**Release Information**:
Quote from changelog: "Added TeammateIdle and TaskCompleted hook events for multi-agent workflows" in Claude Code v2.1.33.

### 13 TeammateTool Operations (Complete List)

**NOT USED in compiler project.** All 13 operations below are exclusive to Agent Teams feature.

**1. spawnTeam**
```json
{
  "operation": "spawnTeam",
  "team_name": "compiler-team"
}
```
- Creates new team
- Initializes `~/.claude/teams/{team_name}/` directory
- Creates `config.json` with team metadata
- Calling session becomes team lead

**2. discoverTeams**
```json
{
  "operation": "discoverTeams"
}
```
- Returns list of all available teams
- Reads `~/.claude/teams/` directory
- Shows team names and basic metadata

**3. requestJoin**
```json
{
  "operation": "requestJoin",
  "team_name": "compiler-team",
  "proposed_name": "helper-agent"
}
```
- Agent requests to join existing team
- Sends join request to team lead's inbox
- Lead must approve/reject

**4. approveJoin** (Lead only)
```json
{
  "operation": "approveJoin",
  "target_agent_id": "helper-agent",
  "request_id": "join-abc123"
}
```
- Team lead approves join request
- Adds agent to team members list
- Creates inbox for new agent

**5. rejectJoin** (Lead only)
```json
{
  "operation": "rejectJoin",
  "target_agent_id": "helper-agent",
  "request_id": "join-abc123",
  "reason": "Team is full"
}
```
- Team lead rejects join request
- Sends rejection reason to requesting agent

**6. write** (Direct message)
```json
{
  "operation": "write",
  "target_agent_id": "worker-1",
  "value": "Focus on parser improvements"
}
```
- Send message to specific teammate
- Writes to `~/.claude/teams/{team}/inboxes/{target}.json`
- Recipient receives next time they check inbox

**7. broadcast** (All agents)
```json
{
  "operation": "broadcast",
  "name": "team-lead",
  "value": "Status check - report progress"
}
```
- Send message to ALL teammates
- Writes to every agent's inbox
- **Cost scales with team size** (N agents = N messages)
- Use sparingly

**8. approvePlan** (Lead only)
```json
{
  "operation": "approvePlan",
  "target_agent_id": "architect",
  "request_id": "plan-xyz789"
}
```
- Approve teammate's implementation plan
- Teammate exits plan mode and begins implementation

**9. rejectPlan** (Lead only)
```json
{
  "operation": "rejectPlan",
  "target_agent_id": "architect",
  "request_id": "plan-xyz789",
  "feedback": "Need more detail on error handling"
}
```
- Reject plan with feedback
- Teammate revises plan based on feedback
- Teammate resubmits for approval

**10. requestShutdown** (Lead only)
```json
{
  "operation": "requestShutdown",
  "target_agent_id": "worker-1",
  "reason": "Task completed, team wrapping up"
}
```
- Request teammate to shut down gracefully
- Teammate can approve or reject
- Does NOT force shutdown

**11. approveShutdown** (Teammate only)
```json
{
  "operation": "approveShutdown",
  "request_id": "shutdown-abc123"
}
```
- Teammate approves shutdown request
- Session exits gracefully
- Inbox and task state preserved

**12. rejectShutdown** (Teammate only)
```json
{
  "operation": "rejectShutdown",
  "request_id": "shutdown-abc123",
  "reason": "Still working on critical task"
}
```
- Teammate rejects shutdown request
- Provides reason for continuing work
- Continues normal operation

**13. cleanup**
```json
{
  "operation": "cleanup"
}
```
- Remove team resources
- Deletes `~/.claude/teams/{team_name}/`
- Deletes `~/.claude/tasks/{team_name}/`
- **CRITICAL**: Only call from lead
- **CRITICAL**: Only call after all teammates shut down
- Fails if teammates still active

**Quote**: "Always use the lead to clean up. Teammates should not run cleanup because their team context may not resolve correctly, potentially leaving resources in an inconsistent state."

**Operation Categories**:
```
Team Lifecycle:    spawnTeam, discoverTeams, cleanup
Membership:        requestJoin, approveJoin, rejectJoin
Communication:     write, broadcast
Plan Approval:     approvePlan, rejectPlan
Shutdown:          requestShutdown, approveShutdown, rejectShutdown
```

## 6. Open Source Artifacts & Code

### Claude Code SDK Source

**Fully open source** as of February 2026:

**Python SDK**:
- Repository: https://github.com/anthropics/claude-agent-sdk-python
- License: MIT
- Package: `pip install claude-agent-sdk`

**Repository Structure**:
```
claude-agent-sdk-python/
├── src/claude_agent_sdk/
│   ├── query.py          # Simple query interface
│   ├── client.py         # Full bidirectional client
│   ├── types.py          # Type definitions
│   ├── _errors.py        # Error classes
│   └── _version.py
├── examples/
│   ├── quick_start.py
│   ├── streaming_mode.py
│   ├── mcp_calculator.py  # Custom tool example
│   └── hooks.py           # Hook examples
└── tests/
```

**Key APIs**:
```python
from claude_agent_sdk import query, ClaudeSDKClient, ClaudeAgentOptions

# Simple query
async for message in query(prompt="Write hello world"):
    print(message)

# Advanced client
async with ClaudeSDKClient(options=options) as client:
    await client.query("Your prompt")
    async for msg in client.receive_response():
        print(msg)
```

**TypeScript SDK**:
- Repository: https://github.com/anthropics/claude-agent-sdk-typescript
- License: NOT DISCLOSED (likely MIT)
- Package: `npm install @anthropic-ai/claude-agent-sdk`
- Documentation: https://docs.claude.com/en/api/agent-sdk/overview

**Demo Applications**:
- Repository: https://github.com/anthropics/claude-agent-sdk-demos
- Example projects showcasing SDK capabilities

### Agent-Teams Docs

**Official Documentation**: https://code.claude.com/docs/en/agent-teams

**Coverage**:
- Enabling agent teams (feature flag)
- Starting and managing teams
- TeammateTool operations
- Task management system
- Best practices
- Troubleshooting
- Limitations

**Full Documentation Index**: https://code.claude.com/docs/llms.txt

**Related Documentation**:
- Hooks Reference: https://code.claude.com/docs/en/hooks
- Subagents: https://code.claude.com/docs/en/sub-agents
- Settings: https://code.claude.com/docs/en/settings
- Best Practices: https://code.claude.com/docs/en/best-practices

### GitHub Repositories

**Official Anthropic Repositories**:

1. **claude-code** (Main CLI tool)
   - URL: https://github.com/anthropics/claude-code
   - Description: Agentic coding tool for terminal
   - Stars: NOT DISCLOSED (likely high)
   - License: NOT DISCLOSED

2. **claude-agent-sdk-python**
   - URL: https://github.com/anthropics/claude-agent-sdk-python
   - License: MIT
   - Used by: Many projects

3. **claude-agent-sdk-typescript**
   - URL: https://github.com/anthropics/claude-agent-sdk-typescript
   - Stars: 752
   - Forks: 83
   - Used by: 527 projects

4. **claude-agent-sdk-demos**
   - URL: https://github.com/anthropics/claude-agent-sdk-demos
   - Example applications

**Community Repositories**:

1. **hesreallyhim/awesome-claude-code**
   - URL: https://github.com/hesreallyhim/awesome-claude-code
   - Curated list of skills, hooks, slash-commands, orchestrators, plugins

2. **MaTriXy/claude-swarm-orchestration**
   - URL: https://github.com/MaTriXy/claude-swarm-orchestration
   - Path: `docs/teammate-api.md`
   - Community documentation for swarm features

3. **Piebald-AI/claude-code-system-prompts**
   - URL: https://github.com/Piebald-AI/claude-code-system-prompts
   - Path: `system-prompts/tool-description-teammatetool.md`
   - Reverse-engineered system prompts

4. **siteboon/claudecodeui**
   - URL: https://github.com/siteboon/claudecodeui
   - Web/mobile GUI for Claude Code
   - License: Free open source

5. **jamesrochabrun/ClaudeCodeSDK**
   - URL: https://github.com/jamesrochabrun/ClaudeCodeSDK
   - Swift implementation of Claude Code SDK

### Gists with Swarm Configs

**1. Kieran Klaassen - Complete Swarm Orchestration**
- URL: https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea
- Title: "Claude Code Swarm Orchestration Skill - Complete guide to multi-agent coordination with TeammateTool, Task system, and all patterns"

**Complete Contents Summary**:

**Core Primitives**:
- Agent, Team, Teammate, Task, Message definitions
- Directory structures (`~/.claude/teams/`, `~/.claude/tasks/`)

**Two Spawn Methods**:
- Subagent (Task only) - synchronous/async, no team membership
- Teammate (Task + team_name + name) - joins team, persistent

**Built-in Agent Types**:
| Type | Tools | Model | Use Case |
|------|-------|-------|----------|
| Bash | Shell only | Inherits | Git, system tasks |
| Explore | Read-only | Haiku | Codebase search |
| Plan | Read-only | Inherits | Architecture |
| general-purpose | All | Inherits | Multi-step work |
| claude-code-guide | Read + Web | Default | Claude Code help |
| statusline-setup | Read/Edit | Sonnet | Status config |

**TeammateTool Operations** (all 13 with schemas):
```javascript
Teammate({ operation: "spawnTeam", team_name: "x" })
Teammate({ operation: "discoverTeams" })
Teammate({ operation: "requestJoin", team_name: "x", proposed_name: "y" })
Teammate({ operation: "approveJoin", target_agent_id: "y", request_id: "..." })
Teammate({ operation: "write", target_agent_id: "y", value: "..." })
Teammate({ operation: "broadcast", name: "lead", value: "..." })
Teammate({ operation: "requestShutdown", target_agent_id: "y", reason: "..." })
Teammate({ operation: "approveShutdown", request_id: "..." })
Teammate({ operation: "approvePlan", target_agent_id: "y", request_id: "..." })
Teammate({ operation: "cleanup" })
```

**Task System**:
```javascript
TaskCreate({ subject: "...", description: "...", activeForm: "..." })
TaskList()
TaskGet({ taskId: "2" })
TaskUpdate({ taskId: "2", owner: "worker-1", status: "in_progress" })
TaskUpdate({ taskId: "3", addBlockedBy: ["1", "2"] })
```

**Directory Structure**:
```
~/.claude/teams/{team-name}/
├── config.json
└── inboxes/{agent}.json

~/.claude/tasks/{team-name}/
├── 1.json
├── 2.json
└── ...
```

**Team Config Schema**:
```json
{
  "name": "project-name",
  "leadAgentId": "team-lead@project",
  "members": [
    {
      "agentId": "worker-1@project",
      "name": "worker-1",
      "agentType": "general-purpose",
      "model": "haiku",
      "color": "#D94A4A",
      "backendType": "in-process",
      "tmuxPaneId": "in-process",
      "cwd": "/project/path"
    }
  ]
}
```

**Message Types** (13 types with full schemas)

**Environment Variables** (auto-provided):
```
CLAUDE_CODE_TEAM_NAME
CLAUDE_CODE_AGENT_ID
CLAUDE_CODE_AGENT_NAME
CLAUDE_CODE_AGENT_TYPE
CLAUDE_CODE_AGENT_COLOR
CLAUDE_CODE_PLAN_MODE_REQUIRED
CLAUDE_CODE_PARENT_SESSION_ID
```

**Spawn Backends**: in-process, tmux, iterm2, auto-detection

**Orchestration Patterns**:
- Parallel Specialists
- Pipeline (Sequential)
- Swarm (Self-organizing)
- Research + Implement
- Plan Approval

**Task File Schema**:
```json
{
  "id": "1",
  "subject": "...",
  "description": "...",
  "status": "in_progress",
  "owner": "worker-1",
  "activeForm": "...",
  "blockedBy": [],
  "blocks": ["3"],
  "createdAt": 1706000000000,
  "updatedAt": 1706000001000
}
```

**Critical Constraints**:
- Workers must use `Teammate({ operation: "write" })` for team communication
- `broadcast` sends N messages to N teammates
- `cleanup` fails if teammates active
- Crashed teammates have 5-minute heartbeat timeout

**Best Practices**:
1. Always call `cleanup` after shutdown approval
2. Use meaningful agent names
3. Write explicit step-by-step prompts
4. Leverage task dependencies over polling
5. Check inboxes for results
6. Match agent type to task scope
7. Prefer targeted `write` over `broadcast`

**2. Kieran Klaassen - Alternative Orchestration**
- URL: https://gist.github.com/kieranklaassen/d2b35569be2c7f1412c64861a219d51f
- Title: "Claude Code Multi-Agent Orchestration System"
- Likely earlier or alternative version of gist #1

**3. ruvnet - Architectural Comparison**
- URL: https://gist.github.com/ruvnet/18dc8d060194017b989d1f8993919ee4
- Title: "Architectural Comparison: Claude Flow V3 vs Claude Code TeammateTool"
- Compares different orchestration approaches

### Compiler Project Artifacts (NOT Open Sourced)

**NOT RELEASED**:
- ❌ Compiler source code (100,000 lines of Rust)
- ❌ Git repository with commit history
- ❌ Test harness code (GCC oracle strategy)
- ❌ Agent prompts and configuration
- ❌ Docker setup scripts
- ❌ Performance benchmarks
- ❌ Agent logs or transcripts
- ❌ README templates for orientation

**Why Not Released**:
1. **Research demonstration** - Goal was proving feasibility, not distributing tool
2. **Quality concerns** - Generated code less efficient than GCC
3. **Incomplete features** - 16-bit x86 code generation still uses GCC
4. **Competitive reasons** - Anthropic's proprietary research insights
5. **Maintenance burden** - Would require ongoing support and updates

**What Was Disclosed**:
- ✅ High-level architecture description
- ✅ Token usage and cost ($20K, 2B input, 140M output)
- ✅ Validation metrics (99.1% GCC torture test pass rate)
- ✅ Real-world compilation results (Linux 6.9, QEMU, FFmpeg, etc.)
- ✅ What worked and what failed
- ✅ Git synchronization strategy (file locks, pull-merge-push)
- ✅ Docker containerization approach

**Access to Related Tools**:
- ✅ Claude Code CLI (public download)
- ✅ Agent SDK (fully open source)
- ✅ Agent Teams documentation (public)
- ✅ Community gists and guides

---

## Sources

- [Building a C compiler with a team of parallel Claudes](https://www.anthropic.com/engineering/building-c-compiler)
- [Orchestrate teams of Claude Code sessions - Claude Code Docs](https://code.claude.com/docs/en/agent-teams)
- [Hooks reference - Claude Code Docs](https://code.claude.com/docs/en/hooks)
- [AddyOsmani.com - Claude Code Swarms](https://addyosmani.com/blog/claude-code-agent-teams/)
- [Claude Code's Hidden Multi-Agent System](https://paddo.dev/blog/claude-code-hidden-swarm/)
- [Claude Code Swarm Orchestration Skill - Kieran Klaassen Gist](https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea)
- [We tasked Opus 4.6 using agent teams to build a C Compiler | Hacker News](https://news.ycombinator.com/item?id=46903616)
- [Claude Code's new hidden feature: Swarms | Hacker News](https://news.ycombinator.com/item?id=46743908)
- [Anthropic's $20,000 Experiment](https://www.webpronews.com/anthropics-20000-experiment-how-16-parallel-ai-agents-built-a-100000-line-c-compiler-from-scratch-in-rust/)
- [Anthropic releases Opus 4.6 with new agent teams | TechCrunch](https://techcrunch.com/2026/02/05/anthropic-releases-opus-4-6-with-new-agent-teams/)
- [GitHub - anthropics/claude-agent-sdk-python](https://github.com/anthropics/claude-agent-sdk-python)
- [GitHub - anthropics/claude-agent-sdk-typescript](https://github.com/anthropics/claude-agent-sdk-typescript)
- [GitHub - anthropics/claude-code](https://github.com/anthropics/claude-code)
- [Release v2.1.33 · anthropics/claude-code](https://github.com/anthropics/claude-code/releases/tag/v2.1.33)
- [GitHub - hesreallyhim/awesome-claude-code](https://github.com/hesreallyhim/awesome-claude-code)
- [GitHub - MaTriXy/claude-swarm-orchestration](https://github.com/MaTriXy/claude-swarm-orchestration)
- [GitHub - Piebald-AI/claude-code-system-prompts](https://github.com/Piebald-AI/claude-code-system-prompts)
- [Claude Code Multi-Agent Orchestration System - Kieran Klaassen Gist](https://gist.github.com/kieranklaassen/d2b35569be2c7f1412c64861a219d51f)
- [Architectural Comparison - ruvnet Gist](https://gist.github.com/ruvnet/18dc8d060194017b989d1f8993919ee4)
- [How Claude Code Got Better by Protecting More Context](https://hyperdev.matsuoka.com/p/how-claude-code-got-better-by-protecting)
- [Claude Opus 4.6 adds adaptive thinking - Laravel News](https://laravel-news.com/claude-opus-4-6)
- [Best Practices for Claude Code](https://code.claude.com/docs/en/best-practices)
