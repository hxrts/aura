use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use async_trait::async_trait;
use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "patchbay-backend"))] {
        use anyhow::{anyhow, Context};
        use std::collections::BTreeMap;
        use super::{topology_adapter, AuthorityNetworkContext, LinkTarget};
    }
}

use super::{
    ArtifactBundle, CanArtifacts, CanFirewall, CanHandoff, CanNat, LabRuntime, NetworkBackendMode,
    NetworkEvent, NetworkLabBackend, RawEventReceipt, TopologySpec,
};

/// Linux-native Patchbay backend.
#[cfg_attr(
    not(all(target_os = "linux", feature = "patchbay-backend")),
    allow(dead_code) // Retain the type shape so non-linux/default builds keep the backend API without compiling the linux-only lab implementation.
)]
pub struct PatchbayBackend {
    artifact_root: PathBuf,
    runtime: Option<LabRuntime>,
    topology: Option<TopologySpec>,
    #[cfg(all(target_os = "linux", feature = "patchbay-backend"))]
    lab: Option<patchbay::Lab>,
    /// Receipts of every applied event, merged into the timeline artifact.
    receipts: Vec<RawEventReceipt>,
    /// `tcpdump` children, one per device, writing pcaps into the lab run dir.
    /// They hold their namespaces open, so they must be stopped before the lab
    /// is dropped; see `stop_captures` and `Drop`.
    #[cfg(all(target_os = "linux", feature = "patchbay-backend"))]
    captures: Vec<(String, std::process::Child)>,
}

#[cfg(all(target_os = "linux", feature = "patchbay-backend"))]
impl Drop for PatchbayBackend {
    fn drop(&mut self) {
        // A capture left running would keep its namespace alive after the lab
        // is gone. Best effort here; `stop_captures` is the reporting path.
        for (_, mut child) in self.captures.drain(..) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

// `patchbay::Lab` is not `Debug`; report provisioning state instead of the lab.
impl std::fmt::Debug for PatchbayBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("PatchbayBackend");
        debug
            .field("artifact_root", &self.artifact_root)
            .field("runtime", &self.runtime)
            .field("topology", &self.topology);
        #[cfg(all(target_os = "linux", feature = "patchbay-backend"))]
        debug.field("lab_provisioned", &self.lab.is_some());
        debug.finish()
    }
}

impl PatchbayBackend {
    pub fn new(artifact_root: &Path) -> Self {
        Self {
            artifact_root: artifact_root.to_path_buf(),
            runtime: None,
            topology: None,
            #[cfg(all(target_os = "linux", feature = "patchbay-backend"))]
            lab: None,
            receipts: Vec::new(),
            #[cfg(all(target_os = "linux", feature = "patchbay-backend"))]
            captures: Vec::new(),
        }
    }
}

cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "patchbay-backend"))] {
        impl PatchbayBackend {
            fn lab(&self) -> Result<&patchbay::Lab> {
                self.lab
                    .as_ref()
                    .ok_or_else(|| anyhow!("patchbay backend has not been provisioned"))
            }

            /// Start one `tcpdump` per device inside its namespace, writing
            /// `<device>.pcap` into the lab run dir. Skipped, with a lab event
            /// in the artifacts' metadata, when `tcpdump` is not on PATH.
            fn start_captures(&mut self, lab: &patchbay::Lab, topology: &TopologySpec) -> Result<()> {
                let Some(run_dir) = lab.run_dir() else {
                    return Ok(());
                };
                let Ok(tcpdump) = which::which("tcpdump") else {
                    return Ok(());
                };
                for authority in &topology.authorities {
                    let device = lab.device_by_name(&authority.device_name).ok_or_else(|| {
                        anyhow!("device {} not found in provisioned lab", authority.device_name)
                    })?;
                    let pcap = run_dir.join(format!("{}.pcap", authority.device_name));
                    // tcpdump's own diagnostics stay next to the capture so a
                    // failed capture explains itself in the artifact bundle.
                    let log = std::fs::File::create(
                        run_dir.join(format!("{}.tcpdump.log", authority.device_name)),
                    )
                    .with_context(|| format!("create tcpdump log for {}", authority.device_name))?;
                    let mut command = std::process::Command::new(&tcpdump);
                    command
                        // Flush per packet so a killed capture still has its data.
                        // No `-Z`: privilege dropping needs initgroups, which the
                        // user namespace denies, and the Nix tcpdump does not
                        // drop on its own.
                        .args(["-i", super::DEVICE_UPLINK_IFACE, "-U", "-w"])
                        .arg(&pcap)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::from(log));
                    let child = device.spawn_command_sync(command).with_context(|| {
                        format!("start packet capture on {}", authority.device_name)
                    })?;
                    self.captures.push((authority.device_name.clone(), child));
                }
                Ok(())
            }

            fn runtime(&self) -> Result<&LabRuntime> {
                self.runtime
                    .as_ref()
                    .ok_or_else(|| anyhow!("patchbay backend has not been provisioned"))
            }

            fn topology(&self) -> Result<&TopologySpec> {
                self.topology
                    .as_ref()
                    .ok_or_else(|| anyhow!("patchbay backend has not been provisioned"))
            }

            fn affected_authorities_for_router(&self, router: &str) -> Vec<String> {
                self.topology
                    .as_ref()
                    .map(|topology| super::authorities_behind_router(topology, router))
                    .unwrap_or_default()
            }

            fn collect_paths(
                root: &Path,
                predicate: &dyn Fn(&Path) -> bool,
                out: &mut Vec<PathBuf>,
            ) -> Result<()> {
                for entry in std::fs::read_dir(root)
                    .with_context(|| format!("read directory {}", root.display()))?
                {
                    let entry = entry?;
                    let path = entry.path();
                    if path.is_dir() {
                        Self::collect_paths(&path, predicate, out)?;
                        continue;
                    }
                    if predicate(&path) {
                        out.push(path);
                    }
                }
                Ok(())
            }

            fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
                std::fs::create_dir_all(dst)
                    .with_context(|| format!("create output directory {}", dst.display()))?;

                for entry in
                    std::fs::read_dir(src).with_context(|| format!("read {}", src.display()))?
                {
                    let entry = entry?;
                    let path = entry.path();
                    let target = dst.join(entry.file_name());
                    if path.is_dir() {
                        Self::copy_dir_recursive(&path, &target)?;
                    } else {
                        std::fs::copy(&path, &target).with_context(|| {
                            format!("copy {} -> {}", path.display(), target.display())
                        })?;
                    }
                }

                Ok(())
            }
        }
    }
}

