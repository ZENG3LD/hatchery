# Coordinator Prompt: {DOMAIN} Pipeline

## Overview

Orchestrate the full {DOMAIN} pipeline using phased agent delegation.
Each phase uses a specialized agent. Wait for completion and verify quality gates before proceeding.

---

## Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `{TARGET}` | What you're building (lowercase) | "bybit", "slider", "auth-service" |
| `{Target}` | PascalCase variant | "Bybit", "Slider", "AuthService" |
| `{DOCS_URL}` | Documentation URL | "https://docs.example.com" |
| `{REFERENCE}` | Reference implementation path | "src/exchanges/kucoin/" |
| `{PACKAGE}` | Build target (crate, module, etc.) | "connectors-v5", "my-app" |
| `{IMPL_AGENT}` | Agent for phases 2-6 | "rust-implementer", "implementer" |
| `{RESEARCH_OUTPUT_DIR}` | Where research files go | "src/exchanges/bybit/research/" |
| `{VERIFICATION_CMD}` | Build/check command | "cargo check --package connectors-v5" |
| `{TEST_CMD}` | Unit test command | "cargo test --package connectors-v5 --test {TARGET}_*" |
| `{INTEGRATION_TEST_CMD}` | Integration test command | "cargo test --test {TARGET}_live -- --ignored --nocapture" |

---

## Pipeline

### Phase 1: Research
```
Agent: research-agent
Prompt: @carousel/01_research.md with {TARGET}, {DOCS_URL}
```

**Wait.** Verify output:
```
ls {RESEARCH_OUTPUT_DIR}
```
Expected: all research files present.

### Phase 2: Implement
```
Agent: {IMPL_AGENT}
Prompt: @carousel/02_implement.md with {TARGET}, {Target}
```

**Wait.** Verify:
```
{VERIFICATION_CMD}
```
Expected: passes without errors.

### Phase 3: Unit Test
```
Agent: {IMPL_AGENT}
Prompt: @carousel/03_unit_test.md with {TARGET}, {Target}
```

**Wait.** Verify: test files created and compile.

### Phase 4: Unit Debug Loop
```
Agent: {IMPL_AGENT}
Prompt: @carousel/04_unit_debug.md with {TARGET}
```

**Repeat until all unit tests pass (max 10 iterations).**

Check:
```
{TEST_CMD}
```
Expected: "0 failed"

### Phase 5: Integration Test
```
Agent: {IMPL_AGENT}
Prompt: @carousel/05_integration_test.md with {TARGET}, {Target}
```

**Wait.** Verify: integration test files created and compile.

### Phase 6: Integration Debug Loop
```
Agent: {IMPL_AGENT}
Prompt: @carousel/06_integration_debug.md with {TARGET}
```

**Repeat until all integration tests pass (max 10 iterations).**

Check:
```
{INTEGRATION_TEST_CMD}
```
Expected: "0 failed" or all remaining failures gracefully skipped with documented reasons.

### Phase 7: Commit & Report
```bash
git add {FILES_TO_COMMIT}
git commit -m "feat({TARGET}): {COMMIT_DESCRIPTION}"
```

Report:
```
✓ {Target} completed
  - Unit tests: X passed
  - Integration tests: Y passed
  - Commit: {hash}
```

---

## Parallel Execution (Optional)

For independent targets, run multiple pipelines simultaneously:

```
[Target A Pipeline]     [Target B Pipeline]     [Target C Pipeline]
       ↓                         ↓                         ↓
   research                  research                  research
       ↓                         ↓                         ↓
   implement                 implement                 implement
       ↓                         ↓                         ↓
  unit test                  unit test                 unit test
       ↓                         ↓                         ↓
  unit debug                 unit debug                unit debug
       ↓                         ↓                         ↓
 integ test                  integ test                integ test
       ↓                         ↓                         ↓
 integ debug                 integ debug               integ debug
       ↓                         ↓                         ↓
   commit                     commit                    commit
```

---

## Troubleshooting

### Research agent returns incomplete data
- Verify {DOCS_URL} is correct and accessible
- May need to split into sub-tasks

### Implementation fails verification
- Check reference implementation for patterns
- Verify all imports and dependencies

### Unit tests fail consistently
- Check research docs for accuracy
- Run single test in isolation with verbose output
- Compare with reference implementation

### Integration tests fail
- Check real API/service availability
- Widen tolerances for live data comparisons
- Add retry logic for transient failures
- Verify credentials/access are configured

---

## How to Use This Template

1. Copy this directory to your project: `cp -r carousel/ my-project/prompts/`
2. Replace all `{VARIABLES}` in every file
3. Adjust phases to your domain (add/remove/modify)
4. Run coordinator prompt with Opus, it delegates phases to Sonnet agents
