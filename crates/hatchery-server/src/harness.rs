//! Harness + observation host: supervises the `hatchery-harness` binary
//! (the observation engine and store run inside it). Arguments come from the
//! operator's configuration; tokens stay in the environment the child
//! inherits (`GATE4AGENT_C2_TOKEN`, operator token), never in arguments.

use crate::error::ServerError;
use crate::subservice::{ManagedProcess, State, Status, Subservice};
use std::path::PathBuf;
use tokio::process::Command;

pub struct HarnessSubservice {
    bin: PathBuf,
    args: Vec<String>,
    process: ManagedProcess,
    failed: Option<String>,
}

impl HarnessSubservice {
    pub fn new(bin: PathBuf, args: Vec<String>) -> Self {
        Self { bin, args, process: ManagedProcess::new(), failed: None }
    }
}

impl Subservice for HarnessSubservice {
    fn name(&self) -> &'static str {
        "harness"
    }

    fn start(&mut self) -> Result<(), ServerError> {
        if self.process.alive() {
            return Ok(());
        }
        let mut command = Command::new(&self.bin);
        command.args(&self.args);
        match self.process.spawn(command) {
            Ok(()) => {
                self.failed = None;
                Ok(())
            }
            Err(error) => {
                self.failed = Some(error.to_string());
                Err(ServerError::sub("harness", error.to_string()))
            }
        }
    }

    fn stop(&mut self) {
        self.process.stop();
    }

    fn status(&mut self) -> Status {
        let state = if self.process.alive() {
            State::Running
        } else if self.failed.is_some() {
            State::Failed
        } else {
            State::Stopped
        };
        Status { name: "harness", state, port: None, detail: self.failed.clone() }
    }
}
