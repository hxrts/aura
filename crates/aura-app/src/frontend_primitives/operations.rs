//! Shared frontend operation labels reused by Layer 7 shells.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrontendUiOperation {
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
    UpdateThreshold,
    ImportDeviceEnrollmentCode,
    StartGuardianCeremony,
    StartMultifactorCeremony,
    CancelGuardianCeremony,
    CancelKeyRotationCeremony,
}

impl FrontendUiOperation {
    #[must_use]
    pub const fn label(self) -> &'static str {
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
            Self::UpdateThreshold => "Update threshold",
            Self::ImportDeviceEnrollmentCode => "Import device enrollment code",
            Self::StartGuardianCeremony => "Start guardian ceremony",
            Self::StartMultifactorCeremony => "Start multifactor ceremony",
            Self::CancelGuardianCeremony => "Cancel guardian ceremony",
            Self::CancelKeyRotationCeremony => "Cancel key rotation ceremony",
        }
    }

    #[must_use]
    pub const fn routes_to_account_setup_modal(self) -> bool {
        matches!(self, Self::CreateAccount)
    }
}

#[cfg(test)]
mod tests {
    use super::FrontendUiOperation;

    #[test]
    fn labels_remain_stable_for_shared_shell_operations() {
        assert_eq!(FrontendUiOperation::CreateAccount.label(), "Create account");
        assert_eq!(
            FrontendUiOperation::ImportDeviceEnrollmentCode.label(),
            "Import device enrollment code"
        );
        assert_eq!(
            FrontendUiOperation::BackgroundSync.label(),
            "Run background sync"
        );
    }

    #[test]
    fn only_create_account_routes_to_account_setup_modal() {
        assert!(FrontendUiOperation::CreateAccount.routes_to_account_setup_modal());
        assert!(!FrontendUiOperation::UpdateThreshold.routes_to_account_setup_modal());
        assert!(!FrontendUiOperation::BackgroundSync.routes_to_account_setup_modal());
    }
}
