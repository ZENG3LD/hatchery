//! Pipeline module - composable orchestration workflows.
//!
//! This module provides a builder pattern for composing different orchestration strategies
//! into complete pipelines. Each pipeline combines:
//! - Topology: agent organization and task assignment
//! - Decomposition: task breakdown strategy
//! - Communication: message passing between agents
//! - Memory: state and knowledge storage
//! - Scheduling: task execution timing
//! - Resilience: failure handling and recovery
//! - Scaling: dynamic agent pool management
//!
//! ## Examples
//!
//! ### Using Presets
//! ```rust
//! use hatchery::pipeline::presets::carousel_preset;
//!
//! let pipeline = carousel_preset()
//!     .name("my-workflow")
//!     .build()
//!     .expect("Failed to build pipeline");
//! ```
//!
//! ### Using TOML Configuration
//! ```rust
//! use hatchery::pipeline::PipelineConfig;
//!
//! // Load from file
//! let config = PipelineConfig::from_file("pipeline.toml")
//!     .expect("Failed to load config");
//! let pipeline = config.into_builder()
//!     .expect("Failed to create builder")
//!     .build()
//!     .expect("Failed to build pipeline");
//!
//! // Or from preset name
//! let config = PipelineConfig::from_preset("carousel")
//!     .expect("Failed to load preset");
//! let pipeline = config.into_builder()
//!     .expect("Failed to create builder")
//!     .build()
//!     .expect("Failed to build pipeline");
//! ```

mod builder;
mod config;
mod orchestrator;
mod presets;
mod runtime;

pub use builder::{Pipeline, PipelineBuilder, PipelineComponents};
pub use config::{ComponentConfig, PipelineConfig, PipelineMetadata};
pub use orchestrator::{OrchestratorConfig, OrchestratorStats, PipelineOrchestrator};
pub use presets::{
    blackboard_preset, carousel_preset, consensus_preset, minimal_preset, ralph_preset,
    swarm_preset,
};
pub use runtime::{PipelineRuntime, RuntimeStats, RuntimeStatus};
