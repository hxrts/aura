use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use cfg_if::cfg_if;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub mod scenario_builder;
pub mod topology_adapter;

mod patchbay_backend;

pub use patchbay_backend::PatchbayBackend;

/// Supported network lab backend modes.
///
/// `Patchbay` is Linux-only (network namespaces); the `mock` backend records
/// events without any network. The former `patchbay-vm` mode was retired: it
/// never ran an Aura process, and Linux coverage comes from a Linux host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkBackendMode {
    Mock,
    Patchbay,
}

impl fmt::Display for NetworkBackendMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mock => f.write_str("mock"),
            Self::Patchbay => f.write_str("patchbay"),
        }
    }
}

impl FromStr for NetworkBackendMode {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "mock" => Ok(Self::Mock),
            "patchbay" => Ok(Self::Patchbay),
            other => Err(anyhow!(
                "unknown network backend '{other}', expected one of: mock, patchbay"
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

/// Deployment-named NAT presets. `topology_adapter::to_patchbay_nat` maps them
/// onto patchbay's RFC 4787 behavior tiers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NatPreset {
    None,
    Home,
    Corporate,
    FullCone,
    Cgnat,
    CloudNat,
    /// RFC 3489 "restricted cone": endpoint-independent mapping with
    /// address-dependent filtering. Sits between `Home` (port-restricted) and
    /// `FullCone`: any port of a contacted host gets back in.
    Restricted,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FirewallPreset {
    Open,
    Home,
    Corporate,
    UdpBlocked,
}

/// Link impairment presets. The named ones map onto patchbay's presets; the
/// stress-lane ones add clustered loss and a bottleneck buffer; `Custom`
/// carries explicit parameters so a scenario can tune without a code change.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LinkConditionPreset {
    Lan,
    Wifi,
    WifiBad,
    Mobile4G,
    Mobile3G,
    Satellite,
    /// Wifi with Gilbert-Elliott bursty loss: fades drop runs of packets
    /// rather than one here and there, which stresses acknowledgment windows
    /// more than the same rate spread evenly.
    WifiBursty,
    /// 4G behind a congested bottleneck: rate-capped with a buffer sized to the
    /// round trip, so loss emerges from queue overflow instead of a dice roll.
    Mobile4GCongested,
    /// Explicit parameters. Loss is in tenths of a percent so the preset stays
    /// `Eq`/`Hash`; `loss_burst_pkts` selects bursty loss, `buffer_ms` bounds
    /// the rate-limiter queue (only with `rate_kbit`).
    Custom {
        latency_ms: u32,
        jitter_ms: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rate_kbit: Option<u32>,
        #[serde(default)]
        loss_tenths_pct: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        loss_burst_pkts: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        buffer_ms: Option<u32>,
    },
}

impl LinkConditionPreset {
    /// Short name for artifact directories and reports, so a stress failure is
    /// attributed to the condition it ran under.
    pub fn slug(&self) -> &'static str {
        match self {
            Self::Lan => "lan",
            Self::Wifi => "wifi",
            Self::WifiBad => "wifi-bad",
            Self::Mobile4G => "mobile-4g",
            Self::Mobile3G => "mobile-3g",
            Self::Satellite => "satellite",
            Self::WifiBursty => "wifi-bursty",
            Self::Mobile4GCongested => "mobile-4g-congested",
            Self::Custom { .. } => "custom",
        }
    }
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
    /// Directory where the backend's own run log lives, if it keeps one. Read
    /// after the backend is dropped to obtain the flushed, complete log.
    fn run_dir(&self) -> Option<PathBuf> {
        None
    }
    /// Stop any packet captures the backend started at provisioning so their
    /// files are complete before artifacts are collected. Idempotent.
    fn stop_captures(&mut self) -> Result<()> {
        Ok(())
    }
    /// Namespace an instance process for `authority_id` must be spawned in.
    /// Backends without isolated namespaces return the host namespace.
    fn instance_namespace(&self, authority_id: &str) -> Result<InstanceNamespace> {
        let _ = authority_id;
        Ok(InstanceNamespace::host())
    }
}

/// Network namespace in which a harness instance process runs.
///
/// Patchbay namespaces are anonymous: they exist only as file descriptors held
/// by the lab and have no `/run/netns` entry, so they cannot be entered with
/// `ip netns exec` or `nsenter`. The only way to put a child in one is to fork
/// from a thread that already sits inside it, which is what [`Self::run_sync`]
/// provides. The launcher forks the PTY child inside that closure.
#[derive(Clone)]
pub struct InstanceNamespace {
    inner: NamespaceInner,
}

#[derive(Clone)]
enum NamespaceInner {
    Host,
    #[cfg(all(target_os = "linux", feature = "patchbay-backend"))]
    Patchbay {
        device: patchbay::Device,
    },
}

impl fmt::Debug for InstanceNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => write!(f, "InstanceNamespace({name})"),
            None => f.write_str("InstanceNamespace(host)"),
        }
    }
}

