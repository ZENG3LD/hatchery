# Overmind — Strategic Coordinator

## Роль

Overmind — LLM-координатор (StreamQueen), который принимает стратегические решения когда эвристики SwarmPool недостаточно.

## Когда активируется

| Событие | Кто триггерит | Что Overmind решает |
|---------|---------------|---------------------|
| 2й decline на задачу | SwarmPool → EscalateToOvermind | Retry с новыми инструкциями / Zerg Rush / Re-decompose |
| Deadlock | Nydus periodic_maintenance | Анализ: что заблокировано, как разрешить |
| Merge conflict | Nydus handle_infestor_approve | Стратегия: requeue / re-decompose / manual |
| Queen recovery failed | Nydus recovery_manager | Решение: respawn / fail task / shutdown |

## Чего НЕ делает

- Не делает рутинный scheduling (→ Nydus)
- Не спавнит Queens напрямую (→ через SwarmPool)
- Не ревьюит код (→ Infestor)
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
