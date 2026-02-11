# Hatchery V3: Overmind + SwarmPool + Hybrid Overlord — PRD

**Project ID:** hatchery-v3-overmind
**Created:** 2026-02-11
**Status:** Planning
**Architecture:** ARCHITECTURE_V3.md

## Context

Nydus монолитно содержит транспорт, эвристики спавна, ревью и стратегические решения. Это создаёт проблемы:
- Overlord стоит ~$9 за ревью (LLM читает сырые диффы)
- Нет стратегического координатора (2й decline → бесконечный retry)
- Zerg Rush и elastic pool намертво вшиты в Nydus event loop
- Нет разделения "механика" vs "решение" vs "стратегия"

V3 разделяет на 4 сущности: Nydus (транспорт), SwarmPool (эвристики спавна), Overlord (цензор), Overmind (стратег).

## Phase 0: Scaffolding

### 0.1 Types
- [ ] prd-0-1: Добавить `OvermindId(String)` в `core/types.rs`, добавить `Overmind(OvermindId)` в `AgentId` enum. Добавить `SwarmPoolAction` enum в новый файл `src/swarm_pool/mod.rs`. Verify: `cargo check` passes.

### 0.2 Module Structure
- [ ] prd-0-2: Создать модули `src/overmind/mod.rs` (empty re-exports), `src/swarm_pool/mod.rs` (SwarmPool struct + config). Добавить `pub mod overmind;` и `pub mod swarm_pool;` в `lib.rs`. Verify: `cargo check` passes.

## Phase 1: Parsers + Code Checks — depends on prd-0-1

### 1.1 Diff Parser
- [ ] prd-1-1: Создать `src/overlord/parsers.rs` с `DiffSummary`, `ChangedFile` structs. Функция `parse_diff_summary(worktree: &Path, base: &str) -> Result<DiffSummary>` парсит `git diff --numstat` output. Unit tests с sample git output. Verify: `cargo test -p hatchery -- parsers::test_diff` passes.

### 1.2 Test Result Parser
- [ ] prd-1-2: В `parsers.rs` добавить `TestResults` struct и `parse_test_results(output: &str) -> TestResults`. Парсит "test result: 10 passed; 2 failed; 1 ignored" формат cargo test. Unit tests. Verify: `cargo test -p hatchery -- parsers::test_results` passes.

### 1.3 Code Quality Scanner
- [ ] prd-1-3: В `parsers.rs` добавить `QualityScan`, `QualityHit` structs. Функция `scan_code_quality(worktree: &Path, base: &str) -> Result<QualityScan>` грепает diff на `TODO`, `STUB`, `MOCK`, `unimplemented!()`, `todo!()`, `placeholder`, пустые функции. Unit tests. Verify: `cargo test -p hatchery -- parsers::test_quality` passes.

### 1.4 Code Check Pipeline
- [ ] prd-1-4: Создать `src/overlord/code_checks.rs` с `CodeCheckVerdict` enum (AllClear, HardReject, NeedsReview). Функция `run_code_checks(worktree, base, verify_cmd, description) -> Result<(CodeCheckVerdict, DiffSummary, Option<TestResults>, QualityScan)>`. Оркестрирует parsers в pipeline: no diff → reject, compile fail → reject, verify fail → reject, 100% stubs → reject, all clean → approve, ambiguous → needs_review. Unit tests. Verify: `cargo test -p hatchery -- code_checks` passes.

### 1.5 Session Summary Builder
- [ ] prd-1-5: В `parsers.rs` добавить `SessionSummary` struct и `build_session_summary()`. Собирает cost, duration, turns, quality_passed, result preview из QueenEvent fields. Verify: `cargo test -p hatchery -- parsers::test_session` passes.

## Phase 2: SwarmPool Extraction — depends on prd-0-2

### 2.1 SwarmPool Core
- [ ] prd-2-1: Имплементировать `SwarmPool` struct в `src/swarm_pool/mod.rs` с `SwarmPoolConfig` (min_queens, max_queens, zerg_threshold, max_retries). Методы: `on_decline(task_id, reason, dag) -> SwarmPoolAction`, `on_dag_change(dag, active, idle) -> Vec<SwarmPoolAction>`, `maintenance(active, idle) -> Vec<SwarmPoolAction>`. Чистый Rust, без LLM. Unit tests. Verify: `cargo test -p hatchery -- swarm_pool` passes.

### 2.2 Извлечь Zerg Rush из Nydus
- [ ] prd-2-2: Перенести логику `should_zerg_rush()` и `plan_zerg_rush()` из `nydus/mod.rs` в SwarmPool. Nydus вызывает `swarm_pool.on_dag_change()` и исполняет возвращённые `SwarmPoolAction`. Удалить zerg rush code из Nydus. Verify: `cargo test --test nydus_integration` passes (обновить тесты).

### 2.3 Извлечь Elastic Pool из Nydus
- [ ] prd-2-3: Перенести auto-scaling логику из `try_schedule_with_preference()` в SwarmPool. Nydus вызывает `swarm_pool.maintenance()` в periodic tick. Verify: `cargo test --test nydus_integration` passes.

### 2.4 Retry Policy в SwarmPool
- [ ] prd-2-4: Перенести decline count tracking и retry/escalate решения из `handle_overlord_reject()` в `swarm_pool.on_decline()`. 1st decline → RetryTask, 2nd decline → EscalateToOvermind. Verify: `cargo test -p hatchery -- swarm_pool::test_decline` passes.

## Phase 3: Hybrid Overlord — depends on Phase 1

