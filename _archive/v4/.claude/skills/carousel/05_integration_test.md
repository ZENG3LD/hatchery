# Phase 5: Integration Testing

## Agent Type
`{IMPL_AGENT}` (e.g., `rust-implementer`, `implementer`)

## Variables
- `{TARGET}` - Target name (lowercase)
- `{Target}` - Target name (PascalCase)

---

## Prompt

```
Write integration tests for {TARGET} that test REAL functionality with LIVE data/services.

Unlike Phase 3 unit tests (which test logic in isolation), these tests validate:
- Real external API/service communication
- Real data accuracy and consistency
- Rate limit behavior under realistic load
- Error recovery and reconnection
- Authentication with real credentials

═══════════════════════════════════════════════════════════════════════════════
REFERENCES
═══════════════════════════════════════════════════════════════════════════════

Unit tests: {UNIT_TEST_FILES} (Phase 3 — already passing)
Research: {RESEARCH_OUTPUT_DIR}

═══════════════════════════════════════════════════════════════════════════════
FILE: {INTEGRATION_TEST_FILE}
═══════════════════════════════════════════════════════════════════════════════

## Setup
- All tests marked as skippable by default (e.g., #[ignore] in Rust, .skip in JS)
- Require real credentials/config from environment variables
- Use sandbox/testnet/staging if available, otherwise use MINIMAL operations on production
- Include cleanup logic (revert changes, close connections, etc.)

## Required Integration Tests:

### Connectivity
- test_live_connection
  - Verify real connection to service
  - Assert: successful response within timeout

### Data Accuracy
- test_live_data_validity
  - Fetch real data and validate structure
  - Assert: data matches expected schema
  - Assert: values are in reasonable ranges

### Real Operations (if applicable)
- test_live_write_operation
  - Perform minimal real operation
  - Assert: operation succeeds
  - Cleanup: revert/undo the operation

### Error Handling
- test_live_rate_limit_handling
  - Send rapid requests
  - Assert: either all succeed or rate limit error is properly returned
  - Assert: no panics or connection drops

### Long-running (if applicable)
- test_live_sustained_connection
  - Maintain connection for extended period (30-60 seconds)
  - Assert: no disconnects or data loss

═══════════════════════════════════════════════════════════════════════════════
PATTERNS
═══════════════════════════════════════════════════════════════════════════════

## Credential Loading
{CREDENTIAL_LOADING_PATTERN}

## Skip by Default
{SKIP_PATTERN}

═══════════════════════════════════════════════════════════════════════════════
RUN
═══════════════════════════════════════════════════════════════════════════════

# Compile check
{INTEGRATION_COMPILE_CMD}

# Run integration tests (requires credentials)
{INTEGRATION_TEST_CMD}
```

---

## Exit Criteria
- Integration test file(s) created
- Tests compile without errors
- Tests are skippable by default (won't break CI without credentials)
