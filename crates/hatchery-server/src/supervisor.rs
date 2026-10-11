//! Lifecycle: start the autostart subservices, answer health, stop everything.

use crate::error::ServerError;
use crate::subservice::{Status, Subservice};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Supervisor {
    inner: Arc<Mutex<Vec<Box<dyn Subservice>>>>,
}

impl Supervisor {
    pub fn new(subservices: Vec<Box<dyn Subservice>>) -> Self {
        Self { inner: Arc::new(Mutex::new(subservices)) }
    }

    /// Start every subservice that is not on-demand. A failure is reported and
    /// does not stop its siblings.
    pub fn start_autostart(&self) -> Vec<(&'static str, ServerError)> {
        let mut failures = Vec::new();
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        for service in guard.iter_mut().filter(|s| !s.on_demand()) {
            if let Err(error) = service.start() {
                failures.push((service.name(), error));
            }
        }
        failures
    }

    /// Start one named subservice (on-demand request path).
    pub fn start_named(&self, name: &str) -> Result<Status, ServerError> {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let service = guard
            .iter_mut()
            .find(|s| s.name() == name)
            .ok_or_else(|| ServerError::Config(format!("unknown subservice {name}")))?;
        service.start()?;
        Ok(service.status())
    }

    pub fn stop_named(&self, name: &str) -> Result<Status, ServerError> {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let service = guard
            .iter_mut()
            .find(|s| s.name() == name)
            .ok_or_else(|| ServerError::Config(format!("unknown subservice {name}")))?;
        service.stop();
        Ok(service.status())
    }

    pub fn statuses(&self) -> Vec<Status> {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        guard.iter_mut().map(|s| s.status()).collect()
    }

    pub fn stop_all(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        for service in guard.iter_mut() {
            service.stop();
        }
    }
}
