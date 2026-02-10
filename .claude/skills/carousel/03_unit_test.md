# Phase 3: Unit Testing

## Agent Type
`{IMPL_AGENT}` (e.g., `rust-implementer`, `implementer`)

## Variables
- `{TARGET}` - Target name (lowercase)
- `{Target}` - Target name (PascalCase)

---

## Prompt

```
Write comprehensive unit tests for {TARGET}.

═══════════════════════════════════════════════════════════════════════════════
REFERENCES
═══════════════════════════════════════════════════════════════════════════════

Test reference: {TEST_REFERENCE_PATH}
Implementation: {IMPL_PATH}

═══════════════════════════════════════════════════════════════════════════════
TEST FILES TO CREATE
═══════════════════════════════════════════════════════════════════════════════

{TEST_FILE_LIST_WITH_DESCRIPTIONS}

Example:

FILE 1: tests/{TARGET}_unit.rs (or __tests__/{TARGET}.test.ts, etc.)

Required tests:

### Basic / Smoke Tests
- test_basic_functionality
- test_initialization

### Core Logic
- test_feature_a_happy_path
- test_feature_a_edge_cases
- test_feature_b_happy_path

### Error Handling
- test_invalid_input
  - Assert: returns error, not panic
- test_missing_dependency
  - Assert: graceful failure

═══════════════════════════════════════════════════════════════════════════════
TEST PATTERNS
═══════════════════════════════════════════════════════════════════════════════

{TESTING_PATTERNS}

Example for Rust:
- Use `#[tokio::test]` for async tests
- Use `-- --nocapture` to see output
- Handle timeouts gracefully with `match`, not `assert`

Example for TypeScript:
- Use describe/it blocks
- Use beforeEach for setup
- Mock external dependencies

═══════════════════════════════════════════════════════════════════════════════
RUN TESTS
═══════════════════════════════════════════════════════════════════════════════

{TEST_CMD}
```

---

## Exit Criteria
- All test files created
- Tests compile / parse without errors
- Tests run (some failures expected — will fix in Phase 4)
