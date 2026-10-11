//! Host for hatchery subservices. See `deploy/README.md`.

pub mod config;
pub mod control;
pub mod error;
#[cfg(feature = "harness")]
pub mod harness;
pub mod subservice;
pub mod supervisor;

pub use config::ServerConfig;
pub use error::ServerError;
pub use subservice::{State, Status, Subservice};
pub use supervisor::Supervisor;

/// Build the subservices enabled by features and configuration.
pub fn build_subservices(config: &ServerConfig) -> Vec<Box<dyn Subservice>> {
    #[allow(unused_mut)]
    let mut services: Vec<Box<dyn Subservice>> = Vec::new();
    #[cfg(feature = "harness")]
    if config.harness_enabled {
        if let Some(bin) = config.harness_bin.clone() {
            services.push(Box::new(harness::HarnessSubservice::new(bin, config.harness_args.clone())));
        }
    }
    let _ = config;
    services
}
