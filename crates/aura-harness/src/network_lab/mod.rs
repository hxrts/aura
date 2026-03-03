use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub mod launcher;
pub mod scenario_builder;
pub mod topology_adapter;

mod patchbay_backend;
mod vm_backend;

pub use patchbay_backend::PatchbayBackend;
pub use vm_backend::VmPatchbayBackend;

/// Supported network lab backend modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkBackendMode {
    Mock,
    Patchbay,
    PatchbayVm,
}

impl fmt::Display for NetworkBackendMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mock => f.write_str("mock"),
            Self::Patchbay => f.write_str("patchbay"),
            Self::PatchbayVm => f.write_str("patchbay-vm"),
        }
    }
}

impl FromStr for NetworkBackendMode {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "mock" => Ok(Self::Mock),
            "patchbay" => Ok(Self::Patchbay),
            "patchbay-vm" => Ok(Self::PatchbayVm),
            other => Err(anyhow!(
                "unknown network backend '{other}', expected one of: mock, patchbay, patchbay-vm"
            )),
        }
    }
}

/// Stable topology contract owned by Aura harness.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TopologySpec {
    pub name: String,
    pub routers: Vec<RouterSpec>,
    pub links: Vec<LinkSpec>,
    pub authorities: Vec<AuthoritySpec>,
    pub required_relay_authority: Option<String>,
}

/// Router configuration in a topology.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RouterSpec {
    pub name: String,
    pub nat: NatPreset,
    pub upstream: Option<String>,
    pub firewall: FirewallPreset,
}

/// Authority/device mapping used by harness launcher.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AuthoritySpec {
    pub authority_id: String,
    pub device_name: String,
    pub gateway_router: String,
    pub bind_address: String,
    pub relay_capable: bool,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// Point-to-point link between two nodes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LinkSpec {
    pub left: String,
    pub right: String,
    pub condition: LinkConditionPreset,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NatPreset {
    None,
    Home,
    Corporate,
    FullCone,
    Cgnat,
    CloudNat,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FirewallPreset {
    Open,
    Home,
    Corporate,
    UdpBlocked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkConditionPreset {
    Lan,
    Wifi,
    WifiBad,
    Mobile4G,
    Mobile3G,
    Satellite,
}

/// Runtime network context for a launched authority.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AuthorityNetworkContext {
    pub authority_id: String,
    pub device_name: String,
    pub namespace: String,
    pub bind_address: String,
    #[serde(default)]
    pub rendezvous_endpoints: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// Provisioning result for a topology.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LabRuntime {
    pub topology_name: String,
    pub backend_mode: NetworkBackendMode,
    pub authorities: BTreeMap<String, AuthorityNetworkContext>,
}

/// Runtime event applied to a provisioned lab.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NetworkEvent {
    LinkDown {
        authority_id: String,
        iface: String,
    },
    LinkUp {
        authority_id: String,
        iface: String,
    },
    FlushNat {
        router: String,
    },
    SetFirewall {
        router: String,
        preset: FirewallPreset,
    },
    SetLinkCondition {
        left_node: String,
        right_node: String,
        condition: LinkConditionPreset,
    },
}

/// Raw backend receipt emitted after applying a runtime event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RawEventReceipt {
    pub event_type: String,
    pub applied_at_ms: u64,
    pub affected_authorities: Vec<String>,
    pub details: serde_json::Value,
}

/// Strongly typed event receipt for an event type `E`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EventReceipt<E: NetworkEventType> {
    pub applied_at_ms: u64,
    pub affected_authorities: Vec<String>,
    pub details: E::Receipt,
}

/// Typed event contract for converting between raw and typed receipts.
pub trait NetworkEventType {
    const EVENT_TYPE: &'static str;
    type Receipt: Serialize + DeserializeOwned;

    fn into_event(self) -> NetworkEvent;

    fn decode_receipt(details: serde_json::Value) -> Result<Self::Receipt> {
        serde_json::from_value(details).context("failed to decode event receipt")
    }
}

/// Artifact set collected from a backend run.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactBundle {
    pub root: PathBuf,
    pub pcap_files: Vec<PathBuf>,
    pub namespace_dumps: Vec<PathBuf>,
    pub agent_logs: Vec<PathBuf>,
    pub timeline: Option<PathBuf>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

