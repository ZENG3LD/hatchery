---
name: ralph
description: Design and launch a Ralph autonomous loop for iterative tasks. Use when the user wants to run a task autonomously until completion using PRD checkboxes.
argument-hint: [task-description]
---

## Ralph: Autonomous Iteration Loop

Ralph — паттерн для задач с чёткими критериями завершения.
Rust CLI (`ralph`) запускает Claude в цикле, по одному критерию за итерацию, пока все не будут выполнены.

### Когда использовать

- Задача разбивается на 10+ конкретных шагов с проверяемым результатом
- Каждый шаг завершается верификацией (cargo check, npm build, test pass)
- Не требует human judgment между итерациями
- Примеры: имплементация по образцу, фикс 20 ошибок, миграция API

### Когда НЕ использовать

- Исследовательские задачи (нет чётких критериев)
- Задачи требующие архитектурных решений между шагами
- Одноразовые задачи на 1-3 шага (проще сделать напрямую)
- Лучше подходит Carousel → используй `/carousel`

### Как спроектировать PRD

PRD = markdown файл с чекбоксами `[ ]` / `[x]`. Ralph находит первый `[ ]` и работает над ним.

**Критично для качества:**
1. **Granularity**: один критерий = одно действие + одна проверка. Не "реализуй весь модуль", а "создай endpoints.rs с cargo check"
2. **Порядок**: критерии идут в порядке зависимостей. Сначала то от чего зависит остальное
3. **Верификация**: каждый критерий должен быть проверяем (компиляция, тест, файл существует)
4. **Контекст**: PRD должен содержать всё что нужно агенту — reference paths, docs URLs, patterns

**Структура PRD:**
```markdown
# PRD: {Task Name}

## Introduction
Что делаем, зачем, reference implementation.

## User Stories
### US-001: {Phase}
**Acceptance Criteria:**
- [ ] Конкретный критерий с проверкой
- [ ] Ещё один критерий
```

Шаблоны PRD: `ralph-cli/templates/GENERIC_PRD.md`, `ralph-cli/templates/CONNECTOR_PRD.md`

### Как запустить

```bash
# Сборка (один раз)
cd ralph-cli && cargo build --release

# Запуск (max 100 итераций, конкретный PRD)
ralph-cli/target/release/ralph -n 100 --prd tasks/01-prd-my-task.md

# Авто-обнаружение всех PRD в tasks/
ralph-cli/target/release/ralph -n 50

# Dry run (посмотреть что будет без запуска Claude)
ralph-cli/target/release/ralph --dry-run --prd tasks/01-prd-my-task.md

# С кастомным конфигом
ralph-cli/target/release/ralph -c ralph.toml --prd tasks/01-prd-my-task.md
```

### CLI опции

```
ralph [OPTIONS]
  -n, --max-iterations <N>    Max iterations [default: 100]
  -p, --prd <PATH>...         PRD file(s), else discover tasks/*prd*.md
  -t, --tasks-dir <DIR>       Tasks directory [default: tasks]
  -c, --config <FILE>         Config file [default: ralph.toml]
  -s, --sleep <SECS>          Sleep between iterations [default: 3]
      --stall-threshold <N>   Stalls before waiting [default: 2]
      --prompts-dir <DIR>     Override embedded prompts
      --watch-pattern <RE>    Process regex for stalls [default: "cargo (test|build|clippy|check)"]
      --dry-run               Show plan without invoking Claude
```

### Опциональный конфиг (`ralph.toml`)

Приоритет: **явный CLI аргумент > ralph.toml > дефолт clap**

```toml
[ralph]
max_iterations = 100
sleep_seconds = 3
stall_threshold = 2
tasks_dir = "tasks"
watch_pattern = "cargo (test|build|clippy|check)"

[prompts]
# dir = "custom/prompts"   # override embedded prompts

[claude]
extra_flags = ["--dangerously-skip-permissions"]
```

### Как контролировать

- **Progress bar** в терминале показывает X/Y критериев
- **progress-*.txt** — лог итераций (автокомпактится при >20KB)
- **Stall detection**: 2 итерации без прогресса → ждёт завершения cargo (кросс-платформенно через sysinfo)
- **Остановка**: Ctrl+C (текущая итерация завершится, прогресс сохранён)

### Движок

`ralph-cli/` — Rust CLI binary. Промпты встроены в бинарь через `include_str!`:
- `ralph-cli/prompts/iteration.md` — инструкции для каждой итерации
- `ralph-cli/prompts/compact.md` — промпт для компактификации progress notes

### Твоя задача

Если пользователь вызвал `/ralph`:
1. Спроектируй PRD под задачу $ARGUMENTS
2. Сохрани в `tasks/XX-prd-{name}.md`
3. Создай пустой `tasks/progress-{name}.txt`
4. Покажи команду запуска: `ralph-cli/target/release/ralph -n 100 --prd tasks/XX-prd-{name}.md`
