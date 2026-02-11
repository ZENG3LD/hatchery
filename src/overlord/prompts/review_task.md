## Review Request

**Queen**: {queen_id}
**Task**: {task_id}
**Branch**: {branch_name}
**Worktree**: {worktree_path}

## Task Description

{task_description}

{verify_section}
## Instructions

1. Run `git diff main..HEAD` in the worktree path to see only the Queen's changes vs main
2. Run `cargo check --workspace` to verify compilation
3. **Run the verification command** (if specified) to validate task completion
4. Review the diff against the task description — does it actually implement what was requested?
5. Respond with your verdict

## Approval Criteria

- Code MUST compile (cargo check passes)
- **Verification command MUST pass** (if specified)
- Changes must match the task description — stub code or placeholder implementations are NOT acceptable
- Code must follow existing patterns and be correct

## Response Format

You MUST end your response with exactly one of:

VERDICT: APPROVE
<summary>Brief description of changes</summary>

OR

VERDICT: REJECT
<reason>What's wrong and needs fixing</reason>