impl InstanceNamespace {
    /// The host network namespace: processes spawn as they do today.
    pub fn host() -> Self {
        Self {
            inner: NamespaceInner::Host,
        }
    }

    #[cfg(all(target_os = "linux", feature = "patchbay-backend"))]
    pub(crate) fn patchbay(device: patchbay::Device) -> Self {
        Self {
            inner: NamespaceInner::Patchbay { device },
        }
    }

    pub fn is_host(&self) -> bool {
        matches!(self.inner, NamespaceInner::Host)
    }

    /// Lab namespace name, or `None` for the host namespace.
    pub fn name(&self) -> Option<&str> {
        match &self.inner {
            NamespaceInner::Host => None,
            #[cfg(all(target_os = "linux", feature = "patchbay-backend"))]
            NamespaceInner::Patchbay { device } => Some(device.ns()),
        }
    }

    /// IPv4 address of the device's default interface inside the lab.
    pub fn lab_ipv4(&self) -> Option<std::net::Ipv4Addr> {
        match &self.inner {
            NamespaceInner::Host => None,
            #[cfg(all(target_os = "linux", feature = "patchbay-backend"))]
            NamespaceInner::Patchbay { device } => device.ip(),
        }
    }

    /// Run `f` inside this namespace and return its result. Anything `f`
    /// forks or binds lives in the namespace. In the host namespace `f` runs
    /// on the calling thread.
    pub fn run_sync<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce() -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        match &self.inner {
            NamespaceInner::Host => f(),
            #[cfg(all(target_os = "linux", feature = "patchbay-backend"))]
            NamespaceInner::Patchbay { device } => device
                .run_sync(f)
                .with_context(|| format!("run inside lab namespace {}", device.ns())),
        }
    }

    /// Rewrite the host part of `bind_address` to this namespace's lab address,
    /// keeping the port. Host-namespace instances keep their address.
    pub fn rewrite_bind_address(&self, bind_address: &str) -> Result<String> {
        let Some(ip) = self.lab_ipv4() else {
            return Ok(bind_address.to_string());
        };
        let (_, port) = bind_address
            .trim()
            .rsplit_once(':')
            .ok_or_else(|| anyhow!("bind_address must be host:port, got {bind_address}"))?;
        Ok(format!("{ip}:{port}"))
    }
}

/// A provisioned network lab owned for the duration of a harness run.
///
/// Owns the backend, the lab runtime description and the Tokio runtime the
/// async backend methods execute on. The current-thread runtime creates no
/// threads of its own, so provisioning can follow
/// [`prepare_native_patchbay`] while the process is still single-threaded.
/// Drop order: instances must be stopped before this is dropped, since the
/// namespaces disappear with it.
pub struct ProvisionedLab {
    runtime: tokio::runtime::Runtime,
    backend: Box<dyn NetworkLabBackend>,
    lab_runtime: LabRuntime,
    /// Receipts of every event applied through this lab, for the timeline.
    receipts: Vec<RawEventReceipt>,
}

/// Artifacts of a finished lab: the live collection plus the complete timeline
/// rebuilt from the flushed lab event log after teardown.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FinishedLabArtifacts {
    pub bundle: ArtifactBundle,
    /// Lab events in the final timeline; zero means the lab wrote no event log.
    pub lab_event_count: usize,
    pub harness_event_count: usize,
}

impl fmt::Debug for ProvisionedLab {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProvisionedLab")
            .field("backend_mode", &self.backend.backend_mode())
            .field("topology", &self.lab_runtime.topology_name)
            .finish()
    }
}

