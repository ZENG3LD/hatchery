use super::common::*;
use super::{Parsed, ProviderParser};
use crate::event::PromptKind;

pub struct CodexParser;

const PROFILE: Profile = Profile {
    answer_markers: &['•'],
    thought_markers: &[],
    result_markers: &['└', '⎿'],
    tool_verbs: &["Ran", "Edited", "Explored", "Read", "Searched", "Added", "Updated", "Deleted", "Called"],
    working_needles: &["esc to interrupt"],
    banner_needles: &["Welcome to Codex"],
    prompts: &[("Sign in with ChatGPT", PromptKind::Login, true), ("Paste or type your API key", PromptKind::ApiKey, true), ("trust the contents of this directory", PromptKind::Trust, true), ("Allow Codex to work in this folder", PromptKind::Trust, true), ("Would you like to run the following command", PromptKind::Permission, true)],
};

impl ProviderParser for CodexParser {
    fn provider(&self) -> &'static str { "codex" }
    fn parse(&mut self, rows: &[String]) -> Parsed { scan(rows, &PROFILE) }
}
