//! Pre-built pipeline configurations for common workflow patterns.
//!
//! This module provides preset pipeline configurations using default settings for all components.
//! Users can customize these by calling additional configuration methods after instantiation.

use super::PipelineBuilder;
use crate::communication::*;
use crate::decomposition::*;
use crate::memory::*;
use crate::resilience::*;
use crate::scaling::*;
use crate::scheduling::*;
use crate::topology::*;

/// Carousel pattern: centralized, DAG decomposition, message bus, multi-tier memory.
///
/// Best for structured multi-phase workflows (research → implement → test → debug).
pub fn carousel_preset() -> PipelineBuilder {
    PipelineBuilder::new()
        .name("carousel")
        .description("Structured multi-phase workflow with DAG decomposition")
        .topology(CentralizedTopology::new(CentralizedConfig::default()))
        .decomposition(DagDecomposition::new(DagDecompositionConfig::default()))
        .communication(MessageBusCommunication::new(MessageBusConfig::default()))
        .memory(MultiTierMemory::new())
        .scheduling(HybridScheduling::new(HybridSchedulingConfig::default()))
        .resilience(RetryResilience::new(RetryConfig::default()))
        .scaling(SmallScaleScaling::new(SmallScaleConfig::default())
            .expect("Failed to create SmallScaleScaling"))
}

/// Ralph pattern: centralized, emergent decomposition, message bus.
///
/// Best for autonomous iterative tasks with PRD checkboxes.
pub fn ralph_preset() -> PipelineBuilder {
    PipelineBuilder::new()
        .name("ralph")
        .description("Autonomous iterative tasks with emergent decomposition")
        .topology(CentralizedTopology::new(CentralizedConfig::default()))
        .decomposition(EmergentDecomposition::new(EmergentConfig::default()))
        .communication(MessageBusCommunication::new(MessageBusConfig::default()))
        .memory(ConversationMemory::new())
        .scheduling(EventDrivenScheduling::new(EventDrivenConfig::default()))
        .resilience(RetryResilience::new(RetryConfig::default()))
        .scaling(SmallScaleScaling::new(SmallScaleConfig::default())
            .expect("Failed to create SmallScaleScaling"))
}

/// Blackboard pattern: blackboard topology, capability decomposition.
///
/// Best for exploratory research tasks where agents self-select based on expertise.
pub fn blackboard_preset() -> PipelineBuilder {
    PipelineBuilder::new()
        .name("blackboard")
        .description("Exploratory research with capability-based decomposition")
        .topology(BlackboardTopology::new(BlackboardConfig::default()))
        .decomposition(CapabilityDecomposition::new(CapabilityConfig::default()))
        .communication(BlackboardCommunication::new(BlackboardCommunicationConfig::default()))
        .memory(DocumentMemory::new())
        .scheduling(EventDrivenScheduling::new(EventDrivenConfig::default()))
        .resilience(RecoveryResilience::new(RecoveryConfig::default()))
        .scaling(MediumScaleScaling::new(MediumScaleConfig::default())
            .expect("Failed to create MediumScaleScaling"))
}

/// Consensus pattern: conversational topology, broadcast communication.
///
/// Best for high-stakes critical decisions requiring multi-agent agreement.
pub fn consensus_preset() -> PipelineBuilder {
    PipelineBuilder::new()
        .name("consensus")
        .description("Multi-agent consensus for critical decisions")
        .topology(ConversationalTopology::new(ConversationalConfig::default()))
        .decomposition(RoleBasedDecomposition::new(RoleBasedConfig::default()))
        .communication(BroadcastCommunication::new(BroadcastConfig::default()))
        .memory(CollaborativeMemory::new())
        .scheduling(TimerBasedScheduling::new(TimerBasedConfig::default()))
        .resilience(ConsensusResilience::new(ConsensusConfig::default()))
        .scaling(SmallScaleScaling::new(SmallScaleConfig::default())
            .expect("Failed to create SmallScaleScaling"))
}

/// Swarm pattern: peer-to-peer topology, work stealing scheduler.
///
/// Best for embarrassingly parallel tasks (e.g., batch processing, data pipeline).
pub fn swarm_preset() -> PipelineBuilder {
    PipelineBuilder::new()
        .name("swarm")
        .description("Parallel task execution with work stealing")
        .topology(PeerToPeerTopology::new(PeerToPeerConfig::default()))
        .decomposition(DagDecomposition::new(DagDecompositionConfig::default()))
        .communication(DirectCommunication::new(DirectConfig::default()))
        .memory(IsolatedMemory::new())
        .scheduling(WorkStealingScheduling::new(WorkStealingConfig::default()))
        .resilience(CircuitBreakerResilience::new(CircuitBreakerConfig::default()))
        .scaling(MediumScaleScaling::new(MediumScaleConfig::default())
            .expect("Failed to create MediumScaleScaling"))
}

/// Minimal pattern: centralized, event-driven, small scale.
///
/// Best for simple single-agent or small-team tasks.
pub fn minimal_preset() -> PipelineBuilder {
    PipelineBuilder::new()
        .name("minimal")
        .description("Minimal pipeline for simple tasks")
        .topology(CentralizedTopology::new(CentralizedConfig::default()))
        .decomposition(DagDecomposition::new(DagDecompositionConfig::default()))
        .communication(DirectCommunication::new(DirectConfig::default()))
        .memory(SharedStateMemory::new())
        .scheduling(EventDrivenScheduling::new(EventDrivenConfig::default()))
        .resilience(RetryResilience::new(RetryConfig::default()))
        .scaling(SmallScaleScaling::new(SmallScaleConfig::default())
            .expect("Failed to create SmallScaleScaling"))
}
