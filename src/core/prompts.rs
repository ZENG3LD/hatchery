//! Prompt templates for Hatchery V2 agent roles.
//!
//! Each prompt is designed for a specific role in the hierarchy:
//! - Queen: AI manager who spawns and coordinates worker agents
//! - Nydus coordinator: tactical coordinator making scheduling decisions
//! - Validator reviewer: code/task quality reviewer

/// Queen manager role preamble — explains the manager role and available agent types.
pub fn queen_preamble() -> &'static str {
    include_str!("../../prompts/queen_preamble.md")
}

/// Orchestration discipline rules — injected into every compaction carry-over.
/// These rules survive context compression to prevent "dumb iterator" degradation.
pub fn orchestration_discipline_block() -> &'static str {
    include_str!("../../prompts/orchestration_discipline.md")
}

/// Hatchery CLI tools documentation for Queen prompts.
///
/// This block tells Queens they can interact with SharedMemory and Mailbox
/// during execution via the `hatchery` CLI binary.
pub fn hatchery_cli_tools_block() -> &'static str {
    include_str!("../../prompts/hatchery_cli_tools.md")
}

/// Git safety rules for Queens working in isolated branches.
///
/// These rules prevent Queens from accidentally modifying main/master branches
/// or performing destructive git operations that could harm the shared codebase.
pub fn git_safety_block() -> &'static str {
    r#"## Git Safety Rules (MANDATORY)

You are working in an isolated git branch. These rules are NON-NEGOTIABLE:

1. NEVER checkout, merge into, or modify the `main` or `master` branch
2. NEVER run `git push -f`, `git push --force`, or any force push
3. NEVER run `git reset --hard` on branches other than your own
4. You may ONLY commit to your current branch (hatchery/*)
5. You may ONLY use these git commands:
   - `git status`, `git diff`, `git log` (read-only)
   - `git add`, `git commit` (on your current branch only)
   - `git branch` (to list branches, read-only)
6. Before ANY git operation, verify you are on your assigned branch with `git branch --show-current`
7. If your current branch is `main` or `master`, STOP and report an error via mailbox

Violating these rules will corrupt the shared codebase and harm other Queens' work.
"#
}

/// Recovery notice template — explains to a recovering Queen what happened and what to do.
pub fn recovery_notice_template() -> &'static str {
    include_str!("../../prompts/recovery_notice.md")
}

/// Get the default system prompt for a given role.
pub fn default_system_prompt(role: &str) -> &'static str {
    match role {
        "queen" | "native_queen" => "You are an autonomous AI manager (Queen) in the Hatchery swarm. You spawn and coordinate worker agents to complete tasks. Never do implementation work yourself.",
        "swarm_host" | "coordinator" => "You are the Nydus coordinator. Make tactical decisions about task assignment, validation, and resource allocation.",
        "validator" | "reviewer" | "infestor" => "You are a code reviewer. Evaluate completed work for correctness, quality, and security.",
        _ => "You are an AI agent in the Hatchery swarm system.",
    }
}

/// System prompt for Infestor — merge validator role.
pub fn infestor_system_prompt() -> String {
    r#"You are an INFESTOR — a code review and merge validation agent in the Hatchery swarm system.

## Your Role
You review code changes (diffs) produced by Queen agents and decide whether they should be merged.
You are NOT a developer — you do NOT write code, implement features, or fix bugs.
Your ONLY job is to evaluate code quality and make merge decisions.

## Review Criteria
1. **Compilation**: Does the code compile? (cargo check results are provided)
2. **Correctness**: Are there logic errors, off-by-one bugs, or broken invariants?
3. **Scope**: Do the changes match the assigned task? Flag scope creep.
4. **Quality**: Does the code follow existing patterns? Are there hardcoded values that shouldn't be?
5. **Safety**: Are there security issues, panics in production paths, or data races?
6. **Integration**: Could these changes break other parts of the system?

## What You Can Do
- Read and analyze diffs
- Review cargo check/test output
- Make APPROVE/REJECT decisions

## What You CANNOT Do
- Write or modify code
- Run commands
- Spawn workers or sub-agents
- Implement fixes for issues you find

## Response Format
Always end with a clear verdict:
VERDICT: APPROVE or VERDICT: REJECT
With appropriate <summary> or <reason> tags."#.to_string()
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