impl ProvisionedLab {
    /// Build the backend for `mode`, provision `topology` and retain both.
    pub fn provision(
        mode: NetworkBackendMode,
        topology: TopologySpec,
        artifact_root: &Path,
    ) -> Result<Self> {
        std::fs::create_dir_all(artifact_root)
            .with_context(|| format!("create lab artifact root {}", artifact_root.display()))?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("build network lab runtime")?;
        let mut backend = build_backend(mode, artifact_root)?;
        let lab_runtime = runtime
            .block_on(backend.provision(topology))
            .context("provision network lab")?;
        Ok(Self {
            runtime,
            backend,
            lab_runtime,
            receipts: Vec::new(),
        })
    }

    pub fn lab_runtime(&self) -> &LabRuntime {
        &self.lab_runtime
    }

    pub fn backend_mode(&self) -> NetworkBackendMode {
        self.backend.backend_mode()
    }

    pub fn authority_context(&self, authority_id: &str) -> Result<&AuthorityNetworkContext> {
        self.lab_runtime
            .authorities
            .get(authority_id)
            .ok_or_else(|| anyhow!("authority {authority_id} is not mapped in the network lab"))
    }

    pub fn instance_namespace(&self, authority_id: &str) -> Result<InstanceNamespace> {
        self.authority_context(authority_id)?;
        self.backend.instance_namespace(authority_id)
    }

    pub fn apply_event(&mut self, event: NetworkEvent) -> Result<RawEventReceipt> {
        let receipt = self.runtime.block_on(self.backend.apply_event(event))?;
        self.receipts.push(receipt.clone());
        Ok(receipt)
    }

    /// Collect artifacts while the lab is alive. The timeline in the returned
    /// bundle is provisional; see [`Self::finish`].
    pub fn collect_artifacts(&self, out_dir: &Path) -> Result<ArtifactBundle> {
        self.runtime
            .block_on(self.backend.collect_artifacts(out_dir))
    }

    /// Collect artifacts, tear the lab down, then rebuild the timeline from the
    /// lab's flushed event log so it covers provisioning through teardown.
    ///
    /// Instances must already be stopped: their namespaces disappear here.
    pub fn finish(mut self, out_dir: &Path) -> Result<FinishedLabArtifacts> {
        self.backend.stop_captures()?;
        let mut bundle = self.collect_artifacts(out_dir)?;
        let lab_events_log = self.backend.run_dir().map(|dir| dir.join(LAB_EVENTS_FILE));
        let Self {
            runtime,
            backend,
            receipts,
            ..
        } = self;
        // Patchbay's event writer is a task on this runtime, so it only makes
        // progress while something blocks on the runtime. Drop the lab inside a
        // block_on and yield afterwards so the writer appends the queued events
        // (including the teardown ones the drop emits) before the final flush;
        // the runtime goes after the lab it served.
        runtime.block_on(async move {
            drop(backend);
            for _ in 0..16 {
                tokio::task::yield_now().await;
            }
        });
        drop(runtime);

        let mut lab_event_count = 0;
        if let Some(log) = lab_events_log {
            let entries = build_network_timeline(&log, &receipts, false)?;
            lab_event_count = entries
                .iter()
                .filter(|entry| matches!(entry, TimelineEntry::Lab { .. }))
                .count();
            // Keep the complete log next to the live copy, which was taken
            // before the flush and may be empty.
            if log.exists() {
                std::fs::copy(&log, bundle.root.join(LAB_EVENTS_FILE))
                    .with_context(|| format!("copy {}", log.display()))?;
            }
            bundle.timeline = Some(write_network_timeline(out_dir, &entries)?);
            bundle
                .metadata
                .insert("timeline_entries".to_string(), entries.len().to_string());
            bundle.metadata.remove("timeline_provisional");
        }
        Ok(FinishedLabArtifacts {
            bundle,
            lab_event_count,
            harness_event_count: receipts.len(),
        })
    }
}

