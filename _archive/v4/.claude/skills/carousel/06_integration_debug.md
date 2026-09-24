# Phase 6: Integration Debug Loop

## Agent Type
`{IMPL_AGENT}` (e.g., `rust-implementer`, `implementer`)

## Variables
- `{TARGET}` - Target name (lowercase)

---

## Prompt

```
Debug and fix failing INTEGRATION tests for {TARGET}.

These are LIVE tests that hit REAL external services. Unlike Phase 4 (unit test debug),
issues here are typically:
- Real API/service response format differs from documentation
- Rate limits triggered during test runs
- Connections dropped by external service
- Staging/sandbox vs production behavior differences
- Authentication edge cases not covered in docs

═══════════════════════════════════════════════════════════════════════════════
PROCESS
═══════════════════════════════════════════════════════════════════════════════

1. Run integration tests:
   {INTEGRATION_TEST_CMD}

2. For EACH failure, identify and fix:

═══════════════════════════════════════════════════════════════════════════════
COMMON INTEGRATION ISSUES
═══════════════════════════════════════════════════════════════════════════════

## Data accuracy failures
- External service may have variable data → widen tolerance
- Values change between requests → add timing tolerance
- Schema may have optional fields not shown in docs

## Connection issues
- Service rate-limits connections → add delays between tests
- Keepalive interval incorrect → check actual requirements
- Response format on production differs from docs → log and adapt

## Rate limit hits
- Add delays between test functions
- Reduce number of rapid requests in rate limit test
- Check if service has separate limits for different endpoints

## Write operation issues
- Sandbox/testnet may be down → gracefully skip with warning
- Minimum thresholds differ → check service requirements
- Operation might partially complete → handle in cleanup logic

## Credential issues
- API key may have restricted permissions → test only what's allowed
- IP restrictions may block → document in test output
- Token expiration during long test runs

═══════════════════════════════════════════════════════════════════════════════
LOOP UNTIL ALL PASS
═══════════════════════════════════════════════════════════════════════════════

Repeat:
1. Run all integration tests
2. Pick first failure
3. Identify: is it a code bug, service issue, or test expectation issue?
4. Fix appropriately:
   - Code bug → fix in implementation code
   - Service issue → adapt test expectations or add retry logic
   - Test expectation → widen tolerances or add skip conditions
5. Run single test to verify fix
6. Run all integration tests
7. If failures remain, go to 2

EXIT only when:
{INTEGRATION_TEST_CMD}

Shows: all passed, 0 failed
Or: All remaining failures are gracefully skipped with documented reasons
```

---

## Exit Criteria
- ALL integration tests pass (or are gracefully skipped with documented reasons)
- No panics or crashes
- {TARGET} works with real external services
