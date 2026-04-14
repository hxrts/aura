use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use async_trait::async_trait;
use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "patchbay-backend"))] {
        use anyhow::{anyhow, Context};
        use std::collections::BTreeMap;
        use super::{topology_adapter, AuthorityNetworkContext};
    }
}

use super::{
    ArtifactBundle, CanArtifacts, CanFirewall, CanHandoff, CanNat, LabRuntime, NetworkBackendMode,
    NetworkEvent, NetworkLabBackend, RawEventReceipt, TopologySpec,
};

/// Linux-native Patchbay backend.
#[derive(Debug)]
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
}

impl PatchbayBackend {
    pub fn new(artifact_root: &Path) -> Self {
        Self {
            artifact_root: artifact_root.to_path_buf(),
            runtime: None,
            topology: None,
            #[cfg(all(target_os = "linux", feature = "patchbay-backend"))]
            lab: None,
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
                    .map(|topology| {
                        topology
                            .authorities
                            .iter()
                            .filter(|authority| authority.gateway_router == router)
                            .map(|authority| authority.authority_id.clone())
                            .collect()
                    })
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
                let opts = patchbay::LabOpts::default()
                    .outdir(patchbay::OutDir::Nested(self.artifact_root.clone()))
                    .label(format!("aura-{}", topology.name));
                let lab = patchbay::Lab::from_config_with_opts(config, opts)
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
                            .ok_or_else(|| anyhow!("unknown authority {}", authority_id))?;
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
                            .ok_or_else(|| anyhow!("unknown authority {}", authority_id))?;
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
                            .ok_or_else(|| anyhow!("router {} not found", router))?;
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
                            .ok_or_else(|| anyhow!("router {} not found", router))?;
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
                        let impair = topology_adapter::to_patchbay_link_condition(*condition);
                        // Link conditions are set on the device's uplink interface.
                        // Find which of the two nodes is a device (vs router) and apply there.
                        let device = lab
                            .device_by_name(left_node)
                            .or_else(|| lab.device_by_name(right_node))
                            .ok_or_else(|| {
                                anyhow!("no device found among nodes {left_node}, {right_node}")
                            })?;
                        device
                            .iface("eth0")
                            .ok_or_else(|| anyhow!("eth0 not found on device {left_node}"))?
                            .set_condition(impair, patchbay::LinkDirection::Both)
                            .await?;

                        RawEventReceipt {
                            event_type: "set_link_condition".to_string(),
                            applied_at_ms: super::unix_time_ms(),
                            affected_authorities: runtime.authorities.keys().cloned().collect(),
                            details: serde_json::json!({
                                "left_node": left_node,
                                "right_node": right_node,
                                "condition": condition,
                                "applied": true
                            }),
                        }
                    }
                };

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

                let mut timeline_candidates = Vec::new();
                Self::collect_paths(
                    &root,
                    &|path| {
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.contains("events"))
                    },
                    &mut timeline_candidates,
                )?;

                let mut metadata = BTreeMap::new();
                metadata.insert("backend".to_string(), "patchbay".to_string());
                metadata.insert("source_run_dir".to_string(), run_dir.display().to_string());

                Ok(ArtifactBundle {
                    root,
                    pcap_files,
                    namespace_dumps,
                    agent_logs,
                    timeline: timeline_candidates.into_iter().next(),
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
}

impl CanNat for PatchbayBackend {}
impl CanFirewall for PatchbayBackend {}
impl CanHandoff for PatchbayBackend {}
impl CanArtifacts for PatchbayBackend {}