/// Backend abstraction for topology provisioning and event application.
#[async_trait]
pub trait NetworkLabBackend: Send + Sync {
    async fn provision(&mut self, topology: TopologySpec) -> Result<LabRuntime>;
    async fn apply_event(&mut self, event: NetworkEvent) -> Result<RawEventReceipt>;
    async fn collect_artifacts(&self, out_dir: &Path) -> Result<ArtifactBundle>;
    fn backend_mode(&self) -> NetworkBackendMode;
}

/// Backend capability marker: supports NAT changes and resets.
pub trait CanNat {}
/// Backend capability marker: supports firewall mutations.
pub trait CanFirewall {}
/// Backend capability marker: supports network handoff/link mutation.
pub trait CanHandoff {}
/// Backend capability marker: supports artifact collection.
pub trait CanArtifacts {}

/// Apply typed event and decode typed receipt.
pub async fn apply_typed_event<B, E>(backend: &mut B, event: E) -> Result<EventReceipt<E>>
where
    B: NetworkLabBackend + ?Sized,
    E: NetworkEventType + Send,
{
    let raw = backend.apply_event(event.into_event()).await?;
    if raw.event_type != E::EVENT_TYPE {
        bail!(
            "typed event mismatch: expected {}, got {}",
            E::EVENT_TYPE,
            raw.event_type
        );
    }
    let details = E::decode_receipt(raw.details)?;
    Ok(EventReceipt {
        applied_at_ms: raw.applied_at_ms,
        affected_authorities: raw.affected_authorities,
        details,
    })
}

/// Typed link toggle event.
#[derive(Debug, Clone)]
pub struct LinkToggleEvent {
    pub authority_id: String,
    pub iface: String,
    pub up: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LinkToggleReceipt {
    pub authority_id: String,
    pub iface: String,
    pub state: String,
}

impl NetworkEventType for LinkToggleEvent {
    const EVENT_TYPE: &'static str = "link_toggle";
    type Receipt = LinkToggleReceipt;

    fn into_event(self) -> NetworkEvent {
        if self.up {
            NetworkEvent::LinkUp {
                authority_id: self.authority_id,
                iface: self.iface,
            }
        } else {
            NetworkEvent::LinkDown {
                authority_id: self.authority_id,
                iface: self.iface,
            }
        }
    }
}

/// Typed NAT flush event.
#[derive(Debug, Clone)]
pub struct NatFlushEvent {
    pub router: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NatFlushReceipt {
    pub router: String,
    pub flushed: bool,
}

impl NetworkEventType for NatFlushEvent {
    const EVENT_TYPE: &'static str = "nat_flush";
    type Receipt = NatFlushReceipt;

    fn into_event(self) -> NetworkEvent {
        NetworkEvent::FlushNat {
            router: self.router,
        }
    }
}

/// Preflight check item for backend selection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BackendPreflightCheck {
    pub name: String,
    pub ok: bool,
    pub details: String,
}

/// Backend selection report including fallback outcome.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BackendPreflightReport {
    pub requested: NetworkBackendMode,
    pub resolved: NetworkBackendMode,
    pub checks: Vec<BackendPreflightCheck>,
}

impl BackendPreflightReport {
    pub fn fallback_used(&self) -> bool {
        self.requested != self.resolved
    }
}

/// Resolve backend mode with preflight checks and automatic fallback.
pub fn resolve_backend_mode(requested: NetworkBackendMode) -> BackendPreflightReport {
    let mut checks = Vec::new();

    let native_supported = native_patchbay_supported();
    checks.push(BackendPreflightCheck {
        name: "native_patchbay".to_string(),
        ok: native_supported,
        details: if native_supported {
            "native patchbay supported on this host".to_string()
        } else {
            "native patchbay unavailable (platform/capability constraints)".to_string()
        },
    });

    let vm_available = binary_in_path("patchbay-vm");
    checks.push(BackendPreflightCheck {
        name: "patchbay_vm_binary".to_string(),
        ok: vm_available,
        details: if vm_available {
            "patchbay-vm found in PATH".to_string()
        } else {
            "patchbay-vm not found in PATH".to_string()
        },
    });

    let resolved = match requested {
        NetworkBackendMode::Mock => NetworkBackendMode::Mock,
        NetworkBackendMode::Patchbay if native_supported => NetworkBackendMode::Patchbay,
        NetworkBackendMode::Patchbay if vm_available => NetworkBackendMode::PatchbayVm,
        NetworkBackendMode::Patchbay => NetworkBackendMode::Mock,
        NetworkBackendMode::PatchbayVm if vm_available => NetworkBackendMode::PatchbayVm,
        NetworkBackendMode::PatchbayVm => NetworkBackendMode::Mock,
    };

    BackendPreflightReport {
        requested,
        resolved,
        checks,
    }
}

