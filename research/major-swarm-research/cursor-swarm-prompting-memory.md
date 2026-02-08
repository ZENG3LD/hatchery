# Cursor FastRender Swarm: Prompting, Memory & Validation Research

**Research Date:** 2026-02-08
**Focus:** Technical details on prompting strategies, memory management, and validation mechanisms in Cursor's multi-agent swarm system (FastRender experiment)

---

## Executive Summary

Cursor's FastRender experiment demonstrated a hierarchical swarm of ~2,000 concurrent agents building a web browser from scratch over one week, generating 3M+ lines of Rust code across 1,000+ files with minimal human intervention. This research extracts specific technical details about **prompting strategies**, **memory/context management**, and **validation mechanisms** from official blog posts, HackerNews discussions, community forums, and Twitter/X posts.

**Key Finding:** The system emphasizes **constraints over instructions**, **explicit domain guidance via git submodules**, **hierarchical planning with judge-based validation**, and **scratchpad-based inter-agent communication**, though many low-level implementation details remain undisclosed.

---

## 1. Scratchpad Rewriting

### What We Know

**Core Mechanism:**
- Communication between agents (Planner/Executor roles) is conducted through writing to/modifying `.cursor/scratchpad.md`
- Used as a persistent communication mechanism for multi-agent workflows
- Allows different agent roles to track progress and pass information while maintaining an audit trail

**Best Practices (from community implementations):**
- **Avoid rewriting entire document** unless necessary
- **Avoid deleting records** left by other roles
- **Append new paragraphs** or mark old paragraphs as outdated instead of overwriting
- Scratchpad should be "frequently rewritten versus being appended to" (from official docs)

**Format/Structure:**
- Markdown file (`.cursor/scratchpad.md`)
- Hook scripts can read and check for markers like "DONE" to determine whether agents should continue
- Agents update scratchpad with "DONE" when complete

**Triggers:**
- Manual: Hook scripts that read/write to scratchpad as part of workflow
- Automatic: Agents reaching context limits automatically summarize

**What's Preserved vs Discarded:**
- Old paragraphs marked as "outdated" rather than deleted
- Specific preservation/compression algorithms **not publicly documented**

### What We Don't Know

- Exact internal format/schema beyond markdown
- Automatic rewrite triggers (token count? time interval?)
- Compression ratios when scratchpad is rewritten
- How conflicts are resolved when multiple agents write simultaneously
- State diffing algorithms

---

## 2. Context Compression / /compact Endpoint

### What We Know

**Automatic Summarization:**
- Triggered when hitting context window limit (e.g., 200k tokens with Sonnet 4)
- Uses a **smaller, faster "flash" model** (not the current working model) for summarization
- Cursor displays "Summarizing Chat Content" and gives agent fresh context window with summary
- Described as "lossy compression" — agent knowledge can degrade after summarization

**Manual Compression:**
- `/summarize` command available (no `/compact` command found in docs)
- Lee Robinson (Cursor team) mentioned wanting to add a `/compress` command for manual control
- Recommendation: Run compression at **70-75% context usage**, not 85-90%, to provide buffer space

**Compression Behavior:**
- Older messages get compressed to make room for new ones
- "Summarized Messages" appear in chat — full context is gone, but key points remain
- Chat history stored as files to improve summarization quality
- Agent given reference to history file to search through and recover crucial details if needed

**Context Limits:**
- Claude Sonnet 4: 200k tokens (~15,000 lines of code)
- Users report summarization sometimes triggered at ~25% context usage (unclear if bug or feature)

### What We Don't Know

- **Exact token threshold** that triggers auto-summarization
- **Compression ratio** achieved (e.g., 10:1? 20:1?)
- **Which flash model** specifically (Haiku? Gemini Flash? GPT-4o-mini?)
- Detailed algorithm for what's preserved vs discarded
- How agents prioritize information during compression
- Recovery success rate when agents need summarized details

