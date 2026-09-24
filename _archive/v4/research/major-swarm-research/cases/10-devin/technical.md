# Devin 2.0 by Cognition: Technical Deep Dive

## 1. Prompts & Prompting Strategy

### Prompt Engineering Best Practices

**Problem Decomposition:**
Effective prompting with Devin involves breaking down complex requirements into clear, actionable prompts suitable for an AI agent. This is identified as a key skill for working with AI agents.

**Specificity Over Vagueness:**
- ❌ Bad: "add unit tests"
- ✅ Good: Specify functionality to test, identify important edge cases, clarify what needs mocking

**Discovery Questions First:**
> "Prompt your agent to explore discovery questions, like 'How does our authentication system function?' You can also ask the agent to identify specific relevant code targets for you to confirm early on."

**Context Provision:**
> "Mention the repository, relevant documentation, and key components involved. Clearly indicating these elements minimizes wasted effort and confusion."

**Scoping Rule:**
> "If the task fits in your head, it probably doesn't need delegation."
> "Look at the auth files" is vague. "Find all files in src/auth/ and src/middleware/ that handle JWT tokens" is actionable.

### Interactive Planning Workflow

**Initial Prompt Processing:**
1. User provides broad or incomplete task description
2. Devin responds within seconds with:
   - Relevant files identified through automatic codebase analysis
   - Key findings from code exploration
   - Preliminary plan with code citations and snippets

**Plan Refinement:**
- User reviews plan to ensure alignment with intent
- User can modify plan before execution begins
- Click on citations to deep-link into Devin IDE for verification
- Explore codebase together before autonomous execution

**Critical Success Factor:**
> "Devin handles clear upfront scoping well, but not mid-task requirement changes. It usually performs worse when you keep telling it more after it starts the task."

This differs fundamentally from human juniors who can be coached through iterative problem-solving.

### SWE-bench Evaluation Prompting

**Standardized Prompt:**
Devin runs end-to-end using a standardized prompt that asks it to edit code given only the GitHub issue description. No other user input is provided during the run.

**Autonomous Navigation:**
- Agent navigates files on its own (no file list provided)
- Must interpret task from issue description alone
- Can terminate earlier than 45-minute limit if task is complete

### Human-in-the-Loop Checkpoints

**Non-Negotiable Review Points:**

1. **Planning Checkpoint:** Review and adjust plan before execution
2. **Pull Request Checkpoint:** Review and approve before merge

This ensures Devin is designed for **human-in-the-loop governance**, not uncontrolled autonomy, with strict human oversight at critical junctures.

---

## 2. Memory & Context Management

### File System as Primary Memory

**Core Strategy:**
> "Devin treats the file system as its memory without prompting"

This provides an alternative to keeping everything in the context window during long-running tasks. The file system serves as persistent, structured memory external to the LLM's context.

### Context Window Limitations

**Performance Degradation Pattern:**
- Warnings appear when sessions reach around **10 ACUs** (approximately 2.5 hours of activity)
- Suspected to be related to effective context window of underlying LLMs
- Session history eventually cannot fit into context window
- Costs for each subsequent LLM call keep going up as history grows

**Product Nudges:**
Cognition added recommendations that users keep sessions under 10 ACUs to maintain optimal performance.

### Context Window Management Solutions

**1M Token Beta with 200k Cap:**
> "Cognition discovered that enabling the 1M token beta while capping usage at 200k tokens helped—this gave the model the perception of having plenty of runway without anxiety-driven shortcuts or degraded performance."

**Rationale:**
Recent models like Sonnet 4.5 are aware of their own context window. This affects behavior:
- Parallelism can burn through context faster
- Models tend to take more cautious approaches as they near token limit
- Providing headroom reduces "anxiety-driven" conservative behavior

### Context Compression (Advanced Architecture)

**From "Don't Build Multi-Agents" Blog:**

For longer tasks, Cognition proposes introducing a new LLM model whose key purpose is to compress a history of actions & conversation into key details, events, and decisions.

**Warning:**
> "This is _hard to get right._ It takes investment into figuring out what ends up being the key information."

### Cloud Persistence

