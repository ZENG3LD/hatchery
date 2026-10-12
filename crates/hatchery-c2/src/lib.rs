//! Headless observe-only aggregation daemon for local Gate4Agent nodes.

#[cfg(any(windows, unix))]
mod runtime;

#[cfg(any(windows, unix))]
pub use runtime::{
    default_c2_control_endpoint, C2Config, C2ConfigError, C2Error, C2NodeConfig, C2Running,
    C2ShutdownHandle, C2Timings, DEFAULT_C2_CONTROL_ENDPOINT,
};

pub use hatchery_c2_protocol as protocol;

/// Node enrollment registry (phase P1 of the enrollment and fleet plan): signed
/// enrollment requests, a neutral first status, operator-driven status changes
/// and key/signature node authentication. Not yet wired into the relay runtime.
pub mod enrollment;

/// Runtime side of enrollment (listener, operator endpoints, peer provisioning, signed call-home).
#[cfg(any(windows, unix))]
pub mod enroll_service;
