# Anthropic C Compiler Swarm: Project Overview

## 1. Overview & Scale

### What Was Built
A production-grade C compiler written in Rust from scratch, capable of compiling the Linux kernel and major real-world software projects. The compiler implements a complete multi-pass architecture including:
- Hand-written recursive descent parser
- Type-checking semantic analysis phase
- Intermediate representation (IR) layer
- Code generation backend targeting x86-64 Linux
- Support for x86, ARM, and RISC-V architectures

### Team & Model
- **Agents**: 16 parallel Claude Opus 4.6 instances
- **Lead Researcher**: Nicholas Carlini (Anthropic Safeguards team)
- **Model**: Claude Opus 4.6 exclusively

### Timeline & Sessions
- **Duration**: Approximately 2 weeks (14 days)
- **Sessions**: Nearly 2,000 Claude Code sessions
- **Development Approach**: Continuous autonomous operation with minimal human intervention

### Code Output
- **Lines of Code**: 100,000 lines of Rust
- **Architecture**: Multi-pass compiler with parser, semantic analyzer, IR layer, and code generator
- **Capabilities**: Handles C preprocessor, complex declaration syntax, implicit type conversions, undefined behavior edge cases

### Token Consumption
- **Input Tokens**: 2 billion
- **Output Tokens**: 140 million
- **Total Cost**: Just under $20,000 in API costs

### Validation Results
- **GCC Torture Test Suite**: 99.1% pass rate (18,234 of 18,397 tests)
- **Compiled Projects**: Linux 6.9 kernel (x86/ARM/RISC-V), QEMU, FFmpeg, SQLite, PostgreSQL, Redis, Doom
- **Bootability**: Successfully boots Linux on multiple architectures

## 2. Architecture

### Flat Structure Design
The system employed a **completely flat, non-hierarchical architecture** with no central orchestrator or master agent directing traffic. As stated in the official blog post: "I haven't yet implemented any other method for communication between agents, nor do I enforce any process for managing high-level goals. I don't use an orchestration agent."

### Lead + Teammates Relationship
**No traditional lead-teammate hierarchy exists** in this implementation. Instead:
- Each agent operates autonomously and independently
- All 16 agents have equal status and capabilities
- No agent coordinates or manages others
- Work distribution occurs through file-based task locking, not delegation

This differs from the later Agent Teams feature released in Claude Code, which does implement a lead/teammate model.

### Agent Roles
The compiler project used two approaches for role assignment:

**Phase 1: Parallel Bug Fixing**
- All 16 agents worked on different failing tests simultaneously
- No role specialization - each agent picked from available test failures
- Natural work distribution through test suite diversity

**Phase 2: Post-99% Pass Rate Specialization**
After achieving 99% test pass rate, agents took on specialized roles:
- **Compiler Completeness**: Getting different open-source projects to compile (SQLite, Redis, libjpeg, MQuickJS, Lua)
- **Code Deduplication**: Eliminating duplicate code patterns
- **Performance Optimization**: Improving compiler runtime performance
- **Output Efficiency**: Optimizing generated assembly code quality
- **Code Review**: Reviewing Rust code quality and design patterns
- **Documentation**: Managing and updating project documentation

### Can Teammates Spawn Sub-Teams?
**No.** The compiler project had no team spawning capability. This was a single-level swarm of 16 agents working in parallel. Each agent operated in complete isolation without the ability to spawn additional agents or form sub-teams.

The later Claude Code Agent Teams feature (released February 2026) explicitly prohibits nested teams: "teammates cannot spawn their own teams or teammates. Only the lead can manage the team."

### Docker Isolation
**Complete containerization** with file system isolation:

**Container Architecture**:
```
1. Bare git repository created as coordination point
2. Each agent gets dedicated Docker container
3. Repository mounted to /upstream (read-only shared state)
4. Agent clones to /workspace (isolated working directory)
5. Agent works, commits, pushes to /upstream
6. Container destroyed after session completes
```

**Isolation Benefits**:
- **No context pollution**: Each agent starts fresh with no conversation history
- **Clean state**: Forces reliance on README files and code comments for orientation
- **Resource independence**: Container crashes don't affect other agents
- **Security**: Failed experiments contained within disposable environments

