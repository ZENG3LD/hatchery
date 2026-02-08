# Phase 1: Swarm Case Research Agent Prompt

## Agent Type
`research-agent` (Sonnet)

## Variables
- `{CASE_NAME}` - Name of the swarm case (e.g., "Cursor FastRender")
- `{CASE_NUMBER}` - Two-digit case number (e.g., "01")
- `{SOURCES}` - Known source URLs to start from
- `{MODE}` - OVERVIEW or TECHNICAL

---

## MODE: OVERVIEW

Collect ALL available information about {CASE_NAME} swarm implementation.

Create file: `cases/{CASE_NUMBER}-{case_slug}-overview.md`

```
Research {CASE_NAME} swarm/multi-agent implementation comprehensively.

Start from these sources: {SOURCES}

Also search for: "{CASE_NAME} swarm", "{CASE_NAME} multi-agent", "{CASE_NAME} agent architecture"
Search HackerNews: "site:news.ycombinator.com {CASE_NAME}"
Search Twitter/X posts from team members.
Search YouTube for demos and talks.

═══════════════════════════════════════════════════════════════════════════════
SECTION 1: Overview & Scale
═══════════════════════════════════════════════════════════════════════════════

- What was built? (product/result)
- Timeline (how long did it take?)
- Number of agents (min, max, average)
- Model(s) used (exact model IDs if available)
- Lines of code generated
- Number of commits/PRs
- Cost (if disclosed)
- Token usage (input/output if disclosed)
- Team size (humans involved)
- Success metrics (benchmarks, tests passed, etc.)

═══════════════════════════════════════════════════════════════════════════════
SECTION 2: Architecture
═══════════════════════════════════════════════════════════════════════════════

- Architecture type: hierarchical / flat / hybrid?
- Number of hierarchy levels (exact depth)
- Agent roles (list ALL roles: planner, worker, judge, reviewer, etc.)
- Fan-out ratio (how many children per parent node?)
- How are agents spawned? (static at start / dynamic on demand?)
- Can agents spawn sub-agents?
- What infrastructure runs agents? (Docker, VMs, local processes, cloud?)

═══════════════════════════════════════════════════════════════════════════════
SECTION 3: Communication
═══════════════════════════════════════════════════════════════════════════════

- Communication model: mailbox / shared memory / message passing / file-based?
- Direction: peer-to-peer / up-only / down-only / bidirectional?
- Message format/schema (JSON? plain text? structured?)
- Push vs pull delivery?
- Latency between agents?
- What happens when agent is busy? (queue, drop, retry?)
- Broadcast capability?
- Can agents communicate across hierarchy levels (skip levels)?

═══════════════════════════════════════════════════════════════════════════════
SECTION 4: Git & Code Integration
═══════════════════════════════════════════════════════════════════════════════

- Git strategy: worktrees / branches / separate repos?
- Who merges? (dedicated agent / each agent / central authority?)
- Conflict resolution: automatic / manual / retry?
- Branch naming convention?
- Commit frequency? (per task / per change / batched?)
- Monorepo or polyrepo?

═══════════════════════════════════════════════════════════════════════════════
SECTION 5: What Worked & What Failed
═══════════════════════════════════════════════════════════════════════════════

- What approaches were tried and FAILED before finding the working one?
- Key insights / lessons learned
- Biggest challenges
- Scaling limitations
- Cost/efficiency observations

═══════════════════════════════════════════════════════════════════════════════
SECTION 6: Open Source & Artifacts
═══════════════════════════════════════════════════════════════════════════════

CRITICAL SECTION — search thoroughly!

- Is ANY part of the swarm infrastructure open source?
- GitHub repos (exact URLs)
- Is the built product open source?
- Are prompts shared publicly?
- Are configuration files / schemas shared?
- Any community replications or clones?
- Related open source frameworks used or inspired by this case

Search: "{CASE_NAME} open source", "{CASE_NAME} github", "{CASE_NAME} source code"
```

---

## MODE: TECHNICAL

Deep dive into technical implementation details of {CASE_NAME} swarm.

Create file: `cases/{CASE_NUMBER}-{case_slug}-technical.md`

