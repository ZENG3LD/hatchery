//! Pipeline builder for composing orchestration strategies.

use crate::communication::Communication;
use crate::decomposition::Decomposition;
use crate::memory::Memory;
use crate::resilience::Resilience;
use crate::scaling::Scaling;
use crate::scheduling::Scheduling;
use crate::topology::Topology;
use anyhow::{anyhow, Result};

/// Builder for constructing a pipeline with all orchestration components.
pub struct PipelineBuilder {
    topology: Option<Box<dyn Topology>>,
    decomposition: Option<Box<dyn Decomposition>>,
    communication: Option<Box<dyn Communication>>,
    memory: Option<Box<dyn Memory>>,
    scheduling: Option<Box<dyn Scheduling>>,
    resilience: Option<Box<dyn Resilience>>,
    scaling: Option<Box<dyn Scaling>>,
    name: Option<String>,
    description: Option<String>,
}

impl PipelineBuilder {
    /// Create a new empty pipeline builder.
    pub fn new() -> Self {
        PipelineBuilder {
            topology: None,
            decomposition: None,
            communication: None,
            memory: None,
            scheduling: None,
            resilience: None,
            scaling: None,
            name: None,
            description: None,
        }
    }

    /// Set the topology component.
    pub fn topology(mut self, t: impl Topology + 'static) -> Self {
        self.topology = Some(Box::new(t));
        self
    }

    /// Set the decomposition component.
    pub fn decomposition(mut self, d: impl Decomposition + 'static) -> Self {
        self.decomposition = Some(Box::new(d));
        self
    }

    /// Set the communication component.
    pub fn communication(mut self, c: impl Communication + 'static) -> Self {
        self.communication = Some(Box::new(c));
        self
    }

    /// Set the memory component.
    pub fn memory(mut self, m: impl Memory + 'static) -> Self {
        self.memory = Some(Box::new(m));
        self
    }

    /// Set the scheduling component.
    pub fn scheduling(mut self, s: impl Scheduling + 'static) -> Self {
        self.scheduling = Some(Box::new(s));
        self
    }

    /// Set the resilience component.
    pub fn resilience(mut self, r: impl Resilience + 'static) -> Self {
        self.resilience = Some(Box::new(r));
        self
    }

    /// Set the scaling component.
    pub fn scaling(mut self, s: impl Scaling + 'static) -> Self {
        self.scaling = Some(Box::new(s));
        self
    }

    /// Set the pipeline name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the pipeline description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Build the pipeline, ensuring all required components are present.
    pub fn build(self) -> Result<Pipeline> {
        let topology = self
            .topology
            .ok_or_else(|| anyhow!("Topology component is required"))?;
        let decomposition = self
            .decomposition
            .ok_or_else(|| anyhow!("Decomposition component is required"))?;
        let communication = self
            .communication
            .ok_or_else(|| anyhow!("Communication component is required"))?;
        let memory = self
            .memory
            .ok_or_else(|| anyhow!("Memory component is required"))?;
        let scheduling = self
            .scheduling
            .ok_or_else(|| anyhow!("Scheduling component is required"))?;
        let resilience = self
            .resilience
            .ok_or_else(|| anyhow!("Resilience component is required"))?;
        let scaling = self
            .scaling
            .ok_or_else(|| anyhow!("Scaling component is required"))?;

        Ok(Pipeline {
            topology,
            decomposition,
            communication,
            memory,
            scheduling,
            resilience,
            scaling,
            name: self.name.unwrap_or_else(|| "unnamed".to_string()),
            description: self.description.unwrap_or_else(|| String::new()),
        })
    }

