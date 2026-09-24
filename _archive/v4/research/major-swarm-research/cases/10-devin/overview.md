# Devin 2.0 by Cognition: Overview & Architecture

## 1. Overview & Scale

### Product Evolution
**Devin 1.0 → 2.0 Transformation:**
- Released April 2025, Devin 2.0 represents a complete overhaul addressing both capability limitations and accessibility barriers
- **83% more tasks per ACU** (Agent Compute Unit) compared to Devin 1.x through improved reasoning, better error recovery, and smarter resource allocation
- Pricing dramatically reduced from $500/month (v1.0) to $20/month Core plan (v2.0)

### Performance Metrics
**Speed & Efficiency (2025 Performance Review):**
- **4x faster** at problem solving compared to previous year
- **2x more efficient** in resource consumption
- **67% merge rate** (up from 34% in 2024)
- **250,000+ PRs merged** since launch across customer base

**Benchmark Results:**
- **SWE-bench:** 13.86% success rate (79 of 570 issues resolved)
- Previous state-of-the-art: 1.96% (Claude 2: 4.80% assisted)
- Evaluated on randomly chosen 25% of SWE-benchmark test set (570 of 2,294 issues)
- 45-minute runtime limit per task

### Enterprise Adoption
**Major Customers:**
- Goldman Sachs (planning hundreds to thousands of Devin instances)
- Citi
- Santander
- Nubank
- EightSleep (ships 3x as many data features with Devin)

**Usage Scale:**
- Thousands of companies using Devin
- Devin now produces 25% of Cognition's own code
- One large organization saved 5-10% of total developer time using Devin for security fixes

### Key Strengths
**What Works Well:**
- Tasks with clear, upfront requirements and verifiable outcomes
- Work that would take a junior engineer 4-8 hours
- Data analysis and quality assurance (unexpectedly strong)
- Vulnerability resolution: **20x efficiency gain** (human: 30 min/vuln, Devin: 1.5 min/vuln)
- Infinitely parallelizable, never sleeps

**What Doesn't Work:**
- Mid-task requirement changes (performs worse with iterative coaching)
- Managing reports or stakeholders
- Handling teammates' emotions
- Code quality still requires human review

---

## 2. Architecture

### Agent-Native Development Philosophy
Devin 2.0 introduces "agent-native IDE experience" — a fundamental shift from adapting human-focused tools to building purpose-designed environments for AI agent workflows.

### Cloud Infrastructure
**Isolated Virtual Machines:**
- Each Devin session runs in its own isolated VM
- Cloud-based IDE powered by Visual Studio Code Web
- Sandboxed compute environment with shell, code editor, and browser
- Eliminates conflicts between parallel sessions

**Tools Available:**
- Terminal (environment setup, tests)
- Code editor (read/modify files)
- Browser (research, interact with web apps)
- MCP server connections (Notion, Sentry, Datadog, etc.)

### Multi-Agent Capabilities

**Parallel Execution Model:**
- Users can spin up **multiple parallel Devin instances**
- Each instance equipped with its own interactive, cloud-based IDE
- One Devin can dispatch sub-tasks to others for concurrent execution
- Engineers shift role from implementer to manager (reviewing PRs, providing feedback)

**Advanced Mode & API:**
- Batch sessions for frequent parallel operations
- Devin API for programmatic session creation and structured result retrieval

### Training Methodology
**Development Approach:**
- Combination of training large language models (similar to GPT-4)
- Aspects from **reinforcement learning** for reasoning and long-term planning
- Enables thousands of decisions, contextual recall, learning over time, and self-correction

### Memory & Context Management
**File System as Memory:**
- Devin treats the file system as its memory without prompting
- Provides alternative to keeping everything in context window during long-running tasks

**Context Window Management:**
- Performance degrades after ~10 ACUs (~2.5 hours of activity)
- Related to effective context window limits of underlying LLMs
- Session history eventually cannot fit, causing costs to increase per LLM call

**Solutions Implemented:**
- Product nudges recommend keeping sessions under 10 ACUs
- Enabled 1M token beta while capping usage at 200k tokens
- Gives model perception of having "plenty of runway" without anxiety-driven shortcuts

