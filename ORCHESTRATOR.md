# Hatchery V2: Orchestrator Memory + PRD

**Role**: Opus координатор. Не пишет код. Запускает агентов, валидирует, переходит между фазами.
**Created**: 2026-02-08

---

## Codebase Map

```
hatchery/
├── src/                          # V1 код (reference, не трогать до Phase X.5 backward compat)
│   ├── lib.rs                    # pub mod queen, swarm_host, brood_lord, types, prd, progress, safety
│   ├── main.rs                   # CLI: spawn, status
│   ├── types.rs                  # Task, Mode, HatcheryConfig, IterationResult, SwarmResult
│   ├── prd.rs                    # PRD parser (checkboxes)
│   ├── progress.rs               # Progress tracking
│   ├── queen/mod.rs              # V1 Queen — PRD iterator, PipeProcess, single/multi-worker
│   ├── swarm_host/
│   │   ├── mod.rs                # V1 SwarmHost — deterministic coordinator, @hatchery: parsing
│   │   ├── shared_memory.rs      # SharedMemory + SharedMemoryState + SwarmTask + Messages
│   │   └── commands.rs           # @hatchery: command enum
│   ├── brood_lord/
│   │   ├── mod.rs                # V1 BroodLord — Opus Manager → L2 coordinators → workers
│   │   ├── types.rs              # SubPrd, Decomposition, L2Status, OpusCommand
│   │   └── global_memory.rs      # GlobalMemory for cross-L2
│   └── safety/
│       ├── mod.rs
│       ├── policy.rs
│       └── worktree.rs           # Git worktree isolation (reuse in v2)
│
├── src/v2/                       # V2 код (создаём с нуля)
│   ├── mod.rs                    # pub mod types, queen, mailbox, task_dag, ...
│   ├── types.rs                  # Phase 1: SwarmMessage, AgentId, TaskId, Visibility...
│   ├── queen/
│   │   ├── mod.rs                # Phase 1: trait Queen
│   │   ├── native.rs             # Phase 1: NativeQueen (wraps PipeProcess)
│   │   └── custom.rs             # Phase 1: CustomQueen (API backends)
│   ├── mailbox/
│   │   ├── mod.rs                # Phase 2: SwarmMailbox
│   │   ├── event_log.rs          # Phase 2: SqliteEventLog
│   │   └── router.rs             # Phase 2: MessageRouter
│   ├── task_dag.rs               # Phase 3: TaskDag
│   ├── compaction.rs             # Phase 3: CompactionStrategy
│   ├── validator.rs              # Phase 3: Validator
│   ├── swarm_host.rs             # Phase 3: SwarmHost v2
│   ├── git/
│   │   └── worktree.rs           # Phase 4: WorktreeManager v2
│   ├── operator/
│   │   └── mod.rs                # Phase 5: OperatorChannel trait + impls
│   ├── brood_lord.rs             # Phase 5: BroodLord v2
│   └── prompts/                  # Phase 7: prompt templates
│
├── ARCHITECTURE_V2.md            # Полная архитектура с Rust кодом
├── PRD_V2.md                     # 120 чекбоксов, 7 фаз
├── ORCHESTRATOR.md               # ← ЭТОТ ФАЙЛ
└── research/
    └── major-swarm-research/     # 22 case files, 5 dissections, revolver analysis
```

---

## Research Index (что давать каким агентам)