---

## 3. Fresh Starts

### What We Know

**Need for Fresh Starts:**
- "We still need periodic fresh starts to combat drift and tunnel vision" (official blog)
- Long conversations cause agents to lose focus — after many turns and summarizations, context accumulates noise
- Agent can get distracted or switch to unrelated tasks

**When Fresh Starts Happen:**
- **End of each cycle:** Judge agent determines whether to continue; if continuing, next iteration starts fresh
- **When drift detected:** If work is drifting, judge wipes slate clean and forces retry
- **User-triggered:** Start fresh when moving to different task/feature or when agent seems confused/makes same mistakes repeatedly
- **Plan drift >15%:** Deviation triggers re-planning in Cursor's planning system

**State Preservation:**
- Executor hands off "important notes, concerns, deviations, findings, thoughts, and feedback" to enable continuous recalibration
- Scratchpad preserves cross-iteration state
- Git commits serve as persistent memory of what was done

**Continuity Mechanisms:**
- Workers provide detailed handoff notes
- Judge agent reviews before starting next iteration
- Git history provides ground truth of completed work

### What We Don't Know

- **Frequency:** How often fresh starts occur (every N cycles? time-based?)
- Exact criteria judge uses to decide "continue" vs "fresh start"
- How much context from previous iterations is injected into fresh start
- Whether there's a sliding window of N most recent iterations

---

## 4. "Constraints over Instructions"

### What We Know

**Core Principle:**
> "Constraints are more effective than instructions. 'No TODOs, no partial implementations' works better than 'remember to finish implementations.'" (official blog)

**Philosophy:**
- Defining boundaries outperforms directive language
- Treat model like "brilliant new hire who knows engineering but not your specific codebase"
- Only instruct for things model doesn't know (e.g., multi-agent collaboration) or domain-specific items
- "It was better to not instruct for things the model knows how to do"

**Concrete Examples:**

| Instruction-Style (❌ Worse) | Constraint-Style (✅ Better) |
|------------------------------|------------------------------|
| "Remember to finish implementations" | "No TODOs, no partial implementations" |
| "Generate many tasks" | "Generate 20-100 tasks" (concrete numerical range) |
| "Add tests for auth.ts" | "Write a test case for auth.ts covering the logout edge case, using the patterns in `__tests__/` and avoiding mocks" |
| "Make it work" | "Do not run test-teardown even if tests are successful—I will do that manually" (prohibition-based constraint) |

**Types of Constraints:**

1. **Prohibition-based:** "Do not X" / "Never Y"
2. **Scope-based:** Numerical ranges (20-100 tasks), concrete limits
3. **Conditional:** "When user request begins with CHANGELOG:, add a changelog entry..."
4. **Self-verification:** "Before generating code, verify: Are you importing from correct SDK version? If not, STOP and FIX."
5. **Role-based:** Planners "do no coding itself" / Workers "focus narrowly on completion"

**Rationale:**
- Constraints create guardrails that prevent entire classes of errors
- Instructions require agent to remember and interpret intent
- Constraints are easier to verify programmatically

### What We Don't Know

- Exact trade-offs: when instructions are still needed
- How constraints are enforced technically (system prompts? validation layers?)
- Whether constraints are compiled into a structured format

---

## 5. Self-Correction Loops

### What We Know

**Error Detection Mechanisms:**

1. **Compiler feedback:**
   - FastRender agents "constantly compiling using Rust compiler and fixing compile errors as they occurred"
   - Rust's strictness provided automatic verification without human review
   - "The project was able to build the whole time"

2. **Test-driven validation:**
   - Write tests first, confirm they fail, then implement code iteratively "until all tests pass"
   - Explicit instruction not to modify tests during implementation

3. **Visual feedback (FastRender-specific):**
   - GPT-5.2's vision capabilities enabled screenshot comparisons against golden samples
   - Agents used this for browser rendering validation

