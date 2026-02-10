---
name: carousel
description: Design a phased agent pipeline (Research → Implement → Unit Test → Unit Debug → Integration Test → Integration Debug) for complex tasks. Use when the user needs to break a task into phases with different agents and quality gates.
argument-hint: [task-description]
---

## Carousel: Phased Agent Pipeline

Carousel — паттерн для сложных задач, разбиваемых на последовательные фазы с разными агентами.
Координатор (Opus) запускает специализированных агентов (Sonnet) по фазам, проверяя quality gates между ними.

### Когда использовать

- Задача естественно разбивается на фазы (research → implement → test → debug)
- Разные фазы требуют разных агентов (research-agent, rust-implementer)
- Нужны quality gates между фазами (не начинай impl пока research не готов)
- Задачу можно параллелить (5 targets одновременно через 5 параллельных пайплайнов)
- Примеры: коннекторы, UI компоненты, интеграции с внешними API

### Когда НЕ использовать

- Задача линейная без фаз → используй `/ralph`
- Нет чёткого reference implementation → сначала research отдельно
- Задача на 1-2 файла → проще напрямую

### Стандартные фазы

```
Phase 1: Research (research-agent)
    ↓ gate: все research файлы созданы
Phase 2: Implement (rust-implementer / implementer)
    ↓ gate: код компилируется
Phase 3: Unit Test (rust-implementer / implementer)
    ↓ gate: тесты компилируются
Phase 4: Unit Debug (rust-implementer / implementer, loop)
    ↓ gate: все unit тесты проходят
Phase 5: Integration Test (rust-implementer / implementer)
    ↓ gate: интеграционные тесты компилируются
Phase 6: Integration Debug (rust-implementer / implementer, loop)
    ↓ gate: интеграционные тесты проходят с реальными данными
Done: commit
```

Фазы можно добавлять/убирать/менять под задачу. 6 — не догма.

**Примечание**: Фазы 5-6 (integration) добавляются когда нужна валидация с реальными данными/API. Для чисто внутренних компонентов достаточно 4 фаз.

### Generic шаблоны

Шаблоны всех 7 файлов (6 фаз + coordinator) лежат в `carousel/`:

```
carousel/
├── 00_coordinator.md         — оркестратор пайплайна
├── 01_research.md            — фаза ресерча
├── 02_implement.md           — фаза имплементации
├── 03_unit_test.md           — фаза unit тестирования
├── 04_unit_debug.md          — фаза unit дебага (loop)
├── 05_integration_test.md    — фаза интеграционных тестов (real API/data)
└── 06_integration_debug.md   — фаза интеграционного дебага (loop)
```

Все шаблоны используют `{VARIABLE}` плейсхолдеры — замени под свой домен.

### Как использовать шаблоны

1. Скопируй `carousel/` в промпты проекта: `cp -r carousel/ my-project/prompts/`
2. Замени все `{VARIABLES}` в каждом файле под свой домен
3. Убери/добавь фазы по необходимости
4. Запусти coordinator prompt — он делегирует фазы агентам

### Параллельное выполнение

Независимые targets можно запускать параллельно:
```
[Target A]       [Target B]       [Target C]
  research         research         research
  implement        implement        implement
  unit test        unit test        unit test
  unit debug       unit debug       unit debug
  integ test       integ test       integ test
  integ debug      integ debug      integ debug
  commit           commit           commit
```

### Существующие инстансы (примеры)

- `v5/prompts/00-06` — exchange connectors (6 phases, domain-specific)
- `v5/prompts/data_providers/00-06` — data providers (6 phases, domain-specific)
- `carousel/` — generic шаблоны (domain-agnostic, с `{VARIABLE}` плейсхолдерами)

### Твоя задача

Если пользователь вызвал `/carousel`:
1. Прочитай generic шаблоны в `carousel/` для структуры
2. Определи фазы для задачи $ARGUMENTS (обычно 6, но может быть меньше)
3. Определи агентов для каждой фазы
4. Определи quality gates
5. Заполни `{VARIABLES}` шаблонов под конкретную задачу
6. Начни выполнение как координатор: запускай фазы последовательно, проверяя gates
