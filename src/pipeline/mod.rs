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
//! ## Example
//! ```rust
//! use hatchery::pipeline::presets::carousel_preset;
//!
//! let pipeline = carousel_preset()
//!     .name("my-workflow")
//!     .build()
//!     .expect("Failed to build pipeline");
//! ```

mod builder;
mod presets;
mod runtime;

pub use builder::{Pipeline, PipelineBuilder, PipelineComponents};
pub use presets::{
    blackboard_preset, carousel_preset, consensus_preset, minimal_preset, ralph_preset,
    swarm_preset,
};
pub use runtime::{PipelineRuntime, RuntimeStats, RuntimeStatus};
