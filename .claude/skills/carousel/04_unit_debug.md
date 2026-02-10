# Phase 4: Unit Debug Loop

## Agent Type
`{IMPL_AGENT}` (e.g., `rust-implementer`, `implementer`)

## Variables
- `{TARGET}` - Target name (lowercase)

---

## Prompt

```
Debug and fix all failing unit tests for {TARGET}.

═══════════════════════════════════════════════════════════════════════════════
PROCESS
═══════════════════════════════════════════════════════════════════════════════

1. Run all unit tests:
   {TEST_CMD}

2. For EACH failure:
   a. Read the error message carefully
   b. Identify the root cause (see common errors below)
   c. Fix the code
   d. Run the single failing test to verify fix
   e. Run all tests again

═══════════════════════════════════════════════════════════════════════════════
COMMON ERRORS AND FIXES
═══════════════════════════════════════════════════════════════════════════════

{ERROR_PATTERNS}

Example structure:

## Error Type 1: {description}
Location: {file}

Checklist:
- [ ] Check A
- [ ] Check B
- [ ] Check C

Debug: {how to get more info}

## Error Type 2: {description}
...

═══════════════════════════════════════════════════════════════════════════════
LOOP UNTIL ALL PASS
═══════════════════════════════════════════════════════════════════════════════

Repeat:
1. Run tests
2. Pick first failure
3. Identify cause
4. Fix code
5. Run single test to verify
6. Run all tests
7. If failures remain, go to 2

EXIT only when:
{TEST_CMD}
Shows: all tests passed, 0 failed
```

---

## Exit Criteria
- ALL unit tests pass
- Output confirms 0 failures