4. **Error return mechanism:**
   - Errors returned to agent, enabling it to react and call tools again with different parameters
   - When command fails and sandbox detected as cause, agent prompted to retry outside sandbox

**Loop Detection:**
- Cursor has built-in loop detection to prevent infinite retry cycles
- Can detect "Unrecoverable agent model looping detected" and halt
- Community reports of both false positives (too aggressive) and missed loops (not aggressive enough)

**Retry Mechanisms:**
- Rate limit or transient errors trigger retries with exponential backoff (community implementation: Ralph Wiggum technique)
- When errors occur, they're logged → agent analyzes → updates guardrails
- Errors are returned to agent for self-correction

**Self-Correction Protocol:**
1. Agent makes change
2. Compile/test/validate
3. If error: analyze error → adjust approach → retry
4. Loop until success or loop detection triggers

**Error Tolerance Strategy (FastRender):**
- System "accepts some error rate" rather than demanding "100% correctness before every single commit"
- Allows "small errors" (API changes, syntax issues) to maintain throughput
- Subsequent commits quickly correct temporary problems
- Trade-off: throughput vs perfect correctness

### What We Don't Know

- **Maximum loop depth:** How many retries before giving up?
- How loop detection algorithm works (pattern matching? state comparison?)
- What criteria differentiate "productive iteration" from "stuck in loop"
- Recovery strategies when loop detection triggers
- Statistics on self-correction success rates in FastRender

---

## 6. Judge Agent

### What We Know

**Role and Timing:**
> "At the end of each cycle, a judge agent determined whether to continue, then the next iteration would start fresh." (official blog)

**Evaluation Process (Cursor 2.2 Multi-Agent Judging):**
- Automatically evaluates all parallel agent runs after completion
- "Analyzes the logic behind each proposed solution and explores the codebase to confirm they're correct"
- Selects best solution and marks with "+1" indicator

**What Judge Evaluates:**
- ✅ **Logical correctness** and codebase validation
- ✅ **Task completion** (did it achieve the plan?)
- ✅ **Code quality** in context of requirements
- ❌ **NOT** code size or minimal changes
- ❌ **NOT** coding style preferences
- ❌ **NOT** architectural choices
- ❌ **NOT** model cost efficiency

**Feedback Format:**
- Comment explaining selection rationale
- Reasoning "offered by the judge agent" as justification
- Root cause determination for failures (if multiple judges used)

**Rejection/Retry Capability:**
- If work is drifting, judge "wipes slate clean and forces retry"
- Can decide to continue vs start fresh iteration
- Cannot synthesize "best of both worlds" from multiple solutions
- Cannot pick and choose elements across different agent outputs

**Multi-Judge System (from general agent eval research):**
- Uses suite of LLM judges to assess different quality aspects
- If all judges pass → overall pass
- If any judge fails → root cause = first judge to fail
- Combines assessments into overall pass/fail score

**Evaluation Metrics:**
- Cohen's Kappa (inter-rater agreement) for measuring judge quality
- LLM judges can reach >80% agreement with human evaluators
- Pass/fail, true/false, numerical, or categorical scoring

### What We Don't Know

- **Specific evaluation prompt** used by judge
- Whether judge uses tests/benchmarks or only code review
- How judge handles subjective decisions (acknowledged as subjective by Cursor team)
- Whether judge can request more information from agents
- Cost of running judge (what model? how long?)
- Success rate: how often does judge select "correct" solution?

---

## 7. Domain-Specific Guidance

### What We Know

**FastRender Approach:**
- Used **git submodules** to include official specifications:
  - `csswg-drafts` (CSS Working Group specs)
  - `tc39-ecma262` (JavaScript spec)
  - `whatwg-dom` (DOM spec)
  - `whatwg-html` (HTML spec)
- Ensures agents had "access to ground-truth reference materials"
- "Intelligently used Git submodules to include official web specifications directly in repo"

