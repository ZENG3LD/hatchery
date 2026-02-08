# PRD: Hatchery Hardening — Real-World Agent Reliability

## Context

Hatchery V3 has working architecture (EventBus, TaskDag, SharedMemory, Queen actors, SwarmHost event loop, git isolation, Validator). But 5 critical gaps prevent real-world usage. This PRD fixes them.

**Reference research**: `research/claude-code-compression-for-hatchery.md`, `research/claude-code-compression-quick-reference.md`

**Key files**:
- `hatchery/src/queen/stream_queen.rs` — long-lived Queen actor
- `hatchery/src/queen/spawn_queen.rs` — per-task Queen actor
- `hatchery/src/queen/spawn_mode.rs` — ClaudeEvent, StreamInput types
- `hatchery/src/queen/handle.rs` — QueenHandle, QueenCommand, QueenEvent
- `hatchery/src/queen/completion.rs` — CompletionDetector
- `hatchery/src/core/prompts.rs` — prompt templates
- `hatchery/src/swarm_host/mod.rs` — SwarmHost orchestrator
- `hatchery/src/core/validator.rs` — Validator
- `hatchery/src/core/task_dag.rs` — TaskDag
- `hatchery/src/mailbox/event_bus.rs` — EventBus

**Verify**: `cd hatchery && cargo check` and `cd hatchery && cargo test`

---

## US-001: Anti-Compression Re-Injection (CRITICAL)

**Problem**: `--append-system-prompt` инжектится при старте процесса. Это ЕДИНСТВЕННЫЙ guaranteed слой (never compressed, never ignored). Но если Queen работает долго (StreamQueen), контекст компрессится — и хотя system prompt выживает, Queen теряет task-specific context. Нужно: (a) детектить компрессию, (b) re-inject discipline + task state как user message.

**Signal**: Claude Code emits `{"type":"system","subtype":"compact_boundary","preTokens":173212}` в NDJSON stdout при компрессии.

**Acceptance Criteria:**

- [ ] `ClaudeEvent` (spawn_mode.rs) распознаёт `compact_boundary` — добавить `is_compact_boundary()` метод, проверяющий `event_type == "system" && subtype == Some("compact_boundary")`
- [ ] StreamQueen stdout_reader при получении `compact_boundary` эмиттит новый `QueenEvent::ContextCompressed { queen_id, pre_tokens, trigger }` в event_tx
- [ ] StreamQueen после детекции компрессии автоматически отправляет recovery message в stdin: discipline block + текущий task description + "Re-read your task and continue"
- [ ] `QueenEvent` enum (handle.rs) получает вариант `ContextCompressed { queen_id: QueenId, pre_tokens: u64, trigger: String }`
- [ ] `format_task_prompt()` в обоих Queens включает `orchestration_discipline_block()` в каждый task prompt (не только при старте) — это carry-over protection
- [ ] SwarmHost handle_event обрабатывает `ContextCompressed` — логирует, обновляет tick state, опционально audit log
- [ ] SpawnQueen: в per-task mode компрессия менее критична (новый процесс на каждый task), но `--append-system-prompt` должен включать discipline block при каждом spawn (проверить что уже так)
- [ ] Тест: `test_compact_boundary_detection` — парсинг ClaudeEvent с subtype compact_boundary
- [ ] Тест: `test_context_compressed_event` — QueenEvent::ContextCompressed сериализуется/десериализуется
- [ ] cargo check && cargo test — 0 errors

---

## US-002: SwarmHost Skill Awareness

**Problem**: SwarmHost раздаёт плоские задачи Queens. Не может сказать Queen "используй /carousel для этого коннектора". Queen знает скилы из system prompt, но SwarmHost не подсказывает какой паттерн применить.

**Solution**: Добавить `skill_hint: Option<String>` в DagTask. SwarmHost при assign включает hint в TaskContext. Queen видит подсказку и выбирает паттерн.

**Acceptance Criteria:**

- [ ] `DagTask` (task_dag.rs) получает поле `skill_hint: Option<String>` — e.g. "carousel", "ralph", "agents-tasks", None
- [ ] `SwarmHost::add_task()` принимает опциональный `skill_hint` параметр
- [ ] `TaskContext` (handle.rs или types.rs) получает поле `skill_hint: Option<String>`
- [ ] `try_schedule()` при построении TaskContext копирует skill_hint из DagTask
- [ ] `format_task_prompt()` в обоих Queens: если skill_hint present, добавляет секцию "## Recommended Execution Pattern\nUse /{skill} pattern for this task. Read the skill docs and follow its phases."
- [ ] main.rs: PRD задачи могут содержать skill hints в описании (парсинг — будущая работа, пока manual через add_task API)
- [ ] Тест: `test_skill_hint_in_task_context` — hint propagates from DagTask через TaskContext в prompt
- [ ] cargo check && cargo test — 0 errors

---

## US-003: File-Based Shared Knowledge (PRD-as-Memory)

**Problem**: SharedMemory живёт в Rust Arc<RwLock> — агенты (Claude процессы) не имеют к ней доступа. QueenEvent::Knowledge определён но никогда не эмиттится. Агенты не шарят знания.

**Solution**: Файловый knowledge store — JSON файл с file lock. Каждая Queen при получении task читает файл. При завершении task пишет результат в файл. SwarmHost синхронизирует с in-memory SharedMemory.