### Agent Compute Unit (ACU) Pricing Model
**Resource Measurement:**
- ACU = normalized measure of computing resources (VM time, model inference, networking bandwidth)
- ~1 ACU = 15 minutes of active work
- ~1 hour = $8-$9 depending on plan

**Pricing Tiers:**
- Core plan: $2.25 per ACU (pay-as-you-go)
- Teams plan: $2.00 per ACU (includes 250 ACUs/month)

**ACU Consumption:**
- Charged during active work or when VM is running
- NOT charged for: idle time, waiting for user responses, long-running tests, repo setup/cloning

---

## 3. Communication & Coordination

### Interactive Planning (Devin 2.0)
**Workflow:**
1. User starts session with broad/incomplete idea
2. Devin responds in seconds with:
   - Relevant files
   - Findings
   - Preliminary plan (with code citations and snippets)
3. User modifies plan to ensure alignment
4. Devin executes autonomously

**Key Features:**
- Automatic codebase analysis
- Deep-link citations directly into Devin IDE
- Human-in-the-loop governance with two non-negotiable checkpoints:
  - **Planning Checkpoint:** Review/adjust plan before execution
  - **Pull Request Checkpoint:** Review/approve before merge

### Agent-to-Agent Dispatch

**How It Works:**
- Later revisions of Devin support multi-agent operation
- One AI agent dispatches tasks to other AI agents
- Each agent runs in isolated VM with own IDE
- Parallel execution for independent tasks
- Sequential coordination for dependent work

**Dispatch Mechanism (NOT DISCLOSED):**
- Internal coordination architecture not publicly documented
- Likely uses message-passing between VMs
- Cognition has not published detailed technical specifications of dispatch protocol

### Integration with Team Workflows
**Slack/Teams Integration:**
- "@" mention Devin in channels
- Natural language requests: "can you pull yesterday's sales by channel?"
- Receives task assignments directly in chat

**GitHub Integration:**
- Creates branches automatically
- Opens pull requests with detailed descriptions
- Commits include test runs and local security scanning

**VS Code Live Share:**
- Real-time collaboration with human engineers
- Maintains presence within existing tools rather than requiring adoption of new platforms

---

## 4. Git & Code Integration

### PR Workflow
**Automated PR Creation:**
- Devin autonomously creates branches and PRs
- Runs tests before initiating PRs
- Performs local security scanning before submission
- Includes detailed commit messages and PR descriptions

**Merge Rate Improvement:**
- 2024: 34% of PRs merged
- 2025: 67% of PRs merged
- Nearly doubled acceptance rate through improved code quality and test coverage

### Code Quality Controls
**Human Review Requirements:**
- Code quality assurance remains necessary
- Testing logic verification required
- Engineers act as final reviewers rather than primary implementers

**Security Scanning:**
- Integration with SAST (Static Application Security Testing)
- Integration with SCA (Software Composition Analysis)
- Gathers security scanning results from code and dependencies
- Provides code changes addressing vulnerabilities based on tool recommendations

### Repository Understanding (DeepWiki)
**Auto-Generated Documentation:**
- Automatically indexes repositories
- Produces wikis with architecture diagrams (Mermaid.js)
- Links to source code
- Summaries of codebase
- Continuously updates as codebase evolves

**Architecture Visualization:**
- Class hierarchies
- Dependency graphs
- Workflow charts
- Clickable diagrams for exploration

**Access Method:**
- Replace `github.com` with `deepwiki.com` in repository URL
- No installation or complex setup required

**Configuration:**
- `.devin/wiki.json` file steers wiki generation behavior
- Supports custom `repo_notes` and `pages` configuration

### MCP (Model Context Protocol) Integration
**What is MCP:**
- Open protocol by Anthropic (November 2024)
- Standardizes how LLMs integrate with external tools/data sources
- Available in Devin's MCP Marketplace (Settings)