**Quoted from official blog**: "A new bare git repo is created, and for each agent, a Docker container is spun up with the repo mounted to `/upstream`. Each agent clones a local copy to `/workspace`, and when it's done, pushes from its own local container to upstream."

## 3. Communication

### TeammateTool
**NOT USED in the compiler project.** TeammateTool is part of the Claude Code Agent Teams feature released in February 2026, which postdates or coincides with the compiler project announcement. The compiler project used a simpler file-based coordination mechanism instead.

TeammateTool features (available in Agent Teams, not used in compiler):
- 13 operations for team coordination
- Direct agent-to-agent messaging
- Plan approval workflows
- Graceful shutdown protocols

### JSON Inbox
**NOT USED in the compiler project.** The inbox system (`~/.claude/teams/{name}/inboxes/{agent}.json`) is part of the Agent Teams feature, not the compiler project.

The compiler project had no direct inter-agent messaging system.

### File Watchers
**NOT USED in the traditional sense.** The compiler project did not use file system event watchers for coordination.

### Message Types
**NOT APPLICABLE** - The compiler project had no message passing system. Communication occurred implicitly through:
- Git commit history
- Lock files in `current_tasks/` directory
- README files and progress documentation
- Code comments and test results

### Delivery Latency
**NOT APPLICABLE** - No message delivery system existed. Coordination latency was determined by:
- Git pull/merge/push cycles
- File lock acquisition timing (optimistic locking via git)
- Container startup/shutdown overhead
- Test harness execution time

## 4. Git & Code Integration

### 16 Agents in Same Monorepo
**Single shared repository architecture** with file-based synchronization:

**Repository Structure**:
```
compiler-project/           # Bare git repo
├── src/                    # Rust source code
├── tests/                  # Test suites
├── current_tasks/          # Lock files for task claiming
│   ├── parse_if_statement.txt
│   ├── codegen_function_definition.txt
│   └── ...
├── README.md               # Agent orientation
└── progress/               # Status documentation
```

### Pull-Merge-Push Workflow
**Continuous synchronization cycle** executed by each agent autonomously:

```bash
# Agent's work loop (simplified)
while true; do
    # 1. Claim a task via file lock
    touch current_tasks/my_task.txt
    git add current_tasks/my_task.txt
    git commit -m "Claim task: my_task"
    git push  # May fail if another agent claimed same task

    # 2. Work on task
    # ... make changes to source code ...

    # 3. Synchronize with other agents
    git pull  # Fetch changes from other agents
    # Claude handles merge conflicts autonomously

    # 4. Push changes
    git commit -m "Implement feature X"
    git push

    # 5. Release lock
    rm current_tasks/my_task.txt
    git add current_tasks/my_task.txt
    git commit -m "Release task: my_task"
    git push
done
```

**Official quote**: "Claude works on the task, then pulls from upstream, merges changes from other agents, pushes its changes, and removes the lock."

### Conflict Resolution Strategy
**Claude autonomously resolves merge conflicts** without human intervention:

**File-Based Lock Conflicts**:
- Two agents attempt to claim same task → git push fails for second agent
- Second agent's push is rejected by git (optimistic locking)
- Rejected agent pulls latest state, sees task claimed, picks different task
- Git's built-in atomicity enforces mutual exclusion

**Official quote**: "To prevent two agents from trying to solve the same problem at the same time, the harness uses a simple synchronization algorithm: Claude takes a 'lock' on a task by writing a text file to `current_tasks/`... if two agents try to claim the same task, git's synchronization forces the second agent to pick a different one."

**Code Merge Conflicts**:
- Agents pull latest changes before pushing
- Claude analyzes conflicting changes from other agents
- Merge conflicts resolved autonomously based on:
  - Code context and semantics
  - Compiler correctness requirements
  - Test suite compatibility
- No human intervention required for conflict resolution

**Quoted**: "Merge conflicts are frequent, but Claude is smart enough to figure that out."

**Conflict Frequency**:
Merge conflicts occurred frequently due to:
- Multiple agents editing shared files (compiler core, type system, code generator)
- Rapid iteration cycles (2,000 sessions over 2 weeks = ~143 sessions/day)
- No file ownership or partition system

