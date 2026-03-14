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

pub(crate) fn classify_invitation_accept_error(
    error: &impl Display,
) -> InvitationAcceptErrorClass {
    let lowered = error.to_string().to_ascii_lowercase();
    if lowered.contains("already accepted") || lowered.contains("not pending") {
        InvitationAcceptErrorClass::AlreadyHandled
    } else {
        InvitationAcceptErrorClass::Other
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
        classify_amp_channel_error, classify_invitation_accept_error, AmpChannelErrorClass,
        InvitationAcceptErrorClass,
    };

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
