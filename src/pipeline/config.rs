//! TOML-based pipeline configuration.
//!
//! This module provides declarative pipeline configuration via TOML files.
//! Instead of writing Rust code to compose pipelines, users can define them
//! in `.toml` files and load them at runtime.
//!
//! ## Example
//! ```toml
//! [pipeline]
//! name = "my-workflow"
//! description = "Custom pipeline for code review"
//!
//! [topology]
//! type = "centralized"
//!
//! [decomposition]
//! type = "dag"
//!
//! [communication]
//! type = "message_bus"
//!
//! [memory]
//! type = "multi_tier"
//!
//! [scheduling]
//! type = "hybrid"
//!
//! [resilience]
//! type = "retry"
//!
//! [scaling]
//! type = "small"
//! ```

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::PipelineBuilder;
use crate::communication::*;
use crate::decomposition::*;
use crate::memory::*;
use crate::resilience::*;
use crate::scaling::*;
use crate::scheduling::*;
use crate::topology::*;

/// TOML-based pipeline configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    pub pipeline: PipelineMetadata,
    #[serde(default)]
    pub topology: ComponentConfig,
    #[serde(default)]
    pub decomposition: ComponentConfig,
    #[serde(default)]
    pub communication: ComponentConfig,
    #[serde(default)]
    pub memory: ComponentConfig,
    #[serde(default)]
    pub scheduling: ComponentConfig,
    #[serde(default)]
    pub resilience: ComponentConfig,
    #[serde(default)]
    pub scaling: ComponentConfig,
}

/// Pipeline metadata (name, description).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineMetadata {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// Component configuration (type + optional settings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentConfig {
    #[serde(rename = "type")]
    pub component_type: String,
    // Future: component-specific settings can be added as extra fields
}

impl Default for ComponentConfig {
    fn default() -> Self {
        Self {
            component_type: String::new(),
        }
    }
}

