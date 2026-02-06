# Ralph Tool Source Analysis

**Research Date:** 2026-02-06
**Purpose:** Understand Ralph's architecture for pure Rust reimplementation

---

## Executive Summary

Ralph is an autonomous AI development loop technique that enables Claude Code and other AI agents to work continuously on tasks until completion. Multiple implementations exist:

1. **Bash Script** (snarktank/ralph) - Original simple loop
2. **NPM Package** (ralph-cli-claude) - Enhanced with monitoring
3. **Rust Implementations** - ralph-cli, ralph-orchestrator, ralphex
4. **Official Plugin** (Anthropic) - Built into Claude Code

The technique was invented by Geoffrey Huntley in May 2025 and has since become widely adopted.

---

## 1. Core Concept & Origin

### The Ralph Wiggum Technique

Named after the character from The Simpsons, the technique embodies persistent iteration despite setbacks.

**Core Philosophy:**
> "Ralph is a Bash loop" - Geoffrey Huntley

**How it works:**
- Feed AI agent a prompt file
- Let it work and modify files
- Read output (including errors)
- Feed prompt back into agent
- Repeat until completion signal detected

**Key Insight:** The prompt never changes, but the environment (files, git history) does. The AI learns by reading its own previous work.

### Origin Timeline

- **May 2025**: Geoffrey Huntley describes the technique
- **Q1 2026**: Y Combinator hackathon teams adopt Ralph
- **Jan 2026**: Anthropic creates official Claude Code plugin
- **Feb 2026**: Multiple Rust implementations emerge

### Notable Results

- 6 repositories generated overnight at YC hackathon
- $50k contract completed for $297 in API costs
- Complete programming language built over 3 months
- YC teams shipping features autonomously

---

## 2. NPM Package Implementations

### ralph-cli-claude (v0.1.5)

**Package:** `ralph-cli-claude` on NPM
**Repository:** https://github.com/frankbria/ralph-claude-code

**Key Features:**
- Autonomous development loops with intelligent exit detection
- Dual-condition exit: completion indicator AND explicit EXIT_SIGNAL
- Session continuity with `--resume` flag (24-hour default expiration)
- Rate limiting (100 calls/hour, configurable)
- Circuit breaker with advanced error detection
- JSON output format with automatic text parsing fallback

**Commands:**
- `ralph` - Main autonomous loop
- `ralph-enable` - Interactive setup wizard
- `ralph-import` - Convert PRDs to Ralph format
- `ralph-monitor` - Live dashboard with tmux integration
- `ralph-migrate` - Version upgrade script

**Installation:**
```bash
git clone https://github.com/frankbria/ralph-claude-code.git
cd ralph-claude-code
./install.sh
```

**Configuration Files:**
- `.ralphrc` - Project settings (permissions, thresholds)
- `.ralph/PROMPT.md` - Development goals
- `.ralph/fix_plan.md` - Prioritized task list
- `.ralph/AGENT.md` - Auto-maintained build/test commands

**Status:** v0.11.4, 465 passing tests, MIT license, ~6.4k stars

### ralphy-cli

**Package:** `ralphy-cli` on NPM
**Repository:** https://github.com/michaelshimeles/ralphy

**Supported Tools:**
- Claude Code
- OpenCode
- Codex
- Cursor
- Qwen-Code
- Factory Droid
- GitHub Copilot
- Gemini CLI

**Usage:**
```bash
npm install -g ralphy-cli
ralphy "add login button"
ralphy --prd PRD.md
```

**Features:**
- PRD files (markdown with checkboxes)
- YML files (parallel groups and dependencies)
- GitHub issues integration

---

## 3. Original Bash Implementation

### snarktank/ralph

**Repository:** https://github.com/snarktank/ralph
**File:** `ralph.sh`

**Architecture:**

```bash
#!/bin/bash

TOOL="amp"  # Default: amp or claude
MAX_ITERATIONS=10

for i in $(seq 1 $MAX_ITERATIONS); do
    if [ "$TOOL" = "amp" ]; then
        OUTPUT=$(cat prompt.md | amp --dangerously-allow-all 2>&1 | tee /dev/stderr)
    elif [ "$TOOL" = "claude" ]; then
        OUTPUT=$(claude --dangerously-skip-permissions --print < CLAUDE.md 2>&1 | tee /dev/stderr)
    fi

    # Check for completion signal
    if echo "$OUTPUT" | grep -q "<promise>COMPLETE</promise>"; then
        echo "Task completed successfully!"
        exit 0
    fi

    sleep 2
done

echo "Max iterations reached"
exit 1
```

**State Files:**
- `prd.json` - Task/branch state
- `progress.txt` - Iteration history log
- `archive/` - Previous runs
- `.last-branch` - Branch tracking

**Key Mechanisms:**

1. **Tool Invocation:**
   - AMP: `cat prompt.md | amp --dangerously-allow-all`
   - Claude: `claude --dangerously-skip-permissions --print < CLAUDE.md`

2. **Completion Detection:**
   ```bash
   grep -q "<promise>COMPLETE</promise>"
   ```

3. **State Management:**
   - Archives previous runs on branch change
   - Logs all output to stderr for visibility
   - Sleeps 2 seconds between iterations

4. **Error Handling:**
   - Uses `|| true` to suppress errors
   - Continues loop regardless of individual failures

---

## 4. PRD JSON Schema

### prd.json Format

