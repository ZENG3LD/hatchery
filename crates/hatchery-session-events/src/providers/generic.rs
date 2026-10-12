use super::common::*;
use super::{Parsed, ProviderParser};

pub struct GenericParser;

const PROFILE: Profile = Profile {
    answer_markers: &['●', '⏺', '•'],
    thought_markers: &['∴'],
    result_markers: &['⎿', '└'],
    tool_verbs: &["Ran", "Edited", "Read"],
    working_needles: &["esc to interrupt"],
    banner_needles: &["Welcome to "],
    prompts: &[],
};

impl ProviderParser for GenericParser {
    fn provider(&self) -> &'static str { "generic" }
    fn parse(&mut self, rows: &[String]) -> Parsed { scan(rows, &PROFILE) }
}