### No Dedicated Merger Agent
**Confirmed: No dedicated merger role.** Each agent handles its own merges and conflict resolution autonomously. The flat architecture has no specialized roles for merge coordination.

Quote confirming flat structure: "I don't use an orchestration agent."

## 5. What Worked & What Failed

### GCC Oracle Strategy (What Worked)

**Problem**: After reaching 99% pass rate, compiling Linux kernel became a bottleneck. Quote: "All 16 agents would hit the same bug, fix that bug, and then overwrite each other's changes" because kernel compilation is "one giant task."

**Solution**: Randomized subset compilation using GCC as ground truth oracle:

**Implementation**:
```python
# Pseudo-code of oracle strategy
def test_kernel_compilation(claude_compiler):
    # Randomly select subset of kernel files to compile with Claude
    kernel_files = get_all_kernel_files()
    claude_subset = random.sample(kernel_files, k=100)
    gcc_subset = [f for f in kernel_files if f not in claude_subset]

    # Compile most files with GCC (known good)
    compile_with_gcc(gcc_subset)

    # Compile remaining files with Claude's compiler
    compile_with_claude(claude_subset)

    # Link everything together and test
    kernel = link_all_objects()
    if kernel.boots():
        # Bug is NOT in Claude's subset
        return "claude_subset_ok"
    else:
        # Bug IS in Claude's subset - refine further
        return "bug_in_claude_subset"
```

**Official quote**: "I wrote a new test harness that randomly compiled most of the kernel using GCC, and only the remaining files with Claude's C Compiler. If the kernel worked, then the problem wasn't in Claude's subset of the files. If it broke, then it could further refine by re-compiling some of these files with GCC."

**Why It Worked**:
- **Enables parallelization**: Each agent tests different random subsets simultaneously
- **Isolates bugs**: Narrows down problematic files through binary search
- **Reduces conflicts**: Agents work on different files, not same bug
- **Provides ground truth**: GCC serves as correctness oracle
- **Scales to large codebases**: Works even when individual files are independently correct but combination fails

**Results**: "This let each agent work in parallel, fixing different bugs in different files, until Claude's compiler could eventually compile all files."

### What Failed

**1. Monolithic Task Assignment**
**Failure**: Linux kernel compilation as single task caused all 16 agents to:
- Encounter the same compilation failure
- Fix the identical bug in parallel
- Overwrite each other's fixes through git conflicts
- Waste resources on duplicate work

**Quote**: "All 16 agents would hit the same bug, fix that bug, and then overwrite each other's changes."

**Lesson**: Tasks must decompose into independent sub-problems for parallel agents to add value. Monolithic tasks serialize work regardless of agent count.

**2. 16-bit x86 Code Generation**
**Failure**: Model unable to implement 16-bit real mode code generator.

**Quote**: "Opus was unable to implement a 16-bit x86 code generator needed to boot into 16-bit real mode. While the compiler can output correct 16-bit x86 via the 66/67 opcode prefixes, the resulting compiled output is over 60kb, far exceeding the 32k code limit enforced by Linux."

**Consequence**: System calls out to GCC for 16-bit compilation phase (boot sector).

**Root Cause**: Likely combination of:
- Obscure x86 real mode semantics
- Tight size constraints (32KB limit)
- Complex opcode prefix requirements
- Limited training data for 16-bit x86 assembly

**3. Assembler and Linker Implementation**
**Partial Failure**: "The very last bits that Claude started automating and are still somewhat buggy."

**Issues**:
- Incomplete object file format generation
- Linking phase errors
- Reliability issues with automated assembler/linker tooling

**4. Code Efficiency Optimization**
**Failure**: Generated code quality significantly worse than GCC.

**Quote**: "Even with all optimizations enabled, it outputs less efficient code than GCC with all optimizations disabled."

**Metrics**: NOT DISCLOSED (specific performance degradation percentages not published)

**Analysis**: This is expected for a 2-week compiler project. GCC represents 37 years of optimization engineering by thousands of experts. The gap demonstrates current AI limitations in:
- Sophisticated optimization strategies
- Deep performance tuning
- Architecture-specific micro-optimizations

### Parallelization Insights

**Key Insight 1: Task Granularity Determines Scaling**
- **Fine-grained tasks** (individual test failures): Near-linear speedup with 16 agents
- **Coarse-grained tasks** (kernel compilation): All agents bottleneck on same issue

