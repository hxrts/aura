//! Shared semantic submission request, handle, and response surfaces.

use super::{ActorId, IntentAction, IntentKind, SharedActionContract, SharedActionId};
use crate::ui_contract::{OperationId, OperationInstanceId, ProjectionRevision};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedActionRequest {
    pub actor: ActorId,
    pub intent: IntentAction,
    pub contract: SharedActionContract,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedActionHandle {
    pub action_id: SharedActionId,
    pub actor: ActorId,
    pub intent: IntentKind,
    pub contract: SharedActionContract,
    pub baseline_revision: Option<ProjectionRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticCommandRequest {
    pub intent: IntentAction,
    pub contract: SharedActionContract,
}

impl SemanticCommandRequest {
    #[must_use]
    pub fn new(intent: IntentAction) -> Self {
        let contract = intent.contract();
        Self { intent, contract }
    }

    #[must_use]
    pub fn kind(&self) -> IntentKind {
        self.intent.kind()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiOperationHandle {
    id: OperationId,
    instance_id: OperationInstanceId,
}

impl UiOperationHandle {
    #[must_use]
    pub const fn new(id: OperationId, instance_id: OperationInstanceId) -> Self {
        Self { id, instance_id }
    }

    #[must_use]
    pub const fn id(&self) -> &OperationId {
        &self.id
    }

    #[must_use]
    pub const fn instance_id(&self) -> &OperationInstanceId {
        &self.instance_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SemanticSubmissionHandle {
    pub ui_operation: Option<UiOperationHandle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionState {
    Accepted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionValueContract {
    None,
    ContactInvitationCode,
    AuthoritativeChannelBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubmissionContract {
    Immediate {
        value: SubmissionValueContract,
    },
    OperationHandle {
        operation_id: OperationId,
        value: SubmissionValueContract,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmittedAction<T> {
    pub value: T,
    pub submission: SubmissionState,
    pub handle: SemanticSubmissionHandle,
}

impl<T> SubmittedAction<T> {
    #[must_use]
    pub fn without_handle(value: T) -> Self {
        Self {
            value,
            submission: SubmissionState::Accepted,
            handle: SemanticSubmissionHandle::default(),
        }
    }

    #[must_use]
    pub fn with_ui_operation(value: T, handle: UiOperationHandle) -> Self {
        Self {
            value,
            submission: SubmissionState::Accepted,
            handle: SemanticSubmissionHandle {
                ui_operation: Some(handle),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticCommandValue {
    None,
    ContactInvitationCode {
        code: String,
    },
    ChannelSelection {
        channel_id: String,
    },
    AuthoritativeChannelBinding {
        channel_id: String,
        context_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticCommandResponse {
    pub submission: SubmissionState,
    pub handle: SemanticSubmissionHandle,
    pub value: SemanticCommandValue,
}

impl SemanticCommandResponse {
    #[must_use]
    pub fn accepted(value: SemanticCommandValue) -> Self {
        Self {
            submission: SubmissionState::Accepted,
            handle: SemanticSubmissionHandle::default(),
            value,
        }
    }

    #[must_use]
    pub fn accepted_without_value() -> Self {
        Self::accepted(SemanticCommandValue::None)
    }

    #[must_use]
    pub fn accepted_contact_invitation_code(code: String) -> Self {
        Self::accepted(SemanticCommandValue::ContactInvitationCode { code })
    }

    #[must_use]
    pub fn accepted_channel_selection(channel_id: String) -> Self {
        Self::accepted(SemanticCommandValue::ChannelSelection { channel_id })
    }

    #[must_use]
    pub fn accepted_authoritative_channel_binding(channel_id: String, context_id: String) -> Self {
        Self::accepted(SemanticCommandValue::AuthoritativeChannelBinding {
            channel_id,
            context_id,
        })
    }
}
