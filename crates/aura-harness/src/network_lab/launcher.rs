use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio::time::Instant;

use anyhow::{anyhow, bail, Context, Result};

use crate::timeouts::blocking_sleep;

/// Agent launch specification used by namespace-aware harness runners.
#[derive(Debug, Clone)]
pub struct AgentLaunchSpec {
    pub authority_id: String,
    pub namespace: Option<String>,
    pub binary: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub readiness_probe: Option<ReadinessProbe>,
}

/// Readiness contract for launched agent processes.
#[derive(Debug, Clone)]
pub enum ReadinessProbe {
    Tcp(SocketAddr),
    LogPattern(String),
}

/// Handle for a launched agent process and its log route.
#[derive(Debug)]
pub struct AgentLaunchHandle {
    pub authority_id: String,
    pub log_path: PathBuf,
    child: Child,
}

impl AgentLaunchHandle {
    pub fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

/// Standardized launcher abstraction for backend-specific runners.
pub trait AgentNamespaceLauncher {
    fn launch(&self, spec: &AgentLaunchSpec) -> Result<AgentLaunchHandle>;
    fn wait_ready(
        &self,
        handle: &mut AgentLaunchHandle,
        probe: &ReadinessProbe,
        timeout: Duration,
    ) -> Result<()>;
}

/// Default launcher implementation supporting `ip netns exec` on Linux.
#[derive(Debug, Clone)]
pub struct StandardAgentLauncher {
    log_root: PathBuf,
}

impl StandardAgentLauncher {
    pub fn new(log_root: impl Into<PathBuf>) -> Self {
        Self {
            log_root: log_root.into(),
        }
    }

    fn build_command(&self, spec: &AgentLaunchSpec) -> Command {
        let mut command = if let Some(namespace) = &spec.namespace {
            let mut cmd = Command::new("ip");
            cmd.args(["netns", "exec", namespace]);
            cmd.arg(&spec.binary);
            cmd
        } else {
            Command::new(&spec.binary)
        };

        command.args(&spec.args);
        command.env("AURA_AUTHORITY_ID", &spec.authority_id);
        for (key, value) in &spec.env {
            command.env(key, value);
        }

        command
    }

    fn ensure_log_path(&self, authority_id: &str) -> Result<PathBuf> {
        fs::create_dir_all(&self.log_root)
            .with_context(|| format!("create log root {}", self.log_root.display()))?;
        Ok(self.log_root.join(format!("{authority_id}.log")))
    }
}

impl AgentNamespaceLauncher for StandardAgentLauncher {
    fn launch(&self, spec: &AgentLaunchSpec) -> Result<AgentLaunchHandle> {
        if !spec.binary.exists() {
            bail!("agent binary does not exist: {}", spec.binary.display());
        }

        let log_path = self.ensure_log_path(&spec.authority_id)?;
        let log = File::create(&log_path)
            .with_context(|| format!("create agent log {}", log_path.display()))?;
        let log_err = log
            .try_clone()
            .with_context(|| format!("clone log handle {}", log_path.display()))?;

        let mut command = self.build_command(spec);
        let child = command
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .spawn()
            .with_context(|| format!("spawn agent process for {}", spec.authority_id))?;

        Ok(AgentLaunchHandle {
            authority_id: spec.authority_id.clone(),
            log_path,
            child,
        })
    }

    fn wait_ready(
        &self,
        handle: &mut AgentLaunchHandle,
        probe: &ReadinessProbe,
        timeout: Duration,
    ) -> Result<()> {
        let deadline = Instant::now() + timeout;

        loop {
            if let Some(status) = handle.child.try_wait()? {
                bail!(
                    "agent {} exited before readiness with status {}",
                    handle.authority_id,
                    status
                );
            }

            let ready = match probe {
                ReadinessProbe::Tcp(addr) => {
                    TcpStream::connect_timeout(addr, Duration::from_millis(150)).is_ok()
                }
                ReadinessProbe::LogPattern(pattern) => {
                    let mut body = String::new();
                    File::open(&handle.log_path)
                        .and_then(|mut file| file.read_to_string(&mut body).map(|_| ()))
                        .is_ok()
                        && body.contains(pattern)
                }
            };

            if ready {
                return Ok(());
            }

            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "agent {} did not satisfy readiness probe within {:?}",
                    handle.authority_id,
                    timeout
                ));
            }

            blocking_sleep(Duration::from_millis(100));
        }
    }
}

/// Launch and optionally block until ready.
pub fn launch_agent(
    launcher: &dyn AgentNamespaceLauncher,
    spec: &AgentLaunchSpec,
    readiness_timeout: Duration,
) -> Result<AgentLaunchHandle> {
    let mut handle = launcher.launch(spec)?;
    if let Some(probe) = &spec.readiness_probe {
        launcher.wait_ready(&mut handle, probe, readiness_timeout)?;
    }
    Ok(handle)
}

/// Write a deterministic process manifest for structured log routing and triage.
pub fn write_launch_manifest(path: &Path, specs: &[AgentLaunchSpec]) -> Result<()> {
    #[derive(serde::Serialize)]
    struct Manifest<'a> {
        agents: Vec<AgentManifest<'a>>,
    }

    #[derive(serde::Serialize)]
    struct AgentManifest<'a> {
        authority_id: &'a str,
        namespace: Option<&'a str>,
        binary: String,
        args: &'a [String],
        env: &'a BTreeMap<String, String>,
    }

    let payload = Manifest {
        agents: specs
            .iter()
            .map(|spec| AgentManifest {
                authority_id: &spec.authority_id,
                namespace: spec.namespace.as_deref(),
                binary: spec.binary.display().to_string(),
                args: &spec.args,
                env: &spec.env,
            })
            .collect(),
    };

    fs::write(path, serde_json::to_vec_pretty(&payload)?)
        .with_context(|| format!("write launch manifest {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_writer_records_namespace_and_env() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("manifest.json");

        let mut env = BTreeMap::new();
        env.insert("AURA_TEST".to_string(), "1".to_string());

        let specs = vec![AgentLaunchSpec {
            authority_id: "alice".to_string(),
            namespace: Some("ns-alice".to_string()),
            binary: PathBuf::from("/bin/echo"),
            args: vec!["ok".to_string()],
            env,
            readiness_probe: None,
        }];

        write_launch_manifest(&path, &specs).unwrap();
        let body = fs::read_to_string(path).unwrap();

        assert!(body.contains("ns-alice"));
        assert!(body.contains("AURA_TEST"));
    }
}
