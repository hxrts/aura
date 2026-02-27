use anyhow::Result;

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

    fn is_healthy(&self) -> bool {
        self.state == BackendState::Running
    }
}
