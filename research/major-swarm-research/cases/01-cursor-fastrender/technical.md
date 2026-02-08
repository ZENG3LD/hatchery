# Cursor FastRender Project - Technical Deep Dive

## 1. Prompts & Prompting Strategy

### Prompting Philosophy

> "A surprising amount of the system's behavior comes down to how we prompt the agents. The harness and models matter, but the prompts matter more." - Cursor Blog

Source: [Cursor Blog - Scaling Agents](https://cursor.com/blog/scaling-agents)

### Core Prompting Principles

#### 1. Constraints Over Instructions

**Principle**: Telling agents what NOT to do is more effective than telling them what TO do

**Why It Works**: Prevents scope creep, reduces ambiguity, creates clear boundaries

**Examples from FastRender AGENTS.md**:

```markdown
# Non-Negotiables (Absolute Constraints)

1. No page-specific hacks
   - FORBIDDEN: hostname/selector special-cases
   - FORBIDDEN: magic numbers for individual sites

2. No pixel nudging
   - MUST: maintain staged pipeline (parse → style → box tree → layout → paint)
   - FORBIDDEN: layout tweaks in paint phase

3. No panics
   - MUST: return errors cleanly
   - MUST: bound all work

4. No partial implementations
   - FORBIDDEN: TODO comments in production code
   - MUST: complete implementations or nothing

5. JavaScript must be bounded
   - MUST: interrupts/timeouts
   - FORBIDDEN: unbounded allocations
```

Source: [FastRender AGENTS.md](https://github.com/wilsonzlin/fastrender/blob/main/AGENTS.md), [Cursor Blog - Agent Best Practices](https://cursor.com/blog/agent-best-practices)

#### 2. Concrete Quantification

**Principle**: Give specific numbers and ranges when discussing scope

**Why It Works**: Prevents agents from under/over-scoping work

**Examples**:
- "90% accuracy + capability, 10% performance + infra" (explicit ratio)
- "timeout -k 10 600" (specific time limits: 10 min max, 10 sec kill grace)
- "up to 20 worktrees" (specific resource limit)
- "target/ must not exceed 400GB" (explicit disk threshold)

Source: [FastRender AGENTS.md](https://github.com/wilsonzlin/fastrender/blob/main/AGENTS.md)

#### 3. Avoid Checkbox Mentality

**Principle**: High-level tasks should not be treated as mechanical checklists

**Why It Works**: Prevents agents from completing tasks without understanding context

**Implication**: Tasks should require judgment and understanding, not just mechanical completion

Source: [Cursor Blog - Agent Best Practices](https://cursor.com/blog/agent-best-practices)

### Actual Prompts & Instructions

#### AGENTS.md - The Master Prompt

The complete AGENTS.md file served as the root prompt for all agents. Key sections:

**Workstreams** (parallel tracks):
```markdown
Pick one workstream, follow its doc. Work proceeds in parallel across all:

1. Rendering engine: Capability buildout (spec-first primitives)
2. Rendering engine: Pageset page loop (fix pages one-by-one)
3. Browser application: Browser chrome (tabs, navigation, address bar)
4. Browser application: Browser responsiveness (performance, not aesthetics)
5. Browser application: Browser page interaction (forms, focus, scrolling)
6. JavaScript support: JS engine (vm-js core, execution, GC)
7. JavaScript support: JS DOM bindings (document, element, events)
8. JavaScript support: JS Web APIs (fetch, URL, timers, storage)
9. JavaScript support: JS HTML integration (script loading, modules, event loop)
```

**Philosophy**:
```markdown
Correct pixels are the product.

Everything else (perf infra, testing, docs, tooling) exists to help ship correct pixels faster.

90/10 rule: Target 90% accuracy + capability, 10% performance + infra.

Data-driven method: Inject, trace, collect, understand, systematize.

Priority order:
1. Panics (crashes)
2. Timeouts (infinite loops)
3. Accuracy failures (wrong output)
4. Hotspots (performance)
5. Polish (UX)
6. Spec expansion (new features)
```

**What Counts as Acceptable Work**:
```markdown
Changes must achieve at least one:
- New capability with regression test
- Bugfix with regression test
- Stability improvement (crash eliminated)
- Termination fix (timeout/loop eliminated)
- New WPT/fixture coverage
- User-visible UX improvement

EXCLUDED WORK:
- Tooling-only changes
- Perf-only changes
- Docs-only changes
(unless directly enabling a measurable win)
```

Source: [FastRender AGENTS.md](https://github.com/wilsonzlin/fastrender/blob/main/AGENTS.md)

#### System Resource Safety - Mandatory Constraints

**Cardinal Rule**:
```markdown
Assume everything can misbehave.

Any code path can devolve into:
- Infinite loops
- Exponential blowups
- Memory explosions
- Deadlocks
- Signal-ignoring hangs

This isn't paranoia — it's operational reality.
```

**External Limits** (non-negotiable):

```bash
# Time limits
timeout -k 10 600 [command]  # 10 min max, 10 sec SIGKILL grace

# Memory limits
bash scripts/run_limited.sh --as 64G -- [command]

# Scope limits
# NEVER: cargo test (runs all)
# ALWAYS: cargo test -p <crate> --test <name>
```

**Forbidden Commands**:
```markdown
FORBIDDEN (no exceptions):
- cargo build/test/check without wrapper scripts
- cargo test without -p <crate>, --test <name>, or --lib
- --all-features or --all-targets flags
- Commands compiling 100+ targets

MANDATORY:
- Always use bash scripts/cargo_agent.sh
- Scope tests: -p, --test, --lib, or --bin
- Wrap with timeout -k 10 600 (SIGKILL fallback essential)
```

Source: [FastRender AGENTS.md](https://github.com/wilsonzlin/fastrender/blob/main/AGENTS.md)

#### Test Organization - Mandatory Structure

```markdown
# Unit Tests: src/
Embedded in source files using #[cfg(test)] modules.
Run with: cargo test --lib

# Integration Tests: tests/
ONLY for:
- Public API testing
- Data-driven runners (fixtures, WPT)
- Allocation-failure harness (custom allocator)

STRICT RULES:
- Exactly 2 integration binaries: tests/integration.rs and tests/allocation_failure.rs
- NEVER create additional tests/*.rs files
- NEVER use #[path = ...] shims
- Filter individual tests using standard test filters

Run with: cargo test --test integration
```

Source: [FastRender AGENTS.md](https://github.com/wilsonzlin/fastrender/blob/main/AGENTS.md)

### Prompting Strategy by Role

#### Root Planner Prompts

**Goal**: Understand entire project scope and decompose into manageable sub-tasks

**Inferred Prompt Structure** (NOT DISCLOSED - inferred from behavior):

```markdown
You are the Root Planner for a browser rendering engine.

Your responsibilities:
1. Understand current project state
2. Identify gaps toward goal completion
3. Create specific, targeted tasks
4. Spawn sub-planners for complex domains

Guidelines:
- Reference concrete code entities (files, functions, tables)
- Avoid abstract descriptions
- Ensure tasks are scoped to complete in reasonable time
- Continuously explore codebase for opportunities
- [CONSTRAINTS FROM AGENTS.MD]
```

#### Sub-Planner Prompts

**Goal**: Domain-specific planning for areas like CSS, JavaScript, Layout

**Inferred Prompt Structure**:

```markdown
You are a Sub-Planner for [DOMAIN: CSS Rendering].

Your responsibilities:
1. Break down domain-specific work into worker tasks
2. Ensure tasks align with specifications ([SPEC REFERENCE])
3. Create tasks that workers can complete independently
4. Monitor progress and adjust plans based on worker feedback

Constraints:
- [DOMAIN-SPECIFIC CONSTRAINTS]
- [SHARED CONSTRAINTS FROM AGENTS.MD]
```

#### Worker Prompts

**Goal**: Execute specific tasks without broader project concern

**Inferred Prompt Structure**:

```markdown
You are a Worker agent.

Your task: [SPECIFIC TASK DESCRIPTION]

Your responsibilities:
1. Complete the assigned task
2. Work on your isolated repository copy
3. Push changes when complete
4. Write detailed handoff document

DO NOT:
- Coordinate with other workers
- Consider broader project scope
- Make decisions outside task scope

Handoff document must include:
- Notes on implementation
- Concerns encountered
- Deviations from plan
- Findings during work
- Thoughts on approach
- Feedback for planner

[CONSTRAINTS FROM AGENTS.MD]
```

#### Judge Prompts

**Goal**: Evaluate project completion

**Inferred Prompt Structure**:

```markdown
You are the Judge agent.

Your responsibility: Determine if the project is complete.

Evaluation criteria:
- [PROJECT GOALS]
- [COMPLETION REQUIREMENTS]
- [QUALITY THRESHOLDS]

Output: CONTINUE or COMPLETE with rationale
```

**Note**: Exact prompts NOT DISCLOSED by Cursor.

### Domain Knowledge in Prompts

**Specification References**: Agents explicitly reference specs in code

**Evidence from Codebase**:
- Specifications included as git submodules:
  - `csswg-drafts` (CSS specifications)
  - `tc39-ecma262` (JavaScript specifications)
  - `whatwg` standards (HTML/DOM specifications)
- Code comments reference spec sections directly

**Example Pattern** (inferred from description):
```rust
// Implements CSS Cascade Level 4, Section 6.2: Cascade Sorting
// Spec: https://www.w3.org/TR/css-cascade-4/#cascade-sort
fn sort_declarations(...) {
    // Implementation
}
```

**Visual Feedback**: Screenshot comparisons against golden samples

Source: [Simon Willison](https://simonwillison.net/2026/Jan/23/fastrender/)

---

## 2. Memory & Context Management

### Context Window Size

**Model Used**: OpenAI GPT-5.2

**Context Window**: NOT EXPLICITLY DISCLOSED

**Inferred Size**: 128K-200K tokens (based on GPT-5 series capabilities)

**Per-Agent Context**: Each agent maintains separate context window

Source: NOT DIRECTLY DISCLOSED

### Compaction Strategy

#### Scratchpad Rewriting

**Method**: Rewrite scratchpad rather than append

**Why**: Prevents unbounded growth and drift

> "Scratchpad rewriting (versus appending)" - Cursor Blog on freshness mechanisms

**Structure**: `.cursor/scratchpad.md` rewritten each cycle

Source: [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases)

#### Automatic Context Summarization

**Method**: NOT FULLY DISCLOSED

**Evidence**: "Automatic context summarization" mentioned as freshness mechanism

**Likely Implementation**: Periodic summarization of long-running context into shorter form

Source: [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases)

#### Self-Reflection Prompts

**Method**: Agents prompted to reflect on their progress and context

**Purpose**: Maintain focus and avoid drift

**Evidence**: "Self-reflection prompts" listed as freshness mechanism

Source: [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases)

### Scratchpad Format

**File**: `.cursor/scratchpad.md`

**Purpose**: Decision point and communication hub

**Structure** (from community implementations, NOT official Cursor format):

```markdown
# Scratchpad

## Background and Motivation
[Established by Planner initially]
- Project goals
- Context for work
- High-level requirements

## Key Challenges and Analysis
[Established by Planner initially]
- Technical challenges identified
- Analysis of problem space
- Considerations and constraints

## High-level Task Breakdown
[Step-by-step implementation plan]
1. [Task 1]
   - Subtask 1.1
   - Subtask 1.2
2. [Task 2]
   - Subtask 2.1
   - Subtask 2.2

## Project Status Board
[Mainly filled by Executor/Worker]
- [x] Completed task 1
- [ ] In progress task 2
- [ ] Pending task 3

## Executor's Feedback or Assistance Requests
[Worker updates, Planner reviews]
- Issue encountered: [description]
- Request: [what's needed]
- Observation: [insights]

## Rules & Tips
[Accumulating learnings across tasks]
- Lesson learned from Task 1: [insight]
- Best practice identified: [pattern]
```

**Communication Protocol**:
- Planner and Executor communicate by writing to or modifying scratchpad.md
- Each role has designated sections to update
- Changes tracked via file modifications

**Completion Detection**:
```bash
# Scripts can read scratchpad and check for markers
if grep -q "DONE" .cursor/scratchpad.md; then
  echo "Task complete"
fi
```

Source: [Cursor Forum - Scratchpad Format](https://forum.cursor.com/t/rules-for-ultra-context-memories-lessons-scratchpad-with-plan-and-act-modes/48792)

### Fresh Starts vs Continuous Context

**Strategy**: **Hybrid Approach**

**Fresh Starts**:
- Each worker starts with clean context for their specific task
- Scratchpad rewritten (not appended) between major phases
- Workers isolated in separate git worktrees (physical separation)

**Continuous Context**:
- Planners maintain longer-term context across cycles
- Handoff documents carry forward important information
- Scratchpad accumulates "Rules & Tips" section with learnings

**Freshness Mechanisms**:
1. Scratchpad rewriting
2. Automatic context summarization
3. Self-reflection prompts

Source: [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases)

### Inter-Agent Shared State

**Git Repository**: Primary shared state

**What's Shared**:
- Codebase (via git)
- Commit history
- Specifications (git submodules)
- AGENTS.md rules
- Test fixtures and results

**What's NOT Shared**:
- Context windows (each agent independent)
- Working directories (isolated worktrees)
- In-progress work (until pushed)

**Synchronization Point**: Git commits and merges

**Conflict Resolution**: At push time, not during development

Source: [Cursor Docs - Worktrees](https://cursor.com/docs/configuration/worktrees)

### Memory Persistence

**NOT DISCLOSED**: Exact mechanism for long-term memory across agent restarts

**Inferred Mechanisms**:
- Git commit history as persistent memory
- Scratchpad.md as session state
- Handoff documents as knowledge transfer
- AGENTS.md rules as immutable memory

---

## 3. Task Distribution & Scheduling

### Pull vs Push

**Architecture**: **Pull-Based** for workers

**Evidence**:
- "Workers pick up tasks" (implies pulling from queue)
- No mention of push-based assignment
- Parallel workers suggest task pool

**Planner Behavior**: **Push** tasks to queue

**Judge Behavior**: **Pull** entire project state for evaluation

Source: [Cursor Blog - Scaling Agents](https://cursor.com/blog/scaling-agents)

### Task Format

**Structure**: NOT FULLY DISCLOSED

**Known Elements**:

```markdown
Task Specification (inferred structure):

## Task ID
[unique identifier]

## Workstream
[CSS Rendering | JS Engine | Layout | etc.]

## Scope
- File(s): [specific file paths]
- Function(s): [specific function names]
- Table(s): [specific data structures]

## Goal
[Concrete, measurable objective]

## Constraints
- [Relevant constraints from AGENTS.MD]
- [Domain-specific requirements]

## References
- Specification: [URL to relevant spec section]
- Related code: [file paths]
- Test coverage: [test file paths]

## Acceptance Criteria
- [ ] Criterion 1
- [ ] Criterion 2
- [ ] Regression test added
```

**Key Characteristic**: "Concrete code entities (tables, files, functions) rather than abstract descriptions"

Source: [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases)

### Dependency Tracking

**Method**: NOT EXPLICITLY DISCLOSED

**Evidence of Dependency Awareness**:
- "The harness itself is able to quite effectively split out and divide the scope and tasks such that it tries to minimize the amount of overlap of work"
- Most commits had no merge conflicts

**Inferred Mechanism**:
- Planners analyze codebase dependencies
- Tasks assigned to minimize file overlap
- Some overlap intentionally allowed ("moments of turbulence")

**Pragmatic Dependency Handling**:
> "One agent knew that other agents were working on the JavaScript engine, and it needed to unblock itself quickly by pulling QuickJS as temporary solution"

This shows agents could make short-term dependency decisions independently.

Source: [Simon Willison](https://simonwillison.net/2026/Jan/23/fastrender/)

### Failure Handling

#### Error Tolerance Philosophy

**Principle**: Accept "a small but stable rate of errors"

**Why**: Prevents serialization bottlenecks from requiring 100% correctness

**Result**: "Overall system continues to make progress at really high throughput"

**Self-Correction**: "Subsequent commits fix API changes and syntax errors quickly"

#### Rust Compiler as Validator

**Continuous Verification**: "The agents were constantly compiling it using the Rust compiler and fixing any compile errors as they occurred"

**Tight Feedback Loop**:
1. Agent writes code
2. Compiler flags errors
3. Agent fixes errors
4. Repeat until clean compile

#### Retry Mechanism

**NOT DISCLOSED**: Exact retry logic not specified

**Evidence of Retry**:
- Agents fix their own compilation errors
- High failure rate in CI (agents kept trying until success)

#### Failure Escalation

**NOT DISCLOSED**: No information on when/how failures escalate to planners

Source: [Simon Willison](https://simonwillison.net/2026/Jan/23/fastrender/), [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases)

### Locking & Concurrency Control

#### Abandoned Approach: Explicit Locking

**Initial Attempt**: Shared files with locking mechanism

**Failure**: "Twenty agents would slow down to the effective throughput of two or three, with most time spent waiting"

**Result**: Abandoned in favor of optimistic concurrency

#### Optimistic Concurrency Control

**Second Attempt**: No locks, conflicts resolved at merge time

**Partial Success**: Reduced lock contention

**Issue**: Agents became risk-averse without hierarchy

#### Final Approach: Isolated Worktrees + Merge Conflicts

**Success**: Each agent works in isolated git worktree

**Concurrency Model**:
- No locking during development
- Conflicts detected at push time
- Workers resolve conflicts when merging
- "Moments of turbulence" allowed to converge naturally

> "Hundreds of workers run concurrently, pushing to the same branch with minimal conflicts" - Cursor Blog

**Why It Worked**: "The harness itself is able to quite effectively split out and divide the scope and tasks such that it tries to minimize the amount of overlap of work"

Source: [Cursor Blog - Scaling Agents](https://cursor.com/blog/scaling-agents), [Simon Willison](https://simonwillison.net/2026/Jan/23/fastrender/)

### Load Balancing

**Method**: NOT EXPLICITLY DISCLOSED

**Evidence**:
- Peak 2,000 concurrent agents implies dynamic scaling
- Multiple VMs with ~300 agents each suggests load distribution
- "Hundreds of concurrent agents" maintained throughout

**Inferred Strategy**:
- Workers pull tasks from shared queue (natural load balancing)
- Task granularity sized for reasonable completion time
- No mention of agent starvation or overload

Source: [Simon Willison](https://simonwillison.net/2026/Jan/23/fastrender/)

---

## 4. Validation & Quality Control

### Work Validation Methods

#### 1. Rust Compiler Verification

**Primary Validator**: Type system and borrow checker

**Continuous Feedback**: "The agents were constantly compiling it using the Rust compiler and fixing any compile errors as they occurred"

**Benefits**:
- Immediate syntax and type error detection
- Memory safety guarantees from borrow checker
- Structural constraints from type system

#### 2. Specification Compliance

**Reference Materials**: Git submodules with official specs
- csswg-drafts (CSS specifications)
- tc39-ecma262 (JavaScript specifications)
- whatwg standards (HTML/DOM specifications)

**Validation Method**: Code comments explicitly reference spec sections

**Objective Criteria**: Clear definition of "correct" behavior from specs

#### 3. Visual Regression Testing

**Method**: Screenshot comparisons against golden samples

**Purpose**: Verify rendering output matches expected pixels

> "Correct pixels are the product" - FastRender Philosophy

#### 4. Test Suite Execution

**Continuous Testing**: Agents run tests after changes

**Test Types**:
- Unit tests in source files (`cargo test --lib`)
- Integration tests (`cargo test --test integration`)
- WPT (Web Platform Tests) subset coverage

#### 5. Handoff Document Review

**Planner Validation**: Planners review worker handoff documents

**Contents Validated**:
- Did work meet task goals?
- Were constraints respected?
- What issues were encountered?
- What feedback for future work?

Source: [Simon Willison](https://simonwillison.net/2026/Jan/23/fastrender/), [FastRender AGENTS.md](https://github.com/wilsonzlin/fastrender/blob/main/AGENTS.md)

### Quality Gates

#### Removed: Integrator Role

**Attempted**: Dedicated integrator agent to review and merge work

**Result**: "Removing an integrator role improved performance"

**Reason**: Created more bottlenecks than it solved

#### Current Quality Gates

**1. Compilation Gate**:
- Code must compile before commit
- Enforced by continuous compilation

**2. Test Gate**:
- Changes must include regression tests
- From AGENTS.md: "Add the regression first, then implement the fix"

**3. Constraint Compliance Gate**:
- Non-negotiables from AGENTS.md must be satisfied
- Enforced by prompts and wrapper scripts

**4. Acceptable Work Definition**:

Must achieve at least one:
- New capability with regression test
- Bugfix with regression test
- Stability improvement (crash eliminated)
- Termination fix (timeout/loop eliminated)
- New WPT/fixture coverage
- User-visible UX improvement

Source: [FastRender AGENTS.md](https://github.com/wilsonzlin/fastrender/blob/main/AGENTS.md)

### Self-Correction Loops

#### Error Detection and Fix Cycle

**Pattern**:
1. Agent writes code
2. Compiler/tests detect errors
3. Agent analyzes errors
4. Agent fixes errors
5. Repeat until clean

**Continuous**: "The agents were constantly compiling it using the Rust compiler and fixing any compile errors as they occurred"

#### Convergence Strategy

**Tolerance**: Accept "small but stable rate of errors"

**Natural Convergence**: "Moments of turbulence" allowed; subsequent commits fix issues

**Example**: API changes in one commit cause compile errors, fixed by later commits

#### Feedback Incorporation

**Worker to Planner**: Handoff documents provide feedback loop

**Planner Adaptation**: Planners adjust future tasks based on worker feedback

**Learning**: "Rules & Tips" section in scratchpad accumulates insights

Source: [Simon Willison](https://simonwillison.net/2026/Jan/23/fastrender/), [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases)

### Feedback Format

#### Handoff Document Structure

**Required Elements** (from blog post):
- **Notes**: Implementation details and decisions
- **Concerns**: Issues or risks identified
- **Deviations**: Any divergence from original plan
- **Findings**: Discoveries during implementation
- **Thoughts**: Reflections on approach
- **Feedback**: Suggestions for planner

**Format**: NOT DISCLOSED (likely Markdown)

**Destination**: Submitted to planner for review

> "When done, they write up a single handoff that the system submits to the planner" - Cursor Blog

Source: [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases)

---

## 5. Mailbox / Inbox Implementation

### Storage Type

**Primary Communication**: Git commits and git worktrees

**Scratchpad**: `.cursor/scratchpad.md` file in repository

**Handoff Documents**: NOT DISCLOSED (likely files or database entries)

**Task Queue**: NOT DISCLOSED (likely in-memory or database)

### Path & Location

#### Worktrees

**Location**: `~/.cursor/worktrees/<repo>/`

**Structure**:
```
~/.cursor/worktrees/
├── fastrender/
│   ├── agent-001-css-cascade/     # Worker 1's worktree
│   ├── agent-002-layout-flex/     # Worker 2's worktree
│   └── agent-003-js-parser/       # Worker 3's worktree
```

**Management**: Automatic creation and cleanup

**Limit**: Up to 20 worktrees per workspace

**Cleanup**: Every 6 hours based on access time

Source: [Cursor Docs - Worktrees](https://cursor.com/docs/configuration/worktrees)

#### Scratchpad

**Location**: `.cursor/scratchpad.md` (in repository root)

**Access**: All agents read/write via file system

**Synchronization**: File-based (potential race conditions managed by git)

### Message Types

#### 1. Task Assignment Messages

**Direction**: Planner → Worker

**Content**:
- Task specification
- Scope and constraints
- Acceptance criteria
- Relevant references

**Format**: NOT DISCLOSED

#### 2. Handoff Documents

**Direction**: Worker → Planner

**Content**:
- Notes, concerns, deviations
- Findings, thoughts, feedback
- Completion status

**Format**: Structured text (likely Markdown)

#### 3. Git Commits

**Direction**: Worker → Repository → Other Agents

**Content**:
- Code changes
- Commit message
- Diff information

**Format**: Standard git commit

#### 4. Scratchpad Updates

**Direction**: Bidirectional (Planner ↔ Worker)

**Content**:
- Project status
- Task breakdown
- Feedback and requests
- Accumulated learnings

**Format**: Markdown file

### Schema

**NOT DISCLOSED**: Exact message schema not published

**Inferred Handoff Schema**:

```json
{
  "task_id": "string",
  "agent_id": "string",
  "status": "completed | failed | partial",
  "notes": "string",
  "concerns": ["string"],
  "deviations": ["string"],
  "findings": ["string"],
  "thoughts": "string",
  "feedback": "string",
  "commits": ["commit-hash-1", "commit-hash-2"],
  "tests_added": ["test-file-path"],
  "timestamp": "iso8601-datetime"
}
```

**Note**: This is INFERRED, not actual disclosed schema.

### Delivery Guarantee

**Git-Based Messages**: **Eventually Consistent**

**Why**: Multiple agents pushing to same branch

**Conflict Resolution**: At push time

**Success Guarantee**: Retry until successful push (inferred from high commit count)

**Lost Messages**: NOT ADDRESSED in public documentation

### Polling vs Event-Driven

**Architecture**: **Likely Hybrid**

**Git-Based**: Polling (agents periodically pull changes)

**Task Queue**: Likely event-driven (workers notified of available tasks)

**Scratchpad**: File-watching or polling (NOT DISCLOSED)

**Evidence for Event-Driven**:
- High throughput (thousands of commits/hour) suggests efficient notification
- "Workers pick up tasks" implies notification mechanism

**Evidence for Polling**:
- Git inherently poll-based
- Simpler implementation

**Reality**: Likely event-driven for task assignment, polling for git state

### Inter-Agent Communication Protocol

**Direct Communication**: **EXPLICITLY FORBIDDEN** between workers

> "Workers do NOT coordinate with other workers" - Cursor Blog

**Allowed Communication Paths**:
- Worker → Git → Other Workers (indirect, via commits)
- Worker → Planner (via handoff documents)
- Planner → Worker (via task assignments)
- Planner → Sub-Planner (spawning)
- Judge → All (read-only evaluation)

**Mailbox Model**: Asynchronous message passing

**No Synchronous RPC**: All communication asynchronous

Source: [Cursor Blog - Scaling Agents](https://cursor.com/blog/scaling-agents)

---

## 6. Open Source Artifacts & Code

### Primary Repository

#### FastRender Browser Engine

**URL**: https://github.com/wilsonzlin/fastrender

**Description**: Complete browser rendering engine generated by agent swarm

**License**: NOT EXPLICITLY STATED in README

**Stars**: 1,400+ (as of Jan 2026)

**Forks**: 101

**Commits**: ~29,858 commits (note: this is total, including pre-swarm development)

**Languages**: Rust (primary), Shell scripts

**Structure**:
```
fastrender/
├── AGENTS.md                    # Master prompt and constraints
├── docs/                        # Internal documentation
│   ├── philosophy.md
│   ├── triage.md
│   ├── ecma_rs_ownership.md
│   ├── test_architecture.md
│   └── webidl_stack.md
├── instructions/                # Workstream-specific guides
├── progress/pages/              # Progress tracking
├── scripts/                     # Safety wrappers
│   ├── cargo_agent.sh
│   └── run_limited.sh
├── src/                         # Source code
│   ├── parsing/
│   ├── styling/
│   ├── layout/
│   ├── text/
│   ├── painting/
│   ├── javascript/
│   ├── ui/
│   ├── sandbox/
│   └── networking/
├── tests/                       # Test suite
│   ├── integration.rs
│   └── allocation_failure.rs
├── vendor/                      # Vendored dependencies
│   ├── ecma-rs/                 # JavaScript engine
│   └── taffy/                   # Layout library
└── Cargo.toml
```

**Relevance**: 100% - This IS the output of the swarm experiment

**Status**: Active development, unstable APIs, not production-ready

**Build Instructions**:
```bash
git submodule update --init vendor/ecma-rs
cargo run --release --features browser_ui --bin browser
```

**Known Issues**:
- Only ~2.3% CI success rate (1,426/63,295 workflow runs)
- 34+ compilation errors reported by users
- 94 warnings
- Performance issues on complex sites

Source: [FastRender GitHub](https://github.com/wilsonzlin/fastrender)

### Key Configuration Files

#### AGENTS.md - Master Prompt

**URL**: https://github.com/wilsonzlin/fastrender/blob/main/AGENTS.md

**Description**: Complete rule set governing all agent behavior

**Format**: Markdown

**Size**: ~4,000 words (estimated)

**License**: Implicitly same as repository

**Relevance**: 100% - This IS the actual prompt used

**Key Sections**:
1. Workstreams
2. Non-Negotiables (6 absolute constraints)
3. Philosophy & Culture
4. What Counts as Acceptable Work
5. System Resource Safety (mandatory limits)
6. Regression Testing Philosophy
7. Test Organization (mandatory structure)
8. Reference Documentation

**Complete Contents**: See Section 1 of this document for detailed breakdown

**Significance**: One of the few actual disclosed prompts for a production agent swarm

Source: [FastRender AGENTS.md](https://github.com/wilsonzlin/fastrender/blob/main/AGENTS.md)

#### Safety Wrapper Scripts

**Files**:
- `scripts/cargo_agent.sh` - Wrapper for cargo commands with timeout and resource limits
- `scripts/run_limited.sh` - Memory-limited execution wrapper

**Purpose**: Enforce resource constraints on all agent operations

**Example Usage**:
```bash
#!/bin/bash
# scripts/cargo_agent.sh
timeout -k 10 600 bash scripts/run_limited.sh --as 64G -- cargo "$@"
```

**Relevance**: Infrastructure to prevent agent runaway processes

### Documentation Files

#### Philosophy Document

**Path**: `docs/philosophy.md` (in repository)

**Content**: "Correct pixels are the product" philosophy

**Relevance**: Core guiding principle for all agent work

#### Triage Document

**Path**: `docs/triage.md` (in repository)

**Content**: Priority ordering system

**Priority Order**:
1. Panics (crashes)
2. Timeouts (infinite loops)
3. Accuracy failures (wrong output)
4. Hotspots (performance)
5. Polish (UX)
6. Spec expansion (new features)

#### Test Architecture

**Path**: `docs/test_architecture.md` (in repository)

**Content**: Test infrastructure design and rationale

### Vendored Dependencies

#### ecma-rs JavaScript Engine

**Path**: `vendor/ecma-rs/` (git submodule)

**Description**: Custom JavaScript runtime vendored in repository

**Relevance**: Shows dependency management for agent-generated code

**Documentation**: `docs/ecma_rs_ownership.md`

#### Taffy Layout Library

**Path**: `vendor/taffy/` (vendored, not submodule)

**Description**: Flexbox and Grid layout engine

**Constraint**: "Keep Taffy vendored - use only vendor/taffy/ for flex/grid; no Cargo updates"

**Reason**: Prevent agents from updating dependencies and breaking working code

### Test Fixtures

**Location**: Repository contains test fixtures and Web Platform Test (WPT) subset

**Purpose**: Regression testing and validation

**Format**: HTML files, expected output screenshots

### Specification Submodules

**Git Submodules**:
- csswg-drafts (CSS specifications)
- tc39-ecma262 (JavaScript specifications)
- whatwg standards (HTML/DOM specifications)

**Purpose**: Agents reference specifications directly during implementation

**Integration**: Code comments cite spec sections

### NOT Open Sourced

The following critical components are **NOT open source**:

#### 1. Agent Orchestration Harness

**What**: The system coordinating agents, assigning tasks, managing lifecycle

**Why Valuable**: Core intellectual property of Cursor's agent infrastructure

**Alternative**: Build custom harness (Cursor Docs provide some guidance)

#### 2. Planner Implementation

**What**: Algorithm for task decomposition and sub-planner spawning

**Why Valuable**: Likely uses sophisticated codebase analysis and planning heuristics

**Alternative**: Use hierarchical planning frameworks (e.g., HTN planning, PDDL)

#### 3. Judge Logic

**What**: Completion criteria evaluation algorithm

**Why Valuable**: Determines when "good enough" is reached

**Alternative**: LLM-as-Judge patterns (various open source implementations)

#### 4. Handoff Document Parser

**What**: System extracting structured data from worker feedback

**Why Valuable**: Closes the feedback loop from workers to planners

**Alternative**: Structured output parsing with JSON schema validation

#### 5. Task Queue Implementation

**What**: Queue/mailbox system distributing work to agents

**Why Valuable**: Load balancing and scheduling algorithms

**Alternative**: Use message queue systems (RabbitMQ, Redis, Kafka) with custom logic

#### 6. Prompt Templates

**What**: Exact prompts sent to GPT-5.2 for each agent role

**Why Valuable**: Prompt engineering is "the most important part" per Cursor

**Alternative**: Reverse engineer from AGENTS.md and community examples

#### 7. Context Management

**What**: Scratchpad compaction, summarization, and memory management algorithms

**Why Valuable**: Prevents context window overflow and agent drift

**Alternative**: Summarization techniques, sliding window approaches

#### 8. Merge Conflict Resolution

**What**: Automated conflict detection and resolution strategies

**Why Valuable**: Enables hundreds of concurrent agents on same branch

**Alternative**: Structural merge tools, semantic conflict detection

#### 9. Infrastructure Code

**What**: VM provisioning, agent spawning, resource monitoring

**Why Valuable**: Operational know-how for running at scale

**Alternative**: Kubernetes-based agent orchestration, custom Docker infrastructure

#### 10. Cost Optimization

**What**: Token usage minimization, caching strategies, model routing

**Why Valuable**: Makes multi-week runs economically feasible

**Alternative**: Prompt caching, smaller models for subtasks, batch processing

### Community Tools & Related Projects

#### 1. Cursor IDE

**URL**: https://cursor.com/

**Description**: Official IDE with built-in agent support (commercial product)

**Relevance**: Provides infrastructure for running similar agent workflows

**Features**:
- Background agents
- Git worktree management
- Multi-agent judging (Cursor 2.2+)
- Parallel agent execution

**Pricing**: Subscription-based (free tier + paid plans)

**Open Source**: NO (proprietary)

#### 2. Void Editor

**URL**: https://github.com/voideditor/void

**Description**: Open-source Cursor alternative

**License**: NOT SPECIFIED (check repository)

**Stars**: NOT DISCLOSED in search results

**Relevance**: Open-source alternative to Cursor IDE

**Features**:
- AI agents on codebase
- Checkpoint and visualize changes
- Any model or host locally

**Agent Swarm Support**: NOT DISCLOSED (likely single-agent focus)

#### 3. AGENTS.md Standard

**URL**: https://agents.md/

**Description**: Community initiative to standardize agent instruction format

**License**: Creative Commons-style (implied)

**Relevance**: Inspired by FastRender's AGENTS.md approach

**Purpose**: "One prompt to rule them all" - reusable instructions across tools

**Adoption**: Growing community adoption across Copilot, Claude, Cursor, Codex

**Example Implementations**:
- FastRender (original)
- Various community .cursorrules files
- Custom agent configurations

Source: [agents.md](https://agents.md/)

#### 4. Cursor Memory Bank

**URL**: https://github.com/vanzan01/cursor-memory-bank

**Description**: Framework for persistent memory in Cursor

**License**: NOT SPECIFIED (check repository)

**Stars**: NOT DISCLOSED in search results

**Relevance**: Addresses memory management challenge

**Features**:
- Modular, documentation-driven framework
- Custom Cursor modes (VAN, PLAN, CREATIVE, IMPLEMENT)
- Persistent memory across sessions
- Visual process maps

**Agent Swarm Support**: Single-agent focus

#### 5. Cursor CLI

**URL**: Part of Cursor IDE (cursor.com)

**Description**: Command-line interface for Cursor agents

**Relevance**: Infrastructure for running agents

**Features** (as of Jan 2026):
- Agent modes (Plan, Ask)
- Cloud handoff (& prefix to push to cloud)
- Background execution
- Model selection via CLI

**Open Source**: NO (proprietary)

Source: [Cursor Changelog](https://cursor.com/changelog)

#### 6. Community Cursor Rules Collections

**Examples**:
- https://gist.github.com/aashari/07cc9c1b6c0debbeb4f4d94a3a81339e (Cursor AI Prompting Rules)
- https://gist.github.com/sshh12/25ad2e40529b269a88b80e7cf1c38084 (Cursor Agent System Prompt)
- https://github.com/digitalchild/cursor-best-practices (Best Practices)

**Description**: Community-curated prompt templates and configuration

**License**: Varies by gist/repository

**Relevance**: Practical examples of prompt engineering for agents

**Contents**: .cursorrules files, AGENTS.md examples, workflow guides

### Academic & Analysis Papers

#### 1. Simon Willison's Interview

**URL**: https://simonwillison.net/2026/Jan/23/fastrender/

**Type**: Blog post with direct interview quotes from Wilson Lin

**License**: Open (blog content)

**Relevance**: 95% - Contains direct insights from project lead

**Key Insights**:
- Peak agent count
- Commit velocity
- Model selection rationale
- Challenges encountered

#### 2. Mrinal's Agent Swarm Analysis

**URL**: https://mrinal.com/articles/agent-swarms-like-the-one-cursor-created/

**Type**: Technical analysis and alternative implementation

**License**: Open (article)

**Relevance**: 70% - Infers patterns and provides alternative architecture

**Contents**:
- Inferred architectural patterns
- Alternative implementation (Autonomy)
- Code examples for mailbox systems
- Infrastructure requirements

#### 3. HackerNews Discussions

**Main Thread**: https://news.ycombinator.com/item?id=46624541 (Scaling Agents)

**Wilson Lin Thread**: https://news.ycombinator.com/item?id=46738853 (FastRender)

**Critical Analysis**: https://news.ycombinator.com/item?id=46646777 (Criticism)

**Type**: Community discussion

**License**: Open (public forum)

**Relevance**: 60% - Contains insights from practitioners and critiques

**Key Insights**:
- Build failure analysis
- Cost estimates
- Code quality concerns
- Alternative approaches

### Docker / Infrastructure Examples

**NOT AVAILABLE**: No Docker files or infrastructure-as-code published for FastRender swarm

**Cursor Background Agents**: Use isolated Ubuntu-based VMs

**Community Examples**: NOT DISCLOSED (no public Docker files found for swarm setup)

**Workaround**: Use Cursor IDE's built-in agent infrastructure (proprietary)

### LLM Prompts & System Prompts

#### Published System Prompts

**Cursor Agent System Prompt (March 2025)**:

**URL**: https://gist.github.com/sshh12/25ad2e40529b269a88b80e7cf1c38084

**License**: Public gist

**Relevance**: 40% - Cursor IDE system prompt, NOT FastRender swarm prompt

**Contents**: General agent instructions for Cursor IDE

**Note**: This is for single-agent Cursor usage, not the multi-agent swarm

**Stars**: NOT DISCLOSED

#### Cursor AI Prompting Rules

**URL**: https://gist.github.com/aashari/07cc9c1b6c0debbeb4f4d94a3a81339e

**Description**: Structured prompting rules for Cursor AI

**Contents**: Three key files to streamline AI behavior

**Relevance**: 30% - General Cursor usage, not swarm-specific

### Performance Benchmarks

**NOT AVAILABLE**: No formal benchmark suite published

**Available Metrics**:
- Commit velocity: thousands of commits/hour
- Agent count: ~2,000 peak concurrent
- Token usage: "trillions" (exact number NOT DISCLOSED)
- LOC generated: ~3M lines
- Timeline: 1 week autonomous operation

**Comparison Baselines**: NONE (no similar projects to compare)

### Replication Attempts

#### Matt Shumer's Browser Swarm

**URL**: https://x.com/mattshumer_/status/2012307116082471161

**Description**: Independent replication using Claude

**Status**: In progress (as of Jan 2026)

**Timeline**: 6 hours progress reported

**Model**: Claude (Anthropic) instead of GPT-5.2

**Open Source**: Promised IF interest, NOT YET RELEASED

**Relevance**: 80% once released - alternative model/infrastructure

---

## Summary

The Cursor FastRender technical implementation demonstrates several key architectural patterns: constraints-based prompting over instructions, role-based hierarchical agents with recursive planning, git worktrees for agent isolation, continuous compiler verification as quality gate, and error tolerance over perfectionism. While the codebase and AGENTS.md rules are open source, critical infrastructure components remain proprietary: orchestration harness, planner algorithms, task distribution system, prompt templates, and context management strategies. The project consumed "trillions of tokens" (exact cost undisclosed, estimated ~$14M), utilized GPT-5.2 for all agent roles, and maintained ~2,000 concurrent agents at peak with thousands of commits per hour. Key technical innovations include scratchpad rewriting for memory management, handoff documents for worker-to-planner feedback, and specification-driven development with git submodule references. The community has adopted the AGENTS.md standard and built related tools (Void editor, Cursor Memory Bank), but no complete open-source replication of the swarm orchestration system exists as of Jan 2026.

---

## Sources

1. [Cursor Blog - Scaling Agents](https://cursor.com/blog/scaling-agents)
2. [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases)
3. [Cursor Blog - Agent Best Practices](https://cursor.com/blog/agent-best-practices)
4. [FastRender GitHub Repository](https://github.com/wilsonzlin/fastrender)
5. [FastRender AGENTS.md](https://github.com/wilsonzlin/fastrender/blob/main/AGENTS.md)
6. [Simon Willison Interview](https://simonwillison.net/2026/Jan/23/fastrender/)
7. [Fortune Magazine Article](https://fortune.com/2026/01/23/cursor-built-web-browser-with-swarm-ai-agents-powered-openai/)
8. [The Decoder Analysis](https://the-decoder.com/cursors-agent-swarm-tackles-one-of-softwares-hardest-problems-and-delivers-a-working-browser/)
9. [Mrinal's Agent Swarm Analysis](https://mrinal.com/articles/agent-swarms-like-the-one-cursor-created/)
10. [Cursor Docs - Worktrees](https://cursor.com/docs/configuration/worktrees)
11. [Cursor Docs - Agents](https://cursor.com/learn/agents)
12. [HackerNews Discussion - Scaling Agents](https://news.ycombinator.com/item?id=46624541)
13. [HackerNews Discussion - FastRender](https://news.ycombinator.com/item?id=46738853)
14. [Creati.ai Analysis](https://creati.ai/ai-news/2026-01-26/cursor-ai-agents-build-web-browser-autonomously/)
15. [agents.md Standard](https://agents.md/)
16. [Cursor Forum - Scratchpad Format](https://forum.cursor.com/t/rules-for-ultra-context-memories-lessons-scratchpad-with-plan-and-act-modes/48792)
17. [Git Worktrees Guide](https://dev.to/arifszn/git-worktrees-the-power-behind-cursors-parallel-agents-19j1)
18. [Matt Shumer Twitter](https://x.com/mattshumer_/status/2012307116082471161)
19. [Cursor Changelog](https://cursor.com/changelog)
20. [Cursor System Prompt Gist](https://gist.github.com/sshh12/25ad2e40529b269a88b80e7cf1c38084)