    /// Build the pipeline with default components for any missing pieces.
    pub fn build_partial(self) -> Result<Pipeline> {
        use crate::communication::DirectCommunication;
        use crate::decomposition::DagDecomposition;
        use crate::memory::SharedStateMemory;
        use crate::resilience::RetryResilience;
        use crate::scaling::SmallScaleScaling;
        use crate::scheduling::EventDrivenScheduling;
        use crate::topology::CentralizedTopology;

        let topology = self.topology.unwrap_or_else(|| {
            Box::new(CentralizedTopology::new(
                crate::topology::CentralizedConfig::default(),
            ))
        });

        let decomposition = self.decomposition.unwrap_or_else(|| {
            Box::new(DagDecomposition::new(
                crate::decomposition::DagDecompositionConfig::default(),
            ))
        });

        let communication = self.communication.unwrap_or_else(|| {
            Box::new(DirectCommunication::new(
                crate::communication::DirectConfig::default(),
            ))
        });

        let memory = self.memory.unwrap_or_else(|| {
            Box::new(SharedStateMemory::new())
        });

        let scheduling = self.scheduling.unwrap_or_else(|| {
            Box::new(EventDrivenScheduling::new(
                crate::scheduling::EventDrivenConfig::default(),
            ))
        });

        let resilience = self.resilience.unwrap_or_else(|| {
            Box::new(RetryResilience::new(
                crate::resilience::RetryConfig::default(),
            ))
        });

        let scaling = self.scaling.unwrap_or_else(|| {
            Box::new(SmallScaleScaling::new(
                crate::scaling::SmallScaleConfig::default(),
            ).expect("Failed to create default SmallScaleScaling"))
        });

        Ok(Pipeline {
            topology,
            decomposition,
            communication,
            memory,
            scheduling,
            resilience,
            scaling,
            name: self.name.unwrap_or_else(|| "unnamed".to_string()),
            description: self.description.unwrap_or_else(|| String::new()),
        })
    }
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// A complete pipeline with all orchestration components.
pub struct Pipeline {
    pub topology: Box<dyn Topology>,
    pub decomposition: Box<dyn Decomposition>,
    pub communication: Box<dyn Communication>,
    pub memory: Box<dyn Memory>,
    pub scheduling: Box<dyn Scheduling>,
    pub resilience: Box<dyn Resilience>,
    pub scaling: Box<dyn Scaling>,
    pub name: String,
    pub description: String,
}

impl Pipeline {
    /// Get the names of all components in this pipeline.
    pub fn component_names(&self) -> PipelineComponents {
        PipelineComponents {
            topology: std::any::type_name_of_val(&*self.topology)
                .split("::")
                .last()
                .unwrap_or("Unknown")
                .to_string(),
            decomposition: std::any::type_name_of_val(&*self.decomposition)
                .split("::")
                .last()
                .unwrap_or("Unknown")
                .to_string(),
            communication: std::any::type_name_of_val(&*self.communication)
                .split("::")
                .last()
                .unwrap_or("Unknown")
                .to_string(),
            memory: std::any::type_name_of_val(&*self.memory)
                .split("::")
                .last()
                .unwrap_or("Unknown")
                .to_string(),
            scheduling: std::any::type_name_of_val(&*self.scheduling)
                .split("::")
                .last()
                .unwrap_or("Unknown")
                .to_string(),
            resilience: std::any::type_name_of_val(&*self.resilience)
                .split("::")
                .last()
                .unwrap_or("Unknown")
                .to_string(),
            scaling: std::any::type_name_of_val(&*self.scaling)
                .split("::")
                .last()
                .unwrap_or("Unknown")
                .to_string(),
        }
    }

    /// Hot-swap the topology component at runtime.
    pub fn swap_topology(&mut self, t: Box<dyn Topology>) -> Box<dyn Topology> {
        std::mem::replace(&mut self.topology, t)
    }

    /// Hot-swap the decomposition component at runtime.
    pub fn swap_decomposition(&mut self, d: Box<dyn Decomposition>) -> Box<dyn Decomposition> {
        std::mem::replace(&mut self.decomposition, d)
    }

    /// Hot-swap the communication component at runtime.
    pub fn swap_communication(&mut self, c: Box<dyn Communication>) -> Box<dyn Communication> {
        std::mem::replace(&mut self.communication, c)
    }

    /// Hot-swap the memory component at runtime.
    pub fn swap_memory(&mut self, m: Box<dyn Memory>) -> Box<dyn Memory> {
        std::mem::replace(&mut self.memory, m)
    }

    /// Hot-swap the scheduling component at runtime.
    pub fn swap_scheduling(&mut self, s: Box<dyn Scheduling>) -> Box<dyn Scheduling> {
        std::mem::replace(&mut self.scheduling, s)
    }

    /// Hot-swap the resilience component at runtime.
    pub fn swap_resilience(&mut self, r: Box<dyn Resilience>) -> Box<dyn Resilience> {
        std::mem::replace(&mut self.resilience, r)
    }

    /// Hot-swap the scaling component at runtime.
    pub fn swap_scaling(&mut self, s: Box<dyn Scaling>) -> Box<dyn Scaling> {
        std::mem::replace(&mut self.scaling, s)
    }
}

/// Component names for a pipeline.
#[derive(Debug, Clone)]
pub struct PipelineComponents {
    pub topology: String,
    pub decomposition: String,
    pub communication: String,
    pub memory: String,
    pub scheduling: String,
    pub resilience: String,
    pub scaling: String,
}

impl PipelineComponents {
    /// Pretty-print the components.
    pub fn display(&self) -> String {
        format!(
            "Pipeline Components:\n  Topology: {}\n  Decomposition: {}\n  Communication: {}\n  Memory: {}\n  Scheduling: {}\n  Resilience: {}\n  Scaling: {}",
            self.topology,
            self.decomposition,
            self.communication,
            self.memory,
            self.scheduling,
            self.resilience,
            self.scaling
        )
    }
}
