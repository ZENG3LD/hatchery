# Phase 2: Implementation

## Agent Type
`{IMPL_AGENT}` (e.g., `rust-implementer`, `implementer`)

## Variables
- `{TARGET}` - Target name (lowercase)
- `{Target}` - Target name (PascalCase)

---

## Prompt

```
Implement {TARGET} following the reference implementation.

═══════════════════════════════════════════════════════════════════════════════
REFERENCE
═══════════════════════════════════════════════════════════════════════════════

Reference implementation: {REFERENCE_PATH}
Research docs: {RESEARCH_OUTPUT_DIR}

Study the reference carefully. Match patterns EXACTLY.

═══════════════════════════════════════════════════════════════════════════════
FILES TO CREATE
═══════════════════════════════════════════════════════════════════════════════

{FILE_LIST_WITH_DESCRIPTIONS}

Example:

FILE 1: {OUTPUT_DIR}/{TARGET}/module_a.rs
- Description of what this file implements
- Key structs/functions to include
- Reference: corresponding file in reference implementation

FILE 2: {OUTPUT_DIR}/{TARGET}/module_b.rs
...

═══════════════════════════════════════════════════════════════════════════════
AFTER EACH FILE
═══════════════════════════════════════════════════════════════════════════════

{VERIFICATION_CMD}

Fix any errors before moving to next file.

═══════════════════════════════════════════════════════════════════════════════
FINALLY
═══════════════════════════════════════════════════════════════════════════════

{REGISTRATION_STEP} (e.g., add module to parent mod.rs, update config, etc.)
```

---

## Exit Criteria
- All files created
- {VERIFICATION_CMD} passes
- {TARGET} registered/exported properly
