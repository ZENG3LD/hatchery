use super::common::*;
use super::{Parsed, ProviderParser};
use crate::event::PromptKind;

pub struct KimiParser;

const PROFILE: Profile = Profile {
    answer_markers: &['•', '●'],
    thought_markers: &[],
    result_markers: &['⎿', '└'],
    tool_verbs: &["Ran", "Edited", "Read"],
    working_needles: &["esc to interrupt"],
    banner_needles: &["Welcome to Kimi"],
    prompts: &[("Trust this folder", PromptKind::Trust, true), ("Run /login", PromptKind::Login, false)],
};

impl ProviderParser for KimiParser {
    fn provider(&self) -> &'static str { "kimi" }
    fn parse(&mut self, rows: &[String]) -> Parsed { scan(rows, &PROFILE) }
}
