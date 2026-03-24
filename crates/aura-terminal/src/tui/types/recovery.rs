use aura_app::ui::types::recovery::{
    Guardian as AppGuardian, GuardianStatus as AppGuardianStatus,
    RecoveryApproval as AppRecoveryApproval, RecoveryProcess as AppRecoveryProcess,
    RecoveryProcessStatus as AppRecoveryProcessStatus, RecoveryState as AppRecoveryState,
};
use iocraft::prelude::Color;

use crate::tui::theme::Theme;

/// Recovery screen tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecoveryTab {
    #[default]
    Guardians,
    Recovery,
    /// Pending requests from others that we can approve.
    Requests,
}

impl RecoveryTab {
    pub fn title(self) -> &'static str {
        match self {
            Self::Guardians => "Guardians",
            Self::Recovery => "Recovery",
            Self::Requests => "Requests",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Guardians => Self::Recovery,
            Self::Recovery => Self::Requests,
            Self::Requests => Self::Guardians,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Guardians => Self::Requests,
            Self::Recovery => Self::Guardians,
            Self::Requests => Self::Recovery,
        }
    }

    pub fn all() -> [Self; 3] {
        [Self::Guardians, Self::Recovery, Self::Requests]
    }
}

/// Guardian status.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GuardianStatus {
    #[default]
    Active,
    Pending,
    Offline,
    Declined,
    Removed,
}

impl GuardianStatus {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Active => "●",
            Self::Offline => "○",
            Self::Pending => "○",
            Self::Declined => "✕",
            Self::Removed => "⊝",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Pending => "Pending",
            Self::Offline => "Offline",
            Self::Declined => "Declined",
            Self::Removed => "Removed",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Active => Theme::SUCCESS,
            Self::Offline => Theme::TEXT_DISABLED,
            Self::Pending => Theme::WARNING,
            Self::Declined | Self::Removed => Theme::ERROR,
        }
    }
}

/// A guardian presentation model.
#[derive(Clone, Debug, Default)]
pub struct Guardian {
    pub id: String,
    pub name: String,
    pub status: GuardianStatus,
    pub has_share: bool,
}

impl From<AppGuardianStatus> for GuardianStatus {
    fn from(status: AppGuardianStatus) -> Self {
        match status {
            AppGuardianStatus::Active => Self::Active,
            AppGuardianStatus::Pending => Self::Pending,
            AppGuardianStatus::Offline => Self::Offline,
            AppGuardianStatus::Revoked => Self::Removed,
        }
    }
}

impl From<&AppGuardian> for Guardian {
    fn from(guardian: &AppGuardian) -> Self {
        Self {
            id: guardian.id.to_string(),
            name: guardian.name.clone(),
            status: guardian.status.into(),
            has_share: true,
        }
    }
}

impl Guardian {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            status: GuardianStatus::Active,
            has_share: false,
        }
    }

    pub fn with_status(mut self, status: GuardianStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_share(mut self) -> Self {
        self.has_share = true;
        self
    }
}

/// Recovery state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecoveryState {
    #[default]
    None,
    Initiated,
    ThresholdMet,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

impl RecoveryState {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Not Started",
            Self::Initiated => "Awaiting Guardian Approvals",
            Self::ThresholdMet => "Threshold Met - Ready to Complete",
            Self::InProgress => "Reconstructing Keys...",
            Self::Completed => "Recovery Completed!",
            Self::Failed => "Recovery Failed",
            Self::Cancelled => "Recovery Cancelled",
        }
    }
}

/// Guardian approval for recovery.
#[derive(Clone, Debug, Default)]
pub struct GuardianApproval {
    pub guardian_name: String,
    pub approved: bool,
}

/// Recovery status.
#[derive(Clone, Debug, Default)]
pub struct RecoveryStatus {
    pub state: RecoveryState,
    pub approvals_received: u32,
    pub threshold: u32,
    pub approvals: Vec<GuardianApproval>,
}

impl RecoveryStatus {
    pub fn from_app_state(state: &AppRecoveryState, guardians: &[AppGuardian]) -> Self {
        match state.active_recovery() {
            Some(process) => Self {
                state: process.status.into(),
                approvals_received: process.approvals_received,
                threshold: process.approvals_required,
                approvals: guardians
                    .iter()
                    .map(|guardian| GuardianApproval {
                        guardian_name: guardian.name.clone(),
                        approved: process.approved_by.contains(&guardian.id),
                    })
                    .collect(),
            },
            None => Self {
                state: RecoveryState::None,
                approvals_received: 0,
                threshold: state.threshold(),
                approvals: Vec::new(),
            },
        }
    }
}

impl From<AppRecoveryProcessStatus> for RecoveryState {
    fn from(status: AppRecoveryProcessStatus) -> Self {
        match status {
            AppRecoveryProcessStatus::Idle => Self::None,
            AppRecoveryProcessStatus::Initiated => Self::Initiated,
            AppRecoveryProcessStatus::WaitingForApprovals => Self::Initiated,
            AppRecoveryProcessStatus::Approved => Self::ThresholdMet,
            AppRecoveryProcessStatus::Completed => Self::Completed,
            AppRecoveryProcessStatus::Failed => Self::Failed,
        }
    }
}

impl From<&AppRecoveryApproval> for GuardianApproval {
    fn from(approval: &AppRecoveryApproval) -> Self {
        Self {
            guardian_name: approval.guardian_id.to_string(),
            approved: true,
        }
    }
}

impl From<&AppRecoveryState> for RecoveryStatus {
    fn from(state: &AppRecoveryState) -> Self {
        match state.active_recovery() {
            Some(process) => Self {
                state: process.status.into(),
                approvals_received: process.approvals_received,
                threshold: process.approvals_required,
                approvals: process
                    .approvals
                    .iter()
                    .map(GuardianApproval::from)
                    .collect(),
            },
            None => Self {
                state: RecoveryState::None,
                approvals_received: 0,
                threshold: state.threshold(),
                approvals: Vec::new(),
            },
        }
    }
}

impl From<&AppRecoveryProcess> for RecoveryStatus {
    fn from(process: &AppRecoveryProcess) -> Self {
        Self {
            state: process.status.into(),
            approvals_received: process.approvals_received,
            threshold: process.approvals_required,
            approvals: process
                .approvals
                .iter()
                .map(GuardianApproval::from)
                .collect(),
        }
    }
}

/// A pending recovery request that we can approve.
#[derive(Clone, Debug, Default)]
pub struct PendingRequest {
    pub id: String,
    pub account_name: String,
    pub approvals_received: u32,
    pub approvals_required: u32,
    pub we_approved: bool,
    pub initiated_at: u64,
}

impl From<&AppRecoveryProcess> for PendingRequest {
    fn from(process: &AppRecoveryProcess) -> Self {
        Self {
            id: process.id.to_string(),
            account_name: process.account_id.to_string(),
            approvals_received: process.approvals_received,
            approvals_required: process.approvals_required,
            we_approved: false,
            initiated_at: process.initiated_at,
        }
    }
}