### 3.1 Verdict Orchestrator
- [ ] prd-3-1: Создать `src/overlord/verdict.rs` с `OverlordVerdict` (Approve/Reject), `ReviewReport` struct, и `run_hybrid_review()` async function. Оркестрирует: parsers → code_checks → если NeedsReview и есть LLM handle → send structured report to LLM → return verdict. Если нет LLM → code_checks verdict is final. Verify: `cargo check` passes.

### 3.2 Подключить Hybrid Pipeline в Nydus
- [ ] prd-3-2: В `nydus/mod.rs` заменить вызов `overlord_handle.review()` на `overlord::verdict::run_hybrid_review()`. Overlord LLM теперь получает structured report вместо raw worktree path. Обновить `handle_overlord_queen_event()` для парсинга `OverlordVerdict`. Verify: `cargo test --test nydus_integration` passes.

### 3.3 Overlord Prompt Update
- [ ] prd-3-3: Обновить Overlord system prompt в `core/prompts.rs`. Убрать инструкции по git diff / cargo check (parsers делают это за него). Оставить: оценка structured report, binary MERGE/DECLINE decision, причина. Verify: `cargo check` passes.

## Phase 4: Overmind Core — depends on prd-0-2

### 4.1 Overmind Events
- [ ] prd-4-1: Создать `src/overmind/events.rs` с `OvermindEvent` enum (TaskEscalation, DeadlockDetected, MergeConflict, BottleneckDetected, QueenRecoveryFailed) и `OvermindCommand` enum (RetryTask, ZergRush, RedecomposeTask, SpawnQueens, FailTask, Shutdown). Verify: `cargo check` passes.

### 4.2 Overmind Handle
- [ ] prd-4-2: Создать `src/overmind/handle.rs` с `OvermindHandle` (wraps QueenHandle). Метод `escalate(event: OvermindEvent) -> Result<()>` форматирует event в structured prompt и отправляет StreamQueen. Verify: `cargo check` passes.

### 4.3 Overmind Spawn
- [ ] prd-4-3: Создать `src/overmind/spawn_overmind.rs` по образцу `spawn_overlord.rs`. Спавнит StreamQueen с Overmind system prompt. Verify: `cargo check` passes.

### 4.4 Overmind Prompt
- [ ] prd-4-4: Создать `src/overmind/prompts.rs` с system prompt. Роль: стратегический координатор. Input: structured escalation reports. Output: JSON с OvermindCommand. Guidelines: 2nd decline → retry с новыми инструкциями, 3rd → redecompose, deadlock → анализ, bottleneck → zerg rush. Verify: `cargo check` passes.

## Phase 5: Wire Overmind → Nydus — depends on Phase 2 AND Phase 4

### 5.1 Overmind Registration в Nydus
- [ ] prd-5-1: Добавить `overmind: Option<OvermindHandle>` и `overmind_event_rx` в Nydus struct. Метод `register_overmind()`. В main `tokio::select!` loop добавить branch для overmind events. Verify: `cargo check` passes.

### 5.2 Command Executor
- [ ] prd-5-2: Создать `handle_overmind_response()` в Nydus. Парсит `OvermindCommand` из result_text. Механически исполняет через SwarmPool и Nydus methods (spawn_queen, task_dag.add_task, etc.). Verify: `cargo check` passes.

### 5.3 Escalation Routing
- [ ] prd-5-3: SwarmPool `EscalateToOvermind` action → Nydus вызывает `overmind_handle.escalate()`. Deadlock detection в periodic_maintenance → `overmind_handle.escalate(DeadlockDetected)`. Merge conflict → `overmind_handle.escalate(MergeConflict)`. Verify: `cargo test --test nydus_integration` passes (добавить тесты на escalation).

## Phase 6: CLI + Tests + Cleanup — depends on Phase 5

### 6.1 CLI Flags
- [ ] prd-6-1: Добавить `--no-overmind` flag в CLI. Когда disabled, SwarmPool escalations → auto-retry (fallback к текущему поведению). Auto-register Overmind when git isolation enabled (как Overlord). Verify: `cargo check` passes.

### 6.2 Integration Tests
- [ ] prd-6-2: Добавить integration tests: Overmind receives escalation → returns ZergRush → Nydus executes. SwarmPool auto zerg rush on bottleneck. Hybrid Overlord auto-approve on clean code. Hybrid Overlord auto-reject on stubs. Verify: `cargo test --test nydus_integration` passes (all tests).

### 6.3 Cleanup
- [ ] prd-6-3: Удалить deprecated zerg rush config из NydusConfig. Убрать мёртвый код. Обновить CLAUDE.md и README. Verify: `cargo test --workspace` passes.

## Quality Gates

| Gate | Check | Command |
|------|-------|---------|
| Phase 0 → 1 | New types compile | `cargo check` |
| Phase 1 → 2 | Parser unit tests pass | `cargo test -- parsers` |
| Phase 2 → 3 | SwarmPool tests + nydus_integration pass | `cargo test --test nydus_integration` |
| Phase 3 → 4 | Hybrid Overlord compiles, tests pass | `cargo test --test nydus_integration` |
| Phase 4 → 5 | Overmind module compiles | `cargo check` |
| Phase 5 → 6 | Full integration with Overmind | `cargo test --test nydus_integration` |
| Phase 6 → Done | All tests pass, no dead code | `cargo test --workspace` |

## Cost Projection (per swarm run)

| Component | Before (V2) | After (V3) |
|-----------|-------------|------------|
| Queens | $12-15 | $12-15 (same) |
| Overlord | $9-10 (LLM reads raw diffs) | $0-2 (mostly auto, LLM only for ambiguous) |
| Overmind | N/A | $1-5 (1-5 activations) |
| **Total** | **$21-25** | **$13-22** |
| **Net savings** | | **$3-12 per run** |