| Ресерч файл | Для какой фазы | Что содержит |
|-------------|---------------|-------------|
| `ARCHITECTURE_V2.md` | ВСЕ фазы | Rust код всех struct/trait/impl — primary source of truth |
| `dissections/goose-rust-internals.md` | Phase 1 (builder), Phase 3 (compaction) | PromptManager builder, 80% threshold, dual-visibility, progressive removal |
| `dissections/claude-code-teams-internals.md` | Phase 1 (protocol), Phase 2 (mailbox), Phase 3 (TaskDag) | 13 TeammateTool ops, file-based inbox, TaskCreate/Update with blockedBy |
| `dissections/openai-swarm-internals.md` | Phase 1 (Queen trait) | Agent abstraction, handoff pattern, stateless loop |
| `dissections/claude-flow-internals.md` | Phase 4 (git) | Git worktrees per agent, ledger pattern |
| `dissections/elizaos-swarm-internals.md` | General reference | Worlds/Rooms, UUID swizzling (voting debunked) |
| `revolver-analysis/synthesis-and-stack.md` | Phase 2 (transport), Phase 3 (compaction) | CLI→swarm mapping tables, transport architecture, tech stack |
| `revolver-analysis/compression-evaluation.md` | Phase 3 (compaction) | Compaction patterns rated EXCELLENT/GOOD/BAD |
| `revolver-analysis/prompting-evaluation.md` | Phase 7 (prompts) | Prompt template patterns for swarm agents |
| `revolver-analysis/transport-evaluation.md` | Phase 2 (mailbox) | Transport patterns: channels, SSE, pipes |
| `src/` (v1 code) | Phase X.5 (backward compat) | Working v1 — must not break |

---

## Orchestration Plan

### Phase 1: Core Types + Queen Trait

```
Pattern: TeamCreate (4 teammates, shared task DAG)
Why: файлы зависят друг от друга (types → trait → native → custom)
     shared task list с blockedBy координирует порядок
     каждый teammate пишет свой файл — нет file conflicts
```

**Teammates:**
| Name | Role | Files | blockedBy |
|------|------|-------|-----------|
| types-agent | Define all v2 types | `v2/mod.rs`, `v2/types.rs` | — |
| queen-trait-agent | Define Queen trait | `v2/queen/mod.rs` | types-agent |
| native-queen-agent | Implement NativeQueen | `v2/queen/native.rs` | queen-trait-agent |
| custom-queen-agent | Implement CustomQueen scaffold | `v2/queen/custom.rs` | queen-trait-agent |

**Research injection per agent:**
- types-agent: ARCHITECTURE_V2.md (lines 79-134), v1 types.rs
- queen-trait-agent: ARCHITECTURE_V2.md (lines 42-78), openai-swarm-internals.md (Agent abstraction)
- native-queen-agent: ARCHITECTURE_V2.md (lines 136-384), v1 queen/mod.rs (PipeProcess), claude-code-teams-internals.md (protocol)
- custom-queen-agent: ARCHITECTURE_V2.md (lines 387-699)

**Validation gate:**
```bash
cd hatchery && cargo check 2>&1
# PASS: 0 errors → Phase 1 complete
# FAIL: fix errors, re-check
```

**Backward compat (1.5):**
- Отдельный rust-implementer после Phase 1 core
- Читает v1 queen/mod.rs + v1 main.rs
- Wires v2 NativeQueen в v1 CLI path
- Gate: `cargo test` проходит (v1 tests)

---

### Phase 2: SwarmMailbox + Message Router

```
Pattern: Carousel (Research → Implement → Test)
Why: нужен research по rusqlite API, потом последовательная имплементация
     1 агент за раз — модули тесно связаны (mailbox uses event_log uses router)
```

**Phases:**
| Step | Agent | Task | Gate |
|------|-------|------|------|
| F1 Research | research-agent | rusqlite API (WAL mode, async patterns), tokio::sync::mpsc | research file exists |
| F2 Implement | rust-implementer | `v2/mailbox/mod.rs`, `event_log.rs`, `router.rs` | `cargo check` |
| F3 Test | rust-implementer | Unit tests: send/recv/broadcast/TTL/SQLite with tempfile | `cargo test` |

**Research injection:**
- ARCHITECTURE_V2.md секция SwarmMailbox + SqliteEventLog + MessageRouter
- claude-code-teams-internals.md (inbox pattern)
- synthesis-and-stack.md секция 1.3 Transport
- transport-evaluation.md

---

### Phase 3: TaskDag + Compaction + Validator → SwarmHost

```
Pattern: TeamCreate (3 parallel) → Ralph (SwarmHost integration)
Why: TaskDag, Compaction, Validator — независимы (parallel team)
     SwarmHost refactor зависит от всех трёх (sequential ralph)
```