/// Load a `TopologySpec` from a TOML file and validate it.
pub fn load_topology(path: &Path) -> Result<TopologySpec> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read network topology {}", path.display()))?;
    let topology: TopologySpec = toml::from_str(&text)
        .with_context(|| format!("parse network topology {}", path.display()))?;
    scenario_builder::validate_topology(&topology)
        .with_context(|| format!("validate network topology {}", path.display()))?;
    Ok(topology)
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

    // Only an actual fallback is recorded; a run that configures a topology
    // refuses it (see the harness binary), so this stays a preflight fact.
    let resolved = match requested {
        NetworkBackendMode::Mock => NetworkBackendMode::Mock,
        NetworkBackendMode::Patchbay if native_supported => NetworkBackendMode::Patchbay,
        NetworkBackendMode::Patchbay => NetworkBackendMode::Mock,
    };

    BackendPreflightReport {
        requested,
        resolved,
        checks,
    }
}

/// Enter the user namespace that the native patchbay backend requires.
///
/// Patchbay refuses to build a lab as real root outside a user namespace, and
/// the namespace must be entered while the process is still single-threaded,
/// so call this from `main` before any runtime exists. Idempotent. On hosts
/// without the native backend it is a no-op.
pub fn prepare_native_patchbay() -> Result<()> {
    cfg_if! {
        if #[cfg(all(target_os = "linux", feature = "patchbay-backend"))] {
            patchbay::init_userns().context("enter user namespace for native patchbay")
        } else {
            Ok(())
        }
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

fn native_patchbay_supported() -> bool {
    cfg_if! {
        if #[cfg(all(target_os = "linux", feature = "patchbay-backend"))] {
            let userns_ok = read_userns_clone_flag().unwrap_or(true);
            userns_ok && patchbay::check_caps().is_ok()
        } else {
            false
        }
    }
}

cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "patchbay-backend"))] {
        fn read_userns_clone_flag() -> Option<bool> {
            let path = "/proc/sys/kernel/unprivileged_userns_clone";
            let text = std::fs::read_to_string(path).ok()?;
            Some(text.trim() == "1")
        }
    }
}

fn unix_time_ms() -> u64 {
    use std::time::UNIX_EPOCH;

    UNIX_EPOCH
        .elapsed()
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

/// Interface name of every lab device's single uplink toward its gateway router.
/// The topology adapter creates devices with exactly this interface.
pub const DEVICE_UPLINK_IFACE: &str = "eth0";

/// Where a `LinkSpec` or `SetLinkCondition` lands in the provisioned lab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum LinkTarget {
    /// A device and its own gateway router: the condition applies on the
    /// device's uplink interface and affects only that authority.
    DeviceUplink {
        authority_id: String,
        device_name: String,
        gateway_router: String,
    },
    /// Two adjacent routers: patchbay exposes no per-interface control on
    /// routers, so the condition applies on the downstream router's downlink
    /// bridge and affects every authority whose gateway chain passes through
    /// that router.
    RouterDownlink {
        downstream_router: String,
        upstream_router: String,
        affected_authorities: Vec<String>,
    },
}

impl LinkTarget {
    pub fn affected_authorities(&self) -> Vec<String> {
        match self {
            Self::DeviceUplink { authority_id, .. } => vec![authority_id.clone()],
            Self::RouterDownlink {
                affected_authorities,
                ..
            } => affected_authorities.clone(),
        }
    }
}

/// Resolve the two endpoints of a link to the node that owns it.
///
/// Fails when either endpoint is not a device or router of `spec`, or when the
/// endpoints are not adjacent (a device with a router that is not its gateway,
/// or two routers without an upstream relation).
pub fn classify_link(spec: &TopologySpec, left: &str, right: &str) -> Result<LinkTarget> {
    let device = |name: &str| {
        spec.authorities
            .iter()
            .find(|authority| authority.device_name == name)
    };
    let router = |name: &str| spec.routers.iter().find(|router| router.name == name);

    match (device(left), device(right)) {
        (Some(_), Some(_)) => bail!(
            "link {left} <-> {right} joins two devices; devices only link to their gateway router"
        ),
        (Some(authority), None) | (None, Some(authority)) => {
            let router_name = if authority.device_name == left {
                right
            } else {
                left
            };
            if router(router_name).is_none() {
                bail!("link {left} <-> {right}: {router_name} is neither a device nor a router");
            }
            if authority.gateway_router != router_name {
                bail!(
                    "link {left} <-> {right}: device {} is behind {}, not {router_name}",
                    authority.device_name,
                    authority.gateway_router
                );
            }
            Ok(LinkTarget::DeviceUplink {
                authority_id: authority.authority_id.clone(),
                device_name: authority.device_name.clone(),
                gateway_router: authority.gateway_router.clone(),
            })
        }
        (None, None) => {
            let (Some(left_router), Some(right_router)) = (router(left), router(right)) else {
                bail!("link {left} <-> {right}: both endpoints must be devices or routers of the topology");
            };
            let downstream = if left_router.upstream.as_deref() == Some(right) {
                left_router
            } else if right_router.upstream.as_deref() == Some(left) {
                right_router
            } else {
                bail!("link {left} <-> {right}: routers are not adjacent (neither is the other's upstream)");
            };
            let upstream = downstream
                .upstream
                .clone()
                .unwrap_or_else(|| "unreachable".to_string());
            Ok(LinkTarget::RouterDownlink {
                affected_authorities: authorities_behind_router(spec, &downstream.name),
                downstream_router: downstream.name.clone(),
                upstream_router: upstream,
            })
        }
    }
}

