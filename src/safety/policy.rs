//! Safe-mode policy — prompt-level enforcement of command restrictions.

/// The safe-mode restrictions prompt, embedded at compile time.
const SAFE_MODE_PROMPT: &str = include_str!("prompts/safe_mode.md");

/// Return the safe-mode restrictions text for injection into worker prompts.
pub fn safe_mode_prompt() -> &'static str {
    SAFE_MODE_PROMPT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_mode_prompt_not_empty() {
        let prompt = safe_mode_prompt();
        assert!(!prompt.is_empty());
        assert!(prompt.contains("FORBIDDEN"));
        assert!(prompt.contains("rm -rf"));
        assert!(prompt.contains("git push"));
        assert!(prompt.contains("sudo"));
    }

    #[test]
    fn test_safe_mode_prompt_contains_allowed() {
        let prompt = safe_mode_prompt();
        assert!(prompt.contains("cargo"));
        assert!(prompt.contains("git status"));
        assert!(prompt.contains("git commit"));
    }
}
