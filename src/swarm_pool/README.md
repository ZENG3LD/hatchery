# SwarmPool — Spawn Heuristics

## Integration with V4 Modular Architecture

**SwarmPool is now dual-mode**: it works as an integrated spawn manager (classic mode) AND its functionality is available as composable modules in the new pipeline architecture.

### Classic Mode (V3, unchanged)
SwarmPool is called by Nydus on events (decline, DAG change, maintenance). Pure Rust heuristics, no LLM.

### Pipeline Mode (V4, new)
SwarmPool functionality split across two categories of modules:

1. **Spawn heuristics** → `ElasticPoolScaling` (scaling module)
   - Zerg Rush mode
   - Min/max Queens maintenance
   - Load-based scaling

2. **Retry policy** → `RetryResilience` (resilience module)
   - Auto-retry on 1st decline
   - Escalate on 2nd decline
   - Backoff strategies

Use them in a pipeline:
```rust
use hatchery::pipeline::PipelineBuilder;
use hatchery::scaling::ElasticPoolScaling;
use hatchery::resilience::RetryResilience;

let pipeline = PipelineBuilder::new()
    .scaling(ElasticPoolScaling::new(config.with_zerg_rush(true)))
    .resilience(RetryResilience::new(config.with_escalation(true)))
    .build()?;
```

Or use presets that include SwarmPool-style behavior:
```rust
use hatchery::pipeline::presets::carousel_preset;
let pipeline = carousel_preset().build()?; // Includes ElasticPool + Retry
```

The core heuristics (when to spawn, when to retry, when to escalate) remain identical across both modes.

---

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
    /// Overlord declined a task
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
| Nydus | Overlord decline | `on_decline()` |
| Nydus | После merge / task complete | `on_dag_change()` |
| Nydus | Periodic tick | `maintenance()` |
| Overmind | Strategic spawn command | Nydus вызывает `SpawnQueens` / `ZergRush` напрямую |

## Файлы

```
src/swarm_pool/
├── mod.rs    # SwarmPool struct, config, actions, impl
└── README.md # This file
```

## V4 Модульная Архитектура

В V4 функциональность SwarmPool доступна в двух формах:

### 1. Classic Mode (Integrated)
Standalone component, вызывается Nydus. Используется как раньше:
```rust
let nydus = Nydus::new(config).await?;
nydus.run().await?; // SwarmPool integrated
```

### 2. Pipeline Mode (Composable Modules)
SwarmPool логика разделена на модули:

**Zerg Rush + Elastic Pool** → `scaling/elastic_pool.rs`:
```rust
use hatchery::scaling::ElasticPoolScaling;

let scaling = ElasticPoolScaling::new(config)
    .with_min_queens(2)
    .with_max_queens(8)
    .with_zerg_rush_threshold(3) // Task blocks 3+ others → zerg
    .with_zerg_rush_queens(5);   // Spawn 5 Queens in zerg mode
```

**Retry Policy** → `resilience/retry.rs`:
```rust
use hatchery::resilience::RetryResilience;

let resilience = RetryResilience::new(config)
    .with_max_retries(1)           // 1st decline → retry
    .with_escalate_after(2)        // 2nd decline → escalate
    .with_backoff(BackoffStrategy::Exponential);
```

**Compose them**:
```rust
let pipeline = PipelineBuilder::new()
    .scaling(scaling)
    .resilience(resilience)
    .build()?;
```

### Mapping: Classic → Pipeline

| Classic SwarmPool Feature | Pipeline Module | Config |
|---------------------------|-----------------|--------|
| Zerg Rush (auto) | `ElasticPoolScaling` | `zerg_rush_threshold: 3` |
| Zerg Rush (manual) | `ElasticPoolScaling` | `.spawn_zerg_rush(task_id, 5)` |
| Elastic Pool (min/max) | `ElasticPoolScaling` | `min_queens: 2, max_queens: 8` |
| Auto-retry on 1st decline | `RetryResilience` | `max_retries: 1` |
| Escalate on 2nd decline | `RetryResilience` | `escalate_after: 2` |
| Decline tracking | `RetryResilience` | `HashMap<TaskId, usize>` |

### Когда использовать Pipeline Mode?

- Нужна кастомная scaling стратегия (например, LargeScale для 100+ Queens)
- Хотите альтернативные retry стратегии (Consensus вместо Retry)
- Интеграция с external resilience systems
- Тестирование разных spawn heuristics (A/B testing)

### Когда использовать Classic Mode?

- Простая setup — работает из коробки с Nydus
- Уже работает, не нужны изменения
- Все компоненты в одном integrated mode

See `../../ARCHITECTURE_V4.md` for full details on modular architecture.
