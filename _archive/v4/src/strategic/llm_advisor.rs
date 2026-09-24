//! LLM-powered strategic advisor — wraps the Overmind concept.

use super::{StrategicAdvisor, StrategicCommand, StrategicEvent};
use anyhow::Result;
use async_trait::async_trait;

/// LLM-powered strategic advisor configuration.
#[derive(Debug, Clone)]
pub struct LlmAdvisorConfig {
    /// Model to use for strategic decisions.
    pub model: String,
    /// Max cost per decision in USD.
    pub max_cost_per_decision: f64,
    /// Whether to fall back to rule-based on error.
    pub fallback_to_rules: bool,
}

impl Default for LlmAdvisorConfig {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-5-20250929".to_string(),
            max_cost_per_decision: 2.0,
            fallback_to_rules: true,
        }
    }
}

/// LLM-powered strategic advisor using Claude for complex decisions.
///
/// Falls back to rule-based decisions if LLM is unavailable or too expensive.
pub struct LlmAdvisor {
    config: LlmAdvisorConfig,
    fallback: super::rule_based::RuleBasedAdvisor,
    total_cost: f64,
}

impl LlmAdvisor {
    pub fn new(config: LlmAdvisorConfig) -> Self {
        Self {
            config,
            fallback: super::rule_based::RuleBasedAdvisor::new(
                super::rule_based::RuleBasedConfig::default(),
            ),
            total_cost: 0.0,
        }
    }

    pub fn total_cost(&self) -> f64 {
        self.total_cost
    }
}

#[async_trait]
impl StrategicAdvisor for LlmAdvisor {
    async fn advise(&mut self, event: StrategicEvent) -> Result<StrategicCommand> {
        // For now, fall back to rule-based decisions.
        // Full LLM integration will connect to the existing Overmind
        // spawn logic via overmind::spawn_overmind.
        //
        // The LLM advisor would:
        // 1. Format the event + context into a prompt
        // 2. Send to Claude via the agent backend
        // 3. Parse the structured response into a StrategicCommand
        // 4. Track cost
        //
        // This keeps the trait contract clean while the real implementation
        // connects to the existing Overmind infrastructure.
        self.fallback.advise(event).await
    }

    fn name(&self) -> &str {
        "llm-advisor"
    }

    fn uses_llm(&self) -> bool {
        true
    }
}