**Key Insight 2: Independence Enables Parallelism**
When test suites had "hundreds of independent failures," agents naturally distributed work by picking different failing tests. This emergent load balancing worked because:
- No coordination overhead required
- Minimal git conflicts (different test files)
- Clear success criteria per test
- Natural work queue (failing test list)

**Key Insight 3: Oracle-Based Testing Enables Parallelization**
The GCC oracle strategy transformed a serial task (kernel compilation) into parallelizable work by:
- Creating multiple independent search spaces (different random file subsets)
- Providing binary feedback (boots or doesn't)
- Enabling simultaneous exploration of different hypotheses

**Key Insight 4: Conflict Frequency Indicates Over-Parallelization**
Quote: "Merge conflicts are frequent" during kernel compilation phase suggests diminishing returns beyond certain agent count. Optimal parallelization likely < 16 agents for tightly coupled work.

**Key Insight 5: Documentation Enables Autonomous Orientation**
Quote: "Each agent is dropped into a fresh container with no context and will spend significant time orienting itself, especially on large projects. To help Claude help itself, extensive READMEs and progress files should be updated frequently with the current status."

**Scaling Factors**:
- ✅ **Scales well**: Independent test failures, modular features, separate files
- ❌ **Scales poorly**: Single compilation unit, tightly coupled code, shared core infrastructure
- 🔄 **Requires adaptation**: Large integrated tasks need oracle strategies or decomposition

## 6. Open Source & Artifacts

### Claude Code SDK
**Open source and publicly available** as of February 2026:

**Official Repositories**:
- Python SDK: https://github.com/anthropics/claude-agent-sdk-python
- TypeScript SDK: https://github.com/anthropics/claude-agent-sdk-typescript
- Demo Applications: https://github.com/anthropics/claude-agent-sdk-demos

**License**: MIT License, governed by Anthropic's Commercial Terms of Service

**SDK Capabilities**:
- Programmatic agent building
- Codebase understanding
- File editing operations
- Command execution
- Complex workflow orchestration
- Custom tool integration via MCP (Model Context Protocol)
- Hooks system for deterministic processing
- Subagent support

**Note**: The SDK was formerly "Claude Code SDK," renamed to "Claude Agent SDK" with migration guide at https://docs.claude.com/en/docs/claude-code/sdk/migration-guide

### Open Source Parts
**Claude Code Core**: https://github.com/anthropics/claude-code
- Agentic coding tool for terminal environments
- Understands codebases
- Handles git workflows
- Natural language command interface
- **License**: NOT DISCLOSED in search results (likely proprietary with limited open source components)

**Community Ecosystem**:
- Awesome Claude Code: https://github.com/hesreallyhim/awesome-claude-code
  - Curated list of skills, hooks, slash-commands, agent orchestrators, applications, plugins

**SDK Components** (Open Source):
- Agent query interface (`query()` function)
- Client library (`ClaudeSDKClient`)
- Tool system (Read, Write, Bash, Browse)
- MCP server support (in-process and external)
- Hooks framework
- Type definitions

### Published Configurations
**Compiler Project Specifics**: NOT DISCLOSED

The following configurations were NOT published:
- ❌ Agent prompts used for compiler development
- ❌ Test harness code for GCC oracle strategy
- ❌ Task decomposition specifications
- ❌ README templates for agent orientation
- ❌ Docker container configurations
- ❌ Git repository structure

**Agent Teams General Configuration** (Published in docs):
```json
// Enable agent teams (settings.json)
{
  "env": {
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1"
  },
  "teammateMode": "in-process" // or "tmux" or "auto"
}
```

**Environment Variables** (Published):
```bash
CLAUDE_CODE_TEAM_NAME="{team-name}"
CLAUDE_CODE_AGENT_ID="{name}@{team}"
CLAUDE_CODE_AGENT_NAME="{name}"
CLAUDE_CODE_AGENT_TYPE="{type}"
CLAUDE_CODE_AGENT_COLOR="#{hex}"
CLAUDE_CODE_PLAN_MODE_REQUIRED="true|false"
CLAUDE_CODE_PARENT_SESSION_ID="{id}"
```

### Gists
**Kieran Klaassen's Comprehensive Guides** (Community-created):

**1. Claude Code Swarm Orchestration Skill**
- URL: https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea
- **Content**: Complete guide to multi-agent coordination with TeammateTool, Task system, and all patterns
- **Covers**: 13 TeammateTool operations, task system schemas, directory structures, environment variables, orchestration patterns

**2. Claude Code Multi-Agent Orchestration System**
- URL: https://gist.github.com/kieranklaassen/d2b35569be2c7f1412c64861a219d51f
- **Content**: Alternative multi-agent orchestration documentation
- **Note**: Likely earlier or alternative version of first gist

**3. Architectural Comparison**
- URL: https://gist.github.com/ruvnet/18dc8d060194017b989d1f8993919ee4
- **Content**: Claude Flow V3 vs Claude Code TeammateTool comparison
- **Useful for**: Understanding design tradeoffs between different orchestration approaches

### Community Tools
**MaTriXy's Claude Swarm Orchestration**:
- Repository: https://github.com/MaTriXy/claude-swarm-orchestration
- **Content**: Documentation for teammate API
- **Path**: `docs/teammate-api.md`

**Piebald AI's System Prompts**:
- Repository: https://github.com/Piebald-AI/claude-code-system-prompts
- **Content**: Tool description for TeammateTool
- **Path**: `system-prompts/tool-description-teammatetool.md`
- **Useful for**: Understanding how TeammateTool is presented to the model

**siteboon's Claude Code UI**:
- Repository: https://github.com/siteboon/claudecodeui
- **Content**: Web/mobile GUI for Claude Code (CloudCLI)
- **License**: Free open source
- **Capabilities**: Remote session management, project management

**obra's Superpowers**:
- Issue: https://github.com/obra/superpowers/issues/429
- **Content**: Feature request for TeammateTool, SendMessage, TaskList support
- **Useful for**: Understanding community demand for agent teams features

### Published Compiler Artifacts
**NOT DISCLOSED**: The 100,000-line compiler itself is **not open sourced**.

Nicholas Carlini and Anthropic have not released:
- ❌ Compiler source code
- ❌ Git repository history
- ❌ Test suites used
- ❌ Training harness code
- ❌ Agent logs or transcripts
- ❌ Performance benchmarks

**Reason**: Likely research demonstration rather than production tool release. Primary value is proving feasibility of multi-agent autonomous development, not distributing the compiler itself.

---

## Sources

- [Building a C compiler with a team of parallel Claudes](https://www.anthropic.com/engineering/building-c-compiler)
- [Anthropic releases Opus 4.6 with new agent teams | TechCrunch](https://techcrunch.com/2026/02/05/anthropic-releases-opus-4-6-with-new-agent-teams/)
- [Orchestrate teams of Claude Code sessions - Claude Code Docs](https://code.claude.com/docs/en/agent-teams)
- [AddyOsmani.com - Claude Code Swarms](https://addyosmani.com/blog/claude-code-agent-teams/)
- [Claude Code's Hidden Multi-Agent System](https://paddo.dev/blog/claude-code-hidden-swarm/)
- [Claude Code Swarm Orchestration Skill - Kieran Klaassen Gist](https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea)
- [We tasked Opus 4.6 using agent teams to build a C Compiler | Hacker News](https://news.ycombinator.com/item?id=46903616)
- [Claude Code's new hidden feature: Swarms | Hacker News](https://news.ycombinator.com/item?id=46743908)
- [Anthropic's $20,000 Experiment](https://www.webpronews.com/anthropics-20000-experiment-how-16-parallel-ai-agents-built-a-100000-line-c-compiler-from-scratch-in-rust/)
- [GitHub - anthropics/claude-agent-sdk-python](https://github.com/anthropics/claude-agent-sdk-python)
- [GitHub - anthropics/claude-agent-sdk-typescript](https://github.com/anthropics/claude-agent-sdk-typescript)
- [GitHub - anthropics/claude-code](https://github.com/anthropics/claude-code)
- [Hooks reference - Claude Code Docs](https://code.claude.com/docs/en/hooks)
- [GitHub - hesreallyhim/awesome-claude-code](https://github.com/hesreallyhim/awesome-claude-code)
