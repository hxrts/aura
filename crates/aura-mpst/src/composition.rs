use serde::{Deserialize, Serialize};

/// Generated host-side startup metadata for one choreography bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositionManifest {
    /// Choreography protocol name from the DSL.
    pub protocol_name: String,
    /// Optional choreography namespace from the DSL.
    pub protocol_namespace: Option<String>,
    /// Fully-qualified choreography name derived from namespace + protocol name.
    pub protocol_qualified_name: String,
    /// Stable host/runtime protocol identifier.
    pub protocol_id: String,
    /// Declared choreography roles in source order.
    pub role_names: Vec<String>,
    /// Required runtime capability identifiers for admission.
    pub required_capabilities: Vec<String>,
    /// Determinism/scheduler policy selector reference.
    pub determinism_policy_ref: Option<String>,
    /// Typed link composition boundaries declared by the choreography.
    pub link_specs: Vec<CompositionLinkSpec>,
    /// Delegation boundary constraints required for runtime reconfiguration.
    pub delegation_constraints: Vec<CompositionDelegationConstraint>,
}

impl CompositionManifest {
    /// Build the fully-qualified choreography name from namespace + protocol name.
    pub fn qualified_name(namespace: Option<&str>, protocol_name: &str) -> String {
        namespace.map_or_else(
            || protocol_name.to_string(),
            |namespace| format!("{namespace}.{protocol_name}"),
        )
    }
}

/// Canonical host/runtime startup defaults for a choreography bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompositionStartupDefaults {
    /// Optional stable host/runtime protocol identifier override.
    pub protocol_id: Option<&'static str>,
    /// Required runtime capability identifiers for admission.
    pub required_capabilities: &'static [&'static str],
    /// Determinism/scheduler policy selector reference.
    pub determinism_policy_ref: &'static str,
}

impl CompositionStartupDefaults {
    /// Default startup policy for manifests without an explicit override.
    pub const fn production_default(protocol_id: &'static str) -> Self {
        Self {
            protocol_id: Some(protocol_id),
            required_capabilities: &[],
            determinism_policy_ref: "aura.vm.prod.default",
        }
    }
}

/// Static `@link` metadata extracted from choreography annotations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositionLinkSpec {
    /// Role declaring the link contract.
    pub role: String,
    /// Bundle identifier referenced by the link annotation.
    pub bundle_id: String,
    /// Interfaces exported by the declaring bundle.
    pub exports: Vec<String>,
    /// Interfaces imported by the declaring bundle.
    pub imports: Vec<String>,
}

/// Host-side delegation constraint for one choreography bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositionDelegationConstraint {
    /// Source role of the constrained delegation edge.
    pub from_role: String,
    /// Destination role of the constrained delegation edge.
    pub to_role: String,
    /// Optional bundle or proof identifier required for this delegation edge.
    pub required_bundle_id: Option<String>,
}

