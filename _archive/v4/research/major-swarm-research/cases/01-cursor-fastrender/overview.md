# Cursor FastRender Project - Overview & Scale

## 1. Overview & Scale

### What Was Built

FastRender is an experimental web browser rendering engine built entirely by autonomous AI agent swarms. The project represents a complete browser implementation including:

- **Rendering Pipeline**: HTML/DOM parsing, CSS cascade, box tree generation, layout algorithms (block, inline, flex, grid), table layout, text processing (bidirectional text, OpenType shaping, line breaking), painting with display lists, and SVG filters
- **JavaScript Runtime**: Custom JavaScript virtual machine (vm-js), event loop with task/microtask queues, script loading (defer/async/module), WebIDL bindings for DOM mutations, and fuel-based VM budgeting
- **Browser Chrome**: Desktop browser application with tabs, navigation, address bar, and rendering workers
- **Networking**: Resource fetching, disk caching
- **Security**: OS-level sandbox (seccomp-bpf for Linux, Seatbelt for macOS, AppContainer for Windows)

**Language**: Rust (full implementation)

**Repository**: https://github.com/wilsonzlin/fastrender (1.4k stars, 101 forks)

### Timeline & Human Involvement

- **Duration**: Approximately **one week** of continuous autonomous operation
- **Human Intervention**: Zero during execution phase after initial setup
- **Initialization**: Wilson Lin (project lead) provided initial direction and specifications
- **Supervision Model**: "One human + autonomous agents" - human set goals, agents executed without further guidance

> "A swarm of AI agents built and ran a web browser for one week with no human intervention" - Fortune Magazine

Source: [Fortune](https://fortune.com/2026/01/23/cursor-built-web-browser-with-swarm-ai-agents-powered-openai/)

### Scale Metrics

#### Lines of Code
- **Total Generated**: ~3 million lines across all iterations
- **Final Codebase**: ~1.6 million lines of pure Rust code
- **File Count**: 1,000+ files
- **Repository Structure**: Organized into functional modules (parsing, styling, layout, text, painting, JavaScript, UI, sandbox, networking)

Source: [Fortune](https://fortune.com/2026/01/23/cursor-built-web-browser-with-swarm-ai-agents-powered-openai/), [The Decoder](https://the-decoder.com/cursors-agent-swarm-tackles-one-of-softwares-hardest-problems-and-delivers-a-working-browser/)

#### Commits & Velocity
- **Total Commits**: ~30,000 commits accumulated
- **Peak Velocity**: "Thousands of commits per hour" during stable operations
- **Commit Frequency**: ~1,000 commits per hour sustained average
- **Total Operations**: 10M+ tool calls over the week

> "~1,000 commits per hour across 10M tool calls over a period of one week" - Cursor Blog

Source: [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases), [Simon Willison](https://simonwillison.net/2026/Jan/23/fastrender/)

#### Agent Count
- **Peak Concurrent Agents**: ~2,000 agents at maximum scale
- **Typical Concurrency**: Hundreds of workers running simultaneously
- **Infrastructure**: Large Linux VMs, each hosting ~300 concurrent agents
- **Agent Types**: Planners, sub-planners, workers, and judge agents

> "At peak they ran ~2,000 agents concurrently" - Simon Willison interview with Wilson Lin

Source: [Simon Willison](https://simonwillison.net/2026/Jan/23/fastrender/)

### Model Used

- **Primary Model**: OpenAI GPT-5.2 (used for planners and workers)
- **Model Selection Rationale**: "GPT-5.2 models are much better at extended autonomous work: following instructions, keeping focus, avoiding drift, and implementing things precisely and completely"
- **Model Comparison**: GPT-5.2 outperformed GPT-5.1-Codex despite the latter being coding-specialized
- **Why Not Anthropic**: "Opus 4.5 tends toward shortcuts and early termination" - less suitable for long-term autonomy

> "GPT-5.2 is a better planner than GPT-5.1-Codex, even though the latter is trained specifically for coding... Instructions were more expansive than merely coding" - Cursor Team

Source: [Cursor Blog - Scaling Agents](https://cursor.com/blog/scaling-agents), [Simon Willison](https://simonwillison.net/2026/Jan/23/fastrender/)

### Cost & Token Usage

#### Token Consumption
- **Scale**: "Trillions of tokens" consumed across all agents
- **Exact Numbers**: NOT DISCLOSED by Cursor

#### Estimated Costs
- **Community Estimate**: ~$14 million (calculated assuming GPT-5.2 Codex pricing at $14 per million output tokens for "trillions" of tokens)
- **Official Cost**: NOT DISCLOSED by Cursor
- **Cost Concerns**: The Fortune article noted "a project running swarms of agents for days or weeks is expensive"

> "Besides being buggy and incomplete, a project running swarms of agents for days or weeks is expensive" - Fortune Magazine

Source: [HackerNews Discussion](https://news.ycombinator.com/item?id=46624541), [Fortune](https://fortune.com/2026/01/23/cursor-built-web-browser-with-swarm-ai-agents-powered-openai/)

### Compilation & Build Status

#### Reality Check
- **Build Success Rate**: Only 1,426 successful workflows out of 63,295 total runs (~2.3% success rate)
- **Local Compilation**: The codebase compiled successfully throughout development locally
- **CI Failures**: Initial GitHub Actions failures misinterpreted as complete build failures
- **Current Status**: Multiple users report 34+ compilation errors and 94 warnings when attempting builds

> "Of 63295 workflow runs, apparently only 1426 have been successful" - HackerNews commenter

Source: [HackerNews Discussion](https://news.ycombinator.com/item?id=46624541), [Simon Willison](https://simonwillison.net/2026/Jan/23/fastrender/)

### Functional Status

#### What Works
- **Simple Sites**: Renders basic HTML/CSS correctly
- **Test Sites**: Successfully loaded github.com/wilsonzlin/fastrender, Wikipedia, and CNN (all usable)
- **Core Systems**: Layout and CSS gradients work well, SVG rendering functional
- **Compilation**: Rust compiler provided continuous verification

> "The browser 'kind of works' according to CEO Michael Truell" - Fortune Magazine

#### Known Issues
- **Complex Sites**: "Exhibits significant performance issues and failed on most complex sites"
- **Image Rendering**: Had trouble rendering some PNG images
- **JavaScript**: Custom JS VM is feature-flagged off; agents used QuickJS as temporary placeholder
- **Code Quality**: Described as "a tangle of spaghetti" with poor organization
- **Text Measurement**: Uses approximations rather than proper font metrics
- **Bidirectional Text**: Not supported in layout implementation

Source: [Simon Willison](https://simonwillison.net/2026/Jan/23/fastrender/), [HackerNews Discussion](https://news.ycombinator.com/item?id=46624541)

---

## 2. Architecture

### Hierarchy Type

**Structure**: **Recursive Hierarchical Tree** with role specialization

The system evolved through three distinct architectural iterations:

#### Iteration 1: Flat Peer Coordination (FAILED)
- **Design**: All agents had equal status and coordinated through shared files
- **Failure Mode**: "Twenty agents would slow down to the effective throughput of two or three, with most time spent waiting"
- **Issue**: Locking mechanisms created bottlenecks and brittleness

#### Iteration 2: Optimistic Concurrency Control (LIMITED SUCCESS)
- **Design**: Removed locks, allowed concurrent writes
- **Failure Mode**: Agents became risk-averse, avoided difficult tasks, made only safe incremental changes
- **Issue**: No hierarchy meant no clear ownership or authority

#### Iteration 3: Recursive Hierarchy (SUCCESS)
- **Design**: Specialized roles with clear responsibilities
- **Result**: Eliminated coordination overhead between workers

> "The right amount of structure is somewhere in the middle. Too little structure and agents conflict, duplicate work, and drift. Too much structure creates fragility" - Cursor Blog

Source: [Cursor Blog - Scaling Agents](https://cursor.com/blog/scaling-agents)

### Hierarchy Depth

**Depth**: **3 levels** (Root Planner → Sub-Planners → Workers) + Judge

```
Root Planner (Level 0)
├── Sub-Planner: CSS Rendering (Level 1)
│   ├── Worker: Cascade Implementation (Level 2)
│   ├── Worker: Selector Matching (Level 2)
│   └── Worker: Computed Values (Level 2)
├── Sub-Planner: JavaScript Engine (Level 1)
│   ├── Worker: Parser (Level 2)
│   ├── Worker: Runtime (Level 2)
│   └── Worker: GC (Level 2)
└── Sub-Planner: Layout (Level 1)
    ├── Worker: Block Layout (Level 2)
    ├── Worker: Inline Layout (Level 2)
    └── Worker: Flexbox (Level 2)

Judge Agent (Orthogonal - evaluates entire tree)
```

**Maximum Depth**: 3 levels (Root → Sub-Planner → Worker)

### Agent Roles

#### Root Planner
- **Responsibility**: Owns the entire scope of user instructions
- **Function**: "Understanding the current state and delivering specific, targeted tasks that would progress toward the goal"
- **Spawning**: Can spawn sub-planners for specific domains
- **Operation**: Continuously explores codebase and creates tasks
- **Output**: Concrete task specifications with file paths and code references

#### Sub-Planners
- **Responsibility**: Domain-specific planning (e.g., CSS rendering, JavaScript engines)
- **Function**: Break down area-specific work into worker tasks
- **Recursion**: "Making planning itself parallel and recursive"
- **Scope**: Focused on specific technical domains

#### Workers
- **Responsibility**: Task execution only
- **Function**: "Pick up tasks and focus entirely on completing them"
- **Isolation**: "Without considering the broader project scope"
- **Coordination**: Workers do NOT coordinate with other workers
- **Operation**: Work on isolated repository copies, push changes, repeat
- **Output**: Single handoff document with "notes, concerns, deviations, findings, thoughts, and feedback"

#### Judge Agent
- **Responsibility**: Determine project completion
- **Function**: "Evaluates project completion at each cycle and determines if iteration should continue"
- **Timing**: Runs at the end of each cycle
- **Decision**: Binary - continue or complete

> "They ended up running planners and sub-planners to create tasks, then having workers execute on those tasks, with each cycle ending with a judge agent deciding if the project was completed or not" - Simon Willison

Source: [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases), [Simon Willison](https://simonwillison.net/2026/Jan/23/fastrender/)

### Fan-Out Ratio

**Planner to Worker Ratio**: NOT DISCLOSED (exact numbers not provided)

**Inferred Patterns**:
- Peak 2,000 concurrent agents suggests high fan-out
- "Hundreds of workers run concurrently" implies 10-100+ workers per planner
- Multiple sub-planners per root planner (one per major domain)

**Workstream Organization**: The AGENTS.md file reveals 8+ parallel workstreams:
1. Rendering capability buildout
2. Pageset page loop
3. Browser chrome
4. Browser responsiveness
5. Browser page interaction
6. JS engine
7. JS DOM bindings
8. JS Web APIs
9. JS HTML integration

Each workstream likely had its own sub-planner and worker pool.

Source: [FastRender AGENTS.md](https://github.com/wilsonzlin/fastrender/blob/main/AGENTS.md)

### Agent Spawning Mechanism

#### How Spawned

**Planner-Initiated**: Planners spawn sub-planners as needed for specific domains

> "Planners can spawn sub-planners for specific areas, making planning itself parallel and recursive" - Cursor Blog

**Infrastructure**: "Single large Linux VM (Virtual Machine) with lots of resources" rather than distributed systems

**Concurrency Management**: Each VM hosted ~300 concurrent agents

**Repository Isolation**: Workers operate on isolated repository copies

Source: [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases)

#### Can Agents Spawn Sub-Agents?

**Planners**: YES - explicitly designed to spawn sub-planners recursively

**Workers**: NO - workers cannot spawn other agents; they only execute tasks

**Judge**: NO - single instance per cycle

**Recursion Limit**: NOT DISCLOSED (appears to stop at sub-planner level in practice)

---

## 3. Communication

### Model Type

**Architecture**: **Mailbox + Handoff Documents** (NOT shared memory)

**Direction**: **Hierarchical Push-Pull**:
- Planners **push** tasks to workers
- Workers **pull** tasks from queue
- Workers **push** handoff documents back to planners
- Judge **pulls** entire project state

### Message Format

#### Task Messages (Planner → Worker)

**Format**: NOT FULLY DISCLOSED

**Known Elements**:
- "Specific, targeted tasks that would progress toward the goal"
- "Concrete code entities (tables, files, functions) rather than abstract descriptions"
- File paths and code references
- Targeted scope boundaries

#### Handoff Documents (Worker → Planner)

**Format**: Structured text document

**Required Contents**:
- Notes
- Concerns
- Deviations from plan
- Findings
- Thoughts
- Feedback for future planning

> "When done, they write up a single handoff that the system submits to the planner" - Cursor Blog

Source: [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases)

#### Scratchpad Format (.cursor/scratchpad.md)

**Purpose**: Decision point for agents to determine next steps

**Structure** (from community implementations):
```markdown
# Scratchpad

## Background and Motivation
[Set by Planner initially]

## Key Challenges and Analysis
[Set by Planner initially]

## High-level Task Breakdown
[Step-by-step implementation plan]

## Project Status Board
[Mainly filled by Executor/Worker]

## Executor's Feedback or Assistance Requests
[Worker updates, Planner reviews]

## Rules & Tips
[Accumulating learnings across tasks]
```

**Communication**: Planner and Executor communicate by writing to or modifying scratchpad.md

**Completion Detection**: Scripts can read scratchpad.md and check for markers like "DONE"

Source: [Cursor Forum - Scratchpad Format](https://forum.cursor.com/t/rules-for-ultra-context-memories-lessons-scratchpad-with-plan-and-act-modes/48792), [Cursor Docs - Agent Best Practices](https://cursor.com/blog/agent-best-practices)

### Push vs Pull

**Task Distribution**: **Pull-based** for workers
- Workers pull tasks from a queue
- No evidence of push-based task assignment

**Status Updates**: **Push-based** from workers
- Workers push handoff documents to planners
- Workers push commits to git branches

**Planning**: **Event-driven** - planners react to worker handoffs

### Message Latency

**NOT DISCLOSED** - no specific latency metrics provided

**Inferred Characteristics**:
- High-throughput system (thousands of commits/hour)
- Disk I/O became bottleneck (not network)
- "Hundreds of agents compiling simultaneously would result in many GB/s reads and writes"

Source: [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases)

### Broadcast Capabilities

**NOT EXPLICITLY DISCLOSED**

**Evidence of Broadcast-Like Behavior**:
- Multiple workers touching identical files allowed
- "Moments of turbulence" that naturally converge
- Suggests some form of state synchronization

### Cross-Level Communication

**Upward**: Workers → Planners via handoff documents

**Downward**: Planners → Workers via task specifications

**Horizontal (Worker-Worker)**: **EXPLICITLY FORBIDDEN**
- "Workers do NOT coordinate with other workers"
- Isolation enforced through separate git worktrees

**Planner-Planner**: NOT DISCLOSED (likely coordinated through root planner)

**Judge Communication**: Judge reads entire project state but does NOT directly communicate with individual agents

---

## 4. Git & Code Integration

### Worktree vs Branch Strategy

**Infrastructure**: **Git Worktrees** for parallel agent isolation

> "Hundreds of workers run concurrently, pushing to the same branch with minimal conflicts" - Cursor Blog

**Worktree Implementation**:
- Each agent operates in isolated git worktree
- Worktrees stored in `~/.cursor/worktrees/<repo>/`
- 1:1 mapping between agents and worktrees
- Automatic creation and management by Cursor
- Up to 20 worktrees maintained per workspace
- Cleanup every 6 hours based on access time

**Shared Repository**: All worktrees share same .git object database (efficient)

Source: [Cursor Docs - Worktrees](https://cursor.com/docs/configuration/worktrees), [Git Worktrees Guide](https://dev.to/arifszn/git-worktrees-the-power-behind-cursors-parallel-agents-19j1)

### Branch Naming & Strategy

**Primary Branch**: All workers push to **same branch** (NOT separate per-agent branches)

**Branch Strategy**: NOT FULLY DISCLOSED

**Inferred Pattern**:
- Main development branch accepts all agent commits
- Minimal branch proliferation
- Focus on high merge velocity over branch isolation

### Who Merges & Conflict Resolution

**Merge Responsibility**: **Workers handle their own merges**

**Conflict Resolution**:
- "Hundreds of workers run concurrently, pushing to the same branch with minimal conflicts"
- Workers responsible for resolving conflicts when they occur
- Optimistic concurrency - conflicts handled at push time
- "Most commits do not have merge conflicts because the harness itself is able to quite effectively split out and divide the scope and tasks"

**Error Tolerance Philosophy**:
- System accepts "a small but stable rate of errors"
- "Moments of turbulence" allowed to naturally converge
- Subsequent commits fix API changes and syntax errors quickly

> "The system accepts a small but stable rate of errors rather than requiring 100% correctness per commit, preventing serialization bottlenecks" - Cursor Blog

Source: [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases), [Simon Willison](https://simonwillison.net/2026/Jan/23/fastrender/)

### Commit Frequency

**Rate**: Thousands of commits per hour during peak operation

**Granularity**: High-frequency commits (agents push changes frequently)

**Verification**: Continuous compilation with Rust compiler

> "The agents were constantly compiling it using the Rust compiler and fixing any compile errors as they occurred" - Simon Willison interview

Source: [Simon Willison](https://simonwillison.net/2026/Jan/23/fastrender/)

### Monorepo vs Polyrepo

**Structure**: **Monorepo**

**Repository**: Single GitHub repository at wilsonzlin/fastrender

**Submodules**: Specifications included as git submodules:
- csswg-drafts (CSS specs)
- tc39-ecma262 (JavaScript specs)
- whatwg standards (HTML/DOM specs)

**Vendor Code**: Includes vendored dependencies in `vendor/` directory:
- ecma-rs (JavaScript engine)
- Taffy (layout library)

Source: [FastRender GitHub](https://github.com/wilsonzlin/fastrender), [Simon Willison](https://simonwillison.net/2026/Jan/23/fastrender/)

---

## 5. What Worked & What Failed

### Failed Approaches

#### Attempt 1: Flat Peer Coordination with Locking

**Design**: All agents equal status, coordinated through shared files with locks

**Failure Symptoms**:
- Massive slowdown: "Twenty agents would slow down to the effective throughput of two or three"
- Most time spent waiting on locks
- Bottlenecks from locking mechanisms
- Brittle system - prone to deadlocks

**Why It Failed**: Serialization bottleneck; coordination overhead exceeded parallel benefits

#### Attempt 2: Optimistic Concurrency Control

**Design**: Removed locks, allowed concurrent writes with conflict resolution

**Partial Success**: Reduced lock contention issues

**Failure Symptoms**:
- Agents became risk-averse
- Avoided difficult tasks
- Made only safe, incremental changes
- Duplicate work across agents
- Drift from original goals

**Why It Failed**: Without hierarchy, agents lacked authority to make bold decisions

#### Attempt 3: Integrator Role (Removed)

**Design**: Dedicated agent to integrate and coordinate work from other agents

**Failure**: "Removing an integrator role improved performance"

**Why It Failed**: Created more bottlenecks than it solved; serialization point for all work

> "Simplification beat complexity—removing an integrator role improved performance" - Cursor Blog

Source: [Cursor Blog - Scaling Agents](https://cursor.com/blog/scaling-agents), [The Decoder](https://the-decoder.com/cursors-agent-swarm-tackles-one-of-softwares-hardest-problems-and-delivers-a-working-browser/)

### What Worked

#### Role-Based Hierarchy

**Success**: Recursive planner → sub-planner → worker architecture

**Key Benefits**:
- Eliminated coordination overhead between workers
- Planners specialize in understanding scope
- Workers specialize in execution
- Clear ownership and authority

#### Isolation Strategy

**Success**: Workers operate on isolated repository copies

**Key Benefits**:
- No coordination needed between workers
- Conflicts handled at merge time, not during development
- Parallel execution without blocking

#### Error Tolerance

**Success**: Accept "small but stable rate of errors"

**Key Benefits**:
- Avoid serialization bottlenecks from requiring 100% correctness
- Natural convergence over time
- Subsequent commits fix earlier mistakes

#### Continuous Verification

**Success**: Rust compiler as continuous verification

**Key Benefits**:
- Immediate feedback on syntax/type errors
- Structural constraints from type system
- Agents constantly compiling and fixing errors

#### Specification-Driven Development

**Success**: Git submodules with W3C/WHATWG/TC39 specifications

**Key Benefits**:
- Agents reference specs directly in code comments
- Clear definition of correct behavior
- Objective validation criteria

> "One of the most critical insights from the FastRender experiment is the validation of Specification-Driven Development (SDD) as the primary interface for autonomous coding" - Creati.ai analysis

Source: [Creati.ai Analysis](https://creati.ai/ai-news/2026-01-26/cursor-ai-agents-build-web-browser-autonomously/)

### Key Insights

#### 1. Prompting Matters Most

> "A surprising amount of the system's behavior comes down to how we prompt the agents. The harness and models matter, but the prompts matter more." - Cursor Blog

**Effective Prompt Strategies**:
- **Constraints over instructions**: "No TODOs, no partial implementations"
- **Concrete quantities**: Give specific numbers and ranges for scope
- **Avoid checkbox mentality**: High-level tasks shouldn't be treated as checklists

Source: [Cursor Blog - Agent Best Practices](https://cursor.com/blog/agent-best-practices)

#### 2. Structure Matters, But Not Too Much

> "The right amount of structure is somewhere in the middle. Too little structure and agents conflict, duplicate work, and drift. Too much structure creates fragility" - Cursor Blog

**Balance Point**: Role specialization without heavy coordination overhead

#### 3. Model Selection Is Critical

**GPT-5.2 Advantages**:
- Better at extended autonomous work
- Better instruction following
- Less drift over time
- Better planning than coding-specialized models

**Why General Models Won**: "Instructions were more expansive than merely coding" - required agent autonomy and harness operation

#### 4. Freshness Mechanisms Prevent Drift

**Effective Techniques**:
- Scratchpad rewriting (not appending)
- Automatic context summarization
- Self-reflection prompts
- Regular state dumps

Source: [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases)

### Challenges & Scaling Limits

#### Disk I/O Bottleneck

**Issue**: "Hundreds of agents compiling simultaneously would result in many GB/s reads and writes of build artifacts"

**Impact**: Disk I/O became the critical constraint, significantly impacting throughput

**Implication**: Single-machine architecture hits scaling limits around 2,000 concurrent agents

#### Code Quality

**Issue**: Codebase described as "a tangle of spaghetti" with deeply nested directories

**Trade-off**: High velocity vs. code organization

**Reality**: Quality control removed because it created bottlenecks

#### Compilation Success Rate

**Issue**: Only ~2.3% of CI workflow runs succeeded (1,426 out of 63,295)

**Caveat**: Local compilation worked throughout; CI failures may be configuration issues

#### Cost Prohibitive

**Issue**: "Expensive to run long-duration multi-agent operations"

**Barrier**: Estimated ~$14M for this experiment (community calculation)

**Implication**: Not yet economically viable for most projects

Source: [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases), [HackerNews Discussion](https://news.ycombinator.com/item?id=46624541)

---

## 6. Open Source & Artifacts

### Official Open Source Repositories

#### 1. FastRender Repository

**URL**: https://github.com/wilsonzlin/fastrender

**Description**: Complete browser rendering engine codebase generated by agent swarm

**License**: NOT EXPLICITLY STATED in README

**Stars**: 1,400+ (as of Jan 2026)

**Key Contents**:
- Full Rust implementation of browser engine
- `AGENTS.md` - Complete agent instructions and constraints
- `docs/` - Internal documentation
- `instructions/` - Workstream-specific guides
- `progress/pages/` - Progress tracking
- `vendor/` - Vendored dependencies (ecma-rs, Taffy)

**Status**: Under heavy development, APIs unstable, not recommended for production

#### 2. AGENTS.md File

**URL**: https://github.com/wilsonzlin/fastrender/blob/main/AGENTS.md

**Significance**: Complete rule set that governed all agent behavior during the swarm

**Contents** (see Technical Report for full breakdown):
- Repo-wide rules for all workstreams
- 6 non-negotiable constraints
- Philosophy and culture guidelines
- System resource safety requirements
- Test architecture specifications
- Git hygiene rules

Source: [FastRender AGENTS.md](https://github.com/wilsonzlin/fastrender/blob/main/AGENTS.md)

### Published Prompts & Configurations

#### AGENTS.md Constraints (Actual Rules Used)

**6 Non-Negotiables**:

1. **No page-specific hacks** - "Avoid hostname/selector special-cases or magic numbers for individual sites"

2. **Spec compliance** - "Implement correct behavior rather than taking 'compat shortcuts' with deviating behavior"

3. **No pixel nudging** - "Maintain staged pipeline (parse → style → box tree → layout → paint)"

4. **No panics** - "Production code must return errors cleanly and bound work"

5. **Keep Taffy vendored** - "Use only vendor/taffy/ for flex/grid; no Cargo updates"

6. **JavaScript must be bounded** - "Engine requires interrupts/timeouts and avoid unbounded allocations"

**Safety Rules** (Mandatory):

All commands wrapped with:
```bash
timeout -k 10 600 bash scripts/run_limited.sh --as 64G -- [command]
```

**Forbidden Commands**:
- `cargo build/test/check` without wrapper scripts
- `cargo test` without `-p <crate>`, `--test <name>`, or `--lib`
- `--all-features` or `--all-targets` flags

**Mandatory Commands**:
```bash
bash scripts/cargo_agent.sh  # Always use this wrapper
```

Source: [FastRender AGENTS.md](https://github.com/wilsonzlin/fastrender/blob/main/AGENTS.md)

#### Cursor Blog Post Prompt Principles

**From "Agent Best Practices" Blog**:

1. **Constraints are more effective than instructions**
   - Example: "No TODOs, no partial implementations"

2. **Give concrete numbers and ranges when discussing quantity of scope**

3. **Avoid 'checkbox mentality' for high-level tasks**

4. **Freshness mechanisms**:
   - Scratchpad rewriting (versus appending)
   - Automatic context summarization
   - Self-reflection prompts

Source: [Cursor Blog - Agent Best Practices](https://cursor.com/blog/agent-best-practices)

### Community Replications

#### Matt Shumer's Browser Swarm

**Announcement**: January 2026 via Twitter

> "Super inspired by @cursor_ai's amazing work, so I decided to build my own long-running agent swarm. Six hours in, they're making real progress towards a working browser. I'm going to keep running this until my Claude Max plan runs out. If there's interest, I'll open-source!"

**Status**: NOT YET OPEN SOURCED (as of research date)

**Model**: Uses Claude (Anthropic) instead of GPT-5.2

Source: [Matt Shumer Twitter](https://x.com/mattshumer_/status/2012307116082471161)

#### AGENTS.md Standard

**URL**: https://agents.md/

**Description**: Community initiative to standardize agent instruction format

**Adoption**: Multiple projects using AGENTS.md convention

**Purpose**: "One prompt to rule them all" - reusable instructions across Copilot, Claude, Cursor, Codex

**Related Implementations**:
- Cursor Memory Bank (https://github.com/vanzan01/cursor-memory-bank)
- Various custom .cursorrules configurations

Source: [agents.md website](https://agents.md/), [Medium Article](https://medium.com/@genyklemberg/one-prompt-to-rule-them-all-how-to-reuse-the-same-markdown-instructions-across-copilot-claude-42693df4df00)

### NOT Open Sourced

The following critical components are **NOT disclosed or open sourced**:

1. **Agent Harness**: The orchestration system coordinating agents
2. **Task Distribution System**: Queue/mailbox implementation
3. **Planner Algorithms**: How planners decompose work and spawn sub-planners
4. **Handoff Document Parser**: How planners interpret worker feedback
5. **Judge Agent Logic**: Completion criteria and evaluation algorithm
6. **Actual Prompt Templates**: Exact prompts sent to GPT-5.2 for each role
7. **Infrastructure Code**: VM setup, agent spawning, resource management
8. **Cost Optimization**: Token usage minimization strategies
9. **Memory Compaction**: Context window management and summarization
10. **Merge Conflict Resolution**: Automated conflict handling logic

### Alternative Open Source Tools

#### Void Editor

**URL**: https://github.com/voideditor/void

**Description**: Open-source Cursor alternative with agent support

**Features**: AI agents on codebase, checkpoint/visualize changes, any model/host locally

**Relevance**: Similar agent-based coding but not a swarm implementation

#### Autonomy (Mrinal's Implementation)

**Article**: https://mrinal.com/articles/agent-swarms-like-the-one-cursor-created/

**Description**: Alternative swarm implementation illustrating concepts

**Status**: Code examples in article, not full repository

**Features**: Parallel agent execution, dynamic spawning, isolated workspaces, secure messaging

### Research Papers & Analyses

1. **Simon Willison Interview**: https://simonwillison.net/2026/Jan/23/fastrender/
   - Direct interview with Wilson Lin
   - Technical insights into implementation

2. **The Decoder Analysis**: https://the-decoder.com/cursors-agent-swarm-tackles-one-of-softwares-hardest-problems-and-delivers-a-working-browser/
   - Architecture evolution analysis

3. **Mrinal's Agent Swarm Analysis**: https://mrinal.com/articles/agent-swarms-like-the-one-cursor-created/
   - Inferred patterns and alternative implementations

4. **HackerNews Discussions**:
   - Main thread: https://news.ycombinator.com/item?id=46624541
   - Wilson Lin thread: https://news.ycombinator.com/item?id=46738853
   - Critical analysis: https://news.ycombinator.com/item?id=46646777

---

## Summary

The Cursor FastRender project represents the most ambitious autonomous multi-agent coding demonstration to date, producing a functional (if buggy) browser in one week with ~3M lines of code, 30,000 commits, and 2,000 concurrent agents at peak. The project validated several key architectural principles: role-based hierarchy over flat coordination, error tolerance over perfectionism, and constraints over instructions in prompting. However, it also revealed significant scaling limits (disk I/O bottlenecks), quality trade-offs (poor code organization), and cost barriers (~$14M estimated). While the resulting codebase (FastRender) and agent rules (AGENTS.md) are open source on GitHub, the critical orchestration infrastructure, prompt templates, and coordination algorithms remain proprietary to Cursor.
