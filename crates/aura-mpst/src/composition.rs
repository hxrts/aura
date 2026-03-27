use crate::{parse_choreography_capability, ChoreographyCapabilityError};
use aura_core::CapabilityName;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const RESERVED_FIRST_PARTY_NAMESPACE_ROOTS: &[&str] = &[
    "amp",
    "auth",
    "chat",
    "consensus",
    "dkd",
    "example",
    "invitation",
    "recovery",
    "relay",
    "rendezvous",
    "sync",
];

const RESERVED_HOST_CAPABILITY_ROOTS: &[&str] = &[
    "read",
    "write",
    "execute",
    "delegate",
    "moderator",
    "flow_charge",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedModuleCapability<'a> {
    module_id: &'a str,
    path_root: &'a str,
}

/// Admitted guard-capability descriptors for one installed module release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedModuleGuardCapabilities {
    module_id: String,
    declared_guard_capabilities: HashSet<CapabilityName>,
}

impl AdmittedModuleGuardCapabilities {
    /// Build an admitted module capability descriptor set.
    pub fn new(
        module_id: &str,
        declared_guard_capabilities: Vec<CapabilityName>,
    ) -> Result<Self, ModuleGuardCapabilityError> {
        validate_module_id(module_id)?;

        let mut declared = HashSet::new();
        for capability in declared_guard_capabilities {
            validate_admitted_module_capability(module_id, &capability)?;
            declared.insert(capability);
        }

        Ok(Self {
            module_id: module_id.to_string(),
            declared_guard_capabilities: declared,
        })
    }

    /// Return the admitted module id that owns this descriptor set.
    #[must_use]
    pub fn module_id(&self) -> &str {
        &self.module_id
    }

    /// Return whether the installed release explicitly declares `capability`.
    #[must_use]
    pub fn declares(&self, capability: &CapabilityName) -> bool {
        self.declared_guard_capabilities.contains(capability)
    }
}

/// Guard-capability admission profile for choreography manifests.
#[derive(Debug, Clone, Copy)]
pub struct GuardCapabilityAdmission<'a> {
    admitted_module_capabilities: &'a [AdmittedModuleGuardCapabilities],
}

impl<'a> GuardCapabilityAdmission<'a> {
    /// Admission profile for first-party-only manifests.
    #[must_use]
    pub const fn first_party_only() -> Self {
        Self {
            admitted_module_capabilities: &[],
        }
    }

    /// Admission profile with installed module capability descriptors.
    #[must_use]
    pub const fn with_admitted_module_capabilities(
        admitted_module_capabilities: &'a [AdmittedModuleGuardCapabilities],
    ) -> Self {
        Self {
            admitted_module_capabilities,
        }
    }
}

/// Errors raised while constructing admitted module capability descriptors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModuleGuardCapabilityError {
    /// Module ids use the lower-case capability segment grammar without `:`.
    #[error(
        "module id `{module_id}` must use the lower-case capability segment grammar without `:`"
    )]
    InvalidModuleId {
        /// Rejected module id.
        module_id: String,
    },

    /// The descriptor contains an invalid module capability declaration.
    #[error(transparent)]
    InvalidCapability(#[from] GuardCapabilityAdmissionError),
}

/// Errors raised while admitting choreography guard capabilities.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GuardCapabilityAdmissionError {
    /// Canonical choreography capability parsing failed.
    #[error(transparent)]
    Canonical(#[from] ChoreographyCapabilityError),

    /// Module capabilities must use the explicit admitted-module namespace.
    #[error("module capability `{value}` must use `module:<module_id>:<capability_path>`")]
    InvalidModuleCapabilityShape {
        /// Rejected capability.
        value: String,
    },

    /// Module-defined capabilities may not claim first-party namespace roots.
    #[error("module capability `{value}` uses reserved first-party namespace root `{root}`")]
    ReservedFirstPartyNamespace {
        /// Rejected capability.
        value: String,
        /// Reserved root that was claimed.
        root: String,
    },

    /// Module-defined capabilities may not claim generic host-owned names.
    #[error("module capability `{value}` uses reserved host-owned capability root `{root}`")]
    ReservedHostCapabilityRoot {
        /// Rejected capability.
        value: String,
        /// Reserved root that was claimed.
        root: String,
    },

    /// Module descriptors must stay inside their admitted namespace.
    #[error(
        "module capability `{value}` does not match admitted module id `{expected_module_id}`"
    )]
    ModuleIdMismatch {
        /// Rejected capability.
        value: String,
        /// Module id the descriptor set owns.
        expected_module_id: String,
    },

    /// Choreography references must target an admitted module release.
    #[error("module capability `{value}` references unadmitted module `{module_id}`")]
    UnadmittedModule {
        /// Rejected capability.
        value: String,
        /// Missing admitted module id.
        module_id: String,
    },

    /// Choreography references must be declared by the installed release.
    #[error("module capability `{value}` is not declared in the installed module release for module `{module_id}`")]
    UndeclaredModuleCapability {
        /// Rejected capability.
        value: String,
        /// Module id that owns the installed release.
        module_id: String,
    },
}

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
    /// Canonical guard capabilities declared by the choreography source.
    pub guard_capabilities: Vec<CapabilityName>,
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

    /// Validate that all declared guard capabilities are admitted choreography names.
    pub fn validate_guard_capabilities(
        &self,
        admission: GuardCapabilityAdmission<'_>,
    ) -> Result<(), GuardCapabilityAdmissionError> {
        for capability in &self.guard_capabilities {
            parse_choreography_capability(capability.as_str())?;
            if let Some(module_capability) = parse_module_capability(capability)? {
                let admitted = admission
                    .admitted_module_capabilities
                    .iter()
                    .find(|descriptor| descriptor.module_id() == module_capability.module_id)
                    .ok_or_else(|| GuardCapabilityAdmissionError::UnadmittedModule {
                        value: capability.as_str().to_string(),
                        module_id: module_capability.module_id.to_string(),
                    })?;
                if !admitted.declares(capability) {
                    return Err(GuardCapabilityAdmissionError::UndeclaredModuleCapability {
                        value: capability.as_str().to_string(),
                        module_id: admitted.module_id().to_string(),
                    });
                }
            }
        }
        Ok(())
    }
}