**Step A: TeamCreate (3 teammates)**
| Name | Files | blockedBy |
|------|-------|-----------|
| taskdag-agent | `v2/task_dag.rs` | — |
| compaction-agent | `v2/compaction.rs` | — |
| validator-agent | `v2/validator.rs` | — |

Gate: `cargo check` для каждого модуля

**Step B: Ralph (SwarmHost refactor)**
- PRD: `tasks/phase3-swarmhost-refactor.md` (10+ checkboxes)
- Каждый шаг: implement piece + `cargo check`
- Agent: rust-implementer
- Gate: `cargo test` включая integration test с mock Queens

**Research injection:**
- taskdag-agent: ARCHITECTURE_V2.md TaskDag section, claude-code-teams-internals.md (Task DAG mechanics), v1 shared_memory.rs
- compaction-agent: ARCHITECTURE_V2.md CompactionStrategy, goose-rust-internals.md (compaction + dual-visibility), compression-evaluation.md
- validator-agent: ARCHITECTURE_V2.md Validator section
- ralph SwarmHost: ARCHITECTURE_V2.md SwarmHost section, v1 swarm_host/mod.rs

---

### Phase 4: Git Coordination

```
Pattern: Ralph
Why: линейная задача, каждый шаг проверяется реальными git операциями
     интеграционные тесты с настоящим git repo
```

- PRD: `tasks/phase4-git-coordination.md` (~8 checkboxes)
- Agent: rust-implementer
- Gate: integration tests с git в /tmp директории

**Research injection:**
- ARCHITECTURE_V2.md Git section
- v1 safety/worktree.rs (existing implementation)
- claude-flow-internals.md (worktree per agent)

---

### Phase 5: BroodLord + OperatorChannel

```
Pattern: Carousel (Research → Implement → Test)
Why: OperatorChannel нуждается в research по SSE/axum
     BroodLord зависит от OperatorChannel
```

| Step | Agent | Task | Gate |
|------|-------|------|------|
| F1 | research-agent | axum SSE, tokio stdin async, event serialization | research file |
| F2 | rust-implementer | `v2/operator/mod.rs`, `v2/brood_lord.rs` | `cargo check` |
| F3 | rust-implementer | Integration tests | `cargo test` |

**Research injection:**
- ARCHITECTURE_V2.md BroodLord + OperatorChannel
- v1 brood_lord/mod.rs + types.rs + global_memory.rs

---

### Phase 6: CustomQueen Backends

```
Pattern: 3x Task parallel (independent agents)
Why: 3 бэкенда полностью независимы, не нужна координация между ними
```

| Agent | Backend | Gate |
|-------|---------|------|
| rust-implementer #1 | Codex sandbox | `cargo check` |
| rust-implementer #2 | OpenAI-compatible HTTP | `cargo check` + mock test |
| rust-implementer #3 | Anthropic HTTP | `cargo check` + mock test |

Потом 1 интеграционный rust-implementer: mixed backend SwarmHost test

**Research injection:**
- ARCHITECTURE_V2.md CustomQueen section
- Codex: нужен свежий research-agent по Codex API

---

### Phase 7: CLI + Integration

```
Pattern: Ralph
Why: линейная задача — CLI flags, TOML config, e2e tests
     каждый шаг проверяем запуском бинарника
```

- PRD: `tasks/phase7-cli-integration.md` (~15 checkboxes)
- Agent: rust-implementer
- Gate: `hatchery spawn --help` показывает новые флаги + e2e tests pass

**Research injection:**
- ARCHITECTURE_V2.md Configuration section
- v1 main.rs (existing CLI)
- prompting-evaluation.md (prompt templates)

---

## Validation Protocol

### Per-Phase Gate

Каждая фаза ОБЯЗАНА пройти gate перед переходом к следующей:

```
Phase complete when:
  1. cargo check --all-targets    → 0 errors
  2. cargo test                   → 0 failures
  3. cargo clippy                 → 0 errors (warnings ok)
  4. Все чекбоксы PRD_V2.md для этой фазы отмечены [x]
```

