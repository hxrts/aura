use std::fmt::Display;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InvitationAcceptErrorClass {
    AlreadyHandled,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AmpChannelErrorClass {
    ChannelStateUnavailable,
    AlreadyExists,
    Other,
}

pub(crate) fn classify_invitation_accept_error(error: &impl Display) -> InvitationAcceptErrorClass {
    let lowered = error.to_string().to_ascii_lowercase();
    if lowered.contains("already accepted") || lowered.contains("not pending") {
        InvitationAcceptErrorClass::AlreadyHandled
    } else {
        InvitationAcceptErrorClass::Other
    }
}

/// Typed outcome of a contact acceptance the inviter did not confirm, keyed on
/// the runtime's stable messages (`aura-agent` `ContactConfirmationError`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContactConfirmationErrorClass {
    Revoked,
    Expired,
    AlreadySettled,
    Unconfirmed,
}

pub(crate) fn classify_contact_confirmation_error(
    error: &impl Display,
) -> Option<ContactConfirmationErrorClass> {
    let lowered = error.to_string().to_ascii_lowercase();
    if lowered.contains("inviter revoked this contact invitation") {
        Some(ContactConfirmationErrorClass::Revoked)
    } else if lowered.contains("contact invitation has expired") {
        Some(ContactConfirmationErrorClass::Expired)
    } else if lowered.contains("contact invitation was already used") {
        Some(ContactConfirmationErrorClass::AlreadySettled)
    } else if lowered.contains("inviter did not confirm this contact invitation") {
        Some(ContactConfirmationErrorClass::Unconfirmed)
    } else {
        None
    }
}

pub(crate) fn classify_amp_channel_error(error: &impl Display) -> AmpChannelErrorClass {
    let lowered = error.to_string().to_ascii_lowercase();
    if lowered.contains("channel state not found") {
        AmpChannelErrorClass::ChannelStateUnavailable
    } else if lowered.contains("already") || lowered.contains("exists") {
        AmpChannelErrorClass::AlreadyExists
    } else {
        AmpChannelErrorClass::Other
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_amp_channel_error, classify_contact_confirmation_error,
        classify_invitation_accept_error, AmpChannelErrorClass, ContactConfirmationErrorClass,
        InvitationAcceptErrorClass,
    };

    #[test]
    fn contact_confirmation_classifier_maps_runtime_messages() {
        let wrapped = |message: &str| format!("accept invitation failed: Invalid: {message}");
        assert_eq!(
            classify_contact_confirmation_error(&wrapped(
                "The inviter revoked this contact invitation"
            )),
            Some(ContactConfirmationErrorClass::Revoked)
        );
        assert_eq!(
            classify_contact_confirmation_error(&wrapped("This contact invitation has expired")),
            Some(ContactConfirmationErrorClass::Expired)
        );
        assert_eq!(
            classify_contact_confirmation_error(&wrapped(
                "This contact invitation was already used"
            )),
            Some(ContactConfirmationErrorClass::AlreadySettled)
        );
        assert_eq!(
            classify_contact_confirmation_error(&wrapped(
                "The inviter did not confirm this contact invitation within 30s; try again when they are online"
            )),
            Some(ContactConfirmationErrorClass::Unconfirmed)
        );
        assert_eq!(classify_contact_confirmation_error(&"storage failed"), None);
    }

    #[test]
    fn invitation_accept_classifier_detects_idempotent_acceptance() {
        assert_eq!(
            classify_invitation_accept_error(&"invitation already accepted"),
            InvitationAcceptErrorClass::AlreadyHandled
        );
        assert_eq!(
            classify_invitation_accept_error(&"invitation not pending"),
            InvitationAcceptErrorClass::AlreadyHandled
        );
        assert_eq!(
            classify_invitation_accept_error(&"permission denied"),
            InvitationAcceptErrorClass::Other
        );
    }

    #[test]
    fn amp_channel_classifier_detects_channel_state_and_exists_conditions() {
        assert_eq!(
            classify_amp_channel_error(&"channel state not found"),
            AmpChannelErrorClass::ChannelStateUnavailable
        );
        assert_eq!(
            classify_amp_channel_error(&"channel already exists"),
            AmpChannelErrorClass::AlreadyExists
        );
        assert_eq!(
            classify_amp_channel_error(&"transport timeout"),
            AmpChannelErrorClass::Other
        );
    }
}
