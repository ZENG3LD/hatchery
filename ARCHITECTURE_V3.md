# Hatchery V3: Overmind Architecture

## Overview

V3 разделяет монолитный Nydus на 4 специализированные сущности с чёткими зонами ответственности.

## Сущности

```
Hatchery CLI
  └── Nydus (ТРАНСПОРТ + МЕХАНИКА)
  │     ├── Route messages между сущностями
  │     ├── Механические PRD updates (по event bus)
  │     ├── Worktree management (create/cleanup/sync)
  │     ├── Rate limit detection → graceful shutdown
  │     └── Базовый scheduling: idle Queen + Ready task → assign
  │
  └── SwarmPool (ЭВРИСТИКИ СПАВНА)
  │     ├── Zerg Rush на блокирующие PRD (механический триггер)
  │     ├── Elastic pool: min/max Queens
  │     ├── Retry policy (1st decline → auto-retry, 2nd → escalate)
  │     ├── Вызывается: автоматически по событиям ИЛИ по команде Overmind
  │     └── API: spawn_queens(n), zerg_rush(task_id, n), kill_queen(id)
  │
  └── Overlord (ЦЕНЗОР — lightweight)
  │     ├── Phase 1: Rust parsers (diff, test results, code quality) — $0
  │     ├── Phase 2: Code checks (TODO/STUB/MOCK/empty work) — $0
  │     ├── Phase 3: если неоднозначно → LLM на structured data — ~$0.50
  │     ├── Решение: MERGE или DECLINE (бинарное, без командования)
  │     └── Вердикт → Nydus → (если decline) → SwarmPool/Overmind
  │
  └── Overmind (СТРАТЕГ — LLM координатор)
        ├── Просыпается ТОЛЬКО на эскалациях:
        │   - 2й decline по одной задаче
        │   - Deadlock (все idle, задачи остались)
        │   - Merge conflict после approval
        │   - Queen recovery failed
        ├── Решает: retry с новыми инструкциями, Zerg Rush, re-decompose PRD
        ├── SpawnPool access: командует спавном через Nydus
        └── Стоимость: ~$0.50-2.00 за активацию, 1-5 раз за сессию
```

## Event Flow

### Happy Path (без проблем)
```
Queen completes task
  → Nydus получает TaskCompleted
  → Nydus вызывает Overlord (parsers → code checks → maybe LLM)
  → Overlord: MERGE
  → Nydus: merge worktree, update PRD [x], recreate worktree
  → Nydus: try_schedule() → следующая задача
```

### Decline + Auto-Retry (SwarmPool)
```
Queen completes → Overlord: DECLINE (стабы/TODO/тесты не прошли)
  → Nydus передаёт decline в SwarmPool
  → SwarmPool: 1st decline → auto-retry (requeue task to any idle Queen)
  → Queen переделывает → Overlord: MERGE → готово
```

### Repeated Decline → Overmind (эскалация)
```
Queen completes → Overlord: DECLINE (2й раз по одной задаче)
  → SwarmPool: 2nd decline → escalate to Overmind
  → Overmind анализирует: rejection history, diff, task description
  → Overmind решает: ZERG_RUSH(task, 3) или REDECOMPOSE(task, subtasks)
  → SwarmPool исполняет: spawn Queens / update DAG
```

### Blocking PRD → Auto Zerg Rush (SwarmPool)
```
Nydus: task_dag shows prd-1 blocks 5 other tasks
  → SwarmPool: bottleneck detected → auto zerg_rush(prd-1, 3)
  → 3 Queens race → winner → Overlord → merge
```

## Nydus: что остаётся

### Оставляем:
- Event bus routing (recv → handle → route)
- TaskDAG state management (complete/fail/requeue)
- PRD file updates (mechanical mark_task_done)
- Worktree management (create/cleanup/sync/recreate)
- IPC listener (queen-status, swarm-status, mailbox)
- Rate limit cascade → shutdown (safety, не strategy)
- Basic scheduling: match idle Queens to Ready tasks
- Post-merge verify scan (mechanical)

### Выносим в SwarmPool:
- Zerg Rush planning (should_zerg_rush, plan_zerg_rush)
- Elastic pool scaling (spawn_queen based on load)
- Retry policy (decline count tracking, retry vs escalate)
- Min/max Queens maintenance

### Выносим в Overmind:
- Strategic decisions on 2nd decline
- Deadlock analysis and resolution
- Task re-decomposition
- Cost-based decisions (budget exceeded → shutdown)

## SwarmPool: детали

