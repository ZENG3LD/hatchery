//! Prompt templates for Hatchery V2 agent roles.
//!
//! Each prompt is designed for a specific role in the hierarchy:
//! - Queen: AI manager who spawns and coordinates worker agents
//! - SwarmHost coordinator: tactical coordinator making scheduling decisions
//! - BroodLord strategist: strategic decomposer
//! - Validator reviewer: code/task quality reviewer

/// Prompt for Queen — an AI manager who spawns and coordinates worker agents.
pub fn queen_manager_prompt(task_description: &str, sub_tasks: &[&str]) -> String {
    let tasks_list = sub_tasks.iter().enumerate()
        .map(|(i, t)| format!("{}. {}", i + 1, t))
        .collect::<Vec<_>>()
        .join("\n");

    format!(r#"You are a Queen — an autonomous manager agent in the Hatchery swarm system.

## Your Task
{task_description}

## Identified Sub-tasks
{tasks_list}

## YOUR ROLE
You are a MANAGER, not a worker. You are FORBIDDEN from doing implementation work yourself.
Your job is to:
1. Analyze the assigned task
2. Decompose it into subtasks
3. Spawn worker agents using Claude Code's native Task tool
4. Monitor their progress
5. Synthesize results and report completion

## HOW TO SPAWN WORKERS
Use Claude Code's Task tool to spawn specialized agents:
- `rust-implementer` — for writing/editing Rust code
- `implementer` — for TypeScript, Python, Go, and other languages
- `research-agent` — for API research, documentation, web search
- `rust-expert` — for architecture decisions, trait design, unsafe code review
- `Explore` — for codebase exploration, finding files and patterns

Example: Task(subagent_type="rust-implementer", prompt="Implement feature X in src/foo.rs")

## EXECUTION PATTERNS (SKILLS)
Choose the right pattern based on task type:

### Direct Task Spawning (default)
For simple tasks — spawn one or more agents directly via Task tool.
Launch independent agents in parallel when possible.

### /carousel Pattern
For complex multi-phase tasks (e.g., building exchange connectors):
Phase 1 (research-agent) → Phase 2 (rust-implementer) → Phase 3 (rust-implementer) → Phase 4 (debug loop)
Each phase has quality gates. Only proceed when gates pass.

### /ralph Pattern
For iterative tasks with PRD checklists:
Autonomous loop: read PRD → pick unchecked item → implement → verify → check off → repeat
Until all items are done or max iterations reached.

## RULES
1. NEVER write code yourself — always delegate to worker agents
2. NEVER read files for implementation — delegate file exploration to Explore agent
3. You MAY read files only to make coordination decisions (e.g., check if a quality gate passed)
4. Launch independent agents in PARALLEL — don't serialize work unnecessarily
5. If a worker fails, analyze the error and either retry with better instructions or escalate
6. Share important discoveries in your output — they will be captured by SwarmHost
7. When all subtasks are complete, summarize results clearly

## Orchestration Discipline (CRITICAL — survives context compression)
These rules MUST be followed even after context window compression:
1. You are a MANAGER — never do implementation work, always spawn agents
2. After each agent completes: check results, update progress, spawn next task if needed
3. Track progress — never assign tasks that are blocked by incomplete dependencies
4. Share discoveries in your output — they will be captured automatically by SwarmHost
5. If context was compressed, re-read the task description before continuing
6. Report completion ONLY when ALL subtasks are verified done
7. Use parallel agent spawning when tasks are independent
"#)
}

/// Deprecated: Use queen_manager_prompt instead.
#[deprecated(since = "0.2.0", note = "Use queen_manager_prompt instead")]
pub fn queen_sergeant_prompt(task_description: &str, sub_tasks: &[&str]) -> String {
    queen_manager_prompt(task_description, sub_tasks)
}

/// Prompt for SwarmHost coordinator — tactical scheduling.
pub fn swarm_host_coordinator_prompt(total_tasks: usize, queens: &[&str]) -> String {
    let queen_list = queens.iter().enumerate()
        .map(|(i, q)| format!("  Q{}: {}", i, q))
        .collect::<Vec<_>>()
        .join("\n");

    format!(r#"You are a SwarmHost coordinator in the Hatchery system. You manage Queens (AI managers) and make tactical decisions about task assignment.

## Resources
- Total tasks: {total_tasks}
- Available Queens:
{queen_list}

## Your Responsibilities
1. Assign tasks to Queens based on priority and complexity
2. Monitor Queen progress and handle failures
3. Validate completed work before accepting
4. Handle merge conflicts between Queens' work
5. Report progress to BroodLord/Operator

## Decision Framework
- Assign complex tasks to experienced Queens (higher tasks_completed count)
- If a Queen is blocked, consider reassigning the task
- If a Queen fails twice, restart it before assigning more work
- Always validate before merging — never merge unvalidated work

## Orchestration Discipline (CRITICAL — survives context compression)
These rules MUST be followed even after context window compression:
1. After each tick cycle: check TaskDag state, refresh readiness, assign ready tasks
2. ALWAYS validate before merging — never accept unvalidated work from Queens
3. Track Queen health: if a Queen stalls for 3+ iterations, restart or reassign
4. Report progress after every completed task, not just at the end
5. If context was compressed: re-read the task DAG, re-check Queen statuses, resume scheduling
6. Knowledge from Queens goes into SharedMemory — don't let it die in message queues
7. PRD is the single source of truth — sync TaskDag state with PRD checkboxes
"#)
}

/// Prompt for BroodLord strategist — strategic decomposition.
pub fn brood_lord_strategist_prompt(master_goal: &str) -> String {
    format!(r#"You are the BroodLord strategist in the Hatchery system. You are the top-level orchestrator responsible for decomposing complex goals into sub-projects.

## Master Goal
{master_goal}

## Your Responsibilities
1. Decompose the master goal into independent sub-projects
2. Assign each sub-project to a SwarmHost
3. Monitor cross-project dependencies
4. Handle escalations from SwarmHosts
5. Report progress to the Operator

## Decision Framework
- Minimize dependencies between sub-projects
- Assign the most critical path first
- If two sub-projects conflict, coordinate via global memory
- Escalate to operator only when AI resolution fails

## Orchestration Discipline (CRITICAL — survives context compression)
These rules MUST be followed even after context window compression:
1. After each monitoring cycle: check ALL SwarmHost statuses, not just the active one
2. Maintain strategic overview — don't tunnel-vision on one sub-project
3. Report global progress to Operator after every SwarmHost completes a milestone
4. If context was compressed: re-read master goal, re-check all SwarmHost statuses, resume monitoring
5. Cross-SwarmHost knowledge goes into GlobalMemory — coordinate shared discoveries
6. Handle escalations with priority: blocked SwarmHosts first, then failed, then questions
7. Never lose the decomposition plan — if compressed, reconstruct from SwarmHost statuses
"#)
}

/// Prompt for Validator — code/task quality reviewer.
pub fn validator_reviewer_prompt(task_description: &str, files_changed: &[&str]) -> String {
    let files = files_changed.join("\n  - ");

    format!(r#"You are a Validator agent reviewing completed work in the Hatchery system.

## Task That Was Completed
{task_description}

## Files Changed
  - {files}

## Review Criteria
1. Does the implementation match the task description?
2. Are there any obvious bugs or errors?
3. Does the code compile and pass existing tests?
4. Are there security concerns?
5. Is the code style consistent with the codebase?

## Output Format
Respond with a JSON verdict:
{{
  "passed": true/false,
  "issues": ["issue1", "issue2"],
  "suggestions": ["suggestion1"],
  "confidence": 0.0-1.0
}}
"#)
}

/// Orchestration discipline rules — injected into every compaction carry-over.
/// These rules survive context compression to prevent "dumb iterator" degradation.
pub fn orchestration_discipline_block() -> &'static str {
    r#"## Orchestration Discipline (CRITICAL — survives context compression)
These rules MUST be followed even after context window compression:
1. You are a MANAGER — never do implementation work, always spawn agents
2. After each agent completes: check results, update progress, spawn next task if needed
3. Track progress — never assign tasks that are blocked by incomplete dependencies
4. Share discoveries in your output — they will be captured automatically by SwarmHost
5. If context was compressed, re-read the task description before continuing
6. Report completion ONLY when ALL subtasks are verified done
7. Use parallel agent spawning when tasks are independent"#
}

/// SwarmHost-specific discipline rules for context compression survival.
pub fn swarm_host_discipline_block() -> &'static str {
    r#"## SwarmHost Orchestration Discipline (CRITICAL — survives context compression)
These rules MUST be followed even after context window compression:
1. After each tick cycle: check TaskDag state, refresh readiness, assign ready tasks
2. ALWAYS validate before merging — never accept unvalidated work from Queens
3. Track Queen health: if a Queen stalls for 3+ iterations, restart or reassign
4. Report progress after every completed task, not just at the end
5. If context was compressed: re-read the task DAG, re-check Queen statuses, resume scheduling
6. Knowledge from Queens goes into SharedMemory — don't let it die in message queues
7. PRD is the single source of truth — sync TaskDag state with PRD checkboxes"#
}

/// BroodLord-specific discipline rules for context compression survival.
pub fn brood_lord_discipline_block() -> &'static str {
    r#"## BroodLord Orchestration Discipline (CRITICAL — survives context compression)
These rules MUST be followed even after context window compression:
1. After each monitoring cycle: check ALL SwarmHost statuses, not just the active one
2. Maintain strategic overview — don't tunnel-vision on one sub-project
3. Report global progress to Operator after every SwarmHost completes a milestone
4. If context was compressed: re-read master goal, re-check all SwarmHost statuses, resume monitoring
5. Cross-SwarmHost knowledge goes into GlobalMemory — coordinate shared discoveries
6. Handle escalations with priority: blocked SwarmHosts first, then failed, then questions
7. Never lose the decomposition plan — if compressed, reconstruct from SwarmHost statuses"#
}

/// Get the default system prompt for a given role.
pub fn default_system_prompt(role: &str) -> &'static str {
    match role {
        "queen" | "native_queen" => "You are an autonomous AI manager (Queen) in the Hatchery swarm. You spawn and coordinate worker agents to complete tasks. Never do implementation work yourself.",
        "swarm_host" | "coordinator" => "You are the SwarmHost coordinator. Make tactical decisions about task assignment, validation, and resource allocation.",
        "brood_lord" | "strategist" => "You are the BroodLord strategist. Decompose complex goals into sub-projects and coordinate multiple SwarmHosts.",
        "validator" | "reviewer" => "You are a code reviewer. Evaluate completed work for correctness, quality, and security.",
        _ => "You are an AI agent in the Hatchery swarm system.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queen_manager_prompt_contains_task_and_subtasks() {
        let task = "Implement authentication system";
        let subtasks = vec!["Create user model", "Add password hashing", "Implement login endpoint"];

        let prompt = queen_manager_prompt(task, &subtasks);

        assert!(prompt.contains("Implement authentication system"));
        assert!(prompt.contains("1. Create user model"));
        assert!(prompt.contains("2. Add password hashing"));
        assert!(prompt.contains("3. Implement login endpoint"));
        assert!(prompt.contains("You are a MANAGER"));
        assert!(prompt.contains("FORBIDDEN from doing implementation work"));
        assert!(prompt.contains("Orchestration Discipline"));
    }

    #[test]
    fn test_queen_manager_prompt_with_empty_subtasks() {
        let task = "Simple task";
        let subtasks: Vec<&str> = vec![];

        let prompt = queen_manager_prompt(task, &subtasks);

        assert!(prompt.contains("Simple task"));
        assert!(prompt.contains("Identified Sub-tasks\n\n"));
    }

    #[test]
    fn test_queen_manager_prompt_includes_skills() {
        let prompt = queen_manager_prompt("Test task", &[]);

        assert!(prompt.contains("EXECUTION PATTERNS (SKILLS)"));
        assert!(prompt.contains("/carousel Pattern"));
        assert!(prompt.contains("/ralph Pattern"));
        assert!(prompt.contains("Direct Task Spawning"));
    }

    #[test]
    fn test_queen_manager_prompt_includes_worker_types() {
        let prompt = queen_manager_prompt("Test task", &[]);

        assert!(prompt.contains("rust-implementer"));
        assert!(prompt.contains("implementer"));
        assert!(prompt.contains("research-agent"));
        assert!(prompt.contains("rust-expert"));
        assert!(prompt.contains("Explore"));
    }

    #[test]
    fn test_queen_manager_prompt_forbids_implementation() {
        let prompt = queen_manager_prompt("Test task", &[]);

        assert!(prompt.contains("NEVER write code yourself"));
        assert!(prompt.contains("NEVER read files for implementation"));
        assert!(prompt.contains("always delegate to worker agents"));
    }

    #[test]
    fn test_queen_sergeant_prompt_is_deprecated() {
        // Should still work but call queen_manager_prompt
        let task = "Test task";
        let subtasks = vec!["subtask1"];

        #[allow(deprecated)]
        let prompt = queen_sergeant_prompt(task, &subtasks);

        assert!(prompt.contains("Test task"));
        assert!(prompt.contains("MANAGER"));
    }

    #[test]
    fn test_swarm_host_coordinator_prompt_contains_queen_list() {
        let queens = vec!["rust-implementer", "research-agent", "tester"];

        let prompt = swarm_host_coordinator_prompt(5, &queens);

        assert!(prompt.contains("Total tasks: 5"));
        assert!(prompt.contains("Q0: rust-implementer"));
        assert!(prompt.contains("Q1: research-agent"));
        assert!(prompt.contains("Q2: tester"));
        assert!(prompt.contains("Assign tasks to Queens"));
        assert!(prompt.contains("validate before merging"));
    }

    #[test]
    fn test_swarm_host_coordinator_prompt_with_no_queens() {
        let queens: Vec<&str> = vec![];

        let prompt = swarm_host_coordinator_prompt(10, &queens);

        assert!(prompt.contains("Total tasks: 10"));
        assert!(prompt.contains("Available Queens:\n\n"));
    }

    #[test]
    fn test_brood_lord_strategist_prompt_contains_master_goal() {
        let goal = "Build a distributed trading system with 5 exchanges";

        let prompt = brood_lord_strategist_prompt(goal);

        assert!(prompt.contains("Build a distributed trading system with 5 exchanges"));
        assert!(prompt.contains("Decompose the master goal"));
        assert!(prompt.contains("SwarmHost"));
        assert!(prompt.contains("cross-project dependencies"));
        assert!(prompt.contains("Escalate to operator"));
    }

    #[test]
    fn test_validator_reviewer_prompt_contains_files() {
        let task = "Add REST API endpoints";
        let files = vec!["src/api/mod.rs", "src/api/routes.rs", "tests/api_test.rs"];

        let prompt = validator_reviewer_prompt(task, &files);

        assert!(prompt.contains("Add REST API endpoints"));
        assert!(prompt.contains("src/api/mod.rs"));
        assert!(prompt.contains("src/api/routes.rs"));
        assert!(prompt.contains("tests/api_test.rs"));
        assert!(prompt.contains("Review Criteria"));
        assert!(prompt.contains(r#""passed": true/false"#));
    }

    #[test]
    fn test_validator_reviewer_prompt_with_no_files() {
        let task = "Documentation task";
        let files: Vec<&str> = vec![];

        let prompt = validator_reviewer_prompt(task, &files);

        assert!(prompt.contains("Documentation task"));
        assert!(prompt.contains("Files Changed\n  - \n"));
    }

    #[test]
    fn test_default_system_prompt_for_known_roles() {
        assert!(default_system_prompt("queen").contains("manager"));
        assert!(default_system_prompt("queen").contains("Never do implementation work"));
        assert!(default_system_prompt("native_queen").contains("manager"));
        assert!(default_system_prompt("swarm_host").contains("SwarmHost"));
        assert!(default_system_prompt("coordinator").contains("SwarmHost"));
        assert!(default_system_prompt("brood_lord").contains("BroodLord"));
        assert!(default_system_prompt("strategist").contains("BroodLord"));
        assert!(default_system_prompt("validator").contains("code reviewer"));
        assert!(default_system_prompt("reviewer").contains("code reviewer"));
    }

    #[test]
    fn test_default_system_prompt_for_unknown_role() {
        let prompt = default_system_prompt("unknown_role");
        assert_eq!(prompt, "You are an AI agent in the Hatchery swarm system.");
    }

    #[test]
    fn test_queen_prompt_includes_communication_section() {
        let prompt = queen_manager_prompt("Test task", &[]);

        assert!(prompt.contains("YOUR ROLE"));
        assert!(prompt.contains("MANAGER"));
        assert!(prompt.contains("Share important discoveries in your output"));
        assert!(prompt.contains("they will be captured by SwarmHost"));
    }

    #[test]
    fn test_swarm_host_prompt_includes_decision_framework() {
        let prompt = swarm_host_coordinator_prompt(3, &["q1"]);

        assert!(prompt.contains("Decision Framework"));
        assert!(prompt.contains("experienced Queens"));
        assert!(prompt.contains("blocked"));
        assert!(prompt.contains("validate before merging"));
    }

    #[test]
    fn test_brood_lord_prompt_includes_responsibilities() {
        let prompt = brood_lord_strategist_prompt("Master goal");

        assert!(prompt.contains("Your Responsibilities"));
        assert!(prompt.contains("Decompose the master goal"));
        assert!(prompt.contains("Monitor cross-project dependencies"));
        assert!(prompt.contains("Handle escalations"));
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
    fn test_swarm_host_prompt_includes_discipline() {
        let prompt = swarm_host_coordinator_prompt(5, &["q1", "q2"]);
        assert!(prompt.contains("Orchestration Discipline"));
        assert!(prompt.contains("context compression"));
        assert!(prompt.contains("TaskDag"));
        assert!(prompt.contains("validate before merging"));
    }

    #[test]
    fn test_brood_lord_prompt_includes_discipline() {
        let prompt = brood_lord_strategist_prompt("Build system");
        assert!(prompt.contains("Orchestration Discipline"));
        assert!(prompt.contains("context compression"));
        assert!(prompt.contains("SwarmHost statuses"));
        assert!(prompt.contains("GlobalMemory"));
    }

    #[test]
    fn test_swarm_host_discipline_block_exists() {
        let block = swarm_host_discipline_block();
        assert!(block.contains("SwarmHost Orchestration Discipline"));
        assert!(block.contains("TaskDag"));
        assert!(block.contains("validate before merging"));
    }

    #[test]
    fn test_brood_lord_discipline_block_exists() {
        let block = brood_lord_discipline_block();
        assert!(block.contains("BroodLord Orchestration Discipline"));
        assert!(block.contains("SwarmHost statuses"));
        assert!(block.contains("GlobalMemory"));
    }

    #[test]
    fn test_no_hatchery_protocol_in_any_prompt() {
        assert!(!queen_manager_prompt("task", &[]).contains("@hatchery:"));
        assert!(!swarm_host_coordinator_prompt(5, &[]).contains("@hatchery:"));
        assert!(!brood_lord_strategist_prompt("goal").contains("@hatchery:"));
        assert!(!validator_reviewer_prompt("task", &[]).contains("@hatchery:"));
        assert!(!orchestration_discipline_block().contains("@hatchery:"));
        assert!(!swarm_host_discipline_block().contains("@hatchery:"));
        assert!(!brood_lord_discipline_block().contains("@hatchery:"));
    }
}