**VM-Based Session Persistence:**
- Each Devin session runs in its own isolated virtual machine
- VM persists state across interactions within a session
- File system changes persist automatically
- Environment setup maintained between commands
- History accessible until session ends

**No Cross-Session Memory (NOT DISCLOSED):**
Technical documentation does not specify if knowledge learned in one session is retained for future sessions. Likely sessions are independent unless explicitly configured otherwise.

### Context in Multi-Agent Scenarios

**The Core Problem (from "Don't Build Multi-Agents"):**

When agents work in parallel without shared context, they make conflicting implicit decisions:

**Principle 1:**
> "Share context, and share full agent traces, not just individual messages"

**Principle 2:**
> "Actions carry implicit decisions, and conflicting decisions carry bad results"

**Flappy Bird Example:**
> "Subagent 1 and subagent 2 cannot not see what the other was doing and so their work ends up being inconsistent with each other."

Simply copying the original task doesn't solve this because:
> "in production systems with multi-turn conversations and tool calls, any number of details could have consequences on task interpretation."

### Recommended Single-Threaded Context Approach

**Simple Linear Architecture:**
> "The simplest way to follow the principles is to use a single-threaded linear agent where context is continuous."

**Claude Code Subagents Example:**
- Spawn subtasks but never run parallel work
- Subtasks only answer questions, not write code
- Reason:
> "The subtask agent lacks context from the main agent that would otherwise be needed."
- Prevents context bloat while maintaining coordination

---

## 3. Task Distribution & Scheduling

### Single Agent vs Multi-Agent Dispatch

**When Single-Agent is Used:**
- Default mode for most Devin tasks
- Tasks requiring continuous context
- Code writing and editing (maintains implicit decisions)
- Situations where mid-task coordination is needed

**When Multi-Agent Dispatch is Used:**
- Truly independent tasks that can run in parallel
- User explicitly spins up multiple Devin instances
- Advanced Mode batch sessions
- API-driven programmatic session creation

### Multi-Agent Operation Capability

**Later Revisions of Devin:**
> "Later revisions of Devin got multi-agent operation capability, where one of the AI agents dispatch task to other AI agents."

**Architecture:**
- Multiple parallel Devins can be spun up
- Each equipped with its own interactive, cloud-based IDE
- One Devin can dispatch sub-tasks to others for concurrent execution
- Each agent runs in isolated VM environment

### Parallelization Model

**User-Controlled Parallelism:**
Engineers with multiple tasks assign each to a separate Devin instance running in parallel. This transforms the engineer's role:
- **From:** Implementer writing all code
- **To:** Manager reviewing PRs, providing feedback, making high-level decisions

**No Conflicts:**
Each isolated VM environment eliminates conflicts between parallel sessions, supporting concurrent task execution.

### Batch Sessions & API

**Advanced Mode:**
> "if you need to frequently run multiple sessions in parallel, you can start batch sessions in Advanced Mode"

**Devin API:**
> "use the Devin API to create sessions and retrieve structured results programmatically"

This enables automated dispatch of multiple tasks to separate Devin instances without manual UI interaction.

### Task Termination Logic

**45-Minute Runtime Limit (SWE-bench Evaluation):**
> "Devin is limited to 45 minutes of runtime, as unlike most agents, it has the capability to run indefinitely. It can choose to terminate earlier if it wants."

This suggests Devin has internal heuristics for determining task completion and can self-terminate when it believes the task is done.

### Scheduling Algorithm (NOT DISCLOSED)

Cognition has not published technical specifications of:
- How tasks are prioritized when multiple Devins run in parallel
- Internal scheduling algorithm for sub-task dispatch
- Load balancing across VM instances
- Resource allocation decisions

---

## 4. Validation & Quality Control

### Pre-Submission Testing

**Automated Test Execution:**
> "Devin helps organizations move faster by reducing the burden on existing development teams, and customers trust Devin through its diligence in running tests and local security scanning tools before initiating Pull Requests."

**Workflow:**
1. Devin completes code changes
2. Runs test suite automatically
3. Performs local security scanning
4. Only then creates branch and PR

### Security Vulnerability Scanning

**Integration with Security Tools:**
- **SAST (Static Application Security Testing):** Code analysis
- **SCA (Software Composition Analysis):** Dependency analysis

