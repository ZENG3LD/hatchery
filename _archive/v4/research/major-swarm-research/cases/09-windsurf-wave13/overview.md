# Windsurf Wave 13 & Cascade: Overview Report

## 1. Overview & Scale

### Wave 13 Key Features (Released December 24, 2025)

**Release Quote**: "Wave 13 introduced first-class support for parallel, multi-agent sessions in Windsurf, along with Git worktrees, side-by-side Cascade panes, and a dedicated terminal profile for more reliable agent execution."

#### SWE-1.5 Model - Free Tier
- **Availability**: Completely free for three months (through March 2026)
- **Performance**: "950 tokens per second—six times faster than Haiku 4.5 and 13 times faster than Sonnet 4.5"
- **Intelligence**: "Near-frontier SWE-1.5, with the same coding performance on SWE-Bench-Pro, but delivered at standard throughput speeds"
- **Architecture**: Frontier-size model with "hundreds of billions of parameters"
- **Training**: Developed using end-to-end reinforcement learning on real-world task environments
- **Benchmark**: Matches Claude Sonnet 4.5 on near-frontier coding performance
- **Infrastructure**: Trained on "state-of-the-art cluster of thousands of GB200 NVL72 chips," possibly the first production model using GB200 generation hardware
- **Base Model**: Built on "strong open-source model" (specific name not disclosed, speculation about Zhipu AI's GLM-4.6)

#### Parallel Agents Capacity
- **Concurrent Agents**: Five separate Cascade agents running simultaneously on different bugs
- **User Interface**: Multi-pane Cascade display for side-by-side agent monitoring
- **Real-world Usage**: "incident.io run[s] four to five Claude Code agents in parallel using Git worktrees"
- **Worktree Limit**: Windsurf maintains up to **20 worktrees per workspace**, automatically removing the oldest by last-access time when limits are exceeded
- **Tool Calls**: Up to 20 tool calls per prompt per agent
- **Conversation History**: Limit of 20 conversations; 21st conversation causes first to be deleted

#### Arena Mode
- **Purpose**: "Allow you to easily compare responses from different models on the same prompt"
- **Battle Groups**: Two curated model categories:
  - **Frontier**: Advanced reasoning models (GPT 5.2, Claude Opus/Sonnet 4.5, Gemini 3 Pro)
  - **Fast**: Speed-optimized models (SWE 1.5, Claude Haiku)
- **Isolation**: "Each model also getting its own worktree for isolation"
- **Anonymous Testing**: "Model identities remain hidden until convergence, when the original model names are revealed and the conversations are reshuffled"
- **Evaluation**: User-driven manual assessment (no automated scoring methodology disclosed)

### Scale Comparison

**Competitive Context (from byteiota analysis)**:
- Google Antigravity: Entirely free access
- Cursor: $20/month for 500 fast requests
- GitHub Copilot: $10/month individual, $19/month business
- Windsurf SWE-1.5: Free for 3 months (standard throughput), paid tier for maximum velocity

---

## 2. Architecture

### Multi-Agent Parallel Architecture

#### Git Worktree Foundation
**Core Mechanism**: "Git worktrees check out different branches into separate directories while sharing the same Git history"

**Technical Implementation**:
- **Storage Location**: `~/.windsurf/worktrees/<repo_name>` with unique random identifiers
- **Isolation**: Each Cascade conversation operates in its own isolated session
- **Shared Resources**: Single `.git` folder shared across all worktrees
- **File Copying**: Only git-tracked files copied to worktrees by default (configurable via `post_setup_worktree` hook)
- **Excluded Files**: `.env` files and non-version-controlled packages excluded from worktrees

**Limitations (from official docs)**:
> "Build systems or tools that rely on relative paths" may malfunction within worktrees since they reside outside the original project directory. This includes `../shared-lib` references, symlinked dependencies, or monorepo path-resolved dependencies.

#### Multi-Pane Cascade Interface
- **Tab System**: Multiple Cascade sessions viewable in separate panes and tabs within the same window
- **Side-by-Side Monitoring**: Developers can "monitor progress and compare outputs of sessions side-by-side"
- **Drag-and-Drop**: Interface supports dragging Cascade tab into main editor window for enhanced comparison
- **Independent Operation**: Each conversation can accept/reject changes and ask follow-up questions without affecting other sessions

#### Dedicated Terminal Architecture
- **Shell**: "Dedicated zsh shell specifically configured for reliability"
- **Configuration**: Preserves environment variables from `.zshrc`
- **Interactive Support**: Supports interactive prompts
- **Variable Inheritance**: "Dedicated zsh terminal with environment variable inheritance"
- **Platform**: Initially launched as opt-in on macOS

**Missing Technical Specifications** (NOT DISCLOSED):
- Process isolation mechanisms
- Signal handling details
- Inter-process communication protocols
- Architectural diagrams
- Performance benchmarks for parallel execution

### Planning Agent Architecture

**Dual-Model System**: "A specialized planning agent continuously refines the long-term plan while your selected model focuses on taking short-term actions"

**Division of Labor**:
- **Long-term Reasoning Model**: Understands overall project context and formulates strategies
- **Short-term Execution Model**: User-selected model focuses on generating code and editing files
- **Planning Artifact**: Persistent `plan.md` file acts as coordination mechanism

**Resource Efficiency**: "More powerful model handles long-term reasoning in the background, while the user-selected model focuses on executing short-term tasks based on the plan"

### Context & Flow Architecture

**Flow State Definition** (from marketing materials): "Cascade utilizes a graph-based reasoning system to map out the entire codebase's logic and dependencies, allowing Windsurf to maintain 'Flow'—a state of persistent context where the AI understands not just the current file, but the architectural intent of the entire project"

**Information Aggregation**:
- File tracking monitors edited and viewed files
- Terminal activity monitoring captures shell commands
- Clipboard integration for content transfer
- Visual element inspection through browser integration
- "Proprietary models built to ingest this shared timeline" (no technical specifications disclosed)

**Context Window**: "32K-64K tokens" of full codebase context

---

## 3. Communication

### Inter-Agent Communication: NOT DISCLOSED

**Finding**: No explicit inter-agent communication mechanism documented.

**Evidence**:
1. Git worktrees provide **isolation**, not collaboration: "Each Cascade conversation operates in its own isolated session"
2. Arena Mode emphasizes **independence**: "Users can work independently in each conversation—accepting or rejecting changes and asking follow-up questions—without affecting other sessions"
3. Worktree convergence is **manual**: After merging changes, user must click "X is better" button to discard other conversations
4. Search results for "inter-agent communication mailbox" returned **zero relevant results**

### Agent-to-Developer Communication

**Real-time Awareness** (single agent to developer):
- Monitors: "edits, commands, conversation history, clipboard, terminal commands"
- Infers intent and adapts in real time
- Reduces context-prompting requirements

**Checkpoints System**:
- Named snapshots of project state
- Stored in `Checkpoints` folder with timestamps
- "Navigate to and revert at any time"
- Documents changes between checkpoints for rollback capability

**Memory System**:
- **Storage**: `~/.codeium/windsurf/memories/`
- **Types**: Memories (auto-generated) + Rules (user-defined)
- **Content**: User stories, architectural decisions, process changes, technical standards
- **Retrieval**: "@-mention" references retrieve summaries, checkpoints, and relevant conversation parts (not full conversations)
- **Three-layer Structure**: Working Memory, Short-Term Memory, Long-Term Memory

### Developer-to-Agent Communication

**Queuing System**: "Users to stack messages for sequential execution while the agent works"

**Todo Lists**: "Complex tasks with automatic updates based on discovered information"

**Cascade Hooks**: "Execute custom commands at key workflow points for auditing"

---

## 4. Git & Code Integration

### Worktree Setup Process

**Initialization**:
1. Switch to "Worktree" mode via toggle in bottom right of Cascade input
2. **Restriction**: "This assignment must occur at the session's start—once a conversation begins, it cannot be relocated to a different worktree"
3. Windsurf organizes worktrees by repository within `~/.windsurf/worktrees/<repo_name>`
4. Each assigned unique random identifier

**Inspection**:
```bash
git worktree list  # View active worktrees from repository directory
```

**Visibility**: "SCM Panel suppresses worktree visibility by default; enable `git.showWindsurfWorktrees` in settings to visualize them"

### Merge Workflow

**From Worktree to Main**:
1. After Cascade completes file modifications in worktree
2. User clicks "merge" button
3. "Integrate those changes back into your main workspace"
4. "Selective acceptance of modifications without committing all experimental work"

**Cleanup**:
- Manual Cascade conversation deletion triggers automatic worktree removal
- Oldest worktrees removed when 20-limit exceeded

### Conflict Handling Strategy

**Prevention via Isolation**: "Git worktrees—a feature since Git 2.5 that was previously niche—suddenly became critical infrastructure. The technology enables multiple working directories from a single repository, each checking out a different branch while sharing the same .git folder. This eliminates duplication and solves the conflict problem that previously blocked parallel agent workflows."

**User Workflow**: "Developers can cd between directories without stashing or committing incomplete work"

**NOT DISCLOSED**:
- Automatic conflict resolution algorithms
- Synchronization protocols when multiple agents edit overlapping files
- Merge strategy customization options
- Conflict detection mechanisms

### Post-Setup Hook

**Trigger**: `post_setup_worktree` hook executes automatically after worktree creation

**Context**:
- Runs within new worktree directory
- `$ROOT_WORKSPACE_PATH` variable references original workspace
- Facilitates file access and command execution

**Use Cases**:
- Copy `.env` files excluded by default
- Establish symlinks for dependencies
- Install packages not tracked in version control

---

## 5. What Worked & What Failed

### What Worked: Parallel Debugging Benefits

#### incident.io Case Study
**Source**: incident.io blog post "How we're shipping faster with Claude Code and Git Worktrees"

**Adoption Scale**: "They've gone from no Claude Code to simultaneously running four or five Claude agents, each working on different features in parallel"

**Workflow Benefits**:
- "Ability to treat AI coding sessions like long-running processes, with ongoing, focused dialogues about specific features"
- "All the context preserved in the branch and worktree"
- "Build times are faster, feature development is accelerated"
- "Spending more time on the creative work of product development rather than fighting with tooling"

**Engineering Practice Shift**: "Senior engineers managing coordination overhead" as parallel agent workflows become validated practice

#### Speed Advantages
- **Token Generation**: 950 tokens/second enables faster multi-agent throughput
- **Speculative Decoding**: "Custom draft model for speculative decoding"
- **Priority System**: "Custom request priority system built for smooth agent sessions under load"
- **Overhead Reduction**: Team "rewrote critical components like lint checking and command execution" to eliminate bottlenecks, reducing overhead by up to 2 seconds per step

### What Failed: Review Burden & Quality Concerns

#### DORA 2025 Report Finding
**Critical Bottleneck**: "PR review time ballooned by approximately 91% in teams using AI, with the human approval loop becoming the choke point"

**Analysis**: "This matches Amdahl's Law: speeding up code only helps if reviews and testing keep pace"

**Time Shift**: "AI is shifting where time gets spent in your development process, developers spend less time writing initial code and more time reviewing and validating it"

**Organizational Amplification**: "AI doesn't create organizational excellence—it amplifies what already exists"

#### Code Quality Warnings
**Volume ≠ Value**: "More code volume does not guarantee better architecture"

**Junior Developer Risk**: Concerns about "junior developers managing coordination overhead" when dealing with multiple parallel agents

**Context Lost**: User reports indicate "crashes during extended sequences, shell path bugs, and AI consistency that fluctuates between releases"

#### Practical Execution Gaps
**Instability Issues** (from byteiota user feedback):
- "Wasted credits and unstable performance"
- "Lower-tier models producing verbose code"
- "Rate-limiting issues interrupting workflow continuity"
- "Crashes during extended sequences"

**Recommendation**: "Testing Windsurf's free tier while maintaining Cursor or Copilit as production backups"

### Missing Data (NOT DISCLOSED)

- **Benchmark Metrics**: No public data on parallel agent efficiency gains
- **Error Rates**: No published statistics on merge conflicts or coordination failures
- **User Satisfaction**: No disclosed user studies or retention metrics for parallel workflows
- **Performance Monitoring**: No tools or dashboards for tracking multi-agent system health

---

## 6. Open Source & Artifacts

### SWE-1.5 Model Access

**Closed Source**: SWE-1.5 is NOT open source

**Disclosed Technical Details**:
- Frontier-size model with hundreds of billions of parameters
- Built on "strong open-source model" after "careful evals and ablations" (base model name NOT disclosed)
- **Speculation**: "Beijing-based Zhipu AI's GLM series of foundational models, with Zhipu AI claiming that SWE-1.5 used its latest flagship GLM-4.6 as the base model" (from SCMP article)
- Trained with "variant of unbiased policy gradient" for long multi-turn trajectories
- Training at "relatively small scale" per authors' acknowledgment

**Inference Infrastructure**:
- Served by Cerebras (not open source)
- "Custom draft model for speculative decoding" (proprietary)

**Training Environment** (NOT OPEN SOURCE):
- "Cascade agent harness on top of a leading open-source base model"
- Three grading mechanisms: classical tests, rubrics, agentic grading with browser-use agents
- GB200 NVL72 cluster infrastructure

### Arena Mode Data

**NOT OPEN SOURCE**: No public datasets, benchmark results, or evaluation frameworks released

**Disclosed Information**:
- Battle Groups configuration (Frontier vs Fast models)
- User-driven evaluation methodology (manual selection)
- Model identity blinding mechanism

**Missing**:
- Comparative performance data
- User preference statistics
- Win-rate matrices between models

### Community Open Source Projects

#### 1. windsurfinabox
**Repository**: `https://github.com/pfcoperez/windsurfinabox`

**Purpose**: "Windsurf's Cascade agent within a Docker image, to be used in headless mode"

**Technical Stack**:
- Shell (69.6%), Dockerfile (30.4%)
- Apache-2.0 License
- Xvfb for virtual X11 display
- xdtool for UI automation

**Architecture**:
- Authentication via `WINDSURF_TOKEN` environment variable
- Automated task cycle: reads `windsurf-instructions.txt` → executes → logs to `windsurf-output.txt`
- Signals completion with "WORK-COMPLETED"
- UID:GID=1000:1000 for workspace permissions

**Use Case**: "Integration into CI/CD pipelines and automated code management workflows"

#### 2. windsurf-demo
**Repository**: `https://github.com/Exafunction/windsurf-demo`

**Purpose**: "Learn hands on how to use the Windsurf Editor"

**Content**: Full-stack web application (Python Flask + JavaScript) demonstrating:
- Deep reasoning over knowledge (multi-file edits)
- Human action integration (contextual awareness)
- Tool access (terminal commands, stacktrace debugging)

**Cascade Capabilities Showcased**:
- "Search through existing codebases"
- "Create multi-file multi-edit changes in a manner that is self-consistent"
- "Reason about the actions that you are taking in the text editor"
- "Suggest terminal commands and execute them"
- "Debug stacktraces by identifying and reasoning about relevant code"

#### 3. windsurf-antigravity-rules
**Repository**: `https://github.com/kinopeee/windsurf-antigravity-rules`

**Purpose**: "Optimized adaptation of cursorrules for Windsurf Cascade"

**Content**: Custom instructions optimized for Windsurf and Antigravity (Google's free AI coding tool)

#### 4. cascade-memory-bank
**Repository**: `https://github.com/GreatScottyMac/cascade-memory-bank`

**Purpose**: "Intelligent project memory system for Windsurf IDE"

**Features**:
- "Empowers Cascade AI to maintain deep context across sessions"
- "Automatically documenting decisions, progress, and architectural evolution"
- "Perfect for complex projects that demand consistent understanding over time"

#### 5. awesome-windsurf
**Repository**: `https://github.com/ichoosetoaccept/awesome-windsurf`

**Purpose**: "A collection of awesome resources for working with the Windsurf code editor"

**Content**: Curated hub of community-contributed prompts, resources, tips

#### 6. ccpm (Claude Code Project Manager)
**Repository**: `https://github.com/automazeio/ccpm`

**Purpose**: "Project management system for Claude Code using GitHub Issues and Git worktrees for parallel agent execution"

**Relevance**: Demonstrates community extending git worktree + parallel agent pattern beyond Windsurf

### MCP (Model Context Protocol) Extensions

**Official Documentation**: `https://docs.windsurf.com/windsurf/cascade/mcp`

**Configuration File**: `~/.codeium/windsurf/mcp_config.json`

**Architecture**: "Windsurf acts as an MCP host, and its integrated AI assistant, Cascade, functions as the MCP client"

**Community MCP Servers** (examples from GitHub):
1. **GitHub MCP Server**: `github/github-mcp-server` (official Docker image: `ghcr.io/github/github-mcp-server`)
2. **Deephaven MCP**: `deephaven/deephaven-mcp`
3. **AWS CodePipeline MCP**: `cuongdev/mcp-codepipeline-server`
4. **LM Studio Integration**: Plugin for VS Code and Windsurf
5. **FastAPI MCP Tools**: Endpoints exposed as MCP tools with authentication

**Open Source MCP Resources**:
- "Official MCP server reference repository or OpenTools for some example servers"
- Community tutorials at `windsurf.com/university/tutorials/configuring-first-mcp-server`

### Windsurf Core: NOT OPEN SOURCE

**Windsurf Editor**: Proprietary (built on VS Code OSS but closed source)

**Cascade Agent**: Proprietary

**Planning Agent**: Proprietary

**Worktree Integration**: Proprietary implementation (uses open-source Git worktrees but Windsurf's orchestration layer is closed)

**Hooks System**: Configuration interface open, execution engine proprietary

### Enterprise Features (NOT OPEN SOURCE)

**Cascade Hooks Documentation**: `https://docs.windsurf.com/windsurf/cascade/hooks`

**Features**:
- Pre-prompt and post-response hooks for compliance
- SOC 2 audit logging capability
- Data sanitization and policy enforcement
- MDM policy deployment for rules/workflows

**Configuration**: Shell scripts with user-level permissions (security responsibility on user)

---

## Summary

Windsurf Wave 13 represents a significant shift toward **parallel multi-agent workflows** via git worktrees, enabling up to 5 concurrent Cascade agents working on isolated branches. The **SWE-1.5 model** (free for 3 months) delivers frontier-level performance at 950 tokens/second, 6-13x faster than competing models.

**Architecture**: Each agent operates in isolated worktrees with dedicated terminals. A dual-model planning system separates long-term strategy from short-term execution. **NO inter-agent communication mechanisms disclosed**—agents work independently and merge manually.

**What Worked**: incident.io demonstrated 4-5 parallel agents accelerating feature development. Speed optimizations (GB200 training, Cerebras serving, custom draft models) enable high-throughput parallel workflows.

**What Failed**: DORA 2025 report shows **91% PR review time increase**, exposing the human bottleneck. User reports cite crashes, inconsistency, and rate-limiting issues. Code volume ≠ quality concerns persist.

**Open Source**: Core Windsurf/Cascade is **proprietary**. Community created Docker containers (windsurfinabox), demo apps, memory systems, and MCP servers. SWE-1.5 base model name **NOT disclosed** (speculation: Zhipu GLM-4.6). No benchmark datasets or Arena Mode results released.

**Key Limitation**: Parallel agents **do not communicate**—they are isolated workers coordinated manually by developers via git merge operations. This is fundamentally different from true swarm architectures with inter-agent messaging.

---

## Sources

- [Windsurf Wave 13 Changelog](https://windsurf.com/changelog/windsurf-next)
- [Windsurf Wave 13: Merry Shipmas](https://windsurf.com/blog/windsurf-wave-13)
- [Windsurf Cascade Documentation](https://windsurf.com/cascade)
- [Cascade Technical Docs](https://docs.windsurf.com/windsurf/cascade/cascade)
- [Windsurf Worktrees Documentation](https://docs.windsurf.com/windsurf/cascade/worktrees)
- [Arena Mode Documentation](https://docs.windsurf.com/windsurf/cascade/arena)
- [Introducing SWE-1.5 (Windsurf Blog)](https://windsurf.com/blog/swe-1-5)
- [Introducing SWE-1.5 (Cognition Blog)](https://cognition.ai/blog/swe-1-5)
- [Windsurf Wave 13: Free SWE-1.5, Parallel Agents (byteiota)](https://byteiota.com/windsurf-wave-13-free-swe-1-5-parallel-agents-escalate-ai-ide-war/)
- [Windsurf Cascade: GPT-5.2, Codex, Gemini 3 (byteiota)](https://byteiota.com/windsurf-cascade-gpt52-codex-gemini-3-jan-2026-2/)
- [incident.io: Shipping Faster with Claude Code and Git Worktrees](https://incident.io/blog/shipping-faster-with-claude-code-and-git-worktrees)
- [DORA Report 2025 Key Takeaways (Faros AI)](https://www.faros.ai/blog/key-takeaways-from-the-dora-report-2025)
- [DORA State of AI-assisted Software Development 2025](https://dora.dev/research/2025/dora-report/)
- [Cascade Hooks Documentation](https://docs.windsurf.com/windsurf/cascade/hooks)
- [Cascade MCP Integration](https://docs.windsurf.com/windsurf/cascade/mcp)
- [GitHub: windsurfinabox](https://github.com/pfcoperez/windsurfinabox)
- [GitHub: windsurf-demo](https://github.com/Exafunction/windsurf-demo)
- [GitHub: windsurf-antigravity-rules](https://github.com/kinopeee/windsurf-antigravity-rules)
- [GitHub: cascade-memory-bank](https://github.com/GreatScottyMac/cascade-memory-bank)
- [GitHub: awesome-windsurf](https://github.com/ichoosetoaccept/awesome-windsurf)
- [GitHub: ccpm](https://github.com/automazeio/ccpm)
- [GitHub: github-mcp-server](https://github.com/github/github-mcp-server)
- [SWE-Bench Pro Leaderboard](https://scale.com/leaderboard/swe_bench_pro_public)
- [AI Coding Tools Face Scrutiny Over Chinese Model Origins (SCMP)](https://www.scmp.com/tech/tech-trends/article/3331451/ai-coding-tools-built-us-firms-face-scrutiny-over-chinese-model-origins)
- [Show HN: Windsurf – Agentic IDE (Hacker News)](https://news.ycombinator.com/item?id=42127882)
- [Windsurf Wave 13 Enables Parallel Multi-Agent Coding (ASCII News)](https://ascii.co.uk/news/article/news-20251226-ae3c8d8c/windsurf-wave-13-enables-parallel-multi-agent-coding-with-gi)
- [Windsurf Wave 10: Planning Mode (wain.blog)](https://wain.blog/en/windsurf-planning-mode-wave10-Kn4HF5NQ/)
- [Understanding Windsurf's Memories System (Arsturn)](https://www.arsturn.com/blog/understanding-windsurf-memories-system-persistent-context)