/// Authorities whose gateway chain passes through `router`, in topology order.
pub fn authorities_behind_router(spec: &TopologySpec, router: &str) -> Vec<String> {
    let upstream_of = |name: &str| -> Option<&str> {
        spec.routers
            .iter()
            .find(|candidate| candidate.name == name)
            .and_then(|candidate| candidate.upstream.as_deref())
    };
    spec.authorities
        .iter()
        .filter(|authority| {
            let mut hop = Some(authority.gateway_router.as_str());
            let mut visited = BTreeSet::new();
            while let Some(current) = hop {
                if current == router {
                    return true;
                }
                if !visited.insert(current) {
                    break;
                }
                hop = upstream_of(current);
            }
            false
        })
        .map(|authority| authority.authority_id.clone())
        .collect()
}

/// File name of the backend network timeline inside an artifact directory.
pub const NETWORK_TIMELINE_FILE: &str = "network_timeline.json";

/// File name of patchbay's own event log inside its run directory.
pub const LAB_EVENTS_FILE: &str = "events.jsonl";

/// One entry of the backend network timeline written as `network_timeline.json`.
///
/// Entries are ordered by `recorded_at_ms`. Lab entries come from the lab's own
/// event log (`events.jsonl`), which patchbay buffers and flushes when the lab
/// is dropped, so a timeline built while the lab is alive is provisional and
/// [`ProvisionedLab::finish`] rebuilds it after teardown.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "source")]
pub enum TimelineEntry {
    /// An event the lab itself emitted (provisioning, link, NAT, firewall,
    /// teardown). `opid` is the lab's monotone operation id.
    Lab {
        recorded_at_ms: u64,
        opid: u64,
        event: serde_json::Value,
    },
    /// A harness-applied `NetworkEvent` and its receipt.
    Harness {
        recorded_at_ms: u64,
        receipt: RawEventReceipt,
    },
}

impl TimelineEntry {
    pub fn recorded_at_ms(&self) -> u64 {
        match self {
            Self::Lab { recorded_at_ms, .. } | Self::Harness { recorded_at_ms, .. } => {
                *recorded_at_ms
            }
        }
    }
}

/// Merge the lab's event log with the harness receipts into one ordered timeline.
///
/// A missing or empty log yields only the harness entries. A malformed line is
/// an error, since a partially read log would misrepresent what happened, with
/// one exception: while the lab is alive (`allow_partial_tail`) the writer may
/// be mid-line, so an unparsable final line is dropped as not yet written.
pub fn build_network_timeline(
    lab_events_log: &Path,
    receipts: &[RawEventReceipt],
    allow_partial_tail: bool,
) -> Result<Vec<TimelineEntry>> {
    let mut entries: Vec<TimelineEntry> = receipts
        .iter()
        .map(|receipt| TimelineEntry::Harness {
            recorded_at_ms: receipt.applied_at_ms,
            receipt: receipt.clone(),
        })
        .collect();

    if lab_events_log.exists() {
        let text = std::fs::read_to_string(lab_events_log)
            .with_context(|| format!("read lab event log {}", lab_events_log.display()))?;
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let parsed: std::result::Result<serde_json::Value, _> = serde_json::from_str(line);
            let event = match parsed {
                Ok(event) => event,
                Err(_) if allow_partial_tail && index + 1 == lines.len() => break,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("parse lab event {}:{}", lab_events_log.display(), index + 1)
                    })
                }
            };
            let opid = event
                .get("opid")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| anyhow!("lab event {} has no opid", index + 1))?;
            let timestamp = event
                .get("timestamp")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("lab event {opid} has no timestamp"))?;
            let recorded_at_ms = chrono::DateTime::parse_from_rfc3339(timestamp)
                .with_context(|| format!("parse lab event {opid} timestamp {timestamp}"))?
                .timestamp_millis();
            entries.push(TimelineEntry::Lab {
                recorded_at_ms: u64::try_from(recorded_at_ms).unwrap_or_default(),
                opid,
                event,
            });
        }
    }

    // Stable: lab entries keep opid order among equal timestamps because the
    // log is appended in opid order.
    entries.sort_by_key(TimelineEntry::recorded_at_ms);
    Ok(entries)
}

