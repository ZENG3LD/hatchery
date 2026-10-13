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
#[cfg(feature = "m4a")]
pub mod hub;
pub mod node_controller;
pub mod drain_consumer;
#[cfg(feature = "m4a")]
pub mod messenger;
#[cfg(feature = "m4a")]
pub mod provision;
#[cfg(feature = "m4a")]
pub mod tasks;
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
    #[cfg(feature = "m4a")]
    if let (true, Some(master), Some(c2_addr)) =
        (config.m4a_enabled, config.m4a_master_key_var.clone(), config.provision_c2_addr.clone())
    {
        services.push(Box::new(provision::ProvisionSubservice::new(provision::ProvisionConfig {
            data_dir: config.data_dir.clone(),
            master_key_var: master,
            c2_addr,
            c2_token_var: config.provision_c2_token_var.clone(),
            interval: std::time::Duration::from_secs(config.provision_interval_secs),
            server_ref: config.m4a_server_ref.clone(),
            mail_domain: config.mail_domain.clone(),
            policy_path: config.policy_path.clone(),
        })));
    }
    #[cfg(feature = "memory")]
    services.push(Box::new(memory::MemorySubservice::new(
        config.memory_bin.clone(),
        &config.data_dir,
    )));
    let _ = config;
    services
}
