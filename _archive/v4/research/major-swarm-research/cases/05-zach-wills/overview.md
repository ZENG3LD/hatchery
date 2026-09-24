# Zach Wills: Managing 20 AI Agents for One Week

## 1. Overview & Scale

### Project Description
Zach Wills built an **internal engineering analytics platform** in one week using a swarm of ~20 parallel AI agents. The platform was designed for engineering organizations to:
- Ingest GitHub repository data
- Detect AI tool usage patterns and adoption rates
- Measure developer velocity and team efficiency impacts
- Track real-world impact of AI coding tools on productivity

The platform includes:
- User authentication
- Background jobs
- LLM integrations
- Third-party API integrations
- ~800 automated tests
- CI/CD pipeline integration

### Scale Metrics
- **Timeline**: 1 week (August 2025)
- **Commits**: ~800
- **Pull Requests**: 100+
- **Tests**: ~800 test cases
- **Agents**: ~20 parallel agents managed simultaneously
- **Cost**: ~$6,000 in Claude Code credits (primarily Opus model)
- **Active Work Hours**: ~3-4 hours per day (cognitive load limited sustained orchestration)
- **Development Environment**: 4+ terminals running simultaneously

### Business Context
The platform was built to replace a ~$30,000/year vendor solution. Wills described it as a "respectable alpha version" that was internally used but not initially public. He later explored potential open-sourcing.

### Key Innovation
The core breakthrough was **stopping linear coding** and instead **orchestrating a swarm of ~20 parallel AI agents**, each working on independent tasks simultaneously. This parallelization eliminated the single-threaded bottleneck of traditional development.

## 2. Architecture

### Organizational Structure

**Human as Central Coordinator**
- Wills acted as the primary orchestrator/conductor
- Not fully autonomous - required continuous human direction and intervention
- Manually interjected "when I saw them going off track (which happened!)"
- Cognitive load: "completely burnt after about 3 hours" of active orchestration

**Specialized Sub-Agent Assembly Line**
The workflow used role-based specialization with handoffs:

1. **Solution Architect** → Generates detailed technical plans
2. **Senior Engineer** → Implements approved designs
3. **Dedicated Tester** → Validates via autonomous test loops
4. **Code Reviewer** → Provides structured feedback (iterative loop back to engineer)

**Parallel Execution Model**
- Independent tasks run concurrently (e.g., backend API + frontend UI + tests + documentation)
- Each agent receives fresh 200k token context window
- Completion time = longest single task (not sum of all tasks)

**Core Trio for Feature Planning** (runs in parallel):
- **Product Manager**: Defines user stories and acceptance criteria
- **UX Designer**: Proposes user flows, wireframes, all states (loading, empty, error, success)
- **Senior Engineer**: Outlines technical approach, risks, dependencies, effort

### Tools & Technology Stack

**Primary IDE**: Claude Code with sub-agents

**MCPs (Model Context Protocol Servers)**:
- **Serena**: Semantic codebase editing toolkit (IDE-like capabilities for deep code understanding)
- **Playwright MCP**: Headless browser control for autonomous test loops
- **Neon Databases MCP**: Isolated branched database management for parallel workstreams
- **Sequential Thinking MCP**: Enforces planning-before-execution discipline
- **Linear MCP**: Ticket creation and task tracking

**Supporting Tools**:
- **Wispr Flow**: Voice-to-text dictation for prompt composition
- **Claude Opus**: Primary model (noted as "significantly better than Sonnet" for this work)
- **GitHub**: Version control and PR-based checkpointing
- **Linear**: Task tracking and memory persistence

### Self-Improving Documentation
**CLAUDE.md File**:
- Self-updating documentation capturing learnings per task
- Agents automatically update this file based on directives
- Acts as persistent memory across agent restarts
- Continuously refined based on project-specific patterns

## 3. Communication

### Human-to-Agent Communication

**Voice Dictation (Primary Input Method)**:
- Used Wispr Flow for initial task specification
- "Dictate and word vomit all of my thoughts to AI" for rich context
- Voice naturally includes more contextual reasoning than typing
- Provides richer context and nuance compared to written prompts

**Slash Commands**:
- `/spike`: Small tasks
- `/tech plan`: Larger planning tasks
- `/task`: Net-new development tasks
- `/fix`: Bug fixes
- Custom commands stored in `~/.claude/commands/` or `./.claude/commands/`

**Command-Driven Workflow**:
Process often started with high-level command → Co-author refined plan with AI → Hand off to agent team for execution

### Agent-to-Agent Communication

**Artifact-Based Handoffs** (NOT direct communication):
- Agents communicate through structured outputs/artifacts
- Examples: tickets, design briefs, code reviews, implementation notes
- Creates clear "audit trail" and prevents context pollution
- Each artifact has structured sections (business context, acceptance criteria, dependencies, etc.)

