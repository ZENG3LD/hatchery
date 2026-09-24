# Coordinator Prompt: Swarm Case Research Pipeline

## Overview

This prompt is for the Opus coordinator agent to run parallel research on real-world AI swarm implementations.

---

## Instructions for Coordinator

You are coordinating deep research into real-world swarm/multi-agent coding systems. For each case, launch 2 agents in parallel.

### Agent Pair Pattern

For each case `{CASE}`:

```
Agent A (Overview): research-agent
Prompt: Read prompts/01_research.md and use it with:
  {CASE_NAME} = case name
  {CASE_NUMBER} = case number (01-11)
  {SOURCES} = known URLs for this case
  Mode: OVERVIEW — collect all available information

Agent B (Technical): research-agent
Prompt: Read prompts/01_research.md and use it with:
  {CASE_NAME} = case name
  {CASE_NUMBER} = case number (01-11)
  {SOURCES} = known URLs for this case
  Mode: TECHNICAL — focus on prompts, mailboxes, memory, open source artifacts
```

**Launch both agents in parallel. Wait for completion.**

### Case Registry

| # | Case | Known Sources |
|---|------|---------------|
| 01 | Cursor FastRender | cursor.com/blog/scaling-agents, cursor.com/blog/self-driving-codebases, github.com/wilsonzlin/fastrender |
| 02 | Anthropic C Compiler | anthropic.com/engineering/building-c-compiler, code.claude.com/docs/en/agent-teams |
| 03 | OpenAI Codex | openai.com/index/introducing-codex, openai.com/index/introducing-gpt-5-3-codex |
| 04 | Kimi K2.5 Agent Swarm | kimi.com/blog/kimi-k2-5.html |
| 05 | Zach Wills 20 Agents | zachwills.net/i-managed-a-swarm-of-20-ai-agents-for-a-week-here-are-the-8-rules-i-learned/ |
| 06 | Claude Code Flight Build | addyosmani.com/blog/claude-code-agent-teams/ |
| 07 | Rokt + Replit Agent 3 | blog.replit.com/introducing-agent-3-our-most-autonomous-agent-yet |
| 08 | GitHub Agent HQ | github.blog/ai-and-ml/github-copilot/how-to-orchestrate-agents-using-mission-control/ |
| 09 | Windsurf Wave 13 | windsurf.com/changelog/windsurf-next |
| 10 | Devin 2.0 | cognition.ai/blog/devin-annual-performance-review-2025, cognition.ai/blog/dont-build-multi-agents |
| 11 | ai16z / elizaOS | github.com/elizaOS/eliza |

### Quality Gate

After both agents complete, verify:
- Overview file exists at `cases/{NN}-{case}-overview.md`
- Technical file exists at `cases/{NN}-{case}-technical.md`
- If open source repos found → flag for deep analysis

### Post-Processing

After all 22 agents complete:
1. Create `cases/00-open-source-artifacts.md` — list of ALL open source repos found
2. Create `cases/00-synthesis.md` — cross-case comparison of architectures
3. Flag repos that need deep clone + analysis

---

## Parallel Execution

Launch all 22 agents (11 pairs) simultaneously:

```
[Case 01 Overview]  [Case 01 Technical]
[Case 02 Overview]  [Case 02 Technical]
[Case 03 Overview]  [Case 03 Technical]
...
[Case 11 Overview]  [Case 11 Technical]
```

---

## Output Structure

```
hatchery/research/major-swarm-research/
├── prompts/
│   ├── 00_coordinator.md     # This file
│   └── 01_research.md        # Research agent prompt template
├── cases/
│   ├── 00-open-source-artifacts.md
│   ├── 00-synthesis.md
│   ├── 01-cursor-fastrender-overview.md
│   ├── 01-cursor-fastrender-technical.md
│   ├── 02-anthropic-compiler-overview.md
│   ├── 02-anthropic-compiler-technical.md
│   └── ...
└── (previous research files)
```