```json
{
  "project": "My Application",
  "branchName": "feature/new-feature",
  "description": "Feature overview",
  "userStories": [
    {
      "id": "US-001",
      "title": "Story headline",
      "description": "Detailed description",
      "acceptanceCriteria": [
        "Criterion 1",
        "Criterion 2"
      ],
      "priority": 1,
      "passes": false,
      "notes": "Additional context"
    }
  ]
}
```

**Field Definitions:**

| Field | Type | Purpose |
|-------|------|---------|
| `project` | string | Application name |
| `branchName` | string | Feature branch identifier |
| `description` | string | Feature overview |
| `id` | string | Story identifier (US-001) |
| `title` | string | Story headline |
| `acceptanceCriteria` | array | Completion requirements |
| `priority` | number | Execution order (1, 2, 3...) |
| `passes` | boolean | Completion status |
| `notes` | string | Additional context |

---

## 5. Agent Prompt Templates

### prompt.md (AMP Template)

**Instructions to AI Agent:**

1. Read `prd.json` to understand tasks
2. Read `progress.txt` to see learnings
3. Pick highest priority story where `passes: false`
4. Verify correct branch checkout
5. Implement single user story
6. Run quality checks (typecheck, lint, test)
7. Commit with format: `feat: [Story ID] - [Story Title]`
8. Update PRD: mark story `passes: true`
9. Append to `progress.txt`:
   - What was implemented
   - Files changed
   - **Learnings for future iterations**
10. Update `AGENTS.md` with reusable patterns
11. When all stories pass, output: `<promise>COMPLETE</promise>`

**Quality Gates:**
- ALL commits must pass quality checks
- Frontend stories must verify UI in browser
- Only mark `passes: true` if fully complete

**Pattern Consolidation:**
- Update "Codebase Patterns" section in progress.txt
- Modify nearby `AGENTS.md` files with reusable insights
- Exclude story-specific details and temporary notes

### CLAUDE.md (Claude Template)

Identical workflow to prompt.md but tailored for Claude Code syntax.

---

## 6. Rust Implementations

### 6.1 ralph-cli (Rust)

**Repository:** https://github.com/mikeyobrien/ralph-orchestrator
**Crate:** `ralph-cli` on crates.io

**Version:** 2.4.4 (released Feb 5, 2026)
**License:** MIT
**Downloads:** 111/month

**Architecture:**

```
ralph-orchestrator/
├── crates/                 # Rust backend logic
│   ├── ralph-core/        # Core orchestration
│   ├── ralph-adapters/    # Backend adapters
│   ├── ralph-proto/       # Protocol definitions
│   ├── ralph-tui/         # Terminal UI
│   └── ralph-telegram/    # Telegram integration
├── backend/ralph-web-server/  # API server
├── frontend/ralph-web/     # React dashboard
├── presets/                # 31 predefined modes
├── prompts/                # Agent templates
└── specs/                  # Plan outputs
```

**Key Dependencies:**
- **CLI Framework:** Clap 4.0 with derive macros
- **Async Runtime:** Tokio with full features
- **TUI:** Ratatui 0.30 and Crossterm 0.28
- **Data:** Serde ecosystem (JSON, YAML)
- **System:** Keyring for credentials, Nix for signals

**Project Metrics:**
- 47K source lines (76.8% Rust, 19.5% TypeScript, 2.6% Python)
- 360 commits, 22 contributors
- ~1.7k GitHub stars

### 6.2 Hat-Based Orchestration

**Core Innovation:** Specialized "hats" represent agent roles.

**Architecture:**
- Each hat handles specific concerns (planning, implementation, validation)
- Event-driven coordination via typed events
- Asynchronous, non-blocking communication
- Central orchestrator routes events between hats

**Key Events:**
- `human.interact` - Agent requests human input (blocks loop)
- `backpressure.rejected` - Validation gate fails
- `LOOP_COMPLETE` - Task finished successfully

**Workflow:**
1. Agent receives prompt + context (specs, memories, tasks)
2. Agent executes and reports results
3. Backpressure gates validate (tests, linting, typechecking)
4. If rejected: loop continues with feedback
5. If passed: mark `LOOP_COMPLETE`
6. Hit iteration ceiling → force termination

**Supported Backends:**
- Claude Code
- Kiro
- Gemini CLI
- Codex
- Amp
- Copilot CLI
- OpenCode

**Backend Initialization:**
```bash
ralph init --backend claude
```

**Likely Implementation Pattern:**
- Strategy pattern for pluggable backends
- Parse backend-specific output
- Map to internal event model

### 6.3 Specification-Driven Development

**Three-Phase Workflow:**

1. **Plan Phase:**
   ```bash
   ralph plan "Build REST API with CRUD operations"
   ```
   Generates:
   - `requirements.md`
   - `design.md`
   - `implementation-plan.md`

2. **Run Phase:**
   ```bash
   ralph run -p "Implementation prompt"
   ```
   Executes against spec

3. **Iterate Phase:**
   Validates against spec until complete

**Benefits:**
- Enforces requirements clarity upfront
- Reduces ambiguity during execution
- Provides validation checkpoints

### 6.4 Human-in-the-Loop (RObot)

**Telegram Integration:**
- Agents emit `human.interact` events
- Loop **blocks** until human responds
- Humans send proactive guidance anytime

**Commands:**
- `/status` - View loop state
- `/tasks` - List current tasks
- `/restart` - Restart loop

