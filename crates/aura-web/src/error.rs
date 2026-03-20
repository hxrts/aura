use aura_app::errors::ErrorCategory;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WebUiOperation {
    BootstrapController,
    StageInitialAccountBootstrap,
    PersistSelectedRuntimeIdentity,
    ClearStorageKey,
    LoadSelectedRuntimeIdentity,
    LoadPendingAccountBootstrap,
    PersistPendingAccountBootstrap,
    LoadPendingDeviceEnrollmentCode,
    PersistPendingDeviceEnrollmentCode,
    ClearPendingDeviceEnrollmentCode,
    ApplyHarnessModeDocumentFlags,
    InstallHarnessInstrumentation,
    ProcessCeremonyAcceptances,
    BackgroundSync,
    RefreshBootstrapSettings,
    InspectBootstrapRuntime,
    MirrorClipboardToHarness,
    NotifyHarnessClipboardDriver,
    WriteSystemClipboard,
    SubmitBootstrapHandoff,
    CreateAccount,
    ImportDeviceEnrollmentCode,
}

impl WebUiOperation {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::BootstrapController => "Bootstrap web runtime",
            Self::StageInitialAccountBootstrap => "Stage initial account bootstrap",
            Self::PersistSelectedRuntimeIdentity => "Persist selected runtime identity",
            Self::ClearStorageKey => "Clear browser storage key",
            Self::LoadSelectedRuntimeIdentity => "Load selected runtime identity",
            Self::LoadPendingAccountBootstrap => "Load pending account bootstrap",
            Self::PersistPendingAccountBootstrap => "Persist pending account bootstrap",
            Self::LoadPendingDeviceEnrollmentCode => "Load pending device enrollment code",
            Self::PersistPendingDeviceEnrollmentCode => "Persist pending device enrollment code",
            Self::ClearPendingDeviceEnrollmentCode => "Clear pending device enrollment code",
            Self::ApplyHarnessModeDocumentFlags => "Apply harness mode document flags",
            Self::InstallHarnessInstrumentation => "Install harness instrumentation",
            Self::ProcessCeremonyAcceptances => "Process ceremony acceptances",
            Self::BackgroundSync => "Run background sync",
            Self::RefreshBootstrapSettings => "Refresh bootstrap settings",
            Self::InspectBootstrapRuntime => "Inspect bootstrap runtime",
            Self::MirrorClipboardToHarness => "Mirror clipboard to harness",
            Self::NotifyHarnessClipboardDriver => "Notify harness clipboard driver",
            Self::WriteSystemClipboard => "Write system clipboard",
            Self::SubmitBootstrapHandoff => "Submit bootstrap handoff",
            Self::CreateAccount => "Create account",
            Self::ImportDeviceEnrollmentCode => "Import device enrollment code",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WebUiError {
    operation: WebUiOperation,
    category: ErrorCategory,
    code: &'static str,
    message: String,
}

impl WebUiError {
    pub(crate) fn new(
        operation: WebUiOperation,
        category: ErrorCategory,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            operation,
            category,
            code,
            message: message.into(),
        }
    }

    pub(crate) fn config(
        operation: WebUiOperation,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self::new(operation, ErrorCategory::Config, code, message)
    }

    pub(crate) fn input(
        operation: WebUiOperation,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self::new(operation, ErrorCategory::Input, code, message)
    }

    pub(crate) fn operation(
        operation: WebUiOperation,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self::new(operation, ErrorCategory::Operation, code, message)
    }

    pub(crate) fn with_operation(&self, operation: WebUiOperation) -> Self {
        Self {
            operation,
            category: self.category,
            code: self.code,
            message: self.message.clone(),
        }
    }

    pub(crate) fn user_message(&self) -> String {
        format!(
            "[{}] {}: {}. Hint: {}",
            self.code,
            self.operation.label(),
            self.message,
            self.category.resolution_hint()
        )
    }
}

impl std::fmt::Display for WebUiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.user_message())
    }
}

impl std::error::Error for WebUiError {}

pub(crate) fn log_web_error(level: &str, error: &WebUiError) {
    let rendered = error.user_message();
    match level {
        "error" => web_sys::console::error_1(&rendered.into()),
        "warn" => web_sys::console::warn_1(&rendered.into()),
        _ => web_sys::console::log_1(&rendered.into()),
    }
}
