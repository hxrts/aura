use aura_amp::capabilities::AmpCapability;
use aura_authorization::biscuit_token::TokenGrantProfile;
use aura_authorization::capabilities::GenericCapability;
use aura_chat::capabilities::ChatCapability;
use aura_core::CapabilityName;
use aura_invitation::capabilities::InvitationCapability;
use aura_rendezvous::capabilities::RendezvousCapability;
use aura_sync::capabilities::SyncCapability;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenCapabilityProfile {
    StandardDevice,
}

#[cfg(test)]
impl TokenCapabilityProfile {
    pub(crate) fn granted_capabilities(self) -> Vec<CapabilityName> {
        let mut out = Vec::new();
        self.extend_profile_grants(&mut out);
        out
    }
}

impl TokenGrantProfile for TokenCapabilityProfile {
    fn extend_profile_grants(&self, out: &mut Vec<CapabilityName>) {
        match self {
            Self::StandardDevice => {
                out.extend(
                    GenericCapability::declared_names()
                        .iter()
                        .map(|cap| cap.as_name()),
                );
                out.push(AmpCapability::Send.as_name());
                out.push(SyncCapability::RequestDigest.as_name());
                out.push(SyncCapability::RequestOps.as_name());
                out.push(SyncCapability::PushOps.as_name());
                out.push(SyncCapability::AnnounceOp.as_name());
                out.push(SyncCapability::PushOp.as_name());
                out.push(RendezvousCapability::Publish.as_name());
                out.push(RendezvousCapability::Connect.as_name());
                out.push(RendezvousCapability::Relay.as_name());
                out.push(ChatCapability::ChannelCreate.as_name());
                out.push(ChatCapability::MessageSend.as_name());
                out.push(InvitationCapability::Send.as_name());
                out.push(InvitationCapability::Accept.as_name());
                out.push(InvitationCapability::Decline.as_name());
                out.push(InvitationCapability::Cancel.as_name());
                out.push(InvitationCapability::Guardian.as_name());
                out.push(InvitationCapability::Channel.as_name());
                out.push(InvitationCapability::DeviceEnroll.as_name());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn capability_set(profile: TokenCapabilityProfile) -> HashSet<String> {
        profile
            .granted_capabilities()
            .into_iter()
            .map(|cap| cap.to_string())
            .collect()
    }

    #[test]
    fn standard_device_profile_grants_exact_documented_set() {
        let expected = HashSet::from([
            "read".to_string(),
            "write".to_string(),
            "execute".to_string(),
            "delegate".to_string(),
            "moderator".to_string(),
            "flow_charge".to_string(),
            "amp:send".to_string(),
            "sync:request_digest".to_string(),
            "sync:request_ops".to_string(),
            "sync:push_ops".to_string(),
            "sync:announce_op".to_string(),
            "sync:push_op".to_string(),
            "rendezvous:publish".to_string(),
            "rendezvous:connect".to_string(),
            "rendezvous:relay".to_string(),
            "chat:channel:create".to_string(),
            "chat:message:send".to_string(),
            "invitation:send".to_string(),
            "invitation:accept".to_string(),
            "invitation:decline".to_string(),
            "invitation:cancel".to_string(),
            "invitation:guardian".to_string(),
            "invitation:channel".to_string(),
            "invitation:device:enroll".to_string(),
        ]);

        assert_eq!(
            capability_set(TokenCapabilityProfile::StandardDevice),
            expected
        );
    }

    #[test]
    fn standard_device_profile_remains_least_privilege() {
        let granted = capability_set(TokenCapabilityProfile::StandardDevice);

        for forbidden in [
            "invitation:device:accept",
            "consensus:initiate",
            "recovery:initiate",
            "sync:epoch:propose_rotation",
        ] {
            assert!(
                !granted.contains(forbidden),
                "standard device profile must not grant {forbidden}"
            );
        }
    }
}