**Process:**
> "Devin can gather security scanning results from both code (SAST) and dependencies (SCA), and based on recommendations provided by security tools, it can provide code changes that address security vulnerabilities that were found."

### Vulnerability Remediation Performance

**20x Efficiency Gain:**
- Human developers: **30 minutes per vulnerability**
- Devin: **1.5 minutes per vulnerability**

**Scale Impact:**
> "One large organization saved 5-10% of total developer time by using Devin for security fixes."

Another organization saw this as a killer use case, with Devin excelling at resolving vulnerabilities flagged by static analysis tools.

### Code Quality Review Requirements

**Human Review Remains Necessary:**
Despite automation, according to the 2025 Performance Review:
> "Human review remains necessary for code quality assurance and testing logic verification"

**Areas Requiring Human Oversight:**
- Code quality assessment
- Testing logic correctness
- Architecture alignment
- Edge case handling

### Pull Request Quality

**Merge Rate Improvement:**
- 2024: 34% of PRs merged
- 2025: 67% of PRs merged

Nearly doubled acceptance rate indicates improvements in:
- Code correctness
- Test coverage
- Adherence to project standards
- PR description quality

### Testing Logic Verification

**Devin's Approach to Tests:**
> "Devin is great at resolving vulnerabilities flagged by static analysis tools."

But remains weaker at:
- Complex testing strategy design
- Edge case identification
- Test coverage optimization

**Best Practice:**
Specify functionality to test, identify important edge cases, and clarify what needs mocking when prompting for test creation.

### Real Data Validation (SWE-bench)

**Evaluation Criteria:**
- Task considered successfully resolved only if it passes all tests in the SWE-bench test suite
- Must match the exact behavior of the original pull request that resolved the issue
- No partial credit for close attempts

**13.86% Success Rate Interpretation:**
- Out of 570 issues, 79 were fully resolved end-to-end
- Previous best assisted system (Claude 2): 4.80%
- Unassisted previous state-of-the-art: 1.96%

This represents significant improvement but still indicates substantial room for growth in handling complex real-world software engineering tasks.

---

## 5. Mailbox / Inbox Implementation (Agent-to-Agent Dispatch)

### High-Level Dispatch Mechanism

**Multi-Agent Operation:**
> "Later revisions of Devin got multi-agent operation capability, where one of the AI agents dispatch task to other AI agents."

### VM-to-VM Communication (NOT DISCLOSED)

**Isolated VM Architecture:**
Each Devin session runs in its own isolated virtual machine. For agent-to-agent dispatch to work, there must be a communication layer between VMs.

**Hypothetical Mechanisms (Not Confirmed by Cognition):**
- Message queue service (e.g., Redis, RabbitMQ)
- RESTful API endpoints between VMs
- Shared database for task state
- WebSocket connections for real-time updates

**Cognition Has Not Published:**
- Detailed technical specifications of dispatch protocol
- Message format between agents
- State synchronization mechanism
- Task result aggregation method

### Task Delegation Pattern

**User-Initiated Delegation:**
Users can explicitly spin up multiple Devin instances, effectively creating a delegation hierarchy:
- Primary Devin receives main task
- User or primary Devin spawns additional Devin instances
- Sub-tasks assigned to parallel instances

**Automatic Dispatch (UNCLEAR):**
It's not explicitly documented whether Devin can autonomously decide to spawn sub-agents or if this always requires human initiation.

### Result Aggregation (NOT DISCLOSED)

**When Multiple Devins Complete Sub-tasks:**
- How are results combined?
- Does a coordinating agent merge PRs from multiple sub-agents?
- How are conflicts resolved if two agents modify the same file?

These questions are not addressed in public documentation.

### Context Sharing Between Agents

**Cognition's Position:**
From "Don't Build Multi-Agents," Cognition emphasizes:
> "ensure your agent's every action is informed by the context of all relevant decisions made by other parts of the system."

**Implementation (NOT DISCLOSED):**
If Devin supports multi-agent dispatch, it must address the context isolation problem Cognition critiques. Possible solutions:
- Shared file system across VMs
- Real-time trace sharing mechanism
- Coordinating agent maintains full context of all sub-agents
- Post-completion result aggregation without real-time coordination