### Validation Commands

```bash
# Quick check (между шагами внутри фазы)
cd hatchery && cargo check 2>&1 | tail -5

# Full gate (между фазами)
cd hatchery && cargo check --all-targets && cargo test && cargo clippy 2>&1 | tail -20

# Specific module test
cd hatchery && cargo test v2::mailbox 2>&1

# Count remaining checkboxes
grep -c '^\- \[ \]' hatchery/PRD_V2.md
```

### Quality Checklist (before advancing phase)

- [ ] Все файлы для этой фазы созданы
- [ ] `cargo check` проходит
- [ ] Unit tests написаны и проходят
- [ ] Новый код НЕ ломает v1 (`cargo test` полный)
- [ ] PRD_V2.md чекбоксы обновлены
- [ ] Коммит в ветке `hatchery-v2-research`

---

## State Tracking

### Current State

```
Phase: 2 COMPLETE, Phase 3 NEXT
Last completed: Phase 2 — mailbox/mod.rs, event_log.rs, router.rs (55 tests pass)
Blocking issues: none
Next action: Phase 3 (TaskDag + Compaction + Validator → SwarmHost)
```

### Phase Progress

| Phase | Status | Pattern | Agents Used | Gate Result |
|-------|--------|---------|-------------|-------------|
| 1. Types + Queen | DONE (28/43 checkboxes) | Sequential Tasks | 3 rust-implementer + 1 implementer | cargo check PASS |
| 1.5 Backward compat | DONE (--backend flag) | Task | 1 rust-implementer | cargo check PASS |
| 2. Mailbox | DONE (26/26 checkboxes) | Carousel | 2 rust-implementer + 1 research | 55 tests PASS |
| 3a. TaskDag | NOT STARTED | TeamCreate | — | — |
| 3b. Compaction | NOT STARTED | TeamCreate | — | — |
| 3c. Validator | NOT STARTED | TeamCreate | — | — |
| 3d. SwarmHost | NOT STARTED | Ralph | — | — |
| 4. Git | NOT STARTED | Ralph | — | — |
| 5. BroodLord | NOT STARTED | Carousel | — | — |
| 6. Backends | NOT STARTED | 3x Task | — | — |
| 7. CLI | NOT STARTED | Ralph | — | — |

### Decisions Log

| Date | Decision | Reason |
|------|----------|--------|
| 2026-02-08 | v2 code goes in `src/v2/` not replacing v1 | backward compatibility, gradual migration |
| 2026-02-08 | TeamCreate for Phase 1 | files have dependency chain, shared DAG coordinates |
| 2026-02-08 | Carousel for Phase 2 | needs rusqlite research, sequential pipeline |
| 2026-02-08 | TeamCreate(3) + Ralph for Phase 3 | independent modules parallel, then sequential integration |
| 2026-02-08 | Ralph for Phase 4, 7 | linear iterative tasks with verification |
| 2026-02-08 | Carousel for Phase 5 | needs SSE crate research |
| 2026-02-08 | 3x Task parallel for Phase 6 | independent backends, no coordination needed |

### Known Risks

| Risk | Mitigation |
|------|-----------|
| v2 types incompatible with v1 | Phase X.5 backward compat step after each phase |
| rusqlite + async tokio friction | Phase 2 research covers this explicitly |
| PipeProcess shared between v1 and v2 | Don't duplicate — import from v1, extend in v2 |
| File conflicts in TeamCreate | Each teammate has exclusive file ownership |
| Context overflow in long Ralph runs | Ralph has built-in progress compaction (>20KB) |

---

## Session Recovery

Если контекст обрезается или сессия прерывается:

1. Прочитай `ORCHESTRATOR.md` — вся память здесь
2. Проверь `PRD_V2.md` — какие чекбоксы отмечены
3. Запусти `cargo check && cargo test` — текущее состояние кода
4. Посмотри `State Tracking` секцию — какая фаза, что дальше
5. Продолжай с того места где остановился
