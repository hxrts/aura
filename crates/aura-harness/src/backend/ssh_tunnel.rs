use anyhow::{bail, Result};

use crate::backend::InstanceBackend;
use crate::config::InstanceConfig;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BackendState {
    Stopped,
    Running,
}

pub struct SshTunnelBackend {
    config: InstanceConfig,
    state: BackendState,
}

impl SshTunnelBackend {
    pub fn new(config: InstanceConfig) -> Self {
        Self {
            config,
            state: BackendState::Stopped,
        }
    }
}

impl InstanceBackend for SshTunnelBackend {
    fn id(&self) -> &str {
        &self.config.id
    }

    fn backend_kind(&self) -> &'static str {
        "ssh_tunnel"
    }

    fn start(&mut self) -> Result<()> {
        self.state = BackendState::Running;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.state = BackendState::Stopped;
        Ok(())
    }

    fn snapshot(&self) -> Result<String> {
        bail!("ssh_tunnel snapshot is not implemented yet")
    }

    fn send_keys(&mut self, _keys: &str) -> Result<()> {
        bail!("ssh_tunnel send_keys is not implemented yet")
    }

    fn tail_log(&self, _lines: usize) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn is_healthy(&self) -> bool {
        self.state == BackendState::Running
    }
}
