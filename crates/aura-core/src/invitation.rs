//! Canonical invitation wire and cache shapes.
//!
//! These serializable observations carry no issuance, import, policy approval or
//! journal-commit provenance. Admission owners must establish those independently.

use crate::effects::amp::ChannelBootstrapPackage;
use crate::types::identifiers::{AuthorityId, CeremonyId, ChannelId, ContextId, InvitationId};
use crate::DeviceId;
use serde::{Deserialize, Serialize};

/// Untrusted invitation wire binding. Runtime authorization requires matching
/// this binding to an exact locally retained, signed setup request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceEnrollmentSetupBinding {
    pub nonce: [u8; 32],
    pub digest: [u8; 32],
}

/// Type of invitation
// aura-security: secret-derive-justified owner=security-refactor expires=before-release remediation=work/2.md device-enrollment variant carries encrypted setup payloads for transfer; field-level justifications track migration to secret wrappers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvitationType {
    /// Invitation to join a home/channel
    Channel {
        /// Home/channel identifier
        #[serde(with = "channel_id_serde")]
        home_id: ChannelId,
        /// Optional nickname suggestion (what the channel/home wants to be called)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nickname_suggestion: Option<String>,
        /// Optional bootstrap key package for provisional AMP messaging.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bootstrap: Option<ChannelBootstrapPackage>,
        /// Whether this invites the recipient into the home this channel
        /// belongs to (`/homeinvite`), not only into the channel.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        home: bool,
    },
    /// Invitation to become a guardian
    Guardian {
        /// Authority to guard
        subject_authority: AuthorityId,
    },
    /// Invitation to become a contact
    Contact {
        /// Optional nickname for the contact
        nickname: Option<String>,
    },

    /// Invitation to enroll a new device for an account authority.
    ///
    /// This is primarily intended for out-of-band transfer (QR/copy-paste) and
    /// carries the key-share material required for the new device to install.
    DeviceEnrollment {
        /// Legacy decode may lack this field; it never authorizes a response.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        setup_binding: Option<DeviceEnrollmentSetupBinding>,
        /// Account authority being modified
        subject_authority: AuthorityId,
        /// Authority the new device was invited as. The new device may re-import
        /// the code after its runtime switches to `subject_authority`, so the
        /// invited identity is carried in the signed invitation itself.
        #[serde(default)]
        invitee_authority: Option<AuthorityId>,
        /// Initiator device id (used for routing acceptance back to the right device runtime)
        initiator_device_id: DeviceId,
        /// Device id being enrolled
        device_id: DeviceId,
        /// Optional nickname suggestion (what the device wants to be called)
        nickname_suggestion: Option<String>,
        /// Key-rotation ceremony identifier
        ceremony_id: CeremonyId,
        /// Pending epoch created during prepare
        pending_epoch: u64,
        /// Encrypted/opaque key package for the invited device
        // aura-security: raw-secret-field-justified owner=security-refactor expires=before-release remediation=work/2.md encrypted enrollment payload; plaintext key packages must use secret wrappers before wrapping.
        key_package: Vec<u8>,
        /// Serialized threshold config metadata for the pending epoch
        // aura-security: raw-secret-field-justified owner=security-refactor expires=before-release remediation=work/2.md enrollment ceremony metadata until envelope payloads move to SecretBytes.
        threshold_config: Vec<u8>,
        /// Untrusted key material: pending-epoch enrollment payload; authentication must resolve expected keys from trusted authority/device state.
        public_key_package: Vec<u8>,
        /// Baseline attested tree operations for the current authority state.
        ///
        /// Fresh invitees need these ops to materialize the pre-enrollment
        /// authority tree before applying the enrollment commit.
        baseline_tree_ops: Vec<Vec<u8>>,
    },
}

mod channel_id_serde {
    use crate::types::identifiers::ChannelId;
    use serde::{Deserialize, Deserializer, Serializer};
    use std::str::FromStr;

    pub fn serialize<S>(value: &ChannelId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<ChannelId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        ChannelId::from_str(&raw).map_err(serde::de::Error::custom)
    }
}

impl InvitationType {
    /// Convert to type string for fact storage
    pub fn as_type_string(&self) -> String {
        match self {
            InvitationType::Channel { .. } => "channel".to_string(),
            InvitationType::Guardian { .. } => "guardian".to_string(),
            InvitationType::Contact { .. } => "contact".to_string(),
            InvitationType::DeviceEnrollment { .. } => "device".to_string(),
        }
    }
}

/// Invitation status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvitationStatus {
    /// Invitation is pending response
    Pending,
    /// Invitation was accepted
    Accepted,
    /// Invitation was declined
    Declined,
    /// Invitation was cancelled by sender
    Cancelled,
    /// Invitation has expired
    Expired,
}

/// Cached invitation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invitation {
    /// Unique invitation identifier
    pub invitation_id: InvitationId,
    /// Context for the invitation
    pub context_id: ContextId,
    /// Sender authority
    pub sender_id: AuthorityId,
    /// Receiver authority
    pub receiver_id: AuthorityId,
    /// Type of invitation
    pub invitation_type: InvitationType,
    /// Current status
    pub status: InvitationStatus,
    /// Creation timestamp (ms)
    pub created_at: u64,
    /// Expiration timestamp (ms), if any
    pub expires_at: Option<u64>,
    /// Optional message
    pub message: Option<String>,
    /// Optional sender-local nickname for the invitee on sent invitations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_nickname: Option<String>,
}

impl Invitation {
    /// Check if invitation is expired
    pub fn is_expired(&self, now_ms: u64) -> bool {
        self.expires_at.map(|exp| now_ms >= exp).unwrap_or(false)
    }

    /// Check if invitation is pending
    pub fn is_pending(&self) -> bool {
        matches!(self.status, InvitationStatus::Pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_channel_wire_shape_preserves_string_id_and_optional_fields() {
        let home_id = ChannelId::from_bytes([218; 32]);
        let invitation = InvitationType::Channel {
            home_id,
            nickname_suggestion: None,
            bootstrap: None,
            home: false,
        };
        let expected = serde_json::json!({"Channel": {"home_id": home_id.to_string()}});
        assert_eq!(serde_json::to_value(&invitation).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<InvitationType>(expected).unwrap(),
            invitation
        );
        let binding = DeviceEnrollmentSetupBinding {
            nonce: [17; 32],
            digest: [18; 32],
        };
        let encoded = serde_json::to_value(&binding).unwrap();
        assert_eq!(
            encoded,
            serde_json::json!({"nonce": ([17; 32].to_vec()), "digest": ([18; 32].to_vec())})
        );
        assert_eq!(
            serde_json::from_value::<DeviceEnrollmentSetupBinding>(encoded).unwrap(),
            binding
        );
    }
}