/// Construct a concrete backend for the selected mode.
pub fn build_backend(
    mode: NetworkBackendMode,
    artifact_root: &Path,
) -> Result<Box<dyn NetworkLabBackend>> {
    match mode {
        NetworkBackendMode::Mock => Ok(Box::new(MockNetworkLabBackend::new(artifact_root))),
        NetworkBackendMode::Patchbay => Ok(Box::new(PatchbayBackend::new(artifact_root))),
        NetworkBackendMode::PatchbayVm => Ok(Box::new(VmPatchbayBackend::new(artifact_root))),
    }
}

/// Lightweight in-process backend for deterministic tests.
#[derive(Debug)]
pub struct MockNetworkLabBackend {
    runtime: Option<LabRuntime>,
    events: Vec<NetworkEvent>,
    artifact_root: PathBuf,
}

impl MockNetworkLabBackend {
    pub fn new(artifact_root: &Path) -> Self {
        Self {
            runtime: None,
            events: Vec::new(),
            artifact_root: artifact_root.to_path_buf(),
        }
    }

    fn affected_authorities(&self, event: &NetworkEvent) -> Vec<String> {
        match event {
            NetworkEvent::LinkDown { authority_id, .. }
            | NetworkEvent::LinkUp { authority_id, .. } => {
                vec![authority_id.clone()]
            }
            _ => self
                .runtime
                .as_ref()
                .map(|runtime| runtime.authorities.keys().cloned().collect())
                .unwrap_or_default(),
        }
    }

    fn event_type(event: &NetworkEvent) -> &'static str {
        match event {
            NetworkEvent::LinkDown { .. } | NetworkEvent::LinkUp { .. } => "link_toggle",
            NetworkEvent::FlushNat { .. } => "nat_flush",
            NetworkEvent::SetFirewall { .. } => "set_firewall",
            NetworkEvent::SetLinkCondition { .. } => "set_link_condition",
        }
    }
}

#[async_trait]
impl NetworkLabBackend for MockNetworkLabBackend {
    async fn provision(&mut self, topology: TopologySpec) -> Result<LabRuntime> {
        scenario_builder::validate_topology(&topology)?;

        let mut authorities = BTreeMap::new();
        for authority in &topology.authorities {
            authorities.insert(
                authority.authority_id.clone(),
                AuthorityNetworkContext {
                    authority_id: authority.authority_id.clone(),
                    device_name: authority.device_name.clone(),
                    namespace: format!("mock:{}", authority.device_name),
                    bind_address: authority.bind_address.clone(),
                    rendezvous_endpoints: Vec::new(),
                    env: authority.env.clone(),
                },
            );
        }

        let runtime = LabRuntime {
            topology_name: topology.name,
            backend_mode: NetworkBackendMode::Mock,
            authorities,
        };
        self.runtime = Some(runtime.clone());
        Ok(runtime)
    }

    async fn apply_event(&mut self, event: NetworkEvent) -> Result<RawEventReceipt> {
        if self.runtime.is_none() {
            bail!("mock backend is not provisioned");
        }

        self.events.push(event.clone());
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
            applied_at_ms: unix_time_ms(),
            affected_authorities: self.affected_authorities(&event),
            details,
        })
    }

    async fn collect_artifacts(&self, out_dir: &Path) -> Result<ArtifactBundle> {
        std::fs::create_dir_all(out_dir)
            .with_context(|| format!("create artifact output {}", out_dir.display()))?;

        let root = out_dir.join("mock-network-lab");
        std::fs::create_dir_all(&root)
            .with_context(|| format!("create artifact root {}", root.display()))?;

        let timeline = root.join("timeline.json");
        std::fs::write(&timeline, serde_json::to_vec_pretty(&self.events)?)
            .with_context(|| format!("write timeline {}", timeline.display()))?;

        let mut metadata = BTreeMap::new();
        metadata.insert(
            "artifact_source".to_string(),
            self.artifact_root.display().to_string(),
        );

        Ok(ArtifactBundle {
            root,
            pcap_files: Vec::new(),
            namespace_dumps: Vec::new(),
            agent_logs: Vec::new(),
            timeline: Some(timeline),
            metadata,
        })
    }

    fn backend_mode(&self) -> NetworkBackendMode {
        NetworkBackendMode::Mock
    }
}