```
Deep technical dive into {CASE_NAME} swarm implementation.

Start from these sources: {SOURCES}

Also search for technical deep-dives, code snippets, configuration examples.

═══════════════════════════════════════════════════════════════════════════════
SECTION 1: Prompts & Prompting Strategy
═══════════════════════════════════════════════════════════════════════════════

- What prompts are used for each agent role?
- Any prompt examples shared publicly? (copy exact text!)
- System prompts vs user prompts?
- How are prompts parameterized (variables, templates)?
- Constraints-based or instructions-based prompting?
- Domain-specific knowledge embedded in prompts?
- How is task context injected into prompts?
- Prompt length / token count if known?

═══════════════════════════════════════════════════════════════════════════════
SECTION 2: Memory & Context Management
═══════════════════════════════════════════════════════════════════════════════

- Context window size per agent?
- Compaction/compression strategy?
- When is compaction triggered? (token threshold? turn count?)
- What is preserved vs discarded during compaction?
- Scratchpad / shared memory format?
- How is state persisted between agent restarts?
- Fresh start strategy (when? what carries over?)
- Inter-agent shared state (what can agents see of each other's work?)

═══════════════════════════════════════════════════════════════════════════════
SECTION 3: Task Distribution & Scheduling
═══════════════════════════════════════════════════════════════════════════════

- Pull-based (agents pick tasks) or push-based (assigned by coordinator)?
- Task format/schema (JSON? YAML? plain text?)
- Task dependency tracking (DAG? blockedBy? sequential?)
- How is task completion verified?
- What happens when a task fails? (retry? reassign? escalate?)
- Task claiming / locking mechanism?
- Load balancing strategy?
- Priority system?

═══════════════════════════════════════════════════════════════════════════════
SECTION 4: Validation & Quality Control
═══════════════════════════════════════════════════════════════════════════════

- How is work validated? (tests? compilation? review agent?)
- Quality gates between phases?
- Self-correction loops (how deep? what triggers?)
- Error detection mechanism?
- Who can reject work? (judge? planner? any agent?)
- Feedback format from validator to worker?
- Human-in-the-loop checkpoints?

═══════════════════════════════════════════════════════════════════════════════
SECTION 5: Mailbox / Inbox Implementation
═══════════════════════════════════════════════════════════════════════════════

- Storage: file-based / database / in-memory / API?
- Exact path/location of mailbox storage?
- Message types (regular, notification, shutdown, task update, etc.)
- Message schema (copy exact JSON if available!)
- Delivery guarantee (at-least-once? exactly-once? best-effort?)
- Ordering guarantee?
- Polling interval or event-driven (file watcher, webhook)?
- Overflow handling?
- Message TTL / expiration?

═══════════════════════════════════════════════════════════════════════════════
SECTION 6: Open Source Artifacts & Code
═══════════════════════════════════════════════════════════════════════════════

CRITICAL SECTION — this is the MOST VALUABLE part!

Search EXHAUSTIVELY for:
- GitHub repos with swarm infrastructure code
- Open source frameworks used
- Published code snippets in blog posts
- Configuration file examples
- Docker/container setup files
- CI/CD pipeline configs
- Any code that can be studied or reused

For EACH artifact found:
- Exact URL
- What it contains
- License
- Stars/forks count
- Last update date
- Relevance to swarm orchestration

Search patterns:
- github.com search: "{CASE_NAME} swarm"
- github.com search: "{CASE_NAME} multi-agent"
- github.com search: "{CASE_NAME} agent orchestration"
- Any repos mentioned in blog posts or discussions
- npm / crates.io / pypi packages related to this case
```

---

## Exit Criteria

### OVERVIEW mode:
- All 6 sections filled with real data (not guessed)
- Sources cited for each claim
- Open source section thoroughly searched

### TECHNICAL mode:
- All 6 sections filled with maximum available detail
- Any code/config examples copied verbatim
- Open source artifacts section is EXHAUSTIVE
- Every GitHub repo found is documented with URL and description

---

## Important Rules

1. ONLY include information from real sources — never invent or guess
2. If information is not available, write "NOT DISCLOSED" — don't fill gaps with assumptions
3. Copy exact quotes when possible
4. Include URLs for ALL sources
5. Open source artifacts are the HIGHEST PRIORITY — search for them exhaustively
6. If you find a relevant GitHub repo, document its structure, key files, and how it relates to swarm orchestration
