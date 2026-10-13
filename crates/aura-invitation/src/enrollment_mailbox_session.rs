//! Canonical enrollment VM mailbox session selectors. These route opaque
//! guarded envelopes; they do not verify enrollment decisions or membership.

use aura_core::hash::hash;
use aura_core::InvitationId;
use uuid::Uuid;

/// The request/response choreography session selected by both original roles.
pub fn original_enrollment_session_id(invitation: &InvitationId) -> Uuid {
    let digest = hash(invitation.as_str().as_bytes());
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

/// The independent negative terminal-notice session bound to the same original
/// invitation and exact signed manifest digest.
pub fn original_terminal_notice_session_id(
    invitation: &InvitationId,
    manifest_digest: &[u8; 32],
) -> Uuid {
    let mut transcript = b"aura.invitation.device-enrollment-terminal-session.v1\0".to_vec();
    transcript.extend_from_slice(manifest_digest);
    transcript.extend_from_slice(invitation.as_str().as_bytes());
    let digest = hash(&transcript);
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

/// A signed pending route may carry only these two distinct original VM
/// sessions. The envelope still passes the normal VM and receipt verification.
pub fn is_original_enrollment_mailbox_session(
    invitation: &InvitationId,
    manifest_digest: &[u8; 32],
    actual: Uuid,
) -> bool {
    actual == original_enrollment_session_id(invitation)
        || actual == original_terminal_notice_session_id(invitation, manifest_digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_original_enrollment_and_exact_manifest_notice_sessions_are_routable() {
        let invitation = InvitationId::new("original-mailbox-session");
        let foreign = InvitationId::new("foreign-mailbox-session");
        let digest = [17; 32];
        let other_digest = [18; 32];
        let request = original_enrollment_session_id(&invitation);
        let notice = original_terminal_notice_session_id(&invitation, &digest);
        assert_ne!(request, notice);
        assert!(is_original_enrollment_mailbox_session(
            &invitation,
            &digest,
            request
        ));
        assert!(is_original_enrollment_mailbox_session(
            &invitation,
            &digest,
            notice
        ));
        assert!(!is_original_enrollment_mailbox_session(
            &foreign, &digest, request
        ));
        assert!(!is_original_enrollment_mailbox_session(
            &invitation,
            &other_digest,
            notice
        ));
    }
}