impl PipelineConfig {
    /// Load pipeline configuration from a TOML file.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let contents = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read config file: {:?}", path.as_ref()))?;
        Self::from_str(&contents)
    }

    /// Parse pipeline configuration from a TOML string.
    pub fn from_str(toml_str: &str) -> Result<Self> {
        toml::from_str(toml_str).context("Failed to parse TOML configuration")
    }

    /// Create a config from a preset name.
    pub fn from_preset(name: &str) -> Result<Self> {
        match name {
            "carousel" => Ok(PipelineConfig {
                pipeline: PipelineMetadata {
                    name: "carousel".to_string(),
                    description: "Structured multi-phase workflow with DAG decomposition"
                        .to_string(),
                },
                topology: ComponentConfig {
                    component_type: "centralized".to_string(),
                },
                decomposition: ComponentConfig {
                    component_type: "dag".to_string(),
                },
                communication: ComponentConfig {
                    component_type: "message_bus".to_string(),
                },
                memory: ComponentConfig {
                    component_type: "multi_tier".to_string(),
                },
                scheduling: ComponentConfig {
                    component_type: "hybrid".to_string(),
                },
                resilience: ComponentConfig {
                    component_type: "retry".to_string(),
                },
                scaling: ComponentConfig {
                    component_type: "small".to_string(),
                },
            }),
            "ralph" => Ok(PipelineConfig {
                pipeline: PipelineMetadata {
                    name: "ralph".to_string(),
                    description: "Autonomous iterative tasks with emergent decomposition"
                        .to_string(),
                },
                topology: ComponentConfig {
                    component_type: "centralized".to_string(),
                },
                decomposition: ComponentConfig {
                    component_type: "emergent".to_string(),
                },
                communication: ComponentConfig {
                    component_type: "message_bus".to_string(),
                },
                memory: ComponentConfig {
                    component_type: "conversation".to_string(),
                },
                scheduling: ComponentConfig {
                    component_type: "event_driven".to_string(),
                },
                resilience: ComponentConfig {
                    component_type: "retry".to_string(),
                },
                scaling: ComponentConfig {
                    component_type: "small".to_string(),
                },
            }),
            "blackboard" => Ok(PipelineConfig {
                pipeline: PipelineMetadata {
                    name: "blackboard".to_string(),
                    description: "Exploratory research with capability-based decomposition"
                        .to_string(),
                },
                topology: ComponentConfig {
                    component_type: "blackboard".to_string(),
                },
                decomposition: ComponentConfig {
                    component_type: "capability".to_string(),
                },
                communication: ComponentConfig {
                    component_type: "blackboard".to_string(),
                },
                memory: ComponentConfig {
                    component_type: "document".to_string(),
                },
                scheduling: ComponentConfig {
                    component_type: "event_driven".to_string(),
                },
                resilience: ComponentConfig {
                    component_type: "recovery".to_string(),
                },
                scaling: ComponentConfig {
                    component_type: "medium".to_string(),
                },
            }),
            "consensus" => Ok(PipelineConfig {
                pipeline: PipelineMetadata {
                    name: "consensus".to_string(),
                    description: "Multi-agent consensus for critical decisions".to_string(),
                },
                topology: ComponentConfig {
                    component_type: "conversational".to_string(),
                },
                decomposition: ComponentConfig {
                    component_type: "role_based".to_string(),
                },
                communication: ComponentConfig {
                    component_type: "broadcast".to_string(),
                },
                memory: ComponentConfig {
                    component_type: "collaborative".to_string(),
                },
                scheduling: ComponentConfig {
                    component_type: "timer_based".to_string(),
                },
                resilience: ComponentConfig {
                    component_type: "consensus".to_string(),
                },
                scaling: ComponentConfig {
                    component_type: "small".to_string(),
                },
            }),
            "swarm" => Ok(PipelineConfig {
                pipeline: PipelineMetadata {
                    name: "swarm".to_string(),
                    description: "Parallel task execution with work stealing".to_string(),
                },
                topology: ComponentConfig {
                    component_type: "peer_to_peer".to_string(),
                },
                decomposition: ComponentConfig {
                    component_type: "dag".to_string(),
                },
                communication: ComponentConfig {
                    component_type: "direct".to_string(),
                },
                memory: ComponentConfig {
                    component_type: "isolated".to_string(),
                },
                scheduling: ComponentConfig {
                    component_type: "work_stealing".to_string(),
                },
                resilience: ComponentConfig {
                    component_type: "circuit_breaker".to_string(),
                },
                scaling: ComponentConfig {
                    component_type: "medium".to_string(),
                },
            }),
            "minimal" => Ok(PipelineConfig {
                pipeline: PipelineMetadata {
                    name: "minimal".to_string(),
                    description: "Minimal pipeline for simple tasks".to_string(),
                },
                topology: ComponentConfig {
                    component_type: "centralized".to_string(),
                },
                decomposition: ComponentConfig {
                    component_type: "dag".to_string(),
                },
                communication: ComponentConfig {
                    component_type: "direct".to_string(),
                },
                memory: ComponentConfig {
                    component_type: "shared_state".to_string(),
                },
                scheduling: ComponentConfig {
                    component_type: "event_driven".to_string(),
                },
                resilience: ComponentConfig {
                    component_type: "retry".to_string(),
                },
                scaling: ComponentConfig {
                    component_type: "small".to_string(),
                },
            }),
            other => Err(anyhow!("Unknown preset: {}", other)),
        }
    }

    /// Convert this configuration into a `PipelineBuilder`.
    pub fn into_builder(self) -> Result<PipelineBuilder> {
        let mut builder = PipelineBuilder::new()
            .name(self.pipeline.name)
            .description(self.pipeline.description);

        // Topology
        builder = match self.topology.component_type.as_str() {
            "centralized" | "" => {
                builder.topology(CentralizedTopology::new(CentralizedConfig::default()))
            }
            "hierarchical" => {
                // HierarchicalTopology requires a root_coordinator AgentId
                // Use a default Nydus coordinator
                use crate::core::types::{AgentId, NydusId};
                builder.topology(HierarchicalTopology::new(
                    HierarchicalConfig::default(),
                    AgentId::Nydus(NydusId("root".to_string())),
                ))
            }
            "peer_to_peer" => {
                builder.topology(PeerToPeerTopology::new(PeerToPeerConfig::default()))
            }
            "blackboard" => {
                builder.topology(BlackboardTopology::new(BlackboardConfig::default()))
            }
            "graph_dag" => builder.topology(GraphDagTopology::new(GraphDagConfig::default())),
            "conversational" => builder.topology(ConversationalTopology::new(
                ConversationalConfig::default(),
            )),
            "hybrid" => {
                // HybridTopology requires a primary topology and selector function.
                // This cannot be constructed from simple config - use PipelineBuilder::topology() directly.
                return Err(anyhow!(
                    "HybridTopology requires custom construction. Use PipelineBuilder::topology() directly with HybridTopology::new()"
                ));
            }
            other => return Err(anyhow!("Unknown topology type: {}", other)),
        };

        // Decomposition
        builder = match self.decomposition.component_type.as_str() {
            "dag" | "" => {
                builder.decomposition(DagDecomposition::new(DagDecompositionConfig::default()))
            }
            "htn" => builder.decomposition(HtnDecomposition::new(HtnConfig::default())),
            "tdag" => builder.decomposition(TdagDecomposition::new(TdagConfig::default())),
            "emergent" => builder.decomposition(EmergentDecomposition::new(EmergentConfig::default())),
            "role_based" => {
                builder.decomposition(RoleBasedDecomposition::new(RoleBasedConfig::default()))
            }
            "capability" => {
                builder.decomposition(CapabilityDecomposition::new(CapabilityConfig::default()))
            }
            other => return Err(anyhow!("Unknown decomposition type: {}", other)),
        };

        // Communication
        builder = match self.communication.component_type.as_str() {
            "direct" | "" => {
                builder.communication(DirectCommunication::new(DirectConfig::default()))
            }
            "broadcast" => {
                builder.communication(BroadcastCommunication::new(BroadcastConfig::default()))
            }
            "blackboard" => builder.communication(BlackboardCommunication::new(
                BlackboardCommunicationConfig::default(),
            )),
            "message_bus" => {
                builder.communication(MessageBusCommunication::new(MessageBusConfig::default()))
            }
            "handoff" => builder.communication(HandoffCommunication::new(HandoffConfig::default())),
            "contract_net" => {
                builder.communication(ContractNetCommunication::new(ContractNetConfig::default()))
            }
            "ripple_effect" => builder.communication(RippleEffectCommunication::new(
                RippleEffectConfig::default(),
            )),
            other => return Err(anyhow!("Unknown communication type: {}", other)),
        };

        // Memory
        builder = match self.memory.component_type.as_str() {
            "shared_state" | "" => builder.memory(SharedStateMemory::new()),
            "conversation" => builder.memory(ConversationMemory::new()),
            "multi_tier" => builder.memory(MultiTierMemory::new()),
            "document" => builder.memory(DocumentMemory::new()),
            "isolated" => builder.memory(IsolatedMemory::new()),
            "session" => builder.memory(SessionMemory::new()),
            "collaborative" => builder.memory(CollaborativeMemory::new()),
            "ontology" => builder.memory(OntologyMemory::new()),
            "rag" => builder.memory(RagMemory::new()),
            other => return Err(anyhow!("Unknown memory type: {}", other)),
        };

        // Scheduling
        builder = match self.scheduling.component_type.as_str() {
            "event_driven" | "" => {
                builder.scheduling(EventDrivenScheduling::new(EventDrivenConfig::default()))
            }
            "timer_based" => {
                builder.scheduling(TimerBasedScheduling::new(TimerBasedConfig::default()))
            }
            "hybrid" => builder.scheduling(HybridScheduling::new(HybridSchedulingConfig::default())),
            "load_balancing" => {
                builder.scheduling(LoadBalancingScheduling::new(LoadBalancingConfig::default()))
            }
            "priority" => builder.scheduling(PriorityScheduling::new(PriorityConfig::default())),
            "llm_realtime" => {
                builder.scheduling(LlmRealtimeScheduling::new(LlmRealtimeConfig::default()))
            }
            "work_stealing" => {
                builder.scheduling(WorkStealingScheduling::new(WorkStealingConfig::default()))
            }
            other => return Err(anyhow!("Unknown scheduling type: {}", other)),
        };

        // Resilience
        builder = match self.resilience.component_type.as_str() {
            "retry" | "" => builder.resilience(RetryResilience::new(RetryConfig::default())),
            "recovery" => builder.resilience(RecoveryResilience::new(RecoveryConfig::default())),
            "degradation" => {
                builder.resilience(DegradationResilience::new(DegradationConfig::default()))
            }
            "consensus" => {
                builder.resilience(ConsensusResilience::new(ConsensusConfig::default()))
            }
            "hitl" => builder.resilience(HitlResilience::new(HitlConfig::default())),
            "circuit_breaker" => {
                builder.resilience(CircuitBreakerResilience::new(CircuitBreakerConfig::default()))
            }
            "failure_tracking" => builder.resilience(FailureTrackingResilience::new(
                FailureTrackingConfig::default(),
            )),
            other => return Err(anyhow!("Unknown resilience type: {}", other)),
        };

        // Scaling
        builder = match self.scaling.component_type.as_str() {
            "small" | "" => builder.scaling(
                SmallScaleScaling::new(SmallScaleConfig::default())
                    .context("Failed to create SmallScaleScaling")?,
            ),
            "medium" => builder.scaling(
                MediumScaleScaling::new(MediumScaleConfig::default())
                    .context("Failed to create MediumScaleScaling")?,
            ),
            "large" => builder.scaling(
                LargeScaleScaling::new(LargeScaleConfig::default())
                    .context("Failed to create LargeScaleScaling")?,
            ),
            "very_large" => builder.scaling(
                VeryLargeScaleScaling::new(VeryLargeScaleConfig::default())
                    .context("Failed to create VeryLargeScaleScaling")?,
            ),
            "edge_cloud" => builder.scaling(
                EdgeCloudScaling::new(EdgeCloudConfig::default())
                    .context("Failed to create EdgeCloudScaling")?,
            ),
            "elastic_pool" => builder.scaling(
                ElasticPoolScaling::new(ElasticPoolConfig::default())
                    .context("Failed to create ElasticPoolScaling")?,
            ),
            other => return Err(anyhow!("Unknown scaling type: {}", other)),
        };

        Ok(builder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_config() {
        let toml = r#"
[pipeline]
name = "test-pipeline"
description = "Test pipeline"

[topology]
type = "centralized"

[decomposition]
type = "dag"

[communication]
type = "message_bus"

[memory]
type = "multi_tier"

[scheduling]
type = "hybrid"

[resilience]
type = "retry"

[scaling]
type = "small"
"#;

        let config = PipelineConfig::from_str(toml).expect("Failed to parse config");
        assert_eq!(config.pipeline.name, "test-pipeline");
        assert_eq!(config.topology.component_type, "centralized");
        assert_eq!(config.decomposition.component_type, "dag");
    }

    #[test]
    fn test_preset_carousel() {
        let config = PipelineConfig::from_preset("carousel").expect("Failed to load preset");
        assert_eq!(config.pipeline.name, "carousel");
        assert_eq!(config.topology.component_type, "centralized");
        assert_eq!(config.decomposition.component_type, "dag");
    }

    #[test]
    fn test_into_builder() {
        let config = PipelineConfig::from_preset("minimal").expect("Failed to load preset");
        let builder = config.into_builder().expect("Failed to convert to builder");
        let pipeline = builder.build().expect("Failed to build pipeline");
        assert_eq!(pipeline.name, "minimal");
    }
}
