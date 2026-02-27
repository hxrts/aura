pub mod local_pty;
pub mod ssh_tunnel;

use anyhow::Result;

use crate::config::{InstanceConfig, InstanceMode};

pub trait InstanceBackend {
    fn id(&self) -> &str;
    fn backend_kind(&self) -> &'static str;
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn snapshot(&self) -> Result<String>;
    fn send_keys(&mut self, keys: &str) -> Result<()>;
    fn tail_log(&self, lines: usize) -> Result<Vec<String>>;
    fn restart(&mut self) -> Result<()> {
        self.stop()?;
        self.start()
    }
    fn is_healthy(&self) -> bool;
}

pub enum BackendHandle {
    Local(local_pty::LocalPtyBackend),
    Ssh(ssh_tunnel::SshTunnelBackend),
}

impl BackendHandle {
    pub fn from_config(config: InstanceConfig) -> Result<Self> {
        match config.mode {
            InstanceMode::Local => Ok(Self::Local(local_pty::LocalPtyBackend::new(config))),
            InstanceMode::Ssh => Ok(Self::Ssh(ssh_tunnel::SshTunnelBackend::new(config))),
        }
    }

    pub fn as_trait_mut(&mut self) -> &mut dyn InstanceBackend {
        match self {
            Self::Local(backend) => backend,
            Self::Ssh(backend) => backend,
        }
    }

    pub fn as_trait(&self) -> &dyn InstanceBackend {
        match self {
            Self::Local(backend) => backend,
            Self::Ssh(backend) => backend,
        }
    }
}