**Checkpoint-Based Progress Sharing**:
- Markdown files
- GitHub PR comments
- Linear tickets
- Agents post automatic status updates to PR threads

**Sequential Handoffs**:
- Output from one agent feeds into the next
- Example: Planning → Implementation → Code Review → Refinement Loop
- Creates an automated assembly line
- Each specialist receives dedicated context window focused on its task

### Coordination Method

**Central Orchestrator Pattern**:
- Wills as main orchestrator maintains high-level oversight
- Assigns tasks to specialized sub-agents
- Each agent operates in isolated context
- Main orchestrator synthesizes parallel outputs (the "reduce" step)

**Complexity-Based Triggering**:
- **LIGHT** (typos, minor edits): No agents needed
- **STANDARD** (new features): Core Trio at standard depth
- **DEEP** (vague or complex): Core Trio with extended investigation scope

**Memory Management**:
- Checkpoint progress to persistent sources (GitHub, Linear, markdown)
- Restart agents with fresh context after major milestones
- Sub-agent specialization prevents single context window exhaustion

## 4. Git & Code Integration

### Branching Strategy

**Branch-Based Isolation**:
- Each parallel workstream operates on separate branch
- 4+ terminals running simultaneously on different branches
- Isolated dev environments prevent conflicts

**Neon Database Branching**:
- Isolated, branched databases per parallel workstream
- Enables true parallel development without database conflicts
- Cornerstone of the parallelization workflow

### PR Workflow

**Frequent PRs**:
- 100+ PRs in one week (~14-20 PRs per day)
- PRs used as progress checkpointing mechanism
- GitHub PR comments serve as agent communication channel

**PR Comments for Coordination**:
- Agents post automatic status updates to PR threads
- Creates audit trail of agent work
- Enables async review and intervention

**Linear Integration**:
- Linear tickets linked to PRs
- Tickets contain structured information from agent planning phase
- Memory persistence across agent restarts

### Merge Strategy

**CI/CD Integration from Day One**:
- ~800 tests maintained throughout
- Tests run autonomously on each PR
- Agents refine code until tests pass

**Autonomous Test Loops**:
- Playwright-based browser automation validates UI changes
- Agents self-correct based on test failures
- "Build → test → validate → fix → complete" cycle
- Minimal human intervention in testing phase

**Code Review Process**:
- Code reviewer agent provides structured feedback
- Severity levels: Blockers, High Priority, Medium Priority
- Specific file:line feedback with actionable suggestions
- Iterative loop until approval

