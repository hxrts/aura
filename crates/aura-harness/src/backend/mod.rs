pub mod local_pty;
pub mod playwright_browser;
pub mod ssh_tunnel;

use anyhow::{bail, Result};

use crate::config::{InstanceConfig, InstanceMode};
use crate::tool_api::ToolKey;

pub trait InstanceBackend {
    fn id(&self) -> &str;
    fn backend_kind(&self) -> &'static str;
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn snapshot(&self) -> Result<String>;
    fn send_keys(&mut self, keys: &str) -> Result<()>;
    fn send_key(&mut self, key: ToolKey, repeat: u16) -> Result<()> {
        let sequence = tool_key_sequence(key);
        let repeat = repeat.max(1);
        for _ in 0..repeat {
            self.send_keys(sequence)?;
        }
        Ok(())
    }
    fn tail_log(&self, lines: usize) -> Result<Vec<String>>;
    fn inject_message(&mut self, _message: &str) -> Result<()> {
        Ok(())
    }
    fn read_clipboard(&mut self) -> Result<String> {
        bail!(
            "clipboard reads are not supported by backend {}",
            self.backend_kind()
        )
    }
    fn authority_id(&mut self) -> Result<Option<String>> {
        Ok(None)
    }
    fn health_check(&self) -> Result<bool> {
        Ok(self.is_healthy())
    }
    fn restart(&mut self) -> Result<()> {
        self.stop()?;
        self.start()
    }
    fn is_healthy(&self) -> bool;
}

fn tool_key_sequence(key: ToolKey) -> &'static str {
    match key {
        ToolKey::Enter => "\r",
        ToolKey::Esc => "\x1b",
        ToolKey::Tab => "\t",
        ToolKey::BackTab => "\x1b[Z",
        ToolKey::Up => "\x1b[A",
        ToolKey::Down => "\x1b[B",
        ToolKey::Right => "\x1b[C",
        ToolKey::Left => "\x1b[D",
        ToolKey::Home => "\x1b[H",
        ToolKey::End => "\x1b[F",
        ToolKey::PageUp => "\x1b[5~",
        ToolKey::PageDown => "\x1b[6~",
        ToolKey::Backspace => "\x7f",
        ToolKey::Delete => "\x1b[3~",
    }
}

pub enum BackendHandle {
    Local(local_pty::LocalPtyBackend),
    Browser(playwright_browser::PlaywrightBrowserBackend),
    Ssh(ssh_tunnel::SshTunnelBackend),
}

impl BackendHandle {
    pub fn from_config(
        config: InstanceConfig,
        pty_rows: Option<u16>,
        pty_cols: Option<u16>,
    ) -> Result<Self> {
        match config.mode {
            InstanceMode::Local => Ok(Self::Local(local_pty::LocalPtyBackend::new(
                config, pty_rows, pty_cols,
            ))),
            InstanceMode::Browser => Ok(Self::Browser(
                playwright_browser::PlaywrightBrowserBackend::new(config),
            )),
            InstanceMode::Ssh => Ok(Self::Ssh(ssh_tunnel::SshTunnelBackend::new(config))),
        }
    }

    pub fn as_trait_mut(&mut self) -> &mut dyn InstanceBackend {
        match self {
            Self::Local(backend) => backend,
            Self::Browser(backend) => backend,
            Self::Ssh(backend) => backend,
        }
    }

    pub fn as_trait(&self) -> &dyn InstanceBackend {
        match self {
            Self::Local(backend) => backend,
            Self::Browser(backend) => backend,
            Self::Ssh(backend) => backend,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BackendHandle;
    use crate::config::{InstanceConfig, InstanceMode};
    use anyhow::Result;
    use std::path::PathBuf;

    #[test]
    fn backend_handle_constructs_browser_variant() -> Result<()> {
        let config = InstanceConfig {
            id: "alice".to_string(),
            mode: InstanceMode::Browser,
            data_dir: PathBuf::from(".tmp/harness/browser-alice"),
            device_id: None,
            bind_address: "127.0.0.1:47001".to_string(),
            demo_mode: false,
            command: None,
            args: vec![],
            env: vec![],
            log_path: None,
            ssh_host: None,
            ssh_user: None,
            ssh_port: None,
            ssh_strict_host_key_checking: true,
            ssh_known_hosts_file: None,
            ssh_fingerprint: None,
            ssh_require_fingerprint: false,
            ssh_dry_run: true,
            remote_workdir: None,
            lan_discovery: None,
            tunnel: None,
        };

        let backend = BackendHandle::from_config(config, Some(40), Some(120))?;
        match backend {
            BackendHandle::Browser(_) => {}
            _ => panic!("expected browser backend"),
        }
        Ok(())
    }
}
