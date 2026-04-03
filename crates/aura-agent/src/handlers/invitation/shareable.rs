use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareableInvitationError {
    InvalidFormat,
    UnsupportedVersion(u8),
    DecodingFailed,
    ParsingFailed,
    SerializationFailed,
}

impl std::fmt::Display for ShareableInvitationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFormat => write!(f, "invalid invite code format"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported version: {}", v),
            Self::DecodingFailed => write!(f, "base64 decoding failed"),
            Self::ParsingFailed => write!(f, "JSON parsing failed"),
            Self::SerializationFailed => write!(f, "JSON serialization failed"),
        }
    }
}

impl std::error::Error for ShareableInvitationError {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShareableInvitation {
    pub version: u8,
    pub invitation_id: InvitationId,
    pub sender_id: AuthorityId,
    #[serde(default)]
    pub context_id: Option<ContextId>,
    pub invitation_type: InvitationType,
    pub expires_at: Option<u64>,
    pub message: Option<String>,
}

fn default_imported_invitation_status() -> InvitationStatus {
    InvitationStatus::Pending
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct StoredImportedInvitation {
    #[serde(flatten)]
    pub(super) shareable: ShareableInvitation,
    #[serde(default = "default_imported_invitation_status")]
    pub(super) status: InvitationStatus,
    #[serde(default)]
    pub(super) created_at: u64,
}

impl StoredImportedInvitation {
    pub(super) fn pending(shareable: ShareableInvitation, created_at: u64) -> Self {
        Self {
            shareable,
            status: InvitationStatus::Pending,
            created_at,
        }
    }
}

impl std::ops::Deref for StoredImportedInvitation {
    type Target = ShareableInvitation;

    fn deref(&self) -> &Self::Target {
        &self.shareable
    }
}

impl ShareableInvitation {
    pub const CURRENT_VERSION: u8 = 1;
    pub const PREFIX: &'static str = "aura";

    pub fn to_code(&self) -> Result<String, ShareableInvitationError> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        let json =
            serde_json::to_vec(self).map_err(|_| ShareableInvitationError::SerializationFailed)?;
        let b64 = URL_SAFE_NO_PAD.encode(&json);
        Ok(format!("{}:v{}:{}", Self::PREFIX, self.version, b64))
    }

    pub fn from_code(code: &str) -> Result<Self, ShareableInvitationError> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        let parts: Vec<&str> = code.split(':').collect();
        if !(3..=5).contains(&parts.len()) {
            return Err(ShareableInvitationError::InvalidFormat);
        }

        if parts[0] != Self::PREFIX {
            return Err(ShareableInvitationError::InvalidFormat);
        }

        let version_str = parts[1];
        if !version_str.starts_with('v') {
            return Err(ShareableInvitationError::InvalidFormat);
        }
        let version: u8 = version_str[1..]
            .parse()
            .map_err(|_| ShareableInvitationError::InvalidFormat)?;

        if version != Self::CURRENT_VERSION {
            return Err(ShareableInvitationError::UnsupportedVersion(version));
        }

        let json = URL_SAFE_NO_PAD
            .decode(parts[2])
            .map_err(|_| ShareableInvitationError::DecodingFailed)?;

        serde_json::from_slice(&json).map_err(|_| ShareableInvitationError::ParsingFailed)
    }

    pub fn sender_addr_from_code(code: &str) -> Option<String> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        let parts: Vec<&str> = code.split(':').collect();
        if parts.len() != 4 && parts.len() != 5 {
            return None;
        }
        if parts[0] != Self::PREFIX {
            return None;
        }

        let decoded = URL_SAFE_NO_PAD.decode(parts[3]).ok()?;
        let addr = String::from_utf8(decoded).ok()?;
        let trimmed = addr.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(trimmed.to_string())
    }

    pub fn sender_device_id_from_code(code: &str) -> Option<DeviceId> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        let parts: Vec<&str> = code.split(':').collect();
        if parts.len() != 5 {
            return None;
        }
        if parts[0] != Self::PREFIX {
            return None;
        }

        let decoded = URL_SAFE_NO_PAD.decode(parts[4]).ok()?;
        let device_id = String::from_utf8(decoded).ok()?;
        device_id.trim().parse().ok()
    }
}

impl From<&Invitation> for ShareableInvitation {
    fn from(inv: &Invitation) -> Self {
        Self {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: inv.invitation_id.clone(),
            sender_id: inv.sender_id,
            context_id: Some(inv.context_id),
            invitation_type: inv.invitation_type.clone(),
            expires_at: inv.expires_at,
            message: inv.message.clone(),
        }
    }
}