**Early Commit Strategy** (Rule #8):
- Commit early and often as safety net
- Preserves progress and enables easy rollback
- Branched approach allows experimentation without risk

### Quality Control

**Database Inspection**:
- Query inspection and performance optimization performed mid-week
- Database management integrated into agent workflow

**Test Coverage**:
- ~800 tests across codebase
- Dedicated tester agent generates comprehensive test suites

## 5. What Worked & What Failed

### The 8 Rules (VERBATIM)

1. **"Align on the Plan, Not Just the Goal"**
2. **"A Long-Running Agent is a Bug, Not a Feature"**
3. **"Actively Manage the AI's Memory"**
4. **"Manage Context with Sub-Agents"**
5. **"Trust the Autonomous Loop"**
6. **"Automate the System, Not Just the Code"**
7. **"Be Ruthless About Restarting"**
8. **"Commit Early and Often"**

### What Worked

**Parallelization Eliminated Bottleneck**:
- Multiple independent tasks completed simultaneously
- Completion time = longest task, not sum of all tasks
- Dramatic velocity increase compared to sequential development

**Autonomous Test Loops Enabled Self-Correction**:
- Agents iteratively fixed code until tests passed
- Reduced human intervention in debugging
- Playwright MCP enabled browser-based validation

**Memory Checkpointing Prevented Context Drift**:
- Self-updating CLAUDE.md captured learnings
- GitHub/Linear integration preserved progress
- Fresh context spawning for specialized roles avoided token limits

**Voice Prompting Provided Richer Context**:
- Dictation naturally included more reasoning
- Faster input than typing
- More nuanced task descriptions

**Specialist Sub-Agents Maintained Quality**:
- Each agent focused on single responsibility
- Dedicated 200k context per specialist
- Prevented context exhaustion from multi-tasking

**Sequential Thinking MCP Reduced Intent Drift**:
- Forces AI to outline plan before executing
- Dramatically reduced agents going off-track
- Kept agents aligned with original goals

**Artifact-Based Handoffs Created Clarity**:
- Structured outputs served as clear interfaces
- Audit trail for debugging
- Prevented context pollution between agents

### What Failed / Challenges

**Extreme Cognitive Load**:
- "Completely burnt after about 3 hours"
- Difficult to scale beyond 3-4 hours of active orchestration
- Constant monitoring required to catch drift

**Requires Ruthlessness About Killing Off-Track Agents**:
- Agents frequently went off track
- Required manual intervention and restarts
- Human judgment critical for course correction

**High Token Costs**:
- $6,000/week in Claude credits
- Equivalent to senior engineer weekly salary
- Chaining agents increases token usage significantly
- Quickly exhausts API quotas (Claude Pro/Max limits)

**Synthesis is the Hardest Part**:
- "Reduce" step where orchestrator consolidates parallel work is most fragile
- Quality depends heavily on synthesis prompt
- Mitigation: Each sub-agent saves output to distinct files

**Non-Determinism Makes Debugging Difficult**:
- Changing one agent prompt ripples unpredictably downstream
- LLM updates cause subtle behavioral changes
- Requires rigorous evaluation suites

**Fragile Prompt Dependencies**:
- Agent definitions brittle like code
- Require testing and versioning
- Lack automation tooling for validation
- Model drift between updates breaks workflows

**Scalability Concerns**:
- Community questioned long-term maintainability of AI-generated code
- Extensive micromanagement contradicts efficiency gains
- Unclear if approach scales beyond alpha prototypes

**Reception of AI-Enhanced Writing**:
- Multiple commenters criticized blog post's "AI-enhanced" tone
- Phrases like "shift from laser to powerful floodlight" felt overly flowery
- Writing "smells like AI" according to community feedback

### Biggest Lessons Learned

**Plan Quality > Implementation Quality**:
- "Cheaper to fix bad plan than bad implementation"
- Co-author plans before execution
- Align on refined plan before handing to agents

**Tight Rhythm Maintains Velocity**:
- Plan → Short execution burst → Checkpoint → Reset
- Prevents cognitive tax
- Keeps velocity high without burnout

**Agent Prompts are Code**:
- Version-controlled like source code
- Monitored for behavioral drift after model updates
- Require same discipline as production code

**Defaults Matter for Velocity**:
- Tickets default to MVP-first delivery
- "80/20" scope - 80% value with 20% effort
- Work >2 days automatically broken into 2-3 smaller tickets

**Fresh Context is a Feature, Not a Bug**:
- Restart agents regularly to prevent drift
- Each milestone = fresh context
- Isolated context per specialist maintains quality

## 6. Open Source & Artifacts

### Analytics Platform Status

**NOT OPEN SOURCE** (as of publication date):
- Built as internally used product for Wills' organization
- Wills stated: "I'll talk to the team. This particular build is an internally used product.. but maybe we could open source it."
- No public repository mentioned

### Shared Tooling

**No Custom Code Shared**:
- No custom parallelization script published
- No agent definition templates released
- No orchestration framework open-sourced

**Referenced Open Source Tools**:
- Serena MCP (already open source)
- Playwright MCP (Microsoft open source)
- Neon Databases MCP (open source)
- Sequential Thinking MCP (open source)
- All are existing MCP servers, not created by Wills

### Documentation Shared

**Blog Posts** (primary artifacts):
- "I Managed a Swarm of 20 AI Agents for a Week and Built a Product. Here Are the 8 Rules I Learned."
- "How to Use Claude Code Subagents to Parallelize Development"
- "How I Used Claude Code Subagents to Create an 18-Month Roadmap in 2 Hours"
- "My 2026 AI Bets (A Time Capsule)"

These blog posts contain:
- Conceptual frameworks
- Workflow descriptions
- Best practices
- Example agent structures (high-level)
- Lessons learned

**No Code Artifacts**:
- No agent definition files (.md) shared
- No command definition files shared
- No CLAUDE.md template provided
- No orchestration scripts published

### Community Response

**Viral Engagement**:
- Hacker News discussion (item #45041906)
- LinkedIn shares and discussions
- Republished on multiple platforms (Salas Blog, etc.)

**Skepticism**:
- Lack of visible product demonstration
- Questioned "production ready" vs alpha status
- Concerns about maintainability
- High cost ($6k/week) questioned

**Interest**:
- Strong interest in parallelization approach
- Questions about implementation details
- Requests for code/templates

## Sources

- [I Managed a Swarm of 20 AI Agents for a Week and Built a Product. Here Are the 8 Rules I Learned.](https://zachwills.net/i-managed-a-swarm-of-20-ai-agents-for-a-week-here-are-the-8-rules-i-learned/)
- [I built a production app in a week by managing a swarm of 20 AI agents - Hacker News](https://news.ycombinator.com/item?id=45041906)
- [My 2026 AI Bets (A Time Capsule)](https://zachwills.net/my-2026-ai-bets-a-time-capsule/)
- [How to Use Claude Code Subagents to Parallelize Development](https://zachwills.net/how-to-use-claude-code-subagents-to-parallelize-development/)