**MCP Server Integration:**
- One-click enable for many MCPs
- Connect service accounts during session
- DeepWiki MCP server provides programmatic access to GitHub repos indexed on DeepWiki
- Tools: `ask_question`, `read_wiki_contents`, `read_wiki_structure`
- Enables connections to Notion, Sentry, Datadog, and other platforms

---

## 5. What Worked & What Failed: The "Don't Build Multi-Agents" Argument

### Cognition's Core Argument Against Multi-Agents

**Source:** Cognition blog post "Don't Build Multi-Agents" (June 2025)

**Main Thesis:**
> "libraries such as OpenAI's Swarm and Microsoft's AutoGen actively push concepts which I believe to be the wrong way of building agents."

Multi-agent architectures are fundamentally unreliable for production systems because they violate two critical principles of context engineering.

### Two Fundamental Principles

**Principle 1 - Context Sharing:**
> "Share context, and share full agent traces, not just individual messages"

**Principle 2 - Implicit Decisions:**
> "Actions carry implicit decisions, and conflicting decisions carry bad results"

Cognition states:
> "Principles 1 & 2 are so critical and rarely worth violating that you should by default rule out any agent architectures that don't abide by them."

### The Flappy Bird Example

**Task:** "build a Flappy Bird clone"

**Subtask 1:** "build a moving game background with green pipes and hit boxes"
**Subtask 2:** "build a bird that you can move up and down"

**Failure Mode:**
- Subagent 1 misinterprets and builds Super Mario Bros. style background
- Subagent 2 creates bird that doesn't match Flappy Bird aesthetics/mechanics
- Coordinating agent attempts to combine fundamentally misaligned components

**Root Cause:**
> "Subagent 1 and subagent 2 cannot not see what the other was doing and so their work ends up being inconsistent with each other."

> "This may seem contrived, but most real-world tasks have many layers of nuance that all have the potential to be miscommunicated."

### Context Isolation Problem

In multi-turn, production systems with tool calls and contextual dependencies, simply copying the original task doesn't prevent miscommunication about implementation details.

> "most real-world tasks have many layers of nuance with potential for miscommunication, and in production systems with multi-turn conversations and tool calls, any number of details could have consequences on task interpretation."

### Performance Degradation Data

**IMPORTANT:** The blog post provides **NO quantitative performance metrics** or empirical testing data. It relies on logical reasoning rather than measured degradation rates.

**Note:** The "180 architectures" and "70% performance degradation" mentioned in search queries were NOT found in Cognition's published materials. These may be confused with other sources or benchmarks.

### Recommended Architectures

**1. Simple Linear Architecture (Preferred):**
- Single-threaded agent with continuous context
- Trade-off: context window overflow on very large tasks

**2. Advanced Long-Context Architecture:**
- Introduces specialized LLM to compress agent history into key details, events, and decisions
- Author notes:
> "This is _hard to get right._ It takes investment into figuring out what ends up being the key information."

### Real-World Implementation in Devin

**Claude Code Subagents (Example from Cognition):**
- Spawn subtasks but never run parallel work
- Subtasks only answer questions, not write code
- Reason:
> "The subtask agent lacks context from the main agent that would otherwise be needed."
- Subagents prevent context bloat while maintaining coordination

**Edit Apply Models (Legacy Pattern):**
- Previous pattern (2024): Large models output markdown explanations → small models execute edits
- Created ambiguity-based errors
- Current solution: Single model handles both decision-making and application

### When Multi-Agent Might Work (Future Speculation)

Cognition expresses skepticism:
> "agents today are not quite able to engage in this style of long-context proactive discourse with much more reliability than you would get with a single agent."

Future possibility:
> "it will come for free as we make our single-threaded agents even better at communicating with humans. When this day comes, it will unlock much greater amounts of parallelism and efficiency."

**Key Architectural Insight:**
> "ensure your agent's every action is informed by the context of all relevant decisions made by other parts of the system."

### Counterpoint: Anthropic's Position