fn validate_module_id(module_id: &str) -> Result<(), ModuleGuardCapabilityError> {
    if module_id.is_empty() || module_id.contains(':') || CapabilityName::parse(module_id).is_err()
    {
        return Err(ModuleGuardCapabilityError::InvalidModuleId {
            module_id: module_id.to_string(),
        });
    }
    Ok(())
}

fn validate_admitted_module_capability(
    module_id: &str,
    capability: &CapabilityName,
) -> Result<(), GuardCapabilityAdmissionError> {
    let parsed = parse_module_capability(capability)?.ok_or_else(|| {
        GuardCapabilityAdmissionError::InvalidModuleCapabilityShape {
            value: capability.as_str().to_string(),
        }
    })?;

    if parsed.module_id != module_id {
        return Err(GuardCapabilityAdmissionError::ModuleIdMismatch {
            value: capability.as_str().to_string(),
            expected_module_id: module_id.to_string(),
        });
    }
    if RESERVED_FIRST_PARTY_NAMESPACE_ROOTS.contains(&parsed.path_root) {
        return Err(GuardCapabilityAdmissionError::ReservedFirstPartyNamespace {
            value: capability.as_str().to_string(),
            root: parsed.path_root.to_string(),
        });
    }
    if RESERVED_HOST_CAPABILITY_ROOTS.contains(&parsed.path_root) {
        return Err(GuardCapabilityAdmissionError::ReservedHostCapabilityRoot {
            value: capability.as_str().to_string(),
            root: parsed.path_root.to_string(),
        });
    }

    Ok(())
}

