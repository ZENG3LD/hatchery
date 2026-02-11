## Hatchery CLI (available via bash)

### Shared Memory — read/write knowledge visible to all Queens
```bash
hatchery memory read --key "api-endpoints"        # read specific key
hatchery memory read --pattern "config:"          # search by pattern
hatchery memory list                               # list all keys
hatchery memory write --key "discovery:auth" --value '{"method":"HMAC"}'
hatchery memory info                               # show metadata
```

### Messaging — communicate with other agents
```bash
hatchery mailbox send --to "queen:Q1" --message "need auth module first"
hatchery mailbox send --to "swarmhost:SH0" --message "found critical bug"
hatchery mailbox read --limit 10
hatchery mailbox read --from "queen:Q0"
```

### Validation — check your work before reporting done

Scope verification to modified crates only:
```bash
# GOOD: scope to the crate you modified
hatchery validate --cmd "cargo check -p <crate-name>"
hatchery validate --cmd "cargo test -p <crate-name>"

# AVOID: full workspace tests are slow and wasteful
# cargo test --workspace  ← only use this for final integration verification
```

When verifying work:
1. Only test the crate(s) you actually modified
2. Use `cargo check -p <crate>` for quick compilation verification
3. Use `cargo test -p <crate>` for unit test verification
4. Reserve `cargo test --workspace` for final integration checks only

### WHEN TO USE
- Share discoveries so other Queens benefit
- Coordinate if your task depends on another Queen's output
- Check messages for updates from Nydus or other Queens
- Validate before reporting completion

### IMPORTANT
- Memory writes are visible to ALL Queens and Nydus within seconds
- Use descriptive key names with namespaces (e.g. "task:T1:result", "config:api-base")
- Don't spam writes — write meaningful, consolidated entries
