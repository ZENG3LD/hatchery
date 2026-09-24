//! Prompt templates for Hatchery V2 agent roles.
//!
//! Each prompt is designed for a specific role in the hierarchy:
//! - Queen: AI manager who spawns and coordinates worker agents
//! - Nydus coordinator: tactical coordinator making scheduling decisions
//! - Validator reviewer: code/task quality reviewer

/// Queen manager role preamble — explains the manager role and available agent types.
pub fn queen_preamble() -> &'static str {
    include_str!("../queen/prompts/preamble.md")
}

/// Orchestration discipline rules — injected into every compaction carry-over.
/// These rules survive context compression to prevent "dumb iterator" degradation.
pub fn orchestration_discipline_block() -> &'static str {
    include_str!("../queen/prompts/orchestration_discipline.md")
}

/// Hatchery CLI tools documentation for Queen prompts.
///
/// This block tells Queens they can interact with SharedMemory and Mailbox
/// during execution via the `hatchery` CLI binary.
pub fn hatchery_cli_tools_block() -> &'static str {
    include_str!("../queen/prompts/hatchery_cli_tools.md")
}

/// Git safety rules for Queens working in isolated branches.
///
/// These rules prevent Queens from accidentally modifying main/master branches
/// or performing destructive git operations that could harm the shared codebase.
pub fn git_safety_block() -> &'static str {
    include_str!("../queen/prompts/git_safety.md")
}

/// Recovery notice template — explains to a recovering Queen what happened and what to do.
pub fn recovery_notice_template() -> &'static str {
    include_str!("../queen/prompts/recovery_notice.md")
}

/// Get the default system prompt for a given role.
pub fn default_system_prompt(role: &str) -> &'static str {
    match role {
        "queen" | "native_queen" => include_str!("prompts/default_queen.md"),
        "swarm_host" | "coordinator" => include_str!("prompts/default_coordinator.md"),
        "validator" | "reviewer" | "overlord" => include_str!("prompts/default_reviewer.md"),
        _ => include_str!("prompts/default_agent.md"),
    }
}

/// System prompt for Overlord — merge validator role.
pub fn overlord_system_prompt() -> String {
    include_str!("../overlord/prompts/system.md").to_string()
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
        assert!(!queen_preamble().contains("@hatchery:"));
        assert!(!orchestration_discipline_block().contains("@hatchery:"));
        assert!(!hatchery_cli_tools_block().contains("@hatchery:"));
        assert!(!recovery_notice_template().contains("@hatchery:"));
    }

    #[test]
    fn test_queen_preamble_exists() {
        let preamble = queen_preamble();
        assert!(preamble.contains("Queen Manager"));
        assert!(preamble.contains("You are a MANAGER"));
        assert!(preamble.contains("CRITICAL MODEL RULE"));
        assert!(preamble.contains("sonnet"));
    }

    #[test]
    fn test_recovery_notice_template_has_placeholders() {
        let template = recovery_notice_template();
        assert!(template.contains("{reason}"));
        assert!(template.contains("{queen_id}"));
        assert!(template.contains("{task_str}"));
        assert!(template.contains("{status_str}"));
        assert!(template.contains("{attempt}"));
        assert!(template.contains("{started}"));
        assert!(template.contains("{discipline}"));
    }

    #[test]
    fn test_git_safety_block_exists() {
        let block = git_safety_block();
        assert!(block.contains("Git Safety Rules"));
        assert!(block.contains("NON-NEGOTIABLE"));
        assert!(block.contains("NEVER checkout"));
        assert!(block.contains("main"));
        assert!(block.contains("master"));
        assert!(block.contains("git push -f"));
        assert!(block.contains("git branch --show-current"));
        assert!(block.contains("hatchery/*"));
    }
}