**Cognition's Skepticism:**
> "agents today are not quite able to engage in this style of long-context proactive discourse with much more reliability than you would get with a single agent."

This suggests even within Devin's multi-agent capabilities, coordination remains a carefully managed challenge.

### Comparison to Traditional Multi-Agent Systems

**Cognition's Critique of OpenAI Swarm & Microsoft AutoGen:**
> "libraries such as OpenAI's Swarm and Microsoft's AutoGen actively push concepts which I believe to be the wrong way of building agents."

**Implication:**
If Devin implements multi-agent dispatch, it likely differs significantly from these frameworks to avoid the context isolation problems Cognition identifies.

### Claude Code Subagents as Reference

**How Claude Code Handles Delegation:**
- Spawn subtasks but never run parallel work
- Subtasks only answer questions, not write code
- Main agent retains all context and decision-making authority
- Subagents used purely for information gathering

**Rationale:**
> "The subtask agent lacks context from the main agent that would otherwise be needed."

This pattern may inform Devin's own multi-agent approach if similar constraints are applied.

### Mailbox/Inbox Technical Specification

**DEFINITIVE STATUS: NOT DISCLOSED**

Cognition has not published:
- Inter-agent message format
- Queue implementation details
- Synchronization primitives
- State machine for task lifecycle
- Error handling when sub-agents fail
- Timeout mechanisms
- Resource cleanup procedures

---

## 6. Open Source Artifacts & Code

### CognitionAI/devin-swebench-results

**Repository URL:** https://github.com/CognitionAI/devin-swebench-results

**Contents:**
- Cognition's results and methodology on SWE-bench
- Evaluation harness code (adapted from original SWE-bench for agent evaluation)
- Devin's code edits for the 570 evaluated issues
- Transparency artifacts showing exactly what changes Devin made

**License:** MIT License

**Key Files:**
- `README.md`: Methodology documentation
- Evaluation code: Adapted eval harness for agents (vs. original LLM-focused eval)
- Results data: Individual issue results

**Evaluation Code:**
> "Code for the adapted eval harness is available at https://github.com/CognitionAI/devin-swebench-results."

### CognitionAI/devin-extension

**Repository URL:** https://github.com/CognitionAI/devin-extension

**Purpose:** NOT DISCLOSED in search results
- Likely browser or IDE extension for Devin integration
- Specific functionality not documented in available materials

### DeepWiki MCP Server

**Open Protocol Implementation:**
MCP (Model Context Protocol) server providing programmatic access to GitHub repos indexed on DeepWiki.com

**Tools Provided:**
1. `ask_question`: Query codebase
2. `read_wiki_contents`: Read generated documentation
3. `read_wiki_structure`: Access architecture structure

**Access:**
- Available in Devin's MCP Marketplace
- One-click enable without configuration
- Open protocol by Anthropic

### SWE-bench Technical Report

**URL:** https://cognition.ai/blog/swe-bench-technical-report

**Published Methodology:**

**Dataset:**
- 2,294 issues and PRs from popular open source Python repositories on GitHub
- Goal: Test system's ability to write real-world code
- Each instance: GitHub issue + PR that resolved it

**Evaluation Approach:**
> "Cognition adapted SWE-bench to evaluate agents, a more general setting than the original eval for LLMs. They run the agent end to end using a standardized prompt that asks it to edit code given only the GitHub issue description. They do not give the agent any other user input during the run."

**Test Set:**
> "Devin was evaluated on a randomly chosen 25% of the SWE-benchmark test set (570 out of the 2,294). This was done to reduce the time it takes for the benchmark to finish, the same strategy the authors used in the original paper."

**Runtime:**
> "Devin is limited to 45 minutes of runtime, as unlike most agents, it has the capability to run indefinitely. It can choose to terminate earlier if it wants."

**Results:**
- 79 of 570 issues resolved (13.86%)
- Previous best: Claude 2 at 4.80% (assisted)
- Previous state-of-the-art: 1.96% (unassisted)

### "Don't Build Multi-Agents" Blog Post

**URL:** https://cognition.ai/blog/dont-build-multi-agents

**Content Type:** Technical opinion piece with examples