**"How we built our multi-agent research system"** (Released day after Cognition's post)

Anthropic demonstrated lead "orchestrator" agent delegating to parallel sub-agents was essential for scaling performance on complex research tasks.

**Performance Data:**
- Multi-agent setup beat single-agent baseline by **90.2%** on internal benchmarks

**Key Difference:**
Anthropic's approach likely maintains better context sharing and coordination than naive multi-agent implementations Cognition critiques.

### When Multi-Agent Works (Community Consensus)

**Good Use Cases:**
- Truly independent tasks (parallel data fetching from different sources)
- Different specialized domains (one agent for frontend, one for backend when minimal interaction)
- Research/analysis tasks where results are aggregated

**Bad Use Cases:**
- Tasks requiring shared implicit decisions
- Mid-task coordination needs
- Situations where one agent's output directly informs another's approach

---

## 6. Open Source & Artifacts

### Published Open Source

**1. devin-swebench-results Repository**
- URL: https://github.com/CognitionAI/devin-swebench-results
- Contents: Cognition's results and methodology on SWE-bench
- Includes evaluation harness code
- Provides Devin's code edits for transparency
- License: MIT License

**2. Devin Extension**
- Repository: https://github.com/CognitionAI/devin-extension
- Purpose: NOT DISCLOSED (likely browser or IDE extension)

**3. DeepWiki MCP Server**
- Open protocol implementation for Model Context Protocol
- Tools provided: `ask_question`, `read_wiki_contents`, `read_wiki_structure`
- Access all GitHub repos indexed on DeepWiki.com

### Benchmark Data Published

**SWE-bench Technical Report:**
- URL: https://cognition.ai/blog/swe-bench-technical-report
- Documents methodology and results
- Evaluation code available in GitHub repo

**Benchmark Methodology:**
- Dataset: 2,294 issues and PRs from popular open source Python repositories
- Test set: 570 issues (randomly chosen 25%)
- Prompt: Standardized, asks to edit code given only GitHub issue description
- No user input during run
- Runtime limit: 45 minutes
- Agent navigates files autonomously (no file list provided)

**Results Transparency:**
- 79 of 570 issues resolved (13.86%)
- Individual issue results available in repository
- Code edits provided for inspection

### Published Research Papers

**NO comprehensive arXiv paper** specifically detailing Devin's technical architecture found.

Cognition announced plans to publish more detailed technical reports, but as of 2025, primary documentation comes from:
- Blog posts (cognition.ai/blog)
- Official documentation (docs.devin.ai)
- SWE-bench technical report

**Mentions in Academic Papers:**
- Devin mentioned in broader AI pair programming research
- Discussed in context of multi-agent collaboration frameworks
- No Cognition-authored academic papers found in arXiv

### Community Open Source Alternatives

**OpenDevin (Now OpenHands):**
- URL: https://github.com/OpenDevin/OpenDevin
- Open-source implementation inspired by Devin
- Not affiliated with Cognition

**Devika:**
- URL: https://github.com/stitionai/devika
- First open-source implementation of Agentic Software Engineer
- Started as open-source alternative to Devin
- Not affiliated with Cognition

### Testing Methodology (NOT Disclosed)

The "Don't Build Multi-Agents" blog post contains:
- NO formal testing protocols
- NO A/B testing results
- NO systematic evaluation methodology
- NO empirical performance degradation data

Arguments based on logical reasoning and illustrative examples rather than quantitative benchmarks.

---

## Sources

### Official Cognition Sources
- [Devin's 2025 Performance Review](https://cognition.ai/blog/devin-annual-performance-review-2025)
- [Don't Build Multi-Agents](https://cognition.ai/blog/dont-build-multi-agents)
- [Devin 2.0 Announcement](https://cognition.ai/blog/devin-2)
- [SWE-bench Technical Report](https://cognition.ai/blog/swe-bench-technical-report)
- [Devin's MCP Marketplace](https://cognition.ai/blog/mcp-marketplace)
- [The DeepWiki MCP Server](https://cognition.ai/blog/deepwiki-mcp-server)
- [Devin Official Docs](https://docs.devin.ai/)
- [Interactive Planning Docs](https://docs.devin.ai/work-with-devin/interactive-planning)
- [DeepWiki Docs](https://docs.devin.ai/work-with-devin/deepwiki)
- [MCP Marketplace Docs](https://docs.devin.ai/work-with-devin/mcp)
- [Billing Docs](https://docs.devin.ai/admin/billing)

### GitHub Repositories
- [CognitionAI/devin-swebench-results](https://github.com/CognitionAI/devin-swebench-results)
- [CognitionAI/devin-extension](https://github.com/CognitionAI/devin-extension)

### News & Analysis
- [Devin 2.0 is here - VentureBeat](https://venturebeat.com/programming-development/devin-2-0-is-here-cognition-slashes-price-of-ai-software-engineer-to-20-per-month-from-500)
- [Goldman Sachs pilots autonomous coder - CNBC](https://www.cnbc.com/2025/07/11/goldman-sachs-autonomous-coder-pilot-marks-major-ai-milestone.html)
- [Meet Devin: Goldman Sachs's AI Engineer - Fast Company](https://www.fastcompany.com/91366706/meet-devin-goldman-sachs-new-ai-software-engineer-that-never-sleeps)
- [Cognition AI raises $400M at $10.2B valuation - TechCrunch](https://techcrunch.com/2025/09/08/cognition-ai-defies-turbulence-with-a-400m-raise-at-10-2b-valuation/)
- [Devin AI - Wikipedia](https://en.wikipedia.org/wiki/Devin_AI)
- [Cognition AI - Wikipedia](https://en.wikipedia.org/wiki/Cognition_AI)
- [Scott Wu - Wikipedia](https://en.wikipedia.org/wiki/Scott_Wu)

### Technical Analysis
- [Agent-Native Development: Deep Dive into Devin 2.0's Technical Design - Medium](https://medium.com/@takafumi.endo/agent-native-development-a-deep-dive-into-devin-2-0s-technical-design-3451587d23c0)
- [Devin 2.0 Explained - Analytics Vidhya](https://www.analyticsvidhya.com/blog/2025/04/devin-2-0/)
- [Devin AI Complete Guide - Digital Applied](https://www.digitalapplied.com/blog/devin-ai-autonomous-coding-complete-guide)

### Hacker News Discussions
- [Devin: AI Software Engineer](https://news.ycombinator.com/item?id=39679787)
- [Don't Build Multi-Agents](https://news.ycombinator.com/item?id=45096962)
- [Devin is now generally available](https://news.ycombinator.com/item?id=42378994)
- [Goldman Sachs doesn't have to hire a $180k software engineer–meet Devin](https://news.ycombinator.com/item?id=44565291)

### Anthropic MCP
- [Model Context Protocol - Anthropic](https://www.anthropic.com/news/model-context-protocol)
- [What is the Model Context Protocol (MCP)?](https://docs.anthropic.com/en/docs/mcp)
- [Model Context Protocol - Wikipedia](https://en.wikipedia.org/wiki/Model_Context_Protocol)

---

## Key Takeaways

1. **Devin 2.0 represents massive scale:** 250k+ PRs merged, 4x faster, 67% merge rate, deployed at Goldman Sachs/Citi/Santander/Nubank

2. **Multi-agent capabilities exist but Cognition is philosophically opposed to naive parallel multi-agent architectures:** The "Don't Build Multi-Agents" post argues context isolation causes failures, though Devin 2.0 supports dispatching tasks to parallel instances

3. **Agent-native development is the paradigm shift:** Purpose-built cloud IDE, Interactive Planning, and VM isolation represent fundamental rethinking of developer tools for AI era

4. **Limited open source from Cognition:** SWE-bench results and evaluation harness are primary open artifacts; no comprehensive technical papers published

5. **Performance claims well-documented for benchmarks (SWE-bench 13.86%) but multi-agent "degradation" data not published:** The "180 architectures" and "70% degradation" figures mentioned in search queries were not found in Cognition's materials

6. **Vulnerability remediation is killer use case:** 20x efficiency gain, 1.5 min vs 30 min per vulnerability

7. **Context management remains key challenge:** Performance degrades after 10 ACUs due to context window limits, mitigated through file system memory and token budget management