**Domain Knowledge Embedding:**
- "It was better to not instruct for things the model knows how to do, only things it doesn't know (e.g. multi-agent collaboration) or that are specific to the relevant domain"
- Leverage model's existing knowledge of Rust, HTML, CSS, etc.
- Only provide domain-specific: collaboration protocols, project structure, custom conventions

**Domain-Specific Rules in Cursor IDE:**
- Stored in `PROJECT_ROOT/.cursor/rules/` using kebab-case `.mdc` files
- Frontend rules: React patterns, state management, UI/UX standards
- Styling rules: CSS methodology, animation guidelines, design system variables, responsive design
- Keep rules focused on: commands to run, patterns to follow, pointers to canonical examples
- Reference files instead of copying contents to prevent staleness

**Skills vs Rules:**
- **Rules (persistent):** Always-active context, stored in `.cursor/rules/`
- **Skills (on-demand):** Loaded dynamically when agent decides they're relevant (keeps context window clean)

**Hierarchical Agent Specialization (FastRender):**
- **Principal Architect Agents:** High-level system design, breaking massive goal into sub-systems
- **Manager Agents:** Oversee specific modules, assign tasks to workers, review against specifications
- **Worker Agents:** Implement specific tasks

**Visual Domain Guidance:**
- GPT-5.2's vision capabilities used for screenshot comparisons
- Golden samples provided as visual reference for browser rendering

### What We Don't Know

- How specifications were chunked/indexed for efficient retrieval
- Whether agents fine-tuned on domain-specific data or just prompted
- How hierarchical agents shared domain knowledge
- Specific prompts used to instruct agents on browser rendering internals

---

## 8. Additional Findings

### Infrastructure & Scaling

**Compute:**
- Ran on single large Linux VM with "lots of resources"
- SSH terminal interface for controlling harness
- Peak ~2,000 agents concurrently
- Thousands of commits per hour

**Coordination:**
- Tree structure planning to minimize overlapping work
- "Harness effectively split out and divide scope such that it minimizes overlap"
- Surprisingly few merge conflicts despite 2,000 concurrent agents
- Workers push to same branch with minimal conflicts

**Storage Challenges:**
- After limiting RAM usage, disk became hotspot
- Hundreds of agents compiling simultaneously → many GB/s reads/writes of build artifacts
- Monolith project exacerbates disk I/O bottleneck

### Autonomous Runtime

- Agents ran up to **one week continuously** with zero human steering
- Humans could only stop the process, not prompt mid-execution
- No interactive debugging or guidance during run

### Memory Bank Pattern (Community)

- Install `AI Memory` Cursor extension
- Create structured documentation across multiple markdown files
- Automatically updated when significant project changes detected
- Acts as persistent memory to prevent same mistakes
- `learned-memories.mdc` rule file for committed learnings

---

## 9. Gaps in Public Knowledge

**Critical Missing Details:**

1. **Scratchpad:**
   - Internal schema beyond markdown
   - Conflict resolution for simultaneous writes
   - State diffing algorithms

2. **Context Compression:**
   - Exact token thresholds
   - Compression ratios
   - Which flash model specifically
   - Preservation/discard algorithms

3. **Fresh Starts:**
   - Frequency (cycles? time? tokens?)
   - Context injection amount from previous iterations

4. **Judge Agent:**
   - Specific evaluation prompt
   - Model used (Sonnet? Haiku?)
   - Success/accuracy statistics

5. **Self-Correction:**
   - Maximum retry depth
   - Loop detection algorithm details
   - Recovery strategies

6. **Domain Guidance:**
   - Specification chunking/indexing strategy
   - How agents searched specs efficiently

---

## 10. Key Takeaways for Swarm Design

### Prompting

1. **Use constraints, not instructions:** "No TODOs" > "Remember to finish"
2. **Be concrete:** "20-100 tasks" > "Many tasks"
3. **Leverage model knowledge:** Only provide domain-specific or collaboration-specific guidance
4. **Embed verification:** "Before X, verify Y" self-checks

