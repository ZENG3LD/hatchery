use super::common::*;
use super::{Parsed, ProviderParser};
use crate::event::PromptKind;

pub struct ClaudeParser;

const PROFILE: Profile = Profile {
    answer_markers: &['●', '⏺'],
    thought_markers: &['∴'],
    result_markers: &['⎿'],
    tool_verbs: &[],
    working_needles: &["esc to interrupt"],
    banner_needles: &["Welcome to Claude Code"],
    prompts: &[("Select login method", PromptKind::Login, true), ("Paste code here", PromptKind::Login, true), ("trust the files in this folder", PromptKind::Trust, true), ("No, and tell Claude what to do differently", PromptKind::Permission, true), ("Choose the text style", PromptKind::Menu, true)],
};

impl ProviderParser for ClaudeParser {
    fn provider(&self) -> &'static str { "claude" }
    fn parse(&mut self, rows: &[String]) -> Parsed { scan(rows, &PROFILE) }
}