fn parse_module_capability(
    capability: &CapabilityName,
) -> Result<Option<ParsedModuleCapability<'_>>, GuardCapabilityAdmissionError> {
    let value = capability.as_str();
    if !value.starts_with("module:") {
        return Ok(None);
    }

    let mut parts = value.split(':');
    let _module_namespace = parts.next();
    let module_id = parts.next().unwrap_or_default();
    let path_root = parts.next().unwrap_or_default();
    if module_id.is_empty() || path_root.is_empty() {
        return Err(
            GuardCapabilityAdmissionError::InvalidModuleCapabilityShape {
                value: value.to_string(),
            },
        );
    }

    Ok(Some(ParsedModuleCapability {
        module_id,
        path_root,
    }))
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
    use super::{
        startup_defaults_for_qualified_name, AdmittedModuleGuardCapabilities, CompositionManifest,
        GuardCapabilityAdmission, ModuleGuardCapabilityError,
    };
    use aura_core::CapabilityName;
    use std::error::Error;

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

    #[test]
    fn validate_guard_capabilities_accepts_canonical_names() -> Result<(), Box<dyn Error>> {
        let manifest = CompositionManifest {
            protocol_name: "TestProtocol".to_string(),
            protocol_namespace: Some("test".to_string()),
            protocol_qualified_name: "test.TestProtocol".to_string(),
            protocol_id: "test.protocol".to_string(),
            role_names: vec!["Alice".to_string(), "Bob".to_string()],
            required_capabilities: Vec::new(),
            guard_capabilities: vec![
                CapabilityName::parse("chat:message:send")?,
                CapabilityName::parse("amp:receive")?,
            ],
            determinism_policy_ref: None,
            link_specs: Vec::new(),
            delegation_constraints: Vec::new(),
        };

        manifest.validate_guard_capabilities(GuardCapabilityAdmission::first_party_only())?;
        Ok(())
    }

    #[test]
    fn validate_guard_capabilities_rejects_legacy_or_unnamespaced_values(
    ) -> Result<(), Box<dyn Error>> {
        let manifest = CompositionManifest {
            protocol_name: "LegacyProtocol".to_string(),
            protocol_namespace: Some("legacy".to_string()),
            protocol_qualified_name: "legacy.LegacyProtocol".to_string(),
            protocol_id: "legacy.protocol".to_string(),
            role_names: vec!["Alice".to_string(), "Bob".to_string()],
            required_capabilities: Vec::new(),
            guard_capabilities: vec![CapabilityName::parse("send_message")?],
            determinism_policy_ref: None,
            link_specs: Vec::new(),
            delegation_constraints: Vec::new(),
        };

        match manifest.validate_guard_capabilities(GuardCapabilityAdmission::first_party_only()) {
            Ok(()) => panic!("legacy capability name must fail manifest validation"),
            Err(error) => assert!(error.to_string().contains("canonical namespaced")),
        }
        Ok(())
    }

    #[test]
    fn admitted_module_capabilities_reject_reserved_roots() -> Result<(), Box<dyn Error>> {
        let first_party = AdmittedModuleGuardCapabilities::new(
            "calendar_pack",
            vec![CapabilityName::parse(
                "module:calendar_pack:invitation:send",
            )?],
        );
        match first_party {
            Ok(_) => panic!("reserved first-party root must fail"),
            Err(error) => {
                assert!(matches!(
                    error,
                    ModuleGuardCapabilityError::InvalidCapability(_)
                ));
                assert!(error
                    .to_string()
                    .contains("reserved first-party namespace root `invitation`"));
            }
        }

        let host_owned = AdmittedModuleGuardCapabilities::new(
            "calendar_pack",
            vec![CapabilityName::parse("module:calendar_pack:write:item")?],
        );
        match host_owned {
            Ok(_) => panic!("reserved host-owned root must fail"),
            Err(error) => assert!(error
                .to_string()
                .contains("reserved host-owned capability root `write`")),
        }
        Ok(())
    }

    #[test]
    fn module_capabilities_do_not_collide_across_modules() -> Result<(), Box<dyn Error>> {
        let alpha = AdmittedModuleGuardCapabilities::new(
            "alpha_module",
            vec![CapabilityName::parse("module:alpha_module:calendar:sync")?],
        )?;
        let beta = AdmittedModuleGuardCapabilities::new(
            "beta_module",
            vec![CapabilityName::parse("module:beta_module:calendar:sync")?],
        )?;
        let admitted = vec![alpha, beta];

        let manifest = CompositionManifest {
            protocol_name: "ModuleProtocol".to_string(),
            protocol_namespace: Some("module_pack".to_string()),
            protocol_qualified_name: "module_pack.ModuleProtocol".to_string(),
            protocol_id: "module.protocol".to_string(),
            role_names: vec!["Alice".to_string(), "Bob".to_string()],
            required_capabilities: Vec::new(),
            guard_capabilities: vec![
                CapabilityName::parse("module:alpha_module:calendar:sync")?,
                CapabilityName::parse("module:beta_module:calendar:sync")?,
            ],
            determinism_policy_ref: None,
            link_specs: Vec::new(),
            delegation_constraints: Vec::new(),
        };

        manifest.validate_guard_capabilities(
            GuardCapabilityAdmission::with_admitted_module_capabilities(&admitted),
        )?;
        Ok(())
    }

    #[test]
    fn validate_guard_capabilities_rejects_undeclared_module_capabilities(
    ) -> Result<(), Box<dyn Error>> {
        let admitted = vec![AdmittedModuleGuardCapabilities::new(
            "calendar_pack",
            vec![CapabilityName::parse("module:calendar_pack:calendar:read")?],
        )?];
        let manifest = CompositionManifest {
            protocol_name: "ModuleProtocol".to_string(),
            protocol_namespace: Some("module_pack".to_string()),
            protocol_qualified_name: "module_pack.ModuleProtocol".to_string(),
            protocol_id: "module.protocol".to_string(),
            role_names: vec!["Alice".to_string(), "Bob".to_string()],
            required_capabilities: Vec::new(),
            guard_capabilities: vec![CapabilityName::parse(
                "module:calendar_pack:calendar:write",
            )?],
            determinism_policy_ref: None,
            link_specs: Vec::new(),
            delegation_constraints: Vec::new(),
        };

        match manifest.validate_guard_capabilities(
            GuardCapabilityAdmission::with_admitted_module_capabilities(&admitted),
        ) {
            Ok(()) => panic!("undeclared module guard capability must fail"),
            Err(error) => assert!(error
                .to_string()
                .contains("is not declared in the installed module release")),
        }
        Ok(())
    }
}