**Routing:**
- Reply-to for context
- `@loop-id` prefix for specific loop

**Architecture Impact:**
Transforms Ralph from purely autonomous to **hybrid human-AI**, enabling mid-execution course correction.

### 6.5 Web Dashboard (Alpha)

**Launch:**
```bash
ralph web --backend-port 4000 --frontend-port 8080
```

**Requirements:**
- Node.js >= 18
- Auto-runs `npm install` if needed

**Features (likely):**
- Loop execution timeline
- Memory/task inspection
- Real-time telemetry
- Iteration count tracking
- Gate status monitoring

**Status:** Explicitly marked "Alpha" - breaking changes expected

### 6.6 ralphex (Standalone CLI)

**Repository:** https://github.com/umputun/ralphex
**Website:** https://ralphex.com/

**Key Differences from Bash:**

1. **State via Markdown Checkboxes:**
   ```markdown
   - [ ] Incomplete task
   - [x] Completed task
   ```

2. **Fresh Sessions per Task:**
   - Each task runs in fresh Claude Code session
   - Prevents quality degradation from token accumulation
   - Minimal context per invocation

3. **Configuration:**
   - Global: `~/.config/ralphex/`
   - Project: `.ralphex/`
   - Customizable agents, prompts, settings

4. **Progress Tracking:**
   - Logs to `progress-plan-<name>.txt`
   - Asynchronous monitoring
   - Web dashboard integration

5. **Plan Management:**
   - Plans live in `docs/plans/`
   - Completed plans → `completed/` folder
   - Auto-detects completed tasks via `[x]` checkboxes

6. **Parallel Execution:**
   - 5 review agents run simultaneously
   - Multi-phase validation loops
   - No manual orchestration needed

**Usage:**
```bash
# Create plan in docs/plans/my-feature.md
ralphex docs/plans/my-feature.md

# Resume from checkpoint
ralphex docs/plans/my-feature.md  # Auto-detects completed tasks
```

**Default:** 50 iterations per plan

---

## 7. Official Anthropic Plugin

**Repository:** https://github.com/anthropics/claude-code
**Plugin:** `plugins/ralph-wiggum/`

### Core Architecture

**Implementation:** Stop hook that intercepts Claude's exit attempts.

**Mechanism:**
1. User runs `/ralph-loop "Task" --completion-promise "DONE"`
2. Claude works on task
3. Claude tries to exit
4. Stop hook blocks exit
5. Stop hook feeds SAME prompt back
6. Repeat until completion

**Key Insight:** Self-referential feedback loop:
- Prompt never changes
- Files persist and accumulate changes
- Git history grows
- Claude reads its own past work

### Commands

#### /ralph-loop

Start autonomous loop in current session.

**Syntax:**
```bash
/ralph-loop "<prompt>" --max-iterations <n> --completion-promise "<text>"
```

**Parameters:**
- `--max-iterations <n>` - Iteration limit (default: unlimited)
- `--completion-promise "<text>"` - Completion signal phrase

**Example:**
```bash
/ralph-loop "Build REST API for todos. Requirements: CRUD, validation, tests. Output <promise>COMPLETE</promise> when done." --completion-promise "COMPLETE" --max-iterations 50
```

**Behavior:**
- Implement API iteratively
- Run tests, see failures
- Fix bugs based on output
- Continue until requirements met
- Output promise when complete

#### /cancel-ralph

Cancel active Ralph loop.

**Syntax:**
```bash
/cancel-ralph
```

### Prompt Writing Best Practices

#### 1. Clear Completion Criteria

**Bad:**
```
Build a todo API and make it good.
```

**Good:**
```
Build a REST API for todos.

When complete:
- All CRUD endpoints working
- Input validation in place
- Tests passing (coverage > 80%)
- README with API docs
- Output: <promise>COMPLETE</promise>
```

#### 2. Incremental Goals

**Bad:**
```
Create a complete e-commerce platform.
```

**Good:**
```
Phase 1: User authentication (JWT, tests)
Phase 2: Product catalog (list/search, tests)
Phase 3: Shopping cart (add/remove, tests)

Output <promise>COMPLETE</promise> when all phases done.
```

#### 3. Self-Correction

**Bad:**
```
Write code for feature X.
```

**Good:**
```
Implement feature X following TDD:
1. Write failing tests
2. Implement feature
3. Run tests
4. If any fail, debug and fix
5. Refactor if needed
6. Repeat until all green
7. Output: <promise>COMPLETE</promise>
```

#### 4. Escape Hatches

**Always use `--max-iterations`:**
```bash
/ralph-loop "Try to implement feature X" --max-iterations 20
```

**In prompt, handle stalls:**
```
After 15 iterations, if not complete:
- Document what's blocking progress
- List what was attempted
- Suggest alternative approaches
```

**Note:** `--completion-promise` uses exact string matching and cannot handle multiple conditions. Always rely on `--max-iterations` as primary safety.

### Philosophy

1. **Iteration > Perfection**
   - Don't aim for perfect first try
   - Let loop refine the work

2. **Failures Are Data**
   - "Deterministically bad" = predictable, informative
   - Use failures to tune prompts

3. **Operator Skill Matters**
   - Success depends on good prompts
   - Not just model quality

4. **Persistence Wins**
   - Keep trying until success
   - Loop handles retry logic

### When to Use Ralph

