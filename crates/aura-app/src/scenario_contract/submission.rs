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
    DeviceEnrollmentSetup,
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
    DeviceEnrollmentSetup {
        setup_code: String,
    },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_export_is_an_immediate_typed_value_without_ceremony_ownership() {
        let request = SemanticCommandRequest::new(IntentAction::ExportDeviceEnrollmentSetup);
        assert_eq!(request.kind(), IntentKind::ExportDeviceEnrollmentSetup);
        assert_eq!(
            request.contract.submission,
            SubmissionContract::Immediate {
                value: SubmissionValueContract::DeviceEnrollmentSetup,
            }
        );
        let response =
            SemanticCommandResponse::accepted(SemanticCommandValue::DeviceEnrollmentSetup {
                setup_code: "opaque retained code".to_string(),
            });
        let encoded = serde_json::to_vec(&response).unwrap();
        let decoded: SemanticCommandResponse = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, response);
        assert!(decoded.handle.ui_operation.is_none());
    }
}

/// Typed refusal before semantic operation admission. It carries no operation handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticCommandRejection {
    pub error: crate::ui_contract::SemanticOperationError,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<SemanticRejectionReason>,
}

/// Stable immediate-validation reasons backed by actual native enum variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRejectionReason {
    NicknameEmpty,
    NicknameTooLong,
    NicknameInvalidChars,
    InvitationInvalidFormat,
    InvitationUnsupportedVersion,
    InvitationSizeLimitExceeded,
    InvitationDecodingFailed,
    InvitationParsingFailed,
    InvitationSerializationFailed,
    InvitationMissingSenderProof,
    InvitationInvalidSenderProof,
    InvitationMissingChannelContext,
    InvitationExpired,
}

impl SemanticCommandRejection {
    /// Project a known concrete admission failure before a shell converts it
    /// into a diagnostic string. Unknown sources supply no typed evidence.
    pub fn from_native_source(original: &(dyn std::error::Error + 'static)) -> Option<Self> {
        use crate::ui_contract::{
            SemanticFailureCode, SemanticFailureDomain, SemanticOperationError,
        };
        use crate::workflows::account::NicknameSuggestionError;
        let mut current = Some(original);
        while let Some(source) = current {
            if let Some(rejection) = source.downcast_ref::<Self>() {
                return Some(rejection.clone());
            }
            if let Some(error) = source.downcast_ref::<NicknameSuggestionError>() {
                let reason = match error {
                    NicknameSuggestionError::Empty => SemanticRejectionReason::NicknameEmpty,
                    NicknameSuggestionError::TooLong { .. } => {
                        SemanticRejectionReason::NicknameTooLong
                    }
                    NicknameSuggestionError::InvalidChars { .. } => {
                        SemanticRejectionReason::NicknameInvalidChars
                    }
                };
                return Some(Self {
                    error: SemanticOperationError::new(
                        SemanticFailureDomain::Command,
                        SemanticFailureCode::InvalidArgument,
                    ),
                    reason: Some(reason),
                });
            }
            if let Some(error) =
                source.downcast_ref::<aura_invitation::shareable::ShareableInvitationError>()
            {
                use aura_invitation::shareable::ShareableInvitationError as E;
                use SemanticRejectionReason as R;
                let (reason, code) = match error {
                    E::InvalidFormat => (
                        R::InvitationInvalidFormat,
                        SemanticFailureCode::InvalidArgument,
                    ),
                    E::UnsupportedVersion(_) => (
                        R::InvitationUnsupportedVersion,
                        SemanticFailureCode::InvalidArgument,
                    ),
                    E::SizeLimitExceeded(_) => (
                        R::InvitationSizeLimitExceeded,
                        SemanticFailureCode::InvalidArgument,
                    ),
                    E::DecodingFailed(_) => (
                        R::InvitationDecodingFailed,
                        SemanticFailureCode::InvalidArgument,
                    ),
                    E::ParsingFailed(_) => (
                        R::InvitationParsingFailed,
                        SemanticFailureCode::InvalidArgument,
                    ),
                    E::SerializationFailed(_) => (
                        R::InvitationSerializationFailed,
                        SemanticFailureCode::SerializationFailure,
                    ),
                    E::MissingSenderProof => (
                        R::InvitationMissingSenderProof,
                        SemanticFailureCode::PermissionDenied,
                    ),
                    E::InvalidSenderProof => (
                        R::InvitationInvalidSenderProof,
                        SemanticFailureCode::PermissionDenied,
                    ),
                    E::MissingChannelContext => (
                        R::InvitationMissingChannelContext,
                        SemanticFailureCode::MissingAuthoritativeContext,
                    ),
                    E::Expired => (R::InvitationExpired, SemanticFailureCode::InvitationExpired),
                };
                return Some(Self {
                    error: SemanticOperationError::new(SemanticFailureDomain::Invitation, code),
                    reason: Some(reason),
                });
            }
            current = source.source();
        }
        None
    }
}

impl std::fmt::Display for SemanticCommandRejection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "semantic admission rejected: {:?}/{:?}",
            self.error.domain, self.error.code
        )
    }
}

impl std::error::Error for SemanticCommandRejection {}

#[cfg(test)]
mod rejection_wire_tests {
    use super::*;
    use crate::ui_contract::{SemanticFailureCode, SemanticFailureDomain, SemanticOperationError};
    #[test]
    fn concrete_native_source_projects_without_using_diagnostic_text() {
        use crate::workflows::account::NicknameSuggestionError;
        let original = NicknameSuggestionError::Empty;
        let rejection = SemanticCommandRejection::from_native_source(&original)
            .expect("actual native enum provides stable reason");
        assert_eq!(
            rejection.reason,
            Some(SemanticRejectionReason::NicknameEmpty)
        );
        let text_only = std::io::Error::other(original.to_string());
        assert!(SemanticCommandRejection::from_native_source(&text_only).is_none());
    }
    #[test]
    fn immediate_rejection_roundtrip_has_no_accepted_operation_custody() {
        let rejection = SemanticCommandRejection {
            error: SemanticOperationError::new(
                SemanticFailureDomain::Command,
                SemanticFailureCode::InvalidArgument,
            ),
            reason: Some(SemanticRejectionReason::NicknameEmpty),
        };
        let wire = serde_json::to_value(&rejection).expect("encode actual typed refusal");
        assert!(wire.get("handle").is_none());
        assert!(wire.get("submission").is_none());
        assert_eq!(
            serde_json::from_value::<SemanticCommandRejection>(wire).expect("decode exact refusal"),
            rejection
        );
    }
}
