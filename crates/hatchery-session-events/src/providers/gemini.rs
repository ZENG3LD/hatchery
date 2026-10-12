use super::common::*;
use super::{Parsed, ProviderParser};
use crate::event::PromptKind;

pub struct GeminiParser;

const PROFILE: Profile = Profile {
    answer_markers: &['✦'],
    thought_markers: &[],
    result_markers: &['⎿', '└'],
    tool_verbs: &["Ran", "Edited", "Read"],
    working_needles: &["esc to cancel"],
    banner_needles: &["Gemini CLI v"],
    prompts: &[("How would you like to authenticate", PromptKind::Login, true), ("Trust folder", PromptKind::Trust, true), ("Allow execution", PromptKind::Permission, true), ("Yes, allow once", PromptKind::Permission, true)],
};

impl ProviderParser for GeminiParser {
    fn provider(&self) -> &'static str { "gemini" }
    fn parse(&mut self, rows: &[String]) -> Parsed { scan(rows, &PROFILE) }
}