**Good For:**
- Well-defined tasks with clear success criteria
- Tasks requiring iteration (getting tests to pass)
- Greenfield projects (can walk away)
- Tasks with automatic verification (tests, linters)

**Not Good For:**
- Tasks requiring human judgment/design
- One-shot operations
- Unclear success criteria
- Production debugging (use targeted debugging)

### Technical Implementation

**Components:**
- `hooks/stop-hook.sh` - Creates feedback loop by blocking exit
- Commands - `/ralph-loop` and `/cancel-ralph` implementations
- Configuration - Plugin config in `.claude-plugin/`

**Key Difference:** Loop happens INSIDE current session, no external bash needed.

---

## 8. Iteration & Completion Detection Mechanisms

### Completion Detection Patterns

#### 1. Promise Tags (Bash & Plugin)

**Format:**
```
<promise>COMPLETE</promise>
```

**Detection:**
```bash
if echo "$OUTPUT" | grep -q "<promise>COMPLETE</promise>"; then
    exit 0
fi
```

**Pros:**
- Simple string matching
- Language-agnostic
- Clear signal

**Cons:**
- Exact match required
- Can't handle variations
- AI must remember exact format

#### 2. Boolean Flags (PRD JSON)

**Format:**
```json
{
  "passes": false  // incomplete
}
```

**Detection:**
```python
all_complete = all(story["passes"] for story in prd["userStories"])
```

**Pros:**
- Structured data
- Easy to parse
- Progress tracking

**Cons:**
- Requires JSON maintenance
- File I/O overhead

#### 3. Markdown Checkboxes (ralphex)

**Format:**
```markdown
- [ ] Task incomplete
- [x] Task complete
```

**Detection:**
```rust
let incomplete_tasks = plan.lines()
    .filter(|line| line.starts_with("- [ ]"))
    .count();

if incomplete_tasks == 0 {
    complete()
}
```

**Pros:**
- Human-readable
- Git-friendly diffs
- Easy to edit manually

**Cons:**
- Parsing complexity
- Checkbox format variations

#### 4. Iteration Limits

**All implementations:**
```bash
MAX_ITERATIONS=10
for i in $(seq 1 $MAX_ITERATIONS); do
    # work
done
```

**Purpose:**
- Safety net for infinite loops
- Force termination on impossible tasks
- Resource control

**Best Practice:** Always set reasonable limit (10-50 iterations)

### State Management Patterns

#### 1. File-Based State (All Implementations)

**Files:**
- `prd.json` / `plan.md` - Task definitions
- `progress.txt` / `progress.log` - Execution history
- `.last-branch` - Branch tracking
- `AGENTS.md` / `CLAUDE.md` - Knowledge base

**Benefits:**
- Persistence across crashes
- Human-readable
- Git-trackable
- Session-independent

#### 2. Git as Memory

**Key Insight:** Git history IS the memory.

**Usage:**
- Commit after each successful iteration
- Agent reads previous commits
- Diffs show what changed
- History shows iteration progression

**Commit Format:**
```
feat: [US-001] - Add user authentication

- Implemented JWT tokens
- Added login endpoint
- Created user model
- Tests: 12 passing

Learnings:
- JWT secret must be env var
- Token expiry: 1 hour default
```

#### 3. Append-Only Logs

**Pattern:**
```bash
echo "Iteration $i complete" >> progress.txt
```

**Benefits:**
- Never loses information
- Chronological record
- AI can read full history
- Human debugging aid

#### 4. Session Continuity (ralph-cli-claude)

**Features:**
- `--resume` flag
- 24-hour session expiration
- Session hijacking prevention
- State serialization

**Use Case:**
- Long-running tasks (overnight)
- Resume after interruption
- Multi-day projects

---

## 9. Stall Detection & Prevention

### Explicit Stall Detection

**None of the implementations have explicit stall detection.**

Instead, they rely on:

#### 1. Iteration Exhaustion

**Safety Net:**
```bash
MAX_ITERATIONS=10
if [ $i -eq $MAX_ITERATIONS ]; then
    echo "Max iterations reached - likely stalled"
    exit 1
fi
```

**When to use:** Always. Primary safety mechanism.

#### 2. Test Failure Feedback

**Pattern:**
```
1. Run tests
2. Tests fail
3. Agent sees failure output
4. Agent fixes bugs
5. Repeat
```

**Key:** Tests provide feedback loop. Without tests, agent can't know if it's making progress.

#### 3. Quality Gates (ralph-orchestrator)

**Backpressure Mechanism:**
```
Agent completes work
→ Run validation (tests, lint, typecheck)
→ If fail: reject with feedback
→ If pass: mark complete
```

**Prevents:**
- Completing broken code
- Moving forward with bugs
- Accumulating technical debt

#### 4. Fresh Sessions (ralphex)

**Pattern:**
- Each task gets fresh Claude session
- Minimal context
- Prevents token accumulation

**Benefits:**
- Avoids model degradation
- Keeps responses sharp
- Resets confusion state

### Implicit Stall Prevention

#### 1. Size Constraints

**Rule:** Stories must fit one context window.

**Reason:** Large stories cause:
- Model confusion
- Incomplete implementations
- Context overflow

**Solution:** Break into small, focused tasks.

#### 2. AGENTS.md Updates

**Pattern:**
- AI discovers patterns/gotchas
- Writes to AGENTS.md
- Future iterations read AGENTS.md
- Avoids repeating mistakes

