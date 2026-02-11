# SwarmPool — Spawn Heuristics

## Роль

SwarmPool — чистый Rust модуль (без LLM) с эвристиками для спавна и управления Queens. Вызывается Nydus механически по событиям или по команде Overmind.

## Эвристики

### Zerg Rush (автоматический)
- Триггер: задача блокирует N+ других задач (configurable threshold)
- Действие: спавнить K Queens на эту задачу, первый завершивший побеждает
- Механический, не требует Overmind

### Elastic Pool
- Min/max Queens поддержание
- Если idle Queens < ready tasks → spawn more
- Если idle Queens > ready tasks × 2 → kill excess

### Retry Policy
- 1st decline → auto-retry (requeue to any idle Queen)
- 2nd decline на ту же задачу → escalate to Overmind
- Tracking: `HashMap<task_id, decline_count>`

## API

```rust
pub struct SwarmPool {
    config: SwarmPoolConfig,
    decline_counts: HashMap<String, usize>,
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
    /// Infestor declined a task
    fn on_decline(&mut self, task_id, reason, dag) -> SwarmPoolAction;

    /// DAG changed (task completed, new ready tasks)
    fn on_dag_change(&mut self, dag, active, idle) -> Vec<SwarmPoolAction>;

    /// Periodic maintenance (pool sizing)
    fn maintenance(&mut self, active, idle) -> Vec<SwarmPoolAction>;
}
```

## Кто вызывает

| Caller | Когда | Метод |
|--------|-------|-------|
| Nydus | Infestor decline | `on_decline()` |
| Nydus | После merge / task complete | `on_dag_change()` |
| Nydus | Periodic tick | `maintenance()` |
| Overmind | Strategic spawn command | Nydus вызывает `SpawnQueens` / `ZergRush` напрямую |

## Файлы

```
src/swarm_pool/
├── mod.rs    # SwarmPool struct, config, actions, impl
└── README.md # This file
```
