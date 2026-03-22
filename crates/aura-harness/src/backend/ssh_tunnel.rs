use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::backend::{DiagnosticBackend, InstanceBackend};
use crate::config::InstanceConfig;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BackendState {
    Stopped,
    Running,
}

pub struct SshTunnelBackend {
    config: InstanceConfig,
    state: BackendState,
    last_probe_ok: bool,
}

impl SshTunnelBackend {
    pub fn new(config: InstanceConfig) -> Self {
        Self {
            config,
            state: BackendState::Stopped,
            last_probe_ok: false,
        }
    }

    fn known_hosts_path(&self) -> PathBuf {
        self.config.ssh_known_hosts_file.clone().unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".ssh")
                .join("known_hosts")
        })
    }

    fn verify_security_defaults(&self) -> Result<()> {
        if !self.config.ssh_strict_host_key_checking {
            bail!(
                "ssh instance {} requires strict host key checking",
                self.config.id
            );
        }
        if self.config.ssh_require_fingerprint
            && self
                .config
                .ssh_fingerprint
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            bail!(
                "ssh instance {} requires a pinned ssh_fingerprint",
                self.config.id
            );
        }
        Ok(())
    }

    fn build_ssh_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=yes".to_string(),
            "-o".to_string(),
            format!("UserKnownHostsFile={}", self.known_hosts_path().display()),
        ];

        if let Some(port) = self.config.ssh_port {
            args.push("-p".to_string());
            args.push(port.to_string());
        }

        if let Some(tunnel) = &self.config.tunnel {
            if tunnel.kind == "ssh" {
                for mapping in &tunnel.local_forward {
                    args.push("-L".to_string());
                    args.push(mapping.clone());
                }
            }
        }

        let mut target = self
            .config
            .ssh_host
            .clone()
            .unwrap_or_else(|| "unknown-host".to_string());
        if let Some(user) = self.config.ssh_user.as_deref() {
            target = format!("{user}@{target}");
        }

        args.push(target);
        args.push("true".to_string());
        args
    }

    fn run_probe(&self) -> Result<()> {
        let args = self.build_ssh_args();
        let output = Command::new("ssh")
            .args(args)
            .output()
            .with_context(|| format!("failed to invoke ssh binary for {}", self.config.id))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("ssh probe failed for {}: {stderr}", self.config.id);
        }
        Ok(())
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
        if self.state == BackendState::Running {
            return Ok(());
        }

        self.verify_security_defaults()?;
        if self.config.ssh_dry_run {
            self.last_probe_ok = true;
            self.state = BackendState::Running;
            return Ok(());
        }

        self.run_probe()?;
        self.last_probe_ok = true;
        self.state = BackendState::Running;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.state = BackendState::Stopped;
        self.last_probe_ok = false;
        Ok(())
    }

    fn health_check(&self) -> Result<bool> {
        Ok(self.state == BackendState::Running && self.last_probe_ok)
    }

    fn is_healthy(&self) -> bool {
        self.state == BackendState::Running && self.last_probe_ok
    }
}

impl DiagnosticBackend for SshTunnelBackend {
    fn diagnostic_screen_snapshot(&self) -> Result<String> {
        bail!("ssh_tunnel snapshot is not implemented yet")
    }

    fn diagnostic_dom_snapshot(&self) -> Result<String> {
        self.diagnostic_screen_snapshot()
    }

    fn wait_for_diagnostic_dom_patterns(
        &self,
        _patterns: &[String],
        _timeout_ms: u64,
    ) -> Option<Result<String>> {
        None
    }

    fn wait_for_diagnostic_target(
        &self,
        _selector: &str,
        _timeout_ms: u64,
    ) -> Option<Result<String>> {
        None
    }

    fn tail_log(&self, lines: usize) -> Result<Vec<String>> {
        let Some(path) = &self.config.log_path else {
            return Ok(Vec::new());
        };

        let body = match fs::read_to_string(path) {
            Ok(body) => body,
            Err(_) => return Ok(Vec::new()),
        };

        let mut result: Vec<String> = body.lines().map(ToOwned::to_owned).collect();
        if result.len() > lines {
            result = result.split_off(result.len() - lines);
        }
        Ok(result)
    }

    fn read_clipboard(&self) -> Result<String> {
        bail!("ssh_tunnel clipboard reads are not implemented yet")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::InstanceMode;

    fn ssh_config() -> InstanceConfig {
        InstanceConfig {
            id: "ssh-test".to_string(),
            mode: InstanceMode::Ssh,
            data_dir: PathBuf::from("/tmp/aura-harness-ssh"),
            device_id: None,
            bind_address: "0.0.0.0:41001".to_string(),
            demo_mode: false,
            command: None,
            args: vec![],
            env: vec![],
            log_path: None,
            ssh_host: Some("example.org".to_string()),
            ssh_user: Some("dev".to_string()),
            ssh_port: Some(22),
            ssh_strict_host_key_checking: true,
            ssh_known_hosts_file: Some(PathBuf::from("/tmp/known_hosts")),
            ssh_fingerprint: Some("SHA256:test".to_string()),
            ssh_require_fingerprint: true,
            ssh_dry_run: true,
            remote_workdir: Some(PathBuf::from("/home/dev/aura")),
            lan_discovery: None,
            tunnel: Some(crate::config::TunnelConfig {
                kind: "ssh".to_string(),
                local_forward: vec!["54101:127.0.0.1:41001".to_string()],
            }),
        }
    }

    #[test]
    fn ssh_backend_enforces_security_defaults() {
        let mut backend = SshTunnelBackend::new(ssh_config());
        if let Err(error) = backend.start() {
            panic!("ssh backend start failed: {error}");
        }
        assert!(backend.is_healthy());
    }

    #[test]
    fn ssh_security_probe_rejects_missing_fingerprint_when_required() {
        let mut config = ssh_config();
        config.ssh_fingerprint = None;

        let mut backend = SshTunnelBackend::new(config);
        let error = match backend.start() {
            Ok(_) => panic!("start must fail without fingerprint"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("ssh_fingerprint"));
    }
}