**Key Artifacts:**
- Two fundamental principles for context engineering
- Flappy Bird example illustrating multi-agent failure modes
- Architectural recommendations (single-threaded linear agent, advanced long-context architecture)
- Real-world examples (Claude Code subagents, Edit Apply Models)

**Notably Missing:**
- NO formal testing protocols
- NO A/B testing results
- NO systematic evaluation methodology
- NO empirical performance degradation data
- NO mention of "180 architectures tested"
- NO mention of "70% performance degradation"

**Nature of Arguments:**
Based on logical reasoning and illustrative examples rather than quantitative benchmarks.

### Published Research Papers

**Status: NO comprehensive arXiv paper found**

**What Exists:**
- Cognition blog posts (primary source of technical information)
- Official documentation (docs.devin.ai)
- SWE-bench technical report

**What Does NOT Exist (As of 2025):**
- Peer-reviewed academic papers on Devin's architecture
- Detailed technical specifications of internal systems
- Training methodology details (beyond "combination of LLMs akin to GPT-4 with aspects from reinforcement learning")
- Reinforcement learning algorithm specifics
- Model architecture details
- Dataset composition for training

**Mentions in Academic Literature:**
Devin is mentioned in broader AI pair programming research and multi-agent collaboration frameworks, but no Cognition-authored academic papers were found.

### Community Open Source Alternatives

**OpenDevin (Now OpenHands):**
- URL: https://github.com/OpenDevin/OpenDevin
- Open-source implementation inspired by Devin
- NOT affiliated with Cognition
- Community-driven development

**Devika:**
- URL: https://github.com/stitionai/devika
- First open-source implementation of Agentic Software Engineer
- Started as open-source alternative to Devin
- NOT affiliated with Cognition

### What Cognition Has NOT Open-Sourced

**Critical Missing Artifacts:**

1. **Agent Architecture:**
   - Internal decision-making logic
   - Task decomposition algorithm
   - Planning system implementation
   - Self-correction mechanisms

2. **Training Infrastructure:**
   - Training datasets
   - Reinforcement learning implementation
   - Reward function design
   - Training pipeline

3. **Multi-Agent Coordination:**
   - Inter-agent communication protocol
   - Task dispatch mechanism
   - Context sharing implementation
   - State synchronization

4. **Cloud IDE:**
   - VM orchestration system
   - Resource allocation algorithm
   - Session management
   - IDE backend implementation

5. **Context Management:**
   - Context compression algorithm
   - File system memory implementation
   - Long-context handling strategies

6. **Interactive Planning:**
   - Plan generation algorithm
   - Codebase analysis heuristics
   - Relevance ranking system
   - Citation extraction mechanism

### Anthropic's Model Context Protocol (MCP)

**Open Standard by Anthropic (November 2024):**
- URL: https://www.anthropic.com/news/model-context-protocol
- Documentation: https://docs.anthropic.com/en/docs/mcp
- GitHub: https://github.com/modelcontextprotocol

**What MCP Provides:**
- Standardizes how AI systems integrate with external tools/data sources
- SDKs available for all major programming languages
- Thousands of MCP servers built by community
- Adopted as de-facto standard for connecting agents to tools and data

**Devin's Integration:**
- MCP Marketplace in Devin Settings
- One-click enable for many MCPs
- Connect service accounts during sessions
- Access to Notion, Sentry, Datadog, etc.

**MCP is Open Source, Devin's MCP Integration Code is NOT:**
While MCP itself is open, Cognition has not open-sourced:
- How Devin internally implements MCP protocol
- Server selection logic
- Integration testing framework
- Error handling for failed MCP connections

---

## 7. The "180 Architectures" and "70% Degradation" Mystery

### Search for Empirical Data

**Extensive Search Conducted For:**
- "180 architectures tested"
- "70% performance degradation"
- Multi-agent benchmark data from Cognition
- A/B testing results
- Systematic evaluation methodology

### Findings: NOT FOUND IN COGNITION MATERIALS

**"Don't Build Multi-Agents" Blog Post:**
- Contains NO quantitative performance metrics
- NO mention of 180 architectures tested
- NO mention of 70% degradation figure
- Arguments based on logical reasoning and illustrative examples