/// Write `entries` as `network_timeline.json` under `out_dir` and return its path.
pub fn write_network_timeline(out_dir: &Path, entries: &[TimelineEntry]) -> Result<PathBuf> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("create timeline directory {}", out_dir.display()))?;
    let path = out_dir.join(NETWORK_TIMELINE_FILE);
    std::fs::write(&path, serde_json::to_vec_pretty(entries)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(path)
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
    async fn preflight_falls_back_to_mock_when_native_unavailable() {
        let report = resolve_backend_mode(NetworkBackendMode::Patchbay);
        assert!(matches!(
            report.resolved,
            NetworkBackendMode::Patchbay | NetworkBackendMode::Mock
        ));
        assert_eq!(
            report.checks.len(),
            1,
            "native support is the only preflight check"
        );
        assert!("patchbay-vm".parse::<NetworkBackendMode>().is_err());
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

    fn chained_topology() -> TopologySpec {
        let router = |name: &str, upstream: Option<&str>| RouterSpec {
            name: name.to_string(),
            nat: NatPreset::Home,
            upstream: upstream.map(str::to_string),
            firewall: FirewallPreset::Home,
        };
        let authority = |id: &str, gateway: &str| AuthoritySpec {
            authority_id: id.to_string(),
            device_name: format!("{id}-dev"),
            gateway_router: gateway.to_string(),
            bind_address: "0.0.0.0:0".to_string(),
            relay_capable: id == "relay",
            env: BTreeMap::new(),
        };
        TopologySpec {
            name: "chained".to_string(),
            routers: vec![
                router("core-dc", None),
                router("edge-b", Some("core-dc")),
                router("leaf-c", Some("edge-b")),
            ],
            links: Vec::new(),
            authorities: vec![
                authority("relay", "core-dc"),
                authority("bob", "edge-b"),
                authority("carol", "leaf-c"),
            ],
            required_relay_authority: Some("relay".to_string()),
        }
    }

    #[test]
    fn device_link_resolves_to_its_uplink_and_only_that_authority() {
        let spec = chained_topology();
        let target = classify_link(&spec, "edge-b", "bob-dev").unwrap();
        assert_eq!(
            target,
            LinkTarget::DeviceUplink {
                authority_id: "bob".to_string(),
                device_name: "bob-dev".to_string(),
                gateway_router: "edge-b".to_string(),
            }
        );
        assert_eq!(target.affected_authorities(), vec!["bob".to_string()]);
    }

    #[test]
    fn router_link_resolves_to_downstream_router_and_everything_behind_it() {
        let spec = chained_topology();
        let target = classify_link(&spec, "core-dc", "edge-b").unwrap();
        assert_eq!(
            target,
            LinkTarget::RouterDownlink {
                downstream_router: "edge-b".to_string(),
                upstream_router: "core-dc".to_string(),
                affected_authorities: vec!["bob".to_string(), "carol".to_string()],
            }
        );
        let leaf = classify_link(&spec, "leaf-c", "edge-b").unwrap();
        assert_eq!(leaf.affected_authorities(), vec!["carol".to_string()]);
    }

    #[test]
    fn non_adjacent_or_unknown_links_are_rejected() {
        let spec = chained_topology();
        assert!(classify_link(&spec, "bob-dev", "core-dc").is_err());
        assert!(classify_link(&spec, "core-dc", "leaf-c").is_err());
        assert!(classify_link(&spec, "core-dc", "nowhere").is_err());
        assert!(classify_link(&spec, "bob-dev", "carol-dev").is_err());
    }
}
