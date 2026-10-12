use super::common::*;
use super::{Parsed, ProviderParser};
use crate::event::PromptKind;

pub struct GrokParser;

const PROFILE: Profile = Profile {
    answer_markers: &['●', '•'],
    thought_markers: &[],
    result_markers: &['⎿', '└'],
    tool_verbs: &["Ran", "Edited", "Read"],
    working_needles: &["Waiting for approval"],
    banner_needles: &["Welcome to Grok"],
    prompts: &[("Approve in your browser", PromptKind::Login, true)],
};

impl ProviderParser for GrokParser {
    fn provider(&self) -> &'static str { "grok" }
    fn parse(&mut self, rows: &[String]) -> Parsed { scan(rows, &PROFILE) }
}
