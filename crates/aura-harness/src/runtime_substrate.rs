use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Result};
use aura_core::AuraFault;
use aura_simulator::async_host::{AsyncHostRequest, AsyncHostResponse, AsyncSimulatorHostBridge};
use tokio::runtime::{Builder, Runtime};

use crate::config::RuntimeSubstrate;

pub struct RuntimeSubstrateController {
    inner: RuntimeSubstrateInner,
}

enum RuntimeSubstrateInner {
    Real,
    Simulator(Box<SimulatorRuntimeSubstrate>),
}

struct SimulatorRuntimeSubstrate {
    runtime: Runtime,
    bridge: AsyncSimulatorHostBridge,
    participants: Vec<String>,
    artifact_dir: Option<PathBuf>,
    started: bool,
}

impl RuntimeSubstrateController {
    pub fn new(
        substrate: RuntimeSubstrate,
        seed: u64,
        participants: Vec<String>,
        artifact_dir: Option<PathBuf>,
    ) -> Result<Self> {
        let inner = match substrate {
            RuntimeSubstrate::Real => RuntimeSubstrateInner::Real,
            RuntimeSubstrate::Simulator => {
                let runtime = Builder::new_current_thread().enable_all().build()?;
                RuntimeSubstrateInner::Simulator(Box::new(SimulatorRuntimeSubstrate {
                    runtime,
                    bridge: AsyncSimulatorHostBridge::new(seed),
                    participants,
                    artifact_dir,
                    started: false,
                }))
            }
        };
        Ok(Self { inner })
    }

    pub fn start(&mut self) -> Result<()> {
        match &mut self.inner {
            RuntimeSubstrateInner::Real => Ok(()),
            RuntimeSubstrateInner::Simulator(simulator) => simulator.start(),
        }
    }

    pub fn fault_delay(&mut self, actor: &str, delay_ms: u64) -> Result<()> {
        match &mut self.inner {
            RuntimeSubstrateInner::Real => {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                Ok(())
            }
            RuntimeSubstrateInner::Simulator(simulator) => {
                simulator.apply_network_condition("delay", vec![actor.to_string()], delay_ms.max(1))
            }
        }
    }

    pub fn fault_loss(&mut self, actor: &str, loss_percent: u8) -> Result<()> {
        match &mut self.inner {
            RuntimeSubstrateInner::Real => {
                bail!(
                    "fault_loss for actor {actor} is only supported with run.runtime_substrate = \"simulator\""
                )
            }
            RuntimeSubstrateInner::Simulator(simulator) => simulator.apply_network_condition(
                &format!("loss:{loss_percent}"),
                vec![actor.to_string()],
                1,
            ),
        }
    }

    pub fn fault_tunnel_drop(&mut self, actor: &str) -> Result<()> {
        match &mut self.inner {
            RuntimeSubstrateInner::Real => {
                bail!(
                    "fault_tunnel_drop for actor {actor} is only supported with run.runtime_substrate = \"simulator\""
                )
            }
            RuntimeSubstrateInner::Simulator(simulator) => {
                simulator.inject_fault(actor, "tunnel_drop", None)
            }
        }
    }

    pub fn finish(&mut self) -> Result<()> {
        match &mut self.inner {
            RuntimeSubstrateInner::Real => Ok(()),
            RuntimeSubstrateInner::Simulator(simulator) => simulator.finish(),
        }
    }
}

impl SimulatorRuntimeSubstrate {
    fn start(&mut self) -> Result<()> {
        if self.started {
            return Ok(());
        }

        self.request(AsyncHostRequest::SetupChoreography {
            protocol: "harness_scenario".to_string(),
            participants: self.participants.clone(),
        })?;
        self.started = true;
        Ok(())
    }

    fn apply_network_condition(
        &mut self,
        condition: &str,
        participants: Vec<String>,
        duration_ticks: u64,
    ) -> Result<()> {
        self.request(AsyncHostRequest::ApplyNetworkCondition {
            condition: condition.to_string(),
            participants,
            duration_ticks,
        })
    }

    fn inject_fault(
        &mut self,
        actor: &str,
        behavior: &str,
        fault: Option<AuraFault>,
    ) -> Result<()> {
        self.request(AsyncHostRequest::InjectFault {
            participant: actor.to_string(),
            behavior: behavior.to_string(),
            fault,
        })
    }

    fn finish(&mut self) -> Result<()> {
        if let Some(artifact_dir) = &self.artifact_dir {
            let transcript_dir = artifact_dir.join("runtime-substrate");
            fs::create_dir_all(&transcript_dir)?;
            let transcript_path = transcript_dir.join("simulator-transcript.json");
            let transcript = self.bridge.transcript().to_vec();
            fs::write(&transcript_path, serde_json::to_vec_pretty(&transcript)?)?;
        }
        Ok(())
    }

    fn request(&mut self, request: AsyncHostRequest) -> Result<()> {
        self.bridge.submit(request.clone());
        let entry = self
            .runtime
            .block_on(self.bridge.resume_next())
            .map_err(anyhow::Error::from)?;
        match entry.response {
            AsyncHostResponse::Ack => Ok(()),
            AsyncHostResponse::Rejected { reason } => {
                bail!("simulator substrate rejected request {request:?}: {reason}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::RuntimeSubstrateController;
    use crate::config::RuntimeSubstrate;

    #[test]
    fn simulator_substrate_writes_transcript_artifact() {
        let tmp = tempdir().unwrap_or_else(|error| panic!("{error}"));
        let mut controller = RuntimeSubstrateController::new(
            RuntimeSubstrate::Simulator,
            7,
            vec!["alice".to_string(), "bob".to_string()],
            Some(tmp.path().to_path_buf()),
        )
        .unwrap_or_else(|error| panic!("{error}"));

        controller.start().unwrap_or_else(|error| panic!("{error}"));
        controller
            .fault_delay("alice", 5)
            .unwrap_or_else(|error| panic!("{error}"));
        controller
            .finish()
            .unwrap_or_else(|error| panic!("{error}"));

        assert!(tmp
            .path()
            .join("runtime-substrate")
            .join("simulator-transcript.json")
            .exists());
    }

    #[test]
    fn real_substrate_rejects_loss_faults() {
        let mut controller = RuntimeSubstrateController::new(
            RuntimeSubstrate::Real,
            7,
            vec!["alice".to_string()],
            None,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let error = controller
            .fault_loss("alice", 50)
            .err()
            .unwrap_or_else(|| panic!("loss fault should fail on real substrate"))
            .to_string();
        assert!(error.contains("run.runtime_substrate = \"simulator\""));
    }
}
