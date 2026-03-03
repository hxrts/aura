use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;

use super::{
    topology_adapter, ArtifactBundle, AuthorityNetworkContext, CanArtifacts, CanFirewall,
    CanHandoff, CanNat, LabRuntime, NetworkBackendMode, NetworkEvent, NetworkLabBackend,
    RawEventReceipt, TopologySpec,
};

/// macOS and cross-platform runner that delegates execution to `patchbay-vm`.
#[derive(Debug)]
pub struct VmPatchbayBackend {
    artifact_root: PathBuf,
    work_dir: PathBuf,
    runtime: Option<LabRuntime>,
    topology: Option<TopologySpec>,
    topology_path: Option<PathBuf>,
    event_log_path: Option<PathBuf>,
}

impl VmPatchbayBackend {
    pub fn new(artifact_root: &Path) -> Self {
        let work_dir = artifact_root.join("patchbay-vm-work");
        Self {
            artifact_root: artifact_root.to_path_buf(),
            work_dir,
            runtime: None,
            topology: None,
            topology_path: None,
            event_log_path: None,
        }
    }

    fn ensure_work_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.work_dir)
            .with_context(|| format!("create VM work directory {}", self.work_dir.display()))
    }

    fn topology_path(&self) -> Result<&Path> {
        self.topology_path
            .as_deref()
            .ok_or_else(|| anyhow!("VM backend is not provisioned"))
    }

    fn append_event_log(&self, event: &NetworkEvent) -> Result<()> {
        let Some(path) = &self.event_log_path else {
            bail!("VM backend is not provisioned");
        };

        let mut events: Vec<NetworkEvent> = if path.exists() {
            let bytes = std::fs::read(path)
                .with_context(|| format!("read VM event log {}", path.display()))?;
            serde_json::from_slice(&bytes).unwrap_or_default()
        } else {
            Vec::new()
        };
        events.push(event.clone());
        std::fs::write(path, serde_json::to_vec_pretty(&events)?)
            .with_context(|| format!("write VM event log {}", path.display()))
    }

    fn run_patchbay_vm(&self) -> Result<()> {
        let topology = self.topology_path()?;
        let output = Command::new("patchbay-vm")
            .arg("run")
            .arg(topology)
            .arg("--work-dir")
            .arg(&self.work_dir)
            .arg("--patchbay-version")
            .arg("git:hxrts/aura")
            .output()
            .context("failed to run patchbay-vm")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("patchbay-vm run failed: {stderr}");
        }

        Ok(())
    }

    fn event_type(event: &NetworkEvent) -> &'static str {
        match event {
            NetworkEvent::LinkDown { .. } | NetworkEvent::LinkUp { .. } => "link_toggle",
            NetworkEvent::FlushNat { .. } => "nat_flush",
            NetworkEvent::SetFirewall { .. } => "set_firewall",
            NetworkEvent::SetLinkCondition { .. } => "set_link_condition",
        }
    }

    fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
        std::fs::create_dir_all(dst)
            .with_context(|| format!("create output directory {}", dst.display()))?;

        for entry in std::fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
            let entry = entry?;
            let path = entry.path();
            let target = dst.join(entry.file_name());
            if path.is_dir() {
                Self::copy_dir_recursive(&path, &target)?;
            } else {
                std::fs::copy(&path, &target)
                    .with_context(|| format!("copy {} -> {}", path.display(), target.display()))?;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl NetworkLabBackend for VmPatchbayBackend {
    async fn provision(&mut self, topology: TopologySpec) -> Result<LabRuntime> {
        super::scenario_builder::validate_topology(&topology)?;
        self.ensure_work_dir()?;

        let topology_path = self.work_dir.join("topology.toml");
        let event_log = self.work_dir.join("event_timeline.json");
        std::fs::write(
            &topology_path,
            topology_adapter::topology_to_toml(&topology)?,
        )
        .with_context(|| format!("write VM topology {}", topology_path.display()))?;
        std::fs::write(&event_log, b"[]")
            .with_context(|| format!("write VM event timeline {}", event_log.display()))?;

        let mut authorities = BTreeMap::new();
        for authority in &topology.authorities {
            authorities.insert(
                authority.authority_id.clone(),
                AuthorityNetworkContext {
                    authority_id: authority.authority_id.clone(),
                    device_name: authority.device_name.clone(),
                    namespace: format!("vm:{}", authority.device_name),
                    bind_address: authority.bind_address.clone(),
                    rendezvous_endpoints: Vec::new(),
                    env: authority.env.clone(),
                },
            );
        }

        let runtime = LabRuntime {
            topology_name: topology.name.clone(),
            backend_mode: NetworkBackendMode::PatchbayVm,
            authorities,
        };

        self.runtime = Some(runtime.clone());
        self.topology = Some(topology);
        self.topology_path = Some(topology_path);
        self.event_log_path = Some(event_log);

        Ok(runtime)
    }

    async fn apply_event(&mut self, event: NetworkEvent) -> Result<RawEventReceipt> {
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| anyhow!("VM backend is not provisioned"))?;

        self.append_event_log(&event)?;
        self.run_patchbay_vm()?;

        let details = match &event {
            NetworkEvent::LinkDown {
                authority_id,
                iface,
            } => serde_json::json!({
                "authority_id": authority_id,
                "iface": iface,
                "state": "down"
            }),
            NetworkEvent::LinkUp {
                authority_id,
                iface,
            } => serde_json::json!({
                "authority_id": authority_id,
                "iface": iface,
                "state": "up"
            }),
            NetworkEvent::FlushNat { router } => serde_json::json!({
                "router": router,
                "flushed": true
            }),
            NetworkEvent::SetFirewall { router, preset } => serde_json::json!({
                "router": router,
                "preset": preset,
                "applied": true
            }),
            NetworkEvent::SetLinkCondition {
                left_node,
                right_node,
                condition,
            } => serde_json::json!({
                "left_node": left_node,
                "right_node": right_node,
                "condition": condition,
                "applied": true
            }),
        };

        Ok(RawEventReceipt {
            event_type: Self::event_type(&event).to_string(),
            applied_at_ms: super::unix_time_ms(),
            affected_authorities: runtime.authorities.keys().cloned().collect(),
            details,
        })
    }

    async fn collect_artifacts(&self, out_dir: &Path) -> Result<ArtifactBundle> {
        std::fs::create_dir_all(out_dir)
            .with_context(|| format!("create artifact output {}", out_dir.display()))?;

        let root = out_dir.join("patchbay-vm");
        Self::copy_dir_recursive(&self.work_dir, &root)?;

        let mut metadata = BTreeMap::new();
        metadata.insert("backend".to_string(), "patchbay-vm".to_string());
        metadata.insert(
            "source_work_dir".to_string(),
            self.work_dir.display().to_string(),
        );
        metadata.insert(
            "artifact_source".to_string(),
            self.artifact_root.display().to_string(),
        );

        let timeline = self
            .event_log_path
            .as_ref()
            .map(|_| root.join("event_timeline.json"));

        Ok(ArtifactBundle {
            root,
            pcap_files: Vec::new(),
            namespace_dumps: Vec::new(),
            agent_logs: Vec::new(),
            timeline,
            metadata,
        })
    }

    fn backend_mode(&self) -> NetworkBackendMode {
        NetworkBackendMode::PatchbayVm
    }
}

impl CanNat for VmPatchbayBackend {}
impl CanFirewall for VmPatchbayBackend {}
impl CanHandoff for VmPatchbayBackend {}
impl CanArtifacts for VmPatchbayBackend {}