**Example:**
```markdown
# Learned Patterns

## JWT Implementation
- Always use environment variable for secret
- Token expiry: 1 hour default
- Refresh tokens required for long sessions

## Common Gotchas
- Postgres timestamps are UTC only
- Frontend expects camelCase, backend uses snake_case
```

#### 3. Progress Logging

**Requirement:** Every iteration logs:
- What was done
- Files changed
- Learnings

**Purpose:**
- Prevent repeated errors
- Document dead ends
- Guide future iterations

#### 4. Circuit Breaker (ralph-cli-claude)

**Features:**
- Multi-line error matching
- 2-stage filtering
- Pattern detection

**Mechanism:**
```
If same error appears 3 times:
→ Stop loop
→ Report pattern
→ Suggest intervention
```

---

## 10. Key Implementation Patterns for Rust Rewrite

### 10.1 Core Loop Structure

```rust
struct RalphLoop {
    max_iterations: usize,
    completion_signal: String,
    current_iteration: usize,
}

impl RalphLoop {
    fn run(&mut self, prompt: &str) -> Result<()> {
        for i in 1..=self.max_iterations {
            self.current_iteration = i;

            // Invoke Claude
            let output = self.invoke_claude(prompt)?;

            // Check for completion
            if output.contains(&self.completion_signal) {
                return Ok(());
            }

            // Sleep between iterations
            std::thread::sleep(Duration::from_secs(2));
        }

        Err(anyhow!("Max iterations reached"))
    }

    fn invoke_claude(&self, prompt: &str) -> Result<String> {
        let output = Command::new("claude")
            .arg("--dangerously-skip-permissions")
            .arg("--print")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?
            .stdin
            .unwrap()
            .write_all(prompt.as_bytes())?;

        // Read output...
        Ok(output)
    }
}
```

### 10.2 PRD JSON Parser

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Prd {
    project: String,
    branch_name: String,
    description: String,
    user_stories: Vec<UserStory>,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserStory {
    id: String,
    title: String,
    description: String,
    acceptance_criteria: Vec<String>,
    priority: u32,
    passes: bool,
    notes: String,
}

impl Prd {
    fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let prd: Prd = serde_json::from_str(&content)?;
        Ok(prd)
    }

    fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    fn next_story(&self) -> Option<&UserStory> {
        self.user_stories
            .iter()
            .filter(|s| !s.passes)
            .min_by_key(|s| s.priority)
    }

    fn all_complete(&self) -> bool {
        self.user_stories.iter().all(|s| s.passes)
    }
}
```

### 10.3 Progress Logger

```rust
struct ProgressLogger {
    path: PathBuf,
}

impl ProgressLogger {
    fn log(&self, entry: &ProgressEntry) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        writeln!(
            file,
            "\n---\n## Iteration {} - {}\n\n{}\n\n### Files Changed:\n{}\n\n### Learnings:\n{}",
            entry.iteration,
            entry.timestamp,
            entry.what_was_done,
            entry.files_changed.join("\n- "),
            entry.learnings
        )?;

        Ok(())
    }
}

struct ProgressEntry {
    iteration: usize,
    timestamp: String,
    what_was_done: String,
    files_changed: Vec<String>,
    learnings: String,
}
```

### 10.4 Completion Detection

```rust
enum CompletionCheck {
    PromiseTag(String),
    AllStoriesPassed,
    MaxIterations,
}

impl CompletionCheck {
    fn check(&self, output: &str, prd: &Prd, iteration: usize, max: usize) -> bool {
        match self {
            Self::PromiseTag(tag) => output.contains(tag),
            Self::AllStoriesPassed => prd.all_complete(),
            Self::MaxIterations => iteration >= max,
        }
    }
}
```

### 10.5 Markdown Checkbox Parser

```rust
use regex::Regex;

struct MarkdownPlan {
    path: PathBuf,
    content: String,
}

impl MarkdownPlan {
    fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            content,
        })
    }

    fn incomplete_tasks(&self) -> Vec<String> {
        let re = Regex::new(r"^- \[ \] (.+)$").unwrap();

        self.content
            .lines()
            .filter_map(|line| re.captures(line))
            .map(|cap| cap[1].to_string())
            .collect()
    }

    fn mark_complete(&mut self, task: &str) -> Result<()> {
        let re = Regex::new(&format!(r"- \[ \] {}", regex::escape(task))).unwrap();
        self.content = re.replace(&self.content, format!("- [x] {}", task)).to_string();
        std::fs::write(&self.path, &self.content)?;
        Ok(())
    }

    fn is_complete(&self) -> bool {
        !self.content.contains("- [ ]")
    }
}
```

### 10.6 Backend Abstraction

```rust
trait Backend {
    fn invoke(&self, prompt: &str) -> Result<String>;
    fn name(&self) -> &str;
}

struct ClaudeBackend {
    flags: Vec<String>,
}

impl Backend for ClaudeBackend {
    fn invoke(&self, prompt: &str) -> Result<String> {
        let mut cmd = Command::new("claude");
        for flag in &self.flags {
            cmd.arg(flag);
        }

        let output = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        // ... implementation
        Ok(output)
    }

    fn name(&self) -> &str {
        "Claude Code"
    }
}

struct AmpBackend {
    // Similar
}

// Factory pattern
fn create_backend(name: &str) -> Result<Box<dyn Backend>> {
    match name {
        "claude" => Ok(Box::new(ClaudeBackend::default())),
        "amp" => Ok(Box::new(AmpBackend::default())),
        _ => Err(anyhow!("Unknown backend: {}", name)),
    }
}
```

### 10.7 Git Integration

```rust
use git2::{Repository, Signature};

