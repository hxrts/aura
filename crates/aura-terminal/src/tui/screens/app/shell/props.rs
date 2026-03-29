use super::*;

use crate::tui::updates::{HarnessCommandReceiver, UiUpdateReceiver};

pub(super) struct RuntimeShellPropsSeed {
    pub nickname_suggestion: String,
    pub show_account_setup: bool,
    pub pending_runtime_bootstrap: bool,
    pub update_rx: Arc<Mutex<Option<UiUpdateReceiver>>>,
    pub harness_command_rx: Arc<Mutex<Option<HarnessCommandReceiver>>>,
    pub bootstrap_handoff_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    pub update_tx: UiUpdateSender,
    pub callbacks: CallbackRegistry,
    #[cfg(feature = "development")]
    pub demo_mode: bool,
    #[cfg(feature = "development")]
    pub demo_alice_code: String,
    #[cfg(feature = "development")]
    pub demo_carol_code: String,
    #[cfg(feature = "development")]
    pub demo_mobile_device_id: String,
    #[cfg(feature = "development")]
    pub demo_mobile_authority_id: String,
}

/// Props for IoApp
///
/// These values are initial seeds only. Screens subscribe to `aura_app` signals
/// for live data and will overwrite these props immediately on mount.
#[derive(Default, Props)]
pub struct IoAppProps {
    // Screen data - initial seeds only (live data comes from signal subscriptions)
    pub channels: Vec<Channel>,
    pub messages: Vec<Message>,
    pub invitations: Vec<Invitation>,
    pub guardians: Vec<Guardian>,
    pub devices: Vec<Device>,
    pub nickname_suggestion: String,
    pub threshold_k: u8,
    pub threshold_n: u8,
    pub mfa_policy: MfaPolicy,
    // Contacts screen data
    pub contacts: Vec<Contact>,
    /// Discovered LAN peers
    pub discovered_peers: Vec<DiscoveredPeerInfo>,
    // Neighborhood screen data
    pub neighborhood_name: String,
    pub homes: Vec<HomeSummary>,
    pub access_level: AccessLevel,
    // Account setup
    /// Whether to show account setup modal on start
    pub show_account_setup: bool,
    /// Whether startup runtime bootstrap is still converging.
    pub pending_runtime_bootstrap: bool,
    // Network status
    /// Unified network status (disconnected, no peers, syncing, synced)
    pub network_status: NetworkStatus,
    /// Transport-level peers (active network connections)
    pub transport_peers: usize,
    /// Online contacts (people you know who are currently online)
    pub known_online: usize,
    // Demo mode
    /// Whether running in demo mode
    #[cfg(feature = "development")]
    pub demo_mode: bool,
    /// Alice's invite code (for demo mode)
    #[cfg(feature = "development")]
    pub demo_alice_code: String,
    /// Carol's invite code (for demo mode)
    #[cfg(feature = "development")]
    pub demo_carol_code: String,
    /// Mobile device id (for demo MFA shortcuts)
    #[cfg(feature = "development")]
    pub demo_mobile_device_id: String,
    /// Mobile authority id (for demo device enrollment)
    #[cfg(feature = "development")]
    pub demo_mobile_authority_id: String,
    // Reactive update channel - receiver wrapped in Arc<Mutex<Option>> for take-once semantics
    /// UI update receiver for reactive updates from callbacks
    pub update_rx: Option<Arc<Mutex<Option<UiUpdateReceiver>>>>,
    /// Dedicated harness command receiver for semantic command ingress.
    pub harness_command_rx: Option<Arc<Mutex<Option<HarnessCommandReceiver>>>>,
    /// Bootstrap handoff notification for terminating the pre-runtime shell generation.
    pub bootstrap_handoff_tx: Option<Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>>,
    /// UI update sender for sending updates from event handlers
    pub update_tx: Option<UiUpdateSender>,
    /// Callback registry for all domain actions
    pub callbacks: Option<CallbackRegistry>,
}

impl IoAppProps {
    pub(super) fn from_runtime_seed(seed: RuntimeShellPropsSeed) -> Self {
        Self {
            channels: Vec::new(),
            messages: Vec::new(),
            invitations: Vec::new(),
            guardians: Vec::new(),
            devices: Vec::new(),
            nickname_suggestion: seed.nickname_suggestion,
            threshold_k: 0,
            threshold_n: 0,
            mfa_policy: MfaPolicy::SensitiveOnly,
            contacts: Vec::new(),
            discovered_peers: Vec::new(),
            neighborhood_name: String::from("Neighborhood"),
            homes: Vec::new(),
            access_level: AccessLevel::Limited,
            show_account_setup: seed.show_account_setup,
            pending_runtime_bootstrap: seed.pending_runtime_bootstrap,
            network_status: NetworkStatus::Disconnected,
            transport_peers: 0,
            known_online: 0,
            #[cfg(feature = "development")]
            demo_mode: seed.demo_mode,
            #[cfg(feature = "development")]
            demo_alice_code: seed.demo_alice_code,
            #[cfg(feature = "development")]
            demo_carol_code: seed.demo_carol_code,
            #[cfg(feature = "development")]
            demo_mobile_device_id: seed.demo_mobile_device_id,
            #[cfg(feature = "development")]
            demo_mobile_authority_id: seed.demo_mobile_authority_id,
            update_rx: Some(seed.update_rx),
            harness_command_rx: Some(seed.harness_command_rx),
            bootstrap_handoff_tx: Some(seed.bootstrap_handoff_tx),
            update_tx: Some(seed.update_tx),
            callbacks: Some(seed.callbacks),
        }
    }
}
