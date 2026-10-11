//! Subservice contract and a supervised-process implementation.

use crate::error::ServerError;
use serde::Serialize;
use std::process::Stdio;
use tokio::process::{Child, Command};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Stopped,
    Running,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub name: &'static str,
    pub state: State,
    /// Loopback port the subservice listens on, when it has one.
    pub port: Option<u16>,
    pub detail: Option<String>,
}

/// A unit the server hosts. `on_demand` subservices are never started by the
/// server itself; they start only through the authenticated control request.
pub trait Subservice: Send {
    fn name(&self) -> &'static str;
    fn on_demand(&self) -> bool {
        false
    }
    fn start(&mut self) -> Result<(), ServerError>;
    fn stop(&mut self);
    fn status(&mut self) -> Status;
}

/// Child-process hosting shared by the harness and memory subservices.
pub struct ManagedProcess {
    child: Option<Child>,
}

impl ManagedProcess {
    pub fn new() -> Self {
        Self { child: None }
    }

    pub fn spawn(&mut self, mut command: Command) -> Result<(), ServerError> {
        command.stdin(Stdio::null()).kill_on_drop(true);
        let child = command.spawn()?;
        self.child = Some(child);
        Ok(())
    }

    /// True while the child is alive; reaps it when it has exited.
    pub fn alive(&mut self) -> bool {
        match self.child.as_mut().map(Child::try_wait) {
            Some(Ok(None)) => true,
            Some(_) => {
                self.child = None;
                false
            }
            None => false,
        }
    }

    /// Ask the child to die; `kill_on_drop` plus the later reap finish the job.
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

impl Default for ManagedProcess {
    fn default() -> Self {
        Self::new()
    }
}
