# Phase 1: Research

## Agent Type
`research-agent`

## Variables
- `{TARGET}` - Target name (lowercase)
- `{DOCS_URL}` - Official documentation URL

---

## Prompt

```
Research {TARGET} for implementation.

Documentation: {DOCS_URL}

Create folder: {RESEARCH_OUTPUT_DIR}

Write the following research files. Each must contain EXACT information
from official documentation — no guessing, no inventing.

═══════════════════════════════════════════════════════════════════════════════
FILE 1: overview.md
═══════════════════════════════════════════════════════════════════════════════

High-level overview:
- What is {TARGET}?
- Key concepts and terminology
- Architecture / components
- Limitations and constraints

═══════════════════════════════════════════════════════════════════════════════
FILE 2: api_reference.md
═══════════════════════════════════════════════════════════════════════════════

All API endpoints / interfaces / entry points:

| Method | Endpoint | Description | Auth Required |
|--------|----------|-------------|---------------|
| ... | ... | ... | ... |

Include exact parameters, types, required/optional.

═══════════════════════════════════════════════════════════════════════════════
FILE 3: data_structures.md
═══════════════════════════════════════════════════════════════════════════════

All data structures with exact field names and types.
Include JSON/payload examples copied from docs.

═══════════════════════════════════════════════════════════════════════════════
FILE 4: authentication.md (if applicable)
═══════════════════════════════════════════════════════════════════════════════

Step-by-step authentication/authorization process.
Include example requests and signatures.

═══════════════════════════════════════════════════════════════════════════════
FILE 5: edge_cases.md
═══════════════════════════════════════════════════════════════════════════════

- Rate limits
- Error codes and responses
- Known quirks and gotchas
- Differences between variants (if any)
```

---

## Exit Criteria
- All research files created in {RESEARCH_OUTPUT_DIR}
- Each file has exact examples from official docs
- No guessed or invented data