/// Resolve canonical startup defaults from a generated choreography qualified name.
#[must_use]
pub fn startup_defaults_for_qualified_name(qualified_name: &str) -> CompositionStartupDefaults {
    match qualified_name {
        "aura_consensus.AuraConsensus" => CompositionStartupDefaults {
            protocol_id: Some("aura.consensus"),
            required_capabilities: &["byzantine_envelope"],
            determinism_policy_ref: "aura.vm.consensus_fallback.prod",
        },
        "amp_transport.AmpTransport" => {
            CompositionStartupDefaults::production_default("aura.amp.transport")
        }
        "dkd_protocol.DkdChoreography" => CompositionStartupDefaults {
            protocol_id: Some("aura.dkg.ceremony"),
            required_capabilities: &["byzantine_envelope", "termination_bounded"],
            determinism_policy_ref: "aura.vm.dkg_ceremony.prod",
        },
        "guardian_auth_relational.GuardianAuthRelational" => {
            CompositionStartupDefaults::production_default(
                "aura.authentication.guardian_auth_relational",
            )
        }
        "invitation.InvitationExchange" => {
            CompositionStartupDefaults::production_default("aura.invitation.exchange")
        }
        "invitation_guardian.GuardianInvitation" => {
            CompositionStartupDefaults::production_default("aura.invitation.guardian")
        }
        "invitation_device_enrollment.DeviceEnrollment" => {
            CompositionStartupDefaults::production_default("aura.invitation.device_enrollment")
        }
        "recovery_protocol.RecoveryProtocol" => CompositionStartupDefaults {
            protocol_id: Some("aura.recovery.grant"),
            required_capabilities: &["termination_bounded"],
            determinism_policy_ref: "aura.vm.recovery_grant.prod",
        },
        "guardian_ceremony.GuardianCeremony" => {
            CompositionStartupDefaults::production_default("aura.recovery.guardian_ceremony")
        }
        "guardian_setup.GuardianSetup" => {
            CompositionStartupDefaults::production_default("aura.recovery.guardian_setup")
        }
        "guardian_membership_change.GuardianMembershipChange" => {
            CompositionStartupDefaults::production_default(
                "aura.recovery.guardian_membership_change",
            )
        }
        "rendezvous.RendezvousExchange" => {
            CompositionStartupDefaults::production_default("aura.rendezvous.exchange")
        }
        "rendezvous_relay.RelayedRendezvous" => {
            CompositionStartupDefaults::production_default("aura.rendezvous.relay")
        }
        "session_coordination.SessionCoordinationChoreography" => {
            CompositionStartupDefaults::production_default("aura.session.coordination")
        }
        "epoch_rotation.EpochRotationProtocol" => CompositionStartupDefaults {
            protocol_id: Some("aura.sync.epoch_rotation"),
            required_capabilities: &["termination_bounded"],
            determinism_policy_ref: "aura.vm.sync_anti_entropy.prod",
        },
        _ => CompositionStartupDefaults {
            protocol_id: None,
            required_capabilities: &[],
            determinism_policy_ref: "aura.vm.prod.default",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{startup_defaults_for_qualified_name, CompositionManifest};

    #[test]
    fn qualified_name_uses_namespace_when_present() {
        assert_eq!(
            CompositionManifest::qualified_name(Some("invitation"), "Exchange"),
            "invitation.Exchange"
        );
        assert_eq!(
            CompositionManifest::qualified_name(None, "Exchange"),
            "Exchange"
        );
    }

    #[test]
    fn startup_defaults_cover_known_vm_manifests() {
        let dkd = startup_defaults_for_qualified_name("dkd_protocol.DkdChoreography");
        assert_eq!(dkd.protocol_id, Some("aura.dkg.ceremony"));
        assert_eq!(
            dkd.required_capabilities,
            ["byzantine_envelope", "termination_bounded"]
        );

        let invitation = startup_defaults_for_qualified_name("invitation.InvitationExchange");
        assert_eq!(invitation.protocol_id, Some("aura.invitation.exchange"));
        assert!(invitation.required_capabilities.is_empty());
        assert_eq!(invitation.determinism_policy_ref, "aura.vm.prod.default");
    }

    #[test]
    fn startup_defaults_cover_all_production_choreographies() {
        let known = [
            ("aura_consensus.AuraConsensus", "aura.consensus"),
            ("amp_transport.AmpTransport", "aura.amp.transport"),
            ("dkd_protocol.DkdChoreography", "aura.dkg.ceremony"),
            (
                "guardian_auth_relational.GuardianAuthRelational",
                "aura.authentication.guardian_auth_relational",
            ),
            ("invitation.InvitationExchange", "aura.invitation.exchange"),
            (
                "invitation_guardian.GuardianInvitation",
                "aura.invitation.guardian",
            ),
            (
                "invitation_device_enrollment.DeviceEnrollment",
                "aura.invitation.device_enrollment",
            ),
            ("recovery_protocol.RecoveryProtocol", "aura.recovery.grant"),
            (
                "guardian_ceremony.GuardianCeremony",
                "aura.recovery.guardian_ceremony",
            ),
            (
                "guardian_setup.GuardianSetup",
                "aura.recovery.guardian_setup",
            ),
            (
                "guardian_membership_change.GuardianMembershipChange",
                "aura.recovery.guardian_membership_change",
            ),
            ("rendezvous.RendezvousExchange", "aura.rendezvous.exchange"),
            (
                "rendezvous_relay.RelayedRendezvous",
                "aura.rendezvous.relay",
            ),
            (
                "session_coordination.SessionCoordinationChoreography",
                "aura.session.coordination",
            ),
            (
                "epoch_rotation.EpochRotationProtocol",
                "aura.sync.epoch_rotation",
            ),
        ];

        for (qualified_name, protocol_id) in known {
            let defaults = startup_defaults_for_qualified_name(qualified_name);
            assert_eq!(
                defaults.protocol_id,
                Some(protocol_id),
                "missing startup defaults for {qualified_name}"
            );
        }
    }
}