struct GitManager {
    repo: Repository,
}

impl GitManager {
    fn new(path: &Path) -> Result<Self> {
        let repo = Repository::open(path)?;
        Ok(Self { repo })
    }

    fn commit(&self, message: &str) -> Result<()> {
        let sig = Signature::now("Ralph Bot", "ralph@example.com")?;
        let tree_id = self.repo.index()?.write_tree()?;
        let tree = self.repo.find_tree(tree_id)?;
        let parent = self.repo.head()?.peel_to_commit()?;

        self.repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            message,
            &tree,
            &[&parent],
        )?;

        Ok(())
    }

    fn current_branch(&self) -> Result<String> {
        let head = self.repo.head()?;
        let branch = head.shorthand().unwrap_or("unknown");
        Ok(branch.to_string())
    }
}
```

### 10.8 Circuit Breaker

```rust
use std::collections::HashMap;

struct CircuitBreaker {
    error_counts: HashMap<String, usize>,
    threshold: usize,
}

impl CircuitBreaker {
    fn new(threshold: usize) -> Self {
        Self {
            error_counts: HashMap::new(),
            threshold,
        }
    }

    fn check(&mut self, output: &str) -> Result<()> {
        // Extract error patterns
        let errors = self.extract_errors(output);

        for error in errors {
            let count = self.error_counts.entry(error.clone()).or_insert(0);
            *count += 1;

            if *count >= self.threshold {
                return Err(anyhow!(
                    "Circuit breaker tripped: repeated error detected: {}",
                    error
                ));
            }
        }

        Ok(())
    }

    fn extract_errors(&self, output: &str) -> Vec<String> {
        // Parse error messages from output
        // Return unique error patterns
        vec![]
    }
}
```

### 10.9 Configuration

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct RalphConfig {
    backend: String,
    max_iterations: usize,
    completion_signal: String,
    session_timeout: u64,
    rate_limit: usize,
    circuit_breaker_threshold: usize,
    dangerous_mode: bool,
}

impl Default for RalphConfig {
    fn default() -> Self {
        Self {
            backend: "claude".to_string(),
            max_iterations: 10,
            completion_signal: "<promise>COMPLETE</promise>".to_string(),
            session_timeout: 86400, // 24 hours
            rate_limit: 100,
            circuit_breaker_threshold: 3,
            dangerous_mode: true,
        }
    }
}

impl RalphConfig {
    fn load() -> Result<Self> {
        let config_path = dirs::config_dir()
            .ok_or_else(|| anyhow!("No config dir"))?
            .join("ralph")
            .join("config.toml");

        if config_path.exists() {
            let content = std::fs::read_to_string(config_path)?;
            let config: RalphConfig = toml::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }
}
```

### 10.10 CLI Structure (using Clap)

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ralph")]
#[command(about = "Autonomous AI development loop")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a Ralph loop
    Run {
        /// Path to PRD file
        #[arg(short, long)]
        prd: PathBuf,

        /// Maximum iterations
        #[arg(short, long, default_value_t = 10)]
        max_iterations: usize,

        /// Backend to use (claude, amp)
        #[arg(short, long, default_value = "claude")]
        backend: String,

        /// Completion signal
        #[arg(short, long, default_value = "<promise>COMPLETE</promise>")]
        completion_signal: String,
    },

    /// Initialize Ralph in current project
    Init {
        /// Backend to use
        #[arg(short, long, default_value = "claude")]
        backend: String,
    },

    /// Convert markdown to PRD JSON
    Convert {
        /// Input markdown file
        input: PathBuf,

        /// Output JSON file
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Monitor active Ralph loops
    Monitor,

    /// Cancel active loop
    Cancel,
}
```

---

## 11. Dependencies Required for Rust Implementation

### Core Dependencies

```toml
[dependencies]
# CLI
clap = { version = "4.5", features = ["derive"] }

# Async runtime
tokio = { version = "1.36", features = ["full"] }

# JSON parsing
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# TOML config
toml = "0.8"

# Error handling
anyhow = "1.0"
thiserror = "1.0"

# Regex
regex = "1.10"

# Git integration
git2 = "0.18"

# File watching (optional)
notify = "6.1"

# Progress bars (optional)
indicatif = "0.17"

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"

# Process execution
subprocess = "0.2"

# System paths
dirs = "5.0"

# HTTP client (for web dashboard)
reqwest = { version = "0.11", features = ["json"] }
axum = "0.7"  # For web server

# TUI (optional)
ratatui = "0.30"
crossterm = "0.28"

# Telegram integration (optional)
teloxide = "0.12"
```

### Optional Dependencies for Advanced Features

```toml
# WebSocket for dashboard
tokio-tungstenite = "0.21"

# Credentials
keyring = "2.3"

# Unix signals
nix = { version = "0.28", features = ["signal"] }