**Acceptance Criteria:**

- [ ] Новый файл `hatchery/src/core/knowledge_file.rs` — `KnowledgeFile` struct с путём к JSON файлу
- [ ] `KnowledgeFile::read()` — читает JSON, возвращает `Vec<KnowledgeEntry>`. File lock (shared/read) через `fs2::FileExt` или `file-lock` crate
- [ ] `KnowledgeFile::write_entry(entry)` — appends entry с exclusive file lock. Entry: `{ queen_id, task_id, key, value, timestamp }`
- [ ] `KnowledgeFile::entries_for(queen_id)` — фильтр по queen_id (mailbox-like)
- [ ] `KnowledgeFile::entries_since(timestamp)` — новые записи с момента
- [ ] `format_task_prompt()` включает recent knowledge entries (последние 10) в секцию "## Shared Knowledge" task prompt
- [ ] SwarmHost при handle_event(TaskCompleted) пишет result в KnowledgeFile (не только в SharedMemory)
- [ ] SwarmHost при try_schedule() читает KnowledgeFile и включает relevant entries в TaskContext
- [ ] Путь к файлу: `{working_dir}/.hatchery/knowledge.jsonl` (JSONL для append-only)
- [ ] Тест: `test_knowledge_file_read_write` — write + read roundtrip
- [ ] Тест: `test_knowledge_file_concurrent` — два writer'а не corruption
- [ ] cargo check && cargo test — 0 errors

---

## US-004: Validator as Git Diff Analyzer

**Problem**: `Validator::AiReview` — заглушка (always passes). Validator::Command просто запускает shell команду. Нет анализа что именно Queen наделала.

**Solution**: Новый `Validator::GitDiff` — смотрит git diff в worktree квины, анализирует изменения, даёт structured вердикт. Для MVP без AI — pattern-based (forbidden patterns, file size limits, test coverage check).

**Acceptance Criteria:**

- [ ] `Validator` enum (validator.rs) получает вариант `GitDiff { working_dir: PathBuf, rules: Vec<DiffRule> }`
- [ ] `DiffRule` enum: `ForbiddenPattern { pattern: String, message: String }`, `MaxFileSize { bytes: u64 }`, `RequireTests { test_glob: String }`, `NoBinaryFiles`
- [ ] `Validator::GitDiff::validate()`: запускает `git diff --cached --stat` и `git diff --cached` в worktree, применяет rules
- [ ] Если rule нарушен — `passed: false` с описанием какие rules failed
- [ ] SwarmHost при `validated_merge()` использует GitDiff validator если worktree isolation включена
- [ ] ValidationResult включает `failed_rules: Vec<String>` для feedback в Queen message
- [ ] Default rules: no `.env` files, no binary files >1MB, no `TODO` in committed code (configurable)
- [ ] Тест: `test_git_diff_validator_clean` — clean diff passes
- [ ] Тест: `test_git_diff_validator_forbidden_pattern` — `.env` file fails
- [ ] cargo check && cargo test — 0 errors

---

## US-005: Mailbox Polling on Task Completion

**Problem**: SpawnQueen не может получать сообщения во время работы (процесс одноразовый). Сообщения от других Queens складываются в queue и включаются в СЛЕДУЮЩИЙ task prompt. Но agent не знает что есть сообщения пока не получит новый task.

**Solution**: Forum-style communication. Queen после завершения task: (1) проверяет mailbox (queued messages), (2) если есть messages — включает их в next task prompt, (3) если нет messages и нет new task — idle.

**Принцип**: агенты работают по форумному типу — принимают инфу только между задачами.

**Acceptance Criteria:**

- [ ] SpawnQueen main loop: после `process_stdout()` завершения, перед idle — проверяет `queued_messages.len()`
- [ ] Если есть queued messages — эмиттит `QueenEvent::MessagesReceived { queen_id, count }` для логирования
- [ ] StreamQueen: при получении `QueenCommand::Message` во время task execution — ставит флаг `has_pending_messages = true`
- [ ] StreamQueen: после `ClaudeEvent::result` (task complete) — если `has_pending_messages`, отправляет aggregated message в stdin: "## Messages received while you were working\n{messages}\n\nProcess these messages and take action if needed."
- [ ] SwarmHost: при `TaskCompleted` event от Queen, проверяет есть ли pending messages для этой Queen, если есть — отправляет через handle.send_message() ДО назначения следующего task
- [ ] `QueenEvent` enum получает вариант `MessagesReceived { queen_id: QueenId, count: usize }` (информационный)
- [ ] Тест: `test_queued_messages_included_in_next_task` — message queued during task appears in next prompt
- [ ] Тест: `test_stream_queen_pending_messages_after_completion` — StreamQueen sends pending messages after result
- [ ] cargo check && cargo test — 0 errors

---

## Dependency Order

```
US-001 (anti-compression) — independent, highest priority
US-002 (skill hints) — independent, can parallel with US-001
US-003 (knowledge file) — independent, can parallel
US-004 (git diff validator) — independent, can parallel
US-005 (mailbox polling) — independent, can parallel
```

All 5 are independent. Can be implemented in parallel by separate agents.

## Verification

After ALL user stories complete:
```bash
cd hatchery && cargo check   # 0 errors
cd hatchery && cargo test    # all pass, 0 failures
```