#[async_trait]
impl NetworkLabBackend for PatchbayBackend {
    async fn provision(&mut self, topology: TopologySpec) -> Result<LabRuntime> {
        super::scenario_builder::validate_topology(&topology)?;

        cfg_if! {
            if #[cfg(all(target_os = "linux", feature = "patchbay-backend"))] {
                let config = topology_adapter::to_patchbay_lab_config(&topology)?;
                let lab = patchbay::Lab::builder()
                    .config(config)
                    .outdir(patchbay::OutDir::Nested(self.artifact_root.clone()))
                    .label(format!("aura-{}", topology.name))
                    .build()
                    .await
                    .context("provision patchbay lab")?;

                let env_vars = lab.env_vars();
                let mut authorities = BTreeMap::new();
                for authority in &topology.authorities {
                    let device = lab.device_by_name(&authority.device_name).ok_or_else(|| {
                        anyhow!(
                            "device {} not found in provisioned lab",
                            authority.device_name
                        )
                    })?;
                    let mut env = authority.env.clone();
                    for (key, value) in &env_vars {
                        env.insert(key.clone(), value.clone());
                    }

                    authorities.insert(
                        authority.authority_id.clone(),
                        AuthorityNetworkContext {
                            authority_id: authority.authority_id.clone(),
                            device_name: authority.device_name.clone(),
                            gateway_router: authority.gateway_router.clone(),
                            namespace: device.ns().to_string(),
                            bind_address: authority.bind_address.clone(),
                            rendezvous_endpoints: Vec::new(),
                            env,
                        },
                    );
                }

                let runtime = LabRuntime {
                    topology_name: topology.name.clone(),
                    backend_mode: NetworkBackendMode::Patchbay,
                    authorities,
                };
                self.start_captures(&lab, &topology)?;
                self.runtime = Some(runtime.clone());
                self.topology = Some(topology);
                self.lab = Some(lab);
                Ok(runtime)
            } else {
                let _ = topology;
                bail!("PatchbayBackend requires Linux with feature 'patchbay-backend' enabled")
            }
        }
    }

    async fn apply_event(&mut self, event: NetworkEvent) -> Result<RawEventReceipt> {
        cfg_if! {
            if #[cfg(all(target_os = "linux", feature = "patchbay-backend"))] {
                let runtime = self.runtime()?;
                let lab = self.lab()?;
                let receipt = match &event {
                    NetworkEvent::LinkDown {
                        authority_id,
                        iface,
                    } => {
                        let context = runtime
                            .authorities
                            .get(authority_id)
                            .ok_or_else(|| anyhow!("unknown authority {authority_id}"))?;
                        let device = lab
                            .device_by_name(&context.device_name)
                            .ok_or_else(|| anyhow!("device {} not found", context.device_name))?;
                        device
                            .iface(iface)
                            .ok_or_else(|| anyhow!("iface {iface} not found on device {}", context.device_name))?
                            .link_down()
                            .await?;
                        RawEventReceipt {
                            event_type: "link_toggle".to_string(),
                            applied_at_ms: super::unix_time_ms(),
                            affected_authorities: vec![authority_id.clone()],
                            details: serde_json::json!({
                                "authority_id": authority_id,
                                "iface": iface,
                                "state": "down"
                            }),
                        }
                    }
                    NetworkEvent::LinkUp {
                        authority_id,
                        iface,
                    } => {
                        let context = runtime
                            .authorities
                            .get(authority_id)
                            .ok_or_else(|| anyhow!("unknown authority {authority_id}"))?;
                        let device = lab
                            .device_by_name(&context.device_name)
                            .ok_or_else(|| anyhow!("device {} not found", context.device_name))?;
                        device
                            .iface(iface)
                            .ok_or_else(|| anyhow!("iface {iface} not found on device {}", context.device_name))?
                            .link_up()
                            .await?;
                        RawEventReceipt {
                            event_type: "link_toggle".to_string(),
                            applied_at_ms: super::unix_time_ms(),
                            affected_authorities: vec![authority_id.clone()],
                            details: serde_json::json!({
                                "authority_id": authority_id,
                                "iface": iface,
                                "state": "up"
                            }),
                        }
                    }
                    NetworkEvent::FlushNat { router } => {
                        let router_handle = lab
                            .router_by_name(router)
                            .ok_or_else(|| anyhow!("router {router} not found"))?;
                        router_handle.flush_nat_state().await?;

                        RawEventReceipt {
                            event_type: "nat_flush".to_string(),
                            applied_at_ms: super::unix_time_ms(),
                            affected_authorities: self.affected_authorities_for_router(router),
                            details: serde_json::json!({
                                "router": router,
                                "flushed": true
                            }),
                        }
                    }
                    NetworkEvent::SetFirewall { router, preset } => {
                        let router_handle = lab
                            .router_by_name(router)
                            .ok_or_else(|| anyhow!("router {router} not found"))?;
                        let firewall = topology_adapter::to_patchbay_firewall(*preset);
                        router_handle.set_firewall(firewall).await?;

                        RawEventReceipt {
                            event_type: "set_firewall".to_string(),
                            applied_at_ms: super::unix_time_ms(),
                            affected_authorities: self.affected_authorities_for_router(router),
                            details: serde_json::json!({
                                "router": router,
                                "preset": preset,
                                "applied": true
                            }),
                        }
                    }
                    NetworkEvent::SetLinkCondition {
                        left_node,
                        right_node,
                        condition,
                    } => {
                        let target = super::classify_link(self.topology()?, left_node, right_node)?;
                        let impairment = topology_adapter::to_patchbay_link_impairment(*condition);
                        match &target {
                            LinkTarget::DeviceUplink { device_name, .. } => {
                                let iface = lab
                                    .device_by_name(device_name)
                                    .ok_or_else(|| anyhow!("device {device_name} not found"))?
                                    .iface(super::DEVICE_UPLINK_IFACE)
                                    .ok_or_else(|| {
                                        anyhow!(
                                            "{} not found on device {device_name}",
                                            super::DEVICE_UPLINK_IFACE
                                        )
                                    })?;
                                match impairment {
                                    Some(impair) => {
                                        iface
                                            .set_condition(impair, patchbay::LinkDirection::Both)
                                            .await?;
                                    }
                                    None => iface.clear_condition(patchbay::LinkDirection::Both).await?,
                                }
                            }
                            LinkTarget::RouterDownlink { downstream_router, .. } => {
                                lab.router_by_name(downstream_router)
                                    .ok_or_else(|| anyhow!("router {downstream_router} not found"))?
                                    .set_downlink_condition(impairment)
                                    .await?;
                            }
                        }

                        RawEventReceipt {
                            event_type: "set_link_condition".to_string(),
                            applied_at_ms: super::unix_time_ms(),
                            affected_authorities: target.affected_authorities(),
                            details: serde_json::json!({
                                "left_node": left_node,
                                "right_node": right_node,
                                "condition": condition,
                                "target": target,
                                "cleared": impairment.is_none(),
                                "applied": true
                            }),
                        }
                    }
                };

                self.receipts.push(receipt.clone());
                Ok(receipt)
            } else {
                let _ = event;
                bail!("PatchbayBackend requires Linux with feature 'patchbay-backend' enabled")
            }
        }
    }

    async fn collect_artifacts(&self, out_dir: &Path) -> Result<ArtifactBundle> {
        cfg_if! {
            if #[cfg(all(target_os = "linux", feature = "patchbay-backend"))] {
                let _ = self.runtime()?;
                let _ = self.topology()?;
                let lab = self.lab()?;
                let Some(run_dir) = lab.run_dir() else {
                    bail!("patchbay lab run directory is unavailable");
                };

                std::fs::create_dir_all(out_dir)
                    .with_context(|| format!("create artifact output {}", out_dir.display()))?;
                let root = out_dir.join("patchbay");
                Self::copy_dir_recursive(run_dir, &root)?;

                let mut pcap_files = Vec::new();
                Self::collect_paths(
                    &root,
                    &|path| path.extension().is_some_and(|ext| ext == "pcap"),
                    &mut pcap_files,
                )?;

                let mut namespace_dumps = Vec::new();
                Self::collect_paths(
                    &root,
                    &|path| {
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.contains("nft") || name.contains("ip-"))
                    },
                    &mut namespace_dumps,
                )?;

                let mut agent_logs = Vec::new();
                Self::collect_paths(
                    &root,
                    &|path| path.extension().is_some_and(|ext| ext == "log"),
                    &mut agent_logs,
                )?;

                // Provisional timeline: patchbay flushes events.jsonl only when
                // the lab drops, so this holds whatever has reached the file so
                // far plus every harness receipt. `ProvisionedLab::finish`
                // rebuilds it after teardown.
                let entries = super::build_network_timeline(
                    &run_dir.join(super::LAB_EVENTS_FILE),
                    &self.receipts,
                    true,
                )?;
                let timeline_path = super::write_network_timeline(out_dir, &entries)?;

                let mut metadata = BTreeMap::new();
                metadata.insert("backend".to_string(), "patchbay".to_string());
                metadata.insert("source_run_dir".to_string(), run_dir.display().to_string());
                metadata.insert("timeline_entries".to_string(), entries.len().to_string());
                metadata.insert("timeline_provisional".to_string(), "true".to_string());

                Ok(ArtifactBundle {
                    root,
                    pcap_files,
                    namespace_dumps,
                    agent_logs,
                    timeline: Some(timeline_path),
                    metadata,
                })
            } else {
                let _ = out_dir;
                bail!("PatchbayBackend requires Linux with feature 'patchbay-backend' enabled")
            }
        }
    }

    fn backend_mode(&self) -> NetworkBackendMode {
        NetworkBackendMode::Patchbay
    }

    fn stop_captures(&mut self) -> Result<()> {
        cfg_if! {
            if #[cfg(all(target_os = "linux", feature = "patchbay-backend"))] {
                use nix::sys::signal::{kill, Signal};
                use nix::unistd::Pid;
                let mut failures = Vec::new();
                for (device, mut child) in self.captures.drain(..) {
                    // SIGTERM lets tcpdump write its trailer; kill if it ignores us.
                    let pid = Pid::from_raw(child.id() as i32);
                    if let Err(error) = kill(pid, Signal::SIGTERM) {
                        failures.push(format!("{device}: signal capture: {error}"));
                        let _ = child.kill();
                    }
                    match child.wait() {
                        Ok(status) if status.success() || status.code().is_none() => {}
                        Ok(status) => failures.push(format!("{device}: tcpdump exited {status}")),
                        Err(error) => failures.push(format!("{device}: wait capture: {error}")),
                    }
                }
                if failures.is_empty() {
                    Ok(())
                } else {
                    bail!("stop packet captures: {}", failures.join("; "))
                }
            } else {
                Ok(())
            }
        }
    }

    fn run_dir(&self) -> Option<PathBuf> {
        cfg_if! {
            if #[cfg(all(target_os = "linux", feature = "patchbay-backend"))] {
                self.lab.as_ref().and_then(|lab| lab.run_dir().map(Path::to_path_buf))
            } else {
                None
            }
        }
    }

    fn instance_namespace(&self, authority_id: &str) -> Result<super::InstanceNamespace> {
        cfg_if! {
            if #[cfg(all(target_os = "linux", feature = "patchbay-backend"))] {
                let context = self
                    .runtime()?
                    .authorities
                    .get(authority_id)
                    .ok_or_else(|| anyhow!("unknown authority {authority_id}"))?;
                let device = self
                    .lab()?
                    .device_by_name(&context.device_name)
                    .ok_or_else(|| anyhow!("device {} not found", context.device_name))?;
                Ok(super::InstanceNamespace::patchbay(device))
            } else {
                let _ = authority_id;
                bail!("PatchbayBackend requires Linux with feature 'patchbay-backend' enabled")
            }
        }
    }
}

impl CanNat for PatchbayBackend {}
impl CanFirewall for PatchbayBackend {}
impl CanHandoff for PatchbayBackend {}
impl CanArtifacts for PatchbayBackend {}
