# Overmind — Strategic Coordinator

## Integration with V4 Modular Architecture

**Overmind is now dual-mode**: it works as a standalone strategic coordinator (classic mode) AND as a composable `Resilience` module in the new pipeline architecture.

### Classic Mode (V3, unchanged)
Overmind is an LLM-powered StreamQueen that handles escalations from SwarmPool. Nydus routes events to Overmind, which returns strategic commands.

### Pipeline Mode (V4, new)
Overmind functionality is available as `OvermindResilience` — a resilience module that makes strategic decisions via LLM. Integrate via:

```rust
use hatchery::pipeline::PipelineBuilder;
use hatchery::resilience::OvermindResilience;

let pipeline = PipelineBuilder::new()
    .resilience(OvermindResilience::new(config))
    .build()?;
```

Or use a preset that includes Overmind-style resilience:
```rust
use hatchery::pipeline::presets::consensus_preset;
let pipeline = consensus_preset().build()?;
```

The core strategic logic (analyze escalation, decide on recovery action) remains identical across both modes.

---

## Роль

Overmind — LLM-координатор (StreamQueen), который принимает стратегические решения когда эвристики SwarmPool недостаточно.

## Когда активируется

| Событие | Кто триггерит | Что Overmind решает |
|---------|---------------|---------------------|
| 2й decline на задачу | SwarmPool → EscalateToOvermind | Retry с новыми инструкциями / Zerg Rush / Re-decompose |
| Deadlock | Nydus periodic_maintenance | Анализ: что заблокировано, как разрешить |
| Merge conflict | Nydus handle_overlord_approve | Стратегия: requeue / re-decompose / manual |
| Queen recovery failed | Nydus recovery_manager | Решение: respawn / fail task / shutdown |

## Чего НЕ делает

- Не делает рутинный scheduling (→ Nydus)
- Не спавнит Queens напрямую (→ через SwarmPool)
- Не ревьюит код (→ Overlord)
- Не принимает решений при 1м decline (→ SwarmPool auto-retry)

## API

```rust
// Overmind получает
enum OvermindEvent {
    TaskEscalation { task_id, retry_count, reasons, review_report },
    DeadlockDetected { idle_queens, remaining_tasks, blocked, ready },
    MergeConflict { task_id, conflicting_files },
    BottleneckDetected { task_id, blocks_count },
    QueenRecoveryFailed { queen_id, task_id, attempts },
}

// Overmind отвечает
enum OvermindCommand {
    RetryTask { task_id, modified_description },
    ZergRush { task_id, num_queens },
    RedecomposeTask { task_id, new_subtasks },
    SpawnQueens { count },
    FailTask { task_id, reason },
    Shutdown { reason },
}
```

## Файлы

```
src/overmind/
├── mod.rs              # Re-exports
├── events.rs           # OvermindEvent + OvermindCommand enums
├── handle.rs           # OvermindHandle (wraps QueenHandle)
├── spawn_overmind.rs   # Spawn as StreamQueen
└── prompts.rs          # System prompt
```

## Стоимость

~$0.50-2.00 за активацию. 1-5 активаций за типичную swarm сессию.
Total: $1-10 за весь ран (vs $0 сейчас, но с лучшими решениями).

## V4 Модульная Архитектура

В V4 Overmind доступен в двух режимах:

### 1. Classic Mode (Integrated)
Standalone component, вызывается Nydus через event bus. Используется как раньше:
```rust
let nydus = Nydus::new(config.with_overmind(true)).await?;
nydus.run().await?;
```

### 2. Pipeline Mode (Composable)
Overmind как resilience module. Интегрируется через `PipelineBuilder`:
```rust
let pipeline = PipelineBuilder::new()
    .resilience(OvermindResilience::new(config))
    .build()?;
```

Логика Overmind идентична в обоих режимах — меняется только способ интеграции.

### Когда использовать Pipeline Mode?

- Нужна кастомная композиция модулей (например, Overmind + RAG memory + P2P topology)
- Хотите hot-swap resilience стратегии в рантайме
- Интеграция с external systems через Protocols (MCP/A2A)
- Тестирование альтернативных resilience подходов (A/B testing)

### Когда использовать Classic Mode?

- Простая setup — один вызов Nydus
- Уже работает, не нужны изменения
- Все компоненты (Nydus + Queen + Overlord + Overmind + SwarmPool) в одном integrated mode

See `../../ARCHITECTURE_V4.md` for full details on modular architecture.