# Rate limiting
governor = "0.6"
```

---

## 12. Comparison: NPM vs Rust Implementations

| Feature | Bash (snarktank) | NPM (ralph-cli-claude) | Rust (ralph-orchestrator) | Rust (ralphex) |
|---------|------------------|------------------------|---------------------------|----------------|
| **Language** | Bash | TypeScript/Node.js | Rust | Rust |
| **Installation** | Copy script | `npm install -g` | `cargo install` | Single binary |
| **State Format** | prd.json | .ralph/ directory | Event-driven | Markdown checkboxes |
| **Completion** | Promise tag | Dual-condition | LOOP_COMPLETE event | All checkboxes |
| **Backends** | Amp, Claude | Claude only | 8 backends | Claude only |
| **Session** | Fresh per run | Resume support | Event-based | Fresh per task |
| **Dashboard** | None | ralph-monitor | React web UI | Async logs |
| **Telegram** | No | No | Yes (human-in-loop) | No |
| **Circuit Breaker** | No | Yes (2-stage) | Yes (backpressure) | No |
| **Rate Limiting** | No | Yes (100/hr) | Configurable | No |
| **Parallel** | No | No | 5 agents | No |
| **TUI** | No | Yes (tmux) | Yes (ratatui) | No |
| **Web Server** | No | No | Yes (axum) | No |
| **Presets** | No | Limited | 31 modes | No |
| **Spec-Driven** | No | No | Yes (3-phase) | Yes (plan files) |
| **Git Integration** | Basic | Advanced | Full | Full |
| **Size** | ~100 lines | ~10K lines | ~47K lines | ~5K lines |
| **Maturity** | Proof of concept | Production-ready | Active development | Stable |
| **License** | MIT | MIT | MIT | MIT |

### Recommendation for Rust Rewrite

**Best starting point:** Hybrid approach:
1. **Core loop:** Based on bash simplicity (snarktank)
2. **State management:** Markdown checkboxes (ralphex)
3. **Backend abstraction:** Strategy pattern (ralph-orchestrator)
4. **Configuration:** TOML files (ralph-cli)
5. **Safety:** Circuit breaker + rate limiting (ralph-cli-claude)

**Why:**
- Start simple, add features incrementally
- Markdown checkboxes are most user-friendly
- Backend abstraction enables future expansion
- Safety features prevent infinite loops

**Initial Features:**
1. Basic loop with max iterations
2. Markdown checkbox parsing
3. Claude Code invocation
4. Git commit after each iteration
5. Progress logging
6. Completion detection (checkboxes + promise tag)

**Future Features:**
- Multiple backends (Amp, OpenCode, etc.)
- Web dashboard
- Telegram integration
- TUI monitor
- Circuit breaker
- Rate limiting
- Session resumption

---

## 13. Architecture Recommendation for Pure Rust Ralph

### Project Structure

```
ralph-rs/
├── Cargo.toml
├── src/
│   ├── main.rs              # CLI entry point
│   ├── lib.rs               # Library exports
│   ├── cli.rs               # Clap command definitions
│   ├── config.rs            # Configuration management
│   ├── loop.rs              # Core loop logic
│   ├── backend/
│   │   ├── mod.rs           # Backend trait
│   │   ├── claude.rs        # Claude implementation
│   │   ├── amp.rs           # Amp implementation
│   │   └── factory.rs       # Backend factory
│   ├── state/
│   │   ├── mod.rs           # State trait
│   │   ├── prd.rs           # PRD JSON format
│   │   ├── markdown.rs      # Markdown checkboxes
│   │   └── converter.rs     # Convert between formats
│   ├── prompt.rs            # Prompt template builder
│   ├── progress.rs          # Progress logging
│   ├── git.rs               # Git operations
│   ├── safety/
│   │   ├── mod.rs           # Safety mechanisms
│   │   ├── circuit_breaker.rs
│   │   └── rate_limiter.rs
│   └── dashboard/           # Optional web UI
│       ├── mod.rs
│       ├── server.rs        # Axum server
│       └── websocket.rs     # Live updates
├── templates/
│   ├── prompt.md            # Amp prompt template
│   └── CLAUDE.md            # Claude prompt template
└── tests/
    ├── integration/
    └── fixtures/
```

### Core Traits

```rust
// Backend abstraction
trait Backend {
    fn invoke(&self, prompt: &str) -> Result<String>;
    fn name(&self) -> &str;
    fn supports_dangerous_mode(&self) -> bool;
}

// State format abstraction
trait State {
    fn load(path: &Path) -> Result<Self> where Self: Sized;
    fn save(&self, path: &Path) -> Result<()>;
    fn next_task(&self) -> Option<Task>;
    fn mark_complete(&mut self, task_id: &str) -> Result<()>;
    fn is_complete(&self) -> bool;
}

