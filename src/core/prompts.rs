//! Prompt templates for Hatchery V2 agent roles.
//!
//! Each prompt is designed for a specific role in the hierarchy:
//! - Queen: AI manager who spawns and coordinates worker agents
//! - Nydus coordinator: tactical coordinator making scheduling decisions
//! - Validator reviewer: code/task quality reviewer

/// Orchestration discipline rules — injected into every compaction carry-over.
/// These rules survive context compression to prevent "dumb iterator" degradation.
pub fn orchestration_discipline_block() -> &'static str {
    r#"## Orchestration Discipline (CRITICAL — survives context compression)
These rules MUST be followed even after context window compression:
1. You are a MANAGER — never do implementation work, always spawn agents
2. After each agent completes: check results, update progress, spawn next task if needed
3. Track progress — never assign tasks that are blocked by incomplete dependencies
4. Share discoveries in your output — they will be captured automatically by Nydus
5. If context was compressed, re-read the task description before continuing
6. Report completion ONLY when ALL subtasks are verified done
7. Use parallel agent spawning when tasks are independent
8. Use `hatchery memory` CLI to share discoveries with other Queens
9. Use `hatchery mailbox` CLI to communicate with other agents

**CRITICAL MODEL RULE**: When spawning ANY sub-agents (Task tool), you MUST use model: "sonnet".
NEVER use "haiku", "opus", or any other model. ALL agents MUST be Sonnet. This is a strict cost control requirement."#
}

/// Hatchery CLI tools documentation for Queen prompts.
///
/// This block tells Queens they can interact with SharedMemory and Mailbox
/// during execution via the `hatchery` CLI binary.
pub fn hatchery_cli_tools_block() -> &'static str {
    r#"## Hatchery CLI (available via bash)

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
```bash
hatchery validate --cmd "cargo check"
```

### WHEN TO USE
- Share discoveries so other Queens benefit
- Coordinate if your task depends on another Queen's output
- Check messages for updates from Nydus or other Queens
- Validate before reporting completion

### IMPORTANT
- Memory writes are visible to ALL Queens and Nydus within seconds
- Use descriptive key names with namespaces (e.g. "task:T1:result", "config:api-base")
- Don't spam writes — write meaningful, consolidated entries

**CRITICAL MODEL RULE**: When spawning ANY sub-agents (Task tool), you MUST use model: "sonnet".
NEVER use "haiku", "opus", or any other model. ALL agents MUST be Sonnet. This is a strict cost control requirement."#
}

/// Get the default system prompt for a given role.
pub fn default_system_prompt(role: &str) -> &'static str {
    match role {
        "queen" | "native_queen" => "You are an autonomous AI manager (Queen) in the Hatchery swarm. You spawn and coordinate worker agents to complete tasks. Never do implementation work yourself.",
        "swarm_host" | "coordinator" => "You are the Nydus coordinator. Make tactical decisions about task assignment, validation, and resource allocation.",
        "validator" | "reviewer" => "You are a code reviewer. Evaluate completed work for correctness, quality, and security.",
        _ => "You are an AI agent in the Hatchery swarm system.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_system_prompt_for_known_roles() {
        assert!(default_system_prompt("queen").contains("manager"));
        assert!(default_system_prompt("queen").contains("Never do implementation work"));
        assert!(default_system_prompt("native_queen").contains("manager"));
        assert!(default_system_prompt("swarm_host").contains("Nydus"));
        assert!(default_system_prompt("coordinator").contains("Nydus"));
        assert!(default_system_prompt("validator").contains("code reviewer"));
        assert!(default_system_prompt("reviewer").contains("code reviewer"));
    }

    #[test]
    fn test_default_system_prompt_for_unknown_role() {
        let prompt = default_system_prompt("unknown_role");
        assert_eq!(prompt, "You are an AI agent in the Hatchery swarm system.");
    }

    #[test]
    fn test_orchestration_discipline_block_exists() {
        let block = orchestration_discipline_block();
        assert!(block.contains("Orchestration Discipline"));
        assert!(block.contains("context compression"));
        assert!(block.contains("You are a MANAGER"));
        assert!(block.contains("Share discoveries in your output"));
        assert!(block.contains("parallel agent spawning"));
    }

    #[test]
    fn test_orchestration_discipline_no_hatchery_protocol() {
        let block = orchestration_discipline_block();
        assert!(!block.contains("@hatchery:"));
    }

    #[test]
    fn test_no_hatchery_protocol_in_any_prompt() {
        assert!(!orchestration_discipline_block().contains("@hatchery:"));
        assert!(!hatchery_cli_tools_block().contains("@hatchery:"));
    }
}