### Memory Management

1. **Scratchpad for inter-agent communication:** Append-mostly, mark outdated
2. **Use lightweight models for summarization:** Preserve expensive models for primary work
3. **Compress proactively:** At 70-75% context, not 90%+
4. **History as searchable files:** Enable recovery of compressed details

### Validation

1. **Judge at end of each cycle:** Decide continue vs fresh start
2. **Accept small error rate for throughput:** Self-correction via subsequent commits
3. **Multiple validation layers:** Compiler + tests + visual + judge
4. **Fresh starts combat drift:** Periodic resets essential for long runs

### Architecture

1. **Hierarchical planning:** Planner → Executor → Workers
2. **Git as persistent memory:** Commits provide ground truth
3. **Domain specs as submodules:** Ground-truth reference materials
4. **Tree structure minimizes conflicts:** Scope division reduces overlap

---

## Sources

### Official Cursor Blog Posts
- [Scaling long-running autonomous coding](https://cursor.com/blog/scaling-agents)
- [Towards self-driving codebases](https://cursor.com/blog/self-driving-codebases)
- [Agent best practices](https://cursor.com/blog/agent-best-practices)

### External Analysis
- [Wilson Lin on FastRender: a browser built by thousands of parallel agents](https://simonwillison.net/2026/Jan/23/fastrender/)
- [FastRender: a browser built by thousands of parallel agents](https://simonw.substack.com/p/fastrender-a-browser-built-by-thousands)
- [Cursor's AI Revolution: Building a Browser from Scratch](https://quasa.io/media/cursor-s-ai-revolution-building-a-browser-from-scratch-with-gpt-5-2-agents-in-just-one-week)

### Community Discussions
- [HackerNews: Wilson Lin on FastRender](https://news.ycombinator.com/item?id=46738853)
- [HackerNews: Agent Swarms, like the one Cursor created](https://news.ycombinator.com/item?id=46784147)
- [Cursor Forum: Ultra Context, Memories, Lessons, Scratchpad](https://forum.cursor.com/t/rules-for-ultra-context-memories-lessons-scratchpad-with-plan-and-act-modes/48792)
- [Cursor Forum: Multi-Agent Judging (Cursor 2.2)](https://forum.cursor.com/t/cursor-2-2-multi-agent-judging/145826)

### Twitter/X Posts
- [Lee Robinson on /compress command](https://x.com/leerob/status/1952411010368753670)
- [Matt Shumer building agent swarm inspired by Cursor](https://x.com/mattshumer_/status/2012307116082471161)
- [Jason Zhou on Memory Bank technique](https://x.com/jasonzhou1993/status/1914649637618901197)

### Documentation
- [Cursor Docs: Summarization](https://cursor.com/docs/agent/chat/summarization)
- [Cursor Docs: Agent Modes](https://cursor.com/docs/agent/modes)
- [Cursor Docs: Rules](https://cursor.com/docs/context/rules)

### Technical Resources
- [Maximizing Cursor's Memory Management](https://www.arsturn.com/blog/efficient-memory-management-cursor-context-handling)
- [Context Engineering for Agentic Swarm Coding](https://www.augmentcode.com/guides/context-engineering-enhancing-agentic-swarm-coding-through-intent-environment-and-system-memory)
- [Replicating Cursor's Agent Mode with E2B and AgentKit](https://e2b.dev/blog/replicating-cursors-agent-mode-with-e2b-and-agentkit)
- [Anthropic: Demystifying Evals for AI Agents](https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents)

---

**Research Compiled By:** Sonnet 4.5 (research-agent)
**Date:** 2026-02-08
**Status:** Comprehensive — extracted all publicly available technical details on prompting, memory, and validation. Many implementation specifics remain proprietary/undisclosed.