// Task abstraction
struct Task {
    id: String,
    title: String,
    description: String,
    priority: u32,
}
```

### Phased Implementation Plan

#### Phase 1: MVP (Week 1)
- [x] CLI structure (clap)
- [x] Basic loop logic
- [x] Markdown checkbox parser
- [x] Claude Code invocation
- [x] Completion detection
- [x] Progress logging

#### Phase 2: Safety (Week 2)
- [ ] Max iterations
- [ ] Circuit breaker
- [ ] Rate limiter
- [ ] Error handling
- [ ] Logging (tracing)

#### Phase 3: Git Integration (Week 3)
- [ ] Auto-commit after iterations
- [ ] Branch management
- [ ] Commit message formatting
- [ ] Git history reading

#### Phase 4: State Formats (Week 4)
- [ ] PRD JSON parser
- [ ] Converter (markdown ↔ JSON)
- [ ] State trait implementation
- [ ] File watching (optional)

#### Phase 5: Multi-Backend (Week 5)
- [ ] Backend trait
- [ ] Amp backend
- [ ] Factory pattern
- [ ] Configuration per backend

#### Phase 6: Advanced Features (Week 6+)
- [ ] Web dashboard
- [ ] TUI monitor
- [ ] Telegram integration
- [ ] Session resumption
- [ ] Parallel execution
- [ ] Spec-driven development

---

## 14. Sources & References

### Official Documentation
- [Anthropic Claude Code - Ralph Wiggum Plugin](https://github.com/anthropics/claude-code/blob/main/plugins/ralph-wiggum/README.md)
- [Ralph by Geoffrey Huntley](https://ghuntley.com/ralph/)

### Original Bash Implementation
- [snarktank/ralph](https://github.com/snarktank/ralph)
- [ralph.sh source](https://github.com/snarktank/ralph/blob/main/ralph.sh)
- [How Ralph Works](https://snarktank.github.io/ralph/)

### NPM Implementations
- [frankbria/ralph-claude-code](https://github.com/frankbria/ralph-claude-code)
- [ralph-cli-claude on npm](https://libraries.io/npm/ralph-cli-claude)
- [ralphy-cli on npm](https://www.npmjs.com/package/ralphy-cli)
- [iannuttall/ralph](https://github.com/iannuttall/ralph)
- [michaelshimeles/ralphy](https://github.com/michaelshimeles/ralphy)

### Rust Implementations
- [mikeyobrien/ralph-orchestrator](https://github.com/mikeyobrien/ralph-orchestrator)
- [ralph-cli on crates.io](https://lib.rs/crates/ralph-cli)
- [umputun/ralphex](https://github.com/umputun/ralphex)
- [ralphex.com](https://ralphex.com/)

### Articles & Analysis
- [Getting Started With Ralph](https://www.aihero.dev/getting-started-with-ralph)
- [Ralph Wiggum: Autonomous Loops for Claude Code](https://paddo.dev/blog/ralph-wiggum-autonomous-loops/)
- [The Ralph Wiggum Playbook](https://paddo.dev/blog/ralph-wiggum-playbook/)
- [Inventing the Ralph Wiggum Loop - Dev Interrupted](https://devinterrupted.substack.com/p/inventing-the-ralph-wiggum-loop-creator)
- [The 'unpossible' logic of Ralph Wiggum–style AI coding](https://tessl.io/blog/unpacking-the-unpossible-logic-of-ralph-wiggumstyle-ai-coding/)
- [Ralph Wiggum loop prompts Claude - The Register](https://www.theregister.com/2026/01/27/ralph_wiggum_claude_loops/)
- [Ralph Wiggum from 'The Simpsons' to AI - VentureBeat](https://venturebeat.com/technology/how-ralph-wiggum-went-from-the-simpsons-to-the-biggest-name-in-ai-right-now)
- [Ralph Wiggum Loop: Bash Coding Agent](https://pasqualepillitteri.it/en/news/192/ralph-wiggum-claude-code-loop-bash-coding-agent)

### Community Resources
- [Ralph Wiggum - Awesome Claude](https://awesomeclaude.ai/ralph-wiggum)
- [Ralph on Claude Hub](https://www.claude-hub.com/resource/github-cli-frankbria-ralph-claude-code-ralph-claude-code/)

---

## 15. Key Takeaways for Rust Implementation

1. **Start Simple:** Basic bash loop is ~100 lines. Prove concept before adding features.

2. **Markdown First:** Checkboxes are most user-friendly state format. Add JSON later.

3. **Safety Critical:** Always implement max iterations and circuit breaker. Infinite loops waste money.

4. **Git is Memory:** Commit after each iteration. History shows progression.

5. **Backend Abstraction:** Strategy pattern enables supporting multiple AI tools.

6. **Prompt Quality:** Success depends on good prompts, not just good code.

7. **Failures Are Data:** Test failures guide next iteration. Embrace them.

8. **File-Based State:** Persist everything to disk. Survive crashes.

9. **Human Override:** Always allow manual intervention. Telegram/TUI optional but valuable.

10. **Iteration Over Perfection:** Let loop refine work. Don't aim for perfect first try.

### Critical Success Factors

1. **Clear completion criteria** in prompts
2. **Automatic verification** (tests, linters)
3. **Small, focused tasks** (fit in one context window)
4. **Feedback loops** (tests provide guidance)
5. **Persistent state** (survive crashes)
6. **Safety limits** (max iterations, circuit breaker)
7. **Progress visibility** (logs, dashboard)
8. **Git integration** (history as memory)

### Anti-Patterns to Avoid

1. **No completion signal** → Infinite loops
2. **Large tasks** → Model confusion
3. **No tests** → Can't verify progress
4. **Stateless** → Lose context on crash
5. **Unlimited iterations** → Wasted API costs
6. **Complex prompts** → Harder to debug
7. **No logging** → Can't debug failures
8. **No git commits** → Lose progress

---

## Conclusion

Ralph is a powerful technique for autonomous AI development. Multiple mature implementations exist, with the Rust ecosystem offering the best combination of performance, safety, and features.

For a pure Rust rewrite eliminating NPM dependencies, recommend starting with ralphex's simplicity (markdown checkboxes, fresh sessions) and adding ralph-orchestrator's safety features (circuit breaker, rate limiting) as the implementation matures.

The key insight: **Simple loops with good prompts beat complex orchestration with bad prompts.** Focus on prompt quality and safety mechanisms first, add advanced features second.