**Other Cognition Publications:**
- SWE-bench technical report: Contains only single-agent Devin performance (13.86%)
- 2025 Performance Review: Year-over-year comparisons (4x faster, 2x more efficient, 67% merge rate)
- Devin 2.0 announcement: 83% more tasks per ACU (Devin 2.0 vs Devin 1.x)

**NO multi-agent vs single-agent empirical comparison published by Cognition**

### Possible Sources of Confusion

**1. Anthropic's Counter-Argument:**
The day after Cognition's "Don't Build Multi-Agents" post, Anthropic released "How we built our multi-agent research system" showing:
> "multi-agent setup beat single-agent baseline by **90.2%** on internal benchmarks"

This is a **positive** result for multi-agents, not degradation.

**2. Other Research:**
The "180" number may come from:
- Hacker News discussions (180 comments on a related post)
- Unrelated AI architecture research
- Community benchmark efforts

**3. Performance Degradation Data:**
The only degradation mentioned in Cognition materials:
- Context performance degradation after 10 ACUs due to context window limits
- NOT multi-agent-specific

### Definitive Conclusion

**"180 architectures tested" and "70% performance degradation" claims:**
- NOT found in Cognition's published materials
- NOT supported by available evidence
- May be confused with other sources or misattributed

**What Cognition Actually Published:**
- Logical argument against naive multi-agent architectures
- Illustrative examples (Flappy Bird)
- Design principles (context sharing, implicit decisions)
- Recommendations (single-threaded linear agent)
- NO quantitative benchmark comparison of multi-agent vs single-agent performance

---

## 8. Key Technical Insights

### Architecture Philosophy

**Agent-Native Design:**
Purpose-built environments for AI workflows rather than adapting human-focused tools represents a fundamental paradigm shift.

**Cloud-First Execution:**
VM isolation, cloud persistence, and scalable parallelization enable capabilities impossible in local execution environments.

### Performance Optimization

**83% Efficiency Gain (Devin 2.0 vs 1.x):**
Achieved through:
- Improved reasoning
- Better error recovery
- Smarter resource allocation

**Specific mechanisms NOT DISCLOSED**

### Context Management Trade-offs

**10 ACU Limit:**
Practical ceiling for maintaining high performance without context degradation. Beyond this, either:
- Start new session
- Implement context compression (difficult to get right)

**File System Memory:**
Elegant solution to context window limitations but requires agent to know when to persist important information.

### Validation Strategy

**Pre-Submission Testing + Human Review:**
Automated testing catches obvious errors, human review ensures architectural alignment. This hybrid approach enables 67% merge rate.

### Multi-Agent Stance

**Cognition's Position:**
Philosophically opposed to naive parallel multi-agent architectures due to context isolation problems, but implements carefully controlled multi-agent dispatch in Devin 2.0.

**Implication:**
Multi-agent capabilities exist but likely with significant guardrails to prevent the failure modes Cognition critiques.

---

## Sources

