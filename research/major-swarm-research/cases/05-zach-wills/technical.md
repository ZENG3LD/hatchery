# Zach Wills: Technical Deep Dive - 20 AI Agents Orchestration

## 1. Prompts & Prompting Strategy

### Voice Dictation as Primary Input

**Wispr Flow Integration**:
- Voice-to-text dictation for initial prompt composition
- Process: "Dictate and word vomit all of my thoughts to AI"
- Voice input naturally includes more contextual reasoning than typing
- Richer context leads to better agent understanding
- Faster than typing for complex task descriptions

**Why Voice Works Better**:
- Captures nuance and intent more naturally
- Includes implicit context (tone, emphasis, thought process)
- Reduces friction in task specification
- Enables stream-of-consciousness planning

### Slash Command Architecture

**Pre-Defined Command System**:
Commands stored in:
- `~/.claude/commands/` (system-wide)
- `./.claude/commands/` (project-specific)

**Core Commands**:
- `/spike`: Small, quick tasks (minimal planning)
- `/tech plan`: Larger planning tasks (full Core Trio)
- `/task`: Net-new development tasks
- `/fix`: Bug fixes
- Custom commands for specific workflows

**Command Structure**:
Each command is a `.md` file defining:
- Trigger phrase
- Complexity analysis logic
- Which agents to invoke
- Output format requirements
- Integration points (Linear, GitHub, etc.)

### Task Decomposition Approach

**Complexity-Based Routing**:

**LIGHT Complexity**:
- Examples: Typos, minor edits, simple refactors
- No agents invoked
- Direct execution

**STANDARD Complexity**:
- Examples: New features, typical development tasks
- Invokes Core Trio at standard depth
- Product Manager + UX Designer + Senior Engineer (parallel)

**DEEP Complexity**:
- Examples: Vague requirements, architectural changes, complex features
- Invokes Core Trio with extended investigation scope
- More thorough research and planning phase

### Co-Authoring Plans Before Execution

**Two-Phase Workflow**:
1. **Planning Phase**: Human + AI co-author detailed plan
2. **Execution Phase**: Hand off plan to agent team

**Key Principle**: "Align on the Plan, Not Just the Goal" (Rule #1)
- "Cheaper to fix bad plan than bad implementation"
- Reduces rework and agent drift
- Creates clear specification for execution agents

**Planning Artifacts**:
- User stories and acceptance criteria
- User flows and wireframes (with all states: loading, empty, error, success)
- Technical approach, risks, dependencies
- Effort estimation
- 80/20 MVP scope definition

### Agent Definition Structure

**Agent Definition Format** (`.md` files):
```markdown
---
name: product-manager
description: [Purpose and specialty]
model: opus
---

[Behavior rules]
[Operating principles]
[Concise working loop - typically 3-5 steps]
[Escalation triggers to other agents]
[Core deliverables]
[Anti-patterns to avoid]
```

**Agent Locations**:
- `~/.claude/agents/` (system-wide)
- `./.claude/agents/` (project-specific)
- Invoked via `/agents` command

**Example Agents**:
- `product-manager`: Defines user stories, acceptance criteria
- `ux-designer`: Proposes flows, wireframes, all UI states
- `senior-software-engineer`: Technical approach, risks, dependencies
- `code-reviewer`: Structured feedback with severity levels

### Sequential Thinking Enforcement

**Sequential Thinking MCP**:
- Forces AI to outline plan before executing
- "Simple but powerful MCP"
- Dramatically reduces "intent drift"
- Keeps agents on track

**How It Works**:
- Agent must break down problem into steps
- Explore alternatives
- Verify solutions
- Revise and refine thoughts as understanding deepens
- Provides meta-cognitive planning layer

### Directive-Based Learning

**Self-Updating CLAUDE.md**:
- Agents automatically update documentation based on directives
- Captures project-specific patterns
- Records what works/doesn't work
- Becomes increasingly refined over time

**Update Triggers**:
- Completion of major tasks
- Discovery of new patterns
- Errors and their solutions
- Performance optimizations

## 2. Memory & Context Management

### Context Isolation Strategy

**200k Token Budget Per Specialist**:
- Each agent receives dedicated full context window
- Product manager uses entire context for user needs
- Engineer doesn't hold product discussions in memory
- Receives only concise ticket artifact

**Benefits of Isolation**:
- Prevents context exhaustion
- Maintains quality across stages
- Reduces cognitive load per agent
- Enables true specialization

### Self-Updating Documentation System

**CLAUDE.md File**:
- **Purpose**: Persistent memory across agent restarts
- **Updates**: Agents automatically update based on learnings
- **Content**: Project-specific patterns, conventions, decisions
- **Evolution**: Continuously refined throughout project

**What Gets Captured**:
- Architectural decisions and rationale
- Common patterns and anti-patterns
- Integration points and APIs
- Error patterns and solutions
- Performance optimization insights

### Checkpoint-Based Memory Persistence

**Multiple Checkpoint Sources**:

**1. Markdown Files**:
- Progress summaries
- Decision logs
- Agent handoff documentation

**2. GitHub PR Comments**:
- Automatic status updates from agents
- Audit trail of agent work
- Progress checkpointing

**3. Linear Tickets**:
- Structured task information
- Business context
- Expected behavior
- Research summary
- Acceptance criteria
- Dependencies
- Implementation notes

**4. Git Commits**:
- Frequent commits preserve progress
- Enable rollback if agent drifts
- Each commit captures incremental work

### Fresh Context Spawning

**"Be Ruthless About Restarting" (Rule #7)**:
- Restart agents with fresh context after major milestones
- Prevents context pollution
- Eliminates accumulated drift
- Maintains agent focus

**Restart Triggers**:
- Completion of major feature
- Shift in task type (planning → implementation)
- Detection of drift
- After checkpoint creation

**Process**:
1. Agent completes phase
2. Progress saved to checkpoint (GitHub/Linear/markdown)
3. Agent terminated
4. New agent spawned with fresh 200k context
5. New agent reads checkpoint to resume

### Artifact-Based Knowledge Transfer

**Structured Handoffs**:
- Agents communicate via artifacts, not shared context
- Examples: Tickets, design briefs, code reviews
- Each artifact has defined structure

**Linear Ticket Structure** (example artifact):
```markdown
## Business Context
[Why this matters]

## Expected Behavior
[User-facing outcomes]

## Research Summary
[Findings from planning phase]

## Acceptance Criteria
- [ ] Criterion 1
- [ ] Criterion 2

## Dependencies
[Other tickets/systems]

## Implementation Notes
[Technical approach, risks, effort]
```

**Benefits**:
- Clear audit trail
- Prevents context pollution
- Enables debugging
- Provides documentation

### Active Memory Management

**"Actively Manage the AI's Memory" (Rule #3)**:
- Don't let context grow unbounded
- Proactively curate what agents remember
- Remove irrelevant information
- Focus context on current task

**Techniques**:
- Periodic context resets
- Specialized sub-agents for different phases
- Artifact-based instead of context-based communication
- Explicit memory directives in prompts

## 3. Task Distribution & Scheduling

### Parallel vs Sequential Execution

**Parallel Execution** (for independent tasks):
- Backend API + Frontend UI + Tests + Documentation run simultaneously
- Completion time = longest single task
- Example: "Integrate Stripe Payments"
  - Backend specialist writes API routes (parallel)
  - Frontend specialist builds UI components (parallel)
  - QA specialist generates tests (parallel)
  - Documentation specialist drafts README (parallel)

**Sequential Handoffs** (for dependent tasks):
- Output from one agent feeds into next
- Example: Planning → Implementation → Code Review → Refinement
- Creates automated assembly line
- Each stage completes before next begins

### Core Trio Pattern (Parallel Planning)

**Three Specialists Run Simultaneously**:
1. **Product Manager**
   - User stories
   - Acceptance criteria
   - Business context

2. **UX Designer**
   - User flows
   - Wireframes
   - All UI states (loading, empty, error, success)

3. **Senior Engineer**
   - Technical approach
   - Risks and dependencies
   - Effort estimation

**Output**: Comprehensive plan ready for implementation

**Synthesis Step**:
- Main orchestrator consolidates three outputs
- Creates unified Linear ticket
- Resolves conflicts/inconsistencies
- Identifies gaps

### Implementation Assembly Line (Sequential)

**Phase 1: Implementation**:
- Senior engineer receives ticket from planning phase
- Implements features
- Commits frequently to branch

**Phase 2: Code Review** (iterative loop):
- Code reviewer agent analyzes implementation
- Provides structured feedback:
  - **Blockers**: Must fix before merge
  - **High Priority**: Should fix
  - **Medium Priority**: Nice to have
- Specific file:line feedback with suggestions

**Phase 3: Refinement Loop**:
- Senior engineer addresses feedback
- Commits fixes
- Returns to code reviewer
- Loop continues until approval

**Phase 4: Testing**:
- Dedicated tester agent receives approved code
- Generates comprehensive test suite
- Runs autonomous test loop

### Autonomous Loop Pattern

**"Trust the Autonomous Loop" (Rule #5)**:
- Agents iterate using tools until task complete
- Not one-shot interactions
- Build → Test → Validate → Fix → Complete cycle

**Test Loop Example**:
1. Tester agent receives implementation
2. Generates test cases
3. Runs tests (Playwright for UI, unit tests for backend)
4. If failures:
   - Analyzes failure
   - Modifies code
   - Re-runs tests
   - Repeats until pass
5. If success: Marks task complete

**Minimal Human Intervention**:
- Loop runs autonomously
- Human monitors for drift
- Intervenes only if agent stuck or off-track

### Dependency Management

**NOT DISCLOSED**: Specific dependency tracking mechanism not described in sources

**Inferred Approach**:
- Linear tickets contain "Dependencies" section
- Tickets reference each other
- Work >2 days broken into 2-3 interconnected tickets
- Smaller tickets enable parallel work on different aspects

### 4+ Terminal Parallelization

**Simultaneous Execution Environments**:
- 4+ terminal windows running concurrently
- Each terminal = separate agent/task
- Different branches per terminal
- Isolated database branches (Neon)

**Orchestrator Role**:
- Human monitors all terminals
- Identifies drift
- Kills/restarts agents as needed
- Synthesizes outputs

### Work Breakdown Heuristics

**Size-Based Splitting**:
- Work estimated >2 days → automatically broken into 2-3 smaller tickets
- Enables faster iteration
- Compounds velocity

**MVP-First Defaults**:
- Tickets default to 80/20 scope
- Deliver 80% of value with 20% of effort
- Ship fast, iterate later

**Pragmatic Estimation**:
- Effort tracked in tickets
- Realistic scope setting
- Avoids over-engineering

## 4. Validation & Quality Control

### Multi-Layer Testing Strategy

**1. Automated Test Generation**:
- Dedicated tester agent writes ~800 tests
- Coverage across:
  - Unit tests
  - Integration tests
  - UI tests (Playwright)
- Tests maintained throughout development

**2. Autonomous Test Loops**:
- Tests run automatically
- Agents self-correct based on failures
- Iterative refinement until pass

**3. CI/CD Integration from Day One**:
- Tests run on every PR
- Prevents regressions
- Ensures "production ready" quality

### Code Review Process

**Structured Feedback Format**:

**Severity Levels**:
- **Blockers**: Must fix before merge (security, data loss, critical bugs)
- **High Priority**: Should fix (performance, maintainability, best practices)
- **Medium Priority**: Nice to have (style, minor optimizations)

**Feedback Structure**:
```markdown
## Blockers
- `file.rs:123`: [Issue description] → [Suggested fix]

## High Priority
- `module.ts:45`: [Issue description] → [Suggested fix]

## Medium Priority
- `component.tsx:67`: [Issue description] → [Suggested fix]
```

**Specific, Actionable**:
- File:line precision
- Clear problem description
- Suggested solution
- Code examples where applicable

### Browser Automation Validation

**Playwright MCP Integration**:
- Headless browser control
- UI interaction testing
- Visual regression detection (inferred)

**Self-Correcting Loops**:
- Agent navigates UI
- Validates behavior
- If failure: Modifies code and retries
- If success: Marks validation complete

**Why Critical**:
- Enables autonomous UI validation
- Reduces manual QA time
- Catches integration issues early

### Database Quality Control

**Mid-Week Optimization**:
- Query inspection performed
- Performance optimization
- Database schema validation

**Neon Database Branching**:
- Isolated databases per workstream
- Safe experimentation
- No cross-contamination

**Quality Checks** (inferred):
- Query performance
- Index usage
- Schema migrations
- Data integrity

### Version Control Quality Gates

**Commit Frequency** (Rule #8: "Commit Early and Often"):
- Frequent commits = safety net
- Easy rollback if agent drifts
- Preserves incremental progress
- ~800 commits / 7 days = ~114 commits/day

**PR-Based Checkpointing**:
- 100+ PRs in one week
- Each PR = checkpoint
- Agent status updates in comments
- Enables intervention points

**Branch-Based Isolation**:
- Separate branches per parallel task
- Prevents conflicts
- Safe experimentation
- Clean merge history

### Human Quality Control

**Active Monitoring**:
- Wills monitored all agents
- Interjected when agents went off-track
- "Which happened!" - drift was common

**Ruthless Restarting** (Rule #7):
- Kill agents that drift
- Don't try to salvage off-track work
- Restart with fresh context
- Cheaper than debugging drift

**Plan Alignment** (Rule #1):
- Review and approve plans before execution
- Co-author with AI for quality
- Prevents bad implementation from bad plans

### Evaluation & Drift Detection

**Non-Determinism Monitoring**:
- Agent prompts version-controlled like code
- Monitored for behavioral drift after model updates
- Rigorous evaluation suites required (specific tools NOT DISCLOSED)

**Challenges**:
- Changing one prompt ripples unpredictably downstream
- LLM updates cause subtle behavioral changes
- Requires constant vigilance

## 5. Mailbox / Inbox Implementation

### NOT DISCLOSED

The sources **do not describe** a specific "mailbox" or "inbox" system for agent-to-agent communication.

### Inferred Coordination Mechanisms

**Artifact Repositories**:
- **Linear Tickets**: Centralized task/ticket system
- **GitHub PRs**: Code review and status updates
- **Markdown Files**: Progress documentation

**Status Updates**:
- Agents post to GitHub PR comments automatically
- Linear tickets updated with progress
- NOT true async message passing

**Human as Router**:
- Wills acted as central coordinator
- Manually routed work between agents
- No autonomous agent-to-agent messaging described

**Sequential Handoffs**:
- Output files from Agent A become input for Agent B
- File-based rather than message-based
- Example: Planning agent outputs ticket → Implementation agent reads ticket

### Contrast with True Multi-Agent Systems

**No Distributed Coordination**:
- No described mechanism for agents discovering each other
- No peer-to-peer communication
- No autonomous task negotiation

**Centralized Orchestration**:
- Human orchestrator assigns all tasks
- Agents don't self-organize
- Top-down rather than emergent

**Synchronous Handoffs**:
- Phase completes → Human reviews → Next phase starts
- Not asynchronous message-passing
- More pipeline than swarm

## 6. Open Source Artifacts & Code

### Published Artifacts: NONE

**No Code Released**:
- No custom parallelization script published
- No agent definition files (.md) shared
- No command definition files shared
- No CLAUDE.md template provided
- No orchestration framework open-sourced

**No Configuration Shared**:
- No specific prompts verbatim
- No agent prompt templates
- No command structures
- High-level descriptions only

### Conceptual Documentation Only

**Blog Posts** (primary shared artifacts):

1. **"I Managed a Swarm of 20 AI Agents for a Week and Built a Product. Here Are the 8 Rules I Learned."**
   - 8 rules (high-level principles)
   - Workflow descriptions
   - Lessons learned
   - No code

2. **"How to Use Claude Code Subagents to Parallelize Development"**
   - Conceptual frameworks
   - Example agent structures (descriptions, not actual code)
   - Workflow patterns
   - No actual .md files

3. **"How I Used Claude Code Subagents to Create an 18-Month Roadmap in 2 Hours"**
   - Specific use case
   - Process description
   - No code

4. **"My 2026 AI Bets (A Time Capsule)"**
   - Future predictions
   - Retrospective insights
   - Philosophy
   - No technical artifacts

### Referenced Open Source Tools

**All Pre-Existing, Not Created by Wills**:

1. **Serena MCP**
   - Semantic codebase editing toolkit
   - Already open source: [https://github.com/oraios/serena](https://github.com/oraios/serena)

2. **Playwright MCP**
   - Headless browser control
   - Microsoft open source: [https://github.com/microsoft/playwright-mcp](https://github.com/microsoft/playwright-mcp)

3. **Sequential Thinking MCP**
   - Planning enforcement
   - Open source: [https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking](https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking)

4. **Neon Databases MCP**
   - Database branching
   - Open source (specific link NOT DISCLOSED in sources)

5. **Linear MCP**
   - Ticket management
   - Integration details NOT DISCLOSED

6. **Claude Code**
   - Primary IDE
   - Anthropic commercial product (not open source)

7. **Wispr Flow**
   - Voice dictation
   - Commercial product (not open source)

### Analytics Platform Status

**NOT OPEN SOURCE**:
- Built as internal tool
- Wills quote: "I'll talk to the team. This particular build is an internally used product.. but maybe we could open source it."
- No follow-up confirmation of open-sourcing
- No repository link provided

**Internal Use Only**:
- Engineering analytics for Wills' organization
- Replaces ~$30k/year vendor solution
- Not publicly accessible

### What Could Be Reconstructed

**From Blog Descriptions**:
- General agent definition structure (name, description, model, working loop)
- High-level command patterns (/spike, /tech plan)
- Workflow sequences (Core Trio → Implementation → Review → Test)
- Architectural patterns (parallel execution, sequential handoffs, artifact-based communication)

**What Cannot Be Reconstructed**:
- Exact agent prompts
- Specific command logic
- CLAUDE.md structure
- Integration code (Linear, GitHub automation)
- Synthesis logic for consolidating parallel outputs
- Drift detection mechanisms
- Custom tooling/scripts

### Community Requests

**Hacker News Comments Included**:
- Requests for code/templates
- Requests for agent definitions
- Questions about implementation specifics
- Interest in reproducing approach

**Wills' Response**:
- Shared conceptual frameworks via blog posts
- Did not release actual code artifacts
- Focus on teaching principles rather than providing tools

### Implications

**High-Level Knowledge Sharing**:
- Philosophy and principles well-documented
- Specific implementation remains proprietary
- Readers must implement from scratch based on concepts

**Barrier to Reproduction**:
- Significant work required to recreate
- No starting templates
- Trial-and-error needed for prompt engineering
- No validation that reproductions match original quality

**Value of Documentation**:
- Proves concept viability
- Provides mental models
- Inspires similar experiments
- Insufficient for direct replication

## Technical Architecture Summary

### What We Know

**Tools & Stack**:
- Claude Code + Sub-agents
- 5 open-source MCPs (Serena, Playwright, Sequential Thinking, Neon, Linear)
- Wispr Flow for voice input
- GitHub for version control
- Linear for task tracking
- Primarily Claude Opus model

**Workflow Patterns**:
- Parallel execution for independent tasks
- Sequential handoffs for dependent work
- Artifact-based communication
- Fresh context spawning
- Autonomous test loops
- Code review loops

**Agent Roles**:
- Product Manager
- UX Designer
- Senior Software Engineer
- Code Reviewer
- Dedicated Tester
- (Others inferred but not specified)

**Human Orchestrator**:
- Central coordinator
- Active monitoring
- Drift detection and intervention
- Plan approval
- Synthesis of parallel outputs

### What We Don't Know

**Specific Implementations**:
- Exact agent prompts
- Command definition logic
- CLAUDE.md structure
- Synthesis algorithms
- Drift detection mechanisms
- Custom automation scripts

**Scale Details**:
- How many agents ran simultaneously (only "~20" specified)
- Specific branching strategy details
- Database schema for analytics platform
- Integration architecture (GitHub ↔ Linear ↔ agents)

**Cost Breakdown**:
- Token usage per agent type
- Input vs output token ratio
- Cost optimization strategies
- Which tasks consumed most tokens

**Quality Metrics**:
- Test coverage percentage
- Bug rates
- Agent drift frequency
- Restart frequency
- Human intervention rate

## Sources

- [I Managed a Swarm of 20 AI Agents for a Week and Built a Product. Here Are the 8 Rules I Learned.](https://zachwills.net/i-managed-a-swarm-of-20-ai-agents-for-a-week-here-are-the-8-rules-i-learned/)
- [How to Use Claude Code Subagents to Parallelize Development](https://zachwills.net/how-to-use-claude-code-subagents-to-parallelize-development/)
- [I built a production app in a week by managing a swarm of 20 AI agents - Hacker News](https://news.ycombinator.com/item?id=45041906)
- [My 2026 AI Bets (A Time Capsule)](https://zachwills.net/my-2026-ai-bets-a-time-capsule/)
- [Serena MCP - GitHub](https://github.com/oraios/serena)
- [Playwright MCP - GitHub](https://github.com/microsoft/playwright-mcp)
- [Sequential Thinking MCP](https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking)
- [Wispr Flow - Voice Dictation](https://wisprflow.ai/)