```rust
pub struct SwarmPool {
    config: SwarmPoolConfig,
    decline_counts: HashMap<String, usize>,  // task_id → decline count
}

pub struct SwarmPoolConfig {
    pub min_queens: usize,          // default: 2
    pub max_queens: usize,          // default: 8
    pub zerg_rush_threshold: usize, // task blocks N+ tasks → auto zerg
    pub zerg_rush_queens: usize,    // how many Queens per zerg rush
    pub max_retries_before_escalate: usize, // default: 1
}

pub enum SwarmPoolAction {
    SpawnQueens(usize),
    ZergRush { task_id: String, num_queens: usize },
    KillQueen(QueenId),
    RetryTask { task_id: String },
    EscalateToOvermind { task_id: String, decline_count: usize, reasons: Vec<String> },
    Noop,
}

impl SwarmPool {
    /// Called on Overlord DECLINE
    pub fn on_decline(&mut self, task_id: &str, reason: &str, dag: &TaskDag) -> SwarmPoolAction;

    /// Called when DAG changes (new ready tasks, completions)
    pub fn on_dag_change(&mut self, dag: &TaskDag, active_queens: usize) -> Vec<SwarmPoolAction>;

    /// Called periodically for pool maintenance
    pub fn maintenance(&mut self, active_queens: usize, idle_queens: usize) -> Vec<SwarmPoolAction>;
}
```

SwarmPool — чистый Rust, без LLM. Детерминистические эвристики.
Nydus вызывает SwarmPool и исполняет его SwarmPoolAction механически.

## Overlord: Hybrid Pipeline

```
┌─────────────────────────────────────────────┐
│ Phase 1: Rust Parsers (всегда, $0)          │
│   parse_diff_summary()                       │
│   parse_test_results()                       │
│   scan_code_quality()                        │
│   build_session_summary()                    │
├─────────────────────────────────────────────┤
│ Phase 2: Deterministic Checks ($0)           │
│   No diff? → AUTO-REJECT "empty work"        │
│   Compilation fails? → AUTO-REJECT           │
│   Verify cmd fails? → AUTO-REJECT            │
│   100% stubs/TODO? → AUTO-REJECT             │
│   All clean? → AUTO-APPROVE                  │
├─────────────────────────────────────────────┤
│ Phase 3: LLM Review (только если ambiguous)  │
│   Structured report → Sonnet → MERGE/DECLINE │
│   Cost: ~$0.50 (вместо $9)                  │
└─────────────────────────────────────────────┘
```

### Что парсим (переиспользуем существующие парсеры):

- **Git diff**: `git diff --stat`, `git diff --numstat` → DiffSummary
- **Cargo test output**: parse "test result: N passed; M failed" → TestResults
- **Code quality**: grep diff for `TODO`, `STUB`, `MOCK`, `unimplemented!()`, `todo!()`, `placeholder` → QualityScan
- **Session activity**: из QueenEvent fields → SessionSummary

## Overmind: LLM Coordinator

### Когда активируется:
1. 2й decline на одну задачу (SwarmPool escalates)
2. Deadlock: все Queens idle, задачи остались
3. Merge conflict после approval
4. Queen recovery failed (3 attempts)

### Что может командовать:
```rust
pub enum OvermindCommand {
    RetryTask { task_id: String, modified_description: Option<String> },
    ZergRush { task_id: String, num_queens: usize },
    RedecomposeTask { task_id: String, new_subtasks: Vec<NewTaskSpec> },
    SpawnQueens { count: usize },
    FailTask { task_id: String, reason: String },
    Shutdown { reason: String },
}
```

### Как общается с Nydus:
- Overmind = StreamQueen (persistent LLM процесс)
- Получает structured escalation reports от Nydus
- Отвечает JSON с OvermindCommand
- Nydus парсит ответ и исполняет механически через SwarmPool

## Migration Path

Каждая фаза backwards-compatible. `--no-overmind` сохраняет текущее поведение.

1. **Phase 0**: Types + module scaffolding (OvermindId, SwarmPool struct)
2. **Phase 1**: Parsers + code checks (чистый Rust, unit tests)
3. **Phase 2**: SwarmPool extraction (вынести zerg/elastic/retry из Nydus)
4. **Phase 3**: Hybrid Overlord (swap review pipeline)
5. **Phase 4**: Overmind core (events, handle, prompt, spawn)
6. **Phase 5**: Wire Overmind → Nydus → SwarmPool
7. **Phase 6**: CLI flags, tests, cleanup