### Official Cognition Documentation
- [Don't Build Multi-Agents](https://cognition.ai/blog/dont-build-multi-agents)
- [SWE-bench Technical Report](https://cognition.ai/blog/swe-bench-technical-report)
- [Devin's 2025 Performance Review](https://cognition.ai/blog/devin-annual-performance-review-2025)
- [Rebuilding Devin for Claude Sonnet 4.5: Lessons and Challenges](https://cognition.ai/blog/devin-sonnet-4-5-lessons-and-challenges)
- [Devin 2.0 Announcement](https://cognition.ai/blog/devin-2)
- [Devin's MCP Marketplace](https://cognition.ai/blog/mcp-marketplace)
- [Interactive Planning Docs](https://docs.devin.ai/work-with-devin/interactive-planning)
- [DeepWiki Docs](https://docs.devin.ai/work-with-devin/deepwiki)
- [MCP Marketplace Docs](https://docs.devin.ai/work-with-devin/mcp)
- [Billing Docs](https://docs.devin.ai/admin/billing)
- [Coding Agents 101: The Art of Actually Getting Things Done](https://devin.ai/agents101)

### GitHub Repositories
- [CognitionAI/devin-swebench-results](https://github.com/CognitionAI/devin-swebench-results)
- [CognitionAI/devin-extension](https://github.com/CognitionAI/devin-extension)
- [Model Context Protocol](https://github.com/modelcontextprotocol)

### Technical Analysis
- [Agent-Native Development: Deep Dive into Devin 2.0's Technical Design - Medium](https://medium.com/@takafumi.endo/agent-native-development-a-deep-dive-into-devin-2-0s-technical-design-3451587d23c0)
- [Devin 2.0 Explained - Analytics Vidhya](https://www.analyticsvidhya.com/blog/2025/04/devin-2-0/)
- [Devin AI Complete Guide - Digital Applied](https://www.digitalapplied.com/blog/devin-ai-autonomous-coding-complete-guide)
- [Why Cognition does not use multi-agent systems - Jason Liu](https://jxnl.co/writing/2025/09/11/why-cognition-does-not-use-multi-agent-systems/)

### Anthropic Resources
- [Model Context Protocol - Anthropic](https://www.anthropic.com/news/model-context-protocol)
- [What is the Model Context Protocol (MCP)?](https://docs.anthropic.com/en/docs/mcp)
- [Model Context Protocol - Wikipedia](https://en.wikipedia.org/wiki/Model_Context_Protocol)

### Community & Analysis
- [Inside the Multi-Agent Debate - Medium](https://snrspeaks.medium.com/inside-the-multi-agent-debate-why-cognition-labs-says-dont-and-anthropic-says-do-carefully-7b8a253e0b1e)
- [Cognition vs Anthropic: Don't Build Multi-Agents/How to Build Multi-Agents - AINews](https://news.smol.ai/issues/25-06-13-cognition-vs-anthropic/)
- [The Multi-Agent Moment](https://trilogyai.substack.com/p/the-multi-agent-moment)
- [Single vs Multi-Agent System?](https://www.philschmid.de/single-vs-multi-agents)
- [Multi-Agent or Not, That Is the Question](https://shashikantjagtap.net/multi-agent-or-not-that-is-the-question/)

### Hacker News Discussions
- [Don't Build Multi-Agents](https://news.ycombinator.com/item?id=45096962)
- [Devin: AI Software Engineer](https://news.ycombinator.com/item?id=39679787)
- [Devin is now generally available](https://news.ycombinator.com/item?id=42378994)

### Founder Background
- [Scott Wu - Wikipedia](https://en.wikipedia.org/wiki/Scott_Wu)
- [Cognition AI - Wikipedia](https://en.wikipedia.org/wiki/Cognition_AI)
- [Inside Devin - Lenny's Newsletter](https://www.lennysnewsletter.com/p/inside-devin-scott-wu)
- [Inside Cognition: Prodigy Culture - Turing Post](https://www.turingpost.com/p/cognition)
- [Report: Cognition Business Breakdown & Founding Story - Contrary Research](https://research.contrary.com/company/cognition)

---

## Critical Takeaways

### What We Know

1. **Prompting Strategy:** Interactive planning with upfront clarity, specificity over vagueness, discovery questions first
2. **Memory Management:** File system as primary memory, 10 ACU context limit, 1M token beta with 200k cap mitigates anxiety
3. **Task Distribution:** User-controlled parallel instances, later revisions support agent-to-agent dispatch
4. **Validation:** 20x efficiency on vulnerability remediation, pre-submission testing, 67% merge rate with human review
5. **Open Source:** SWE-bench evaluation harness, results data, DeepWiki MCP server

### What We DON'T Know

1. **Mailbox/Inbox Implementation:** Inter-agent communication protocol NOT DISCLOSED
2. **Training Details:** Reinforcement learning specifics, reward functions, datasets
3. **Internal Architecture:** Decision-making logic, planning system, self-correction mechanisms
4. **Multi-Agent Coordination:** Context sharing implementation, state synchronization
5. **"180 Architectures" Claim:** NOT FOUND in Cognition materials despite extensive search

### Most Important Technical Principle

**From "Don't Build Multi-Agents":**
> "ensure your agent's every action is informed by the context of all relevant decisions made by other parts of the system."

This principle underlies all of Devin's architectural decisions, even when implementing multi-agent capabilities.