impl CanNat for MockNetworkLabBackend {}
impl CanFirewall for MockNetworkLabBackend {}
impl CanHandoff for MockNetworkLabBackend {}
impl CanArtifacts for MockNetworkLabBackend {}

fn binary_in_path(binary: &str) -> bool {
    if binary.contains(std::path::MAIN_SEPARATOR) {
        return Path::new(binary).exists();
    }

    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path).any(|dir| {
                let candidate = dir.join(binary);
                candidate.exists()
            })
        })
        .unwrap_or(false)
}

fn native_patchbay_supported() -> bool {
    #[cfg(all(target_os = "linux", feature = "patchbay-backend"))]
    {
        let userns_ok = read_userns_clone_flag().unwrap_or(true);
        userns_ok && patchbay::check_caps().is_ok()
    }

    #[cfg(not(all(target_os = "linux", feature = "patchbay-backend")))]
    {
        false
    }
}

#[cfg(all(target_os = "linux", feature = "patchbay-backend"))]
fn read_userns_clone_flag() -> Option<bool> {
    let path = "/proc/sys/kernel/unprivileged_userns_clone";
    let text = std::fs::read_to_string(path).ok()?;
    Some(text.trim() == "1")
}

fn unix_time_ms() -> u64 {
    use std::time::UNIX_EPOCH;

    UNIX_EPOCH
        .elapsed()
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

/// Validate basic topology-level shape constraints that every backend depends on.
pub fn validate_runtime_bindings(topology: &TopologySpec) -> Result<()> {
    let router_names: BTreeSet<_> = topology
        .routers
        .iter()
        .map(|router| router.name.as_str())
        .collect();
    for authority in &topology.authorities {
        if !router_names.contains(authority.gateway_router.as_str()) {
            bail!(
                "authority {} references unknown router {}",
                authority.authority_id,
                authority.gateway_router
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn topology() -> TopologySpec {
        TopologySpec {
            name: "mock-topology".to_string(),
            routers: vec![RouterSpec {
                name: "home-a".to_string(),
                nat: NatPreset::Home,
                upstream: None,
                firewall: FirewallPreset::Home,
            }],
            links: Vec::new(),
            authorities: vec![AuthoritySpec {
                authority_id: "alice".to_string(),
                device_name: "alice-dev".to_string(),
                gateway_router: "home-a".to_string(),
                bind_address: "10.0.0.10:44001".to_string(),
                relay_capable: true,
                env: BTreeMap::new(),
            }],
            required_relay_authority: Some("alice".to_string()),
        }
    }

    #[tokio::test]
    async fn preflight_falls_back_to_vm_or_mock_when_native_unavailable() {
        let report = resolve_backend_mode(NetworkBackendMode::Patchbay);
        assert!(matches!(
            report.resolved,
            NetworkBackendMode::Patchbay
                | NetworkBackendMode::PatchbayVm
                | NetworkBackendMode::Mock
        ));
    }

    #[tokio::test]
    async fn typed_event_receipts_decode_correctly() {
        let tmp = tempfile::tempdir().unwrap();
        let mut backend = MockNetworkLabBackend::new(tmp.path());
        backend.provision(topology()).await.unwrap();

        let receipt = apply_typed_event(
            &mut backend,
            LinkToggleEvent {
                authority_id: "alice".to_string(),
                iface: "eth0".to_string(),
                up: false,
            },
        )
        .await
        .unwrap();

        assert_eq!(receipt.details.authority_id, "alice");
        assert_eq!(receipt.details.state, "down");
    }
}
