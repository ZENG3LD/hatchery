//! Host for hatchery subservices. See `deploy/README.md`.

pub mod config;
pub mod control;
pub mod error;
#[cfg(feature = "harness")]
pub mod harness;
#[cfg(feature = "m4a")]
pub mod m4a;
#[cfg(feature = "memory")]
pub mod memory;
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
    #[cfg(feature = "m4a")]
    if config.m4a_enabled {
        services.push(Box::new(m4a::M4aSubservice::new(
            &config.data_dir,
            config.m4a_master_key_var.clone(),
        )));
    }
    #[cfg(feature = "memory")]
    services.push(Box::new(memory::MemorySubservice::new(
        config.memory_bin.clone(),
        &config.data_dir,
    )));
    let _ = config;
    services
}
