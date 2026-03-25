use super::*;

/// Error returned when IoContextBuilder cannot build an IoContext.
#[derive(Debug, Clone)]
pub enum ContextBuildError {
    /// Required field was not set
    MissingField(&'static str),
}

impl std::fmt::Display for ContextBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextBuildError::MissingField(field) => {
                write!(f, "IoContextBuilder: missing required field '{field}'")
            }
        }
    }
}

impl std::error::Error for ContextBuildError {}

/// Builder for constructing IoContext with flexible configuration.
///
/// # Example
///
/// ```rust,ignore
/// let ctx = IoContext::builder()
///     .with_app_core(app_core)
///     .with_base_path(PathBuf::from("./data"))
///     .with_device_id("device-1".to_string())
///     .with_mode(TuiMode::Production)
///     .build()?;
/// ```
#[derive(Default)]
pub struct IoContextBuilder {
    app_core: Option<InitializedAppCore>,
    base_path: Option<PathBuf>,
    device_id: Option<String>,
    mode: Option<TuiMode>,
    has_existing_account: bool,
    pending_runtime_bootstrap: bool,
    #[cfg_attr(feature = "development", doc = "Demo configuration fields")]
    #[cfg(feature = "development")]
    demo_hints: Option<crate::demo::DemoHints>,
    #[cfg(feature = "development")]
    demo_bridge: Option<Arc<crate::demo::SimulatedBridge>>,
    #[cfg(feature = "development")]
    demo_mobile_agent: Option<Arc<AuraAgent>>,
    #[cfg(feature = "development")]
    demo_mobile_device_id: Option<String>,
    #[cfg(feature = "development")]
    demo_mobile_authority_id: Option<String>,
}

impl IoContextBuilder {
    /// Create a new builder with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the initialized AppCore (required).
    #[must_use]
    pub fn with_app_core(mut self, app_core: InitializedAppCore) -> Self {
        self.app_core = Some(app_core);
        self
    }

    /// Set the base path for account files (required).
    #[must_use]
    pub fn with_base_path(mut self, path: PathBuf) -> Self {
        self.base_path = Some(path);
        self
    }

    /// Set the device ID string (required).
    #[must_use]
    pub fn with_device_id(mut self, id: String) -> Self {
        self.device_id = Some(id);
        self
    }

    /// Set the TUI mode (required).
    #[must_use]
    pub fn with_mode(mut self, mode: TuiMode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// Set whether an existing account is present (default: false).
    #[must_use]
    pub fn with_existing_account(mut self, exists: bool) -> Self {
        self.has_existing_account = exists;
        self
    }

    #[must_use]
    pub fn with_pending_runtime_bootstrap(mut self, pending: bool) -> Self {
        self.pending_runtime_bootstrap = pending;
        self
    }

    cfg_if! {
        if #[cfg(feature = "development")] {
            /// Set demo hints for development mode.
            pub fn with_demo_hints(mut self, hints: crate::demo::DemoHints) -> Self {
                self.demo_hints = Some(hints);
                self
            }

            /// Set the demo bridge for routing commands to simulated agents.
            pub fn with_demo_bridge(mut self, bridge: Arc<crate::demo::SimulatedBridge>) -> Self {
                self.demo_bridge = Some(bridge);
                self
            }

            /// Set the demo Mobile agent for device enrollment flows.
            pub fn with_demo_mobile_agent(mut self, agent: Arc<AuraAgent>) -> Self {
                self.demo_mobile_agent = Some(agent);
                self
            }

            /// Set the demo Mobile device id for MFA shortcuts.
            pub fn with_demo_mobile_device_id(mut self, device_id: String) -> Self {
                self.demo_mobile_device_id = Some(device_id);
                self
            }

            pub fn with_demo_mobile_authority_id(mut self, authority_id: String) -> Self {
                self.demo_mobile_authority_id = Some(authority_id);
                self
            }
        }
    }

    /// Build the IoContext, returning an error if required fields are missing.
    pub fn build(self) -> Result<IoContext, ContextBuildError> {
        let app_core = self
            .app_core
            .ok_or(ContextBuildError::MissingField("app_core"))?;
        let base_path = self
            .base_path
            .ok_or(ContextBuildError::MissingField("base_path"))?;
        let device_id = self
            .device_id
            .ok_or(ContextBuildError::MissingField("device_id"))?;
        // Mode is required in the builder but no longer used here. Keep the
        // check to preserve the public API contract.
        let _mode = self.mode.ok_or(ContextBuildError::MissingField("mode"))?;

        let tasks = Arc::new(UiTaskOwner::new());
        let operational = Arc::new(OperationalHandler::new(
            app_core.raw().clone(),
            tasks.clone(),
        ));
        let snapshots = SnapshotHelper::new(app_core.raw().clone(), device_id.clone());
        let toasts = ToastHelper::new();

        let has_existing_account = Arc::new(std::sync::atomic::AtomicBool::new(
            self.has_existing_account,
        ));
        let account_files = AccountFilesHelper::new(base_path, device_id, has_existing_account);

        let invited_lan_peers = Arc::new(RwLock::new(HashSet::new()));
        let current_context = Arc::new(RwLock::new(None));
        let channel_modes = Arc::new(RwLock::new(HashMap::new()));
        let ceremony_handles = Arc::new(RwLock::new(HashMap::new()));
        let requested_shell_exit = Arc::new(std::sync::Mutex::new(None));

        let dispatch = DispatchHelper::new(
            operational.clone(),
            snapshots.clone(),
            toasts.clone(),
            account_files.clone(),
            invited_lan_peers.clone(),
            current_context.clone(),
            channel_modes.clone(),
        );

        Ok(IoContext {
            app_core,
            operational,
            dispatch,
            snapshots,
            toasts,
            account_files,
            #[cfg(feature = "development")]
            demo_hints: self.demo_hints,
            #[cfg(feature = "development")]
            demo_bridge: self.demo_bridge,
            #[cfg(feature = "development")]
            demo_mobile_agent: self.demo_mobile_agent,
            #[cfg(feature = "development")]
            demo_mobile_device_id: self.demo_mobile_device_id,
            #[cfg(feature = "development")]
            demo_mobile_authority_id: self.demo_mobile_authority_id,
            invited_lan_peers,
            current_context,
            channel_modes,
            ceremony_handles,
            tasks,
            pending_runtime_bootstrap: self.pending_runtime_bootstrap,
            requested_shell_exit,
        })
    }
}
