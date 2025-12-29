//! Ceremony lifecycle policy matrix (K/A + fallbacks).

use super::AgreementMode;

/// Key-generation policy for a ceremony.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyGenerationPolicy {
    /// K1: Single-signer (no DKG).
    K1SingleSigner,
    /// K2: Dealer-based DKG (trusted coordinator).
    K2DealerBased,
    /// K3: Consensus-finalized DKG.
    K3ConsensusDkg,
    /// Non-DKG derivation (e.g., DKD).
    Dkd,
    /// Not applicable (no keygen step).
    NotApplicable,
}

/// Ceremony flows that require explicit lifecycle policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CeremonyFlow {
    AuthorityBootstrap,
    DeviceEnrollment,
    DeviceMfaRotation,
    GuardianSetupRotation,
    RecoveryApproval,
    RecoveryExecution,
    AmpEpochBump,
    Invitation,
    GroupBlockCreation,
    AmpBootstrap,
    RendezvousSecureChannel,
    OtaActivation,
    DkdCeremony,
    DeviceRemoval,
}

/// Lifecycle policy for a ceremony.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CeremonyLifecyclePolicy {
    pub keygen: KeyGenerationPolicy,
    pub agreement_sequence: &'static [AgreementMode],
    pub fallback: &'static [AgreementMode],
}

const A3_ONLY: [AgreementMode; 1] = [AgreementMode::ConsensusFinalized];
const A2_A3: [AgreementMode; 2] =
    [AgreementMode::CoordinatorSoftSafe, AgreementMode::ConsensusFinalized];
const A1_A2_A3: [AgreementMode; 3] = [
    AgreementMode::Provisional,
    AgreementMode::CoordinatorSoftSafe,
    AgreementMode::ConsensusFinalized,
];
const FALLBACK_NONE: [AgreementMode; 0] = [];
const FALLBACK_A2: [AgreementMode; 1] = [AgreementMode::CoordinatorSoftSafe];
const FALLBACK_A1_A2: [AgreementMode; 2] =
    [AgreementMode::Provisional, AgreementMode::CoordinatorSoftSafe];

/// Return the policy for a given ceremony flow.
pub fn policy_for(flow: CeremonyFlow) -> CeremonyLifecyclePolicy {
    match flow {
        CeremonyFlow::AuthorityBootstrap => CeremonyLifecyclePolicy {
            keygen: KeyGenerationPolicy::K1SingleSigner,
            agreement_sequence: &A3_ONLY,
            fallback: &FALLBACK_NONE,
        },
        CeremonyFlow::DeviceEnrollment => CeremonyLifecyclePolicy {
            keygen: KeyGenerationPolicy::K2DealerBased,
            agreement_sequence: &A1_A2_A3,
            fallback: &FALLBACK_A1_A2,
        },
        CeremonyFlow::DeviceMfaRotation => CeremonyLifecyclePolicy {
            keygen: KeyGenerationPolicy::K3ConsensusDkg,
            agreement_sequence: &A2_A3,
            fallback: &FALLBACK_A2,
        },
        CeremonyFlow::GuardianSetupRotation => CeremonyLifecyclePolicy {
            keygen: KeyGenerationPolicy::K3ConsensusDkg,
            agreement_sequence: &A2_A3,
            fallback: &FALLBACK_A2,
        },
        CeremonyFlow::RecoveryApproval => CeremonyLifecyclePolicy {
            keygen: KeyGenerationPolicy::NotApplicable,
            agreement_sequence: &A2_A3,
            fallback: &FALLBACK_A2,
        },
        CeremonyFlow::RecoveryExecution => CeremonyLifecyclePolicy {
            keygen: KeyGenerationPolicy::NotApplicable,
            agreement_sequence: &A2_A3,
            fallback: &FALLBACK_A2,
        },
        CeremonyFlow::AmpEpochBump => CeremonyLifecyclePolicy {
            keygen: KeyGenerationPolicy::NotApplicable,
            agreement_sequence: &A1_A2_A3,
            fallback: &FALLBACK_A1_A2,
        },
        CeremonyFlow::Invitation => CeremonyLifecyclePolicy {
            keygen: KeyGenerationPolicy::NotApplicable,
            agreement_sequence: &A3_ONLY,
            fallback: &FALLBACK_NONE,
        },
        CeremonyFlow::GroupBlockCreation => CeremonyLifecyclePolicy {
            keygen: KeyGenerationPolicy::K3ConsensusDkg,
            agreement_sequence: &A1_A2_A3,
            fallback: &FALLBACK_A1_A2,
        },
        CeremonyFlow::AmpBootstrap => CeremonyLifecyclePolicy {
            keygen: KeyGenerationPolicy::NotApplicable,
            agreement_sequence: &A1_A2_A3,
            fallback: &FALLBACK_A1_A2,
        },
        CeremonyFlow::RendezvousSecureChannel => CeremonyLifecyclePolicy {
            keygen: KeyGenerationPolicy::NotApplicable,
            agreement_sequence: &A1_A2_A3,
            fallback: &FALLBACK_A1_A2,
        },
        CeremonyFlow::OtaActivation => CeremonyLifecyclePolicy {
            keygen: KeyGenerationPolicy::NotApplicable,
            agreement_sequence: &A2_A3,
            fallback: &FALLBACK_A2,
        },
        CeremonyFlow::DkdCeremony => CeremonyLifecyclePolicy {
            keygen: KeyGenerationPolicy::Dkd,
            agreement_sequence: &A2_A3,
            fallback: &FALLBACK_A2,
        },
        CeremonyFlow::DeviceRemoval => CeremonyLifecyclePolicy {
            keygen: KeyGenerationPolicy::K3ConsensusDkg,
            agreement_sequence: &A2_A3,
            fallback: &FALLBACK_A2,
        },
    }
}

impl CeremonyLifecyclePolicy {
    /// First agreement mode in the policy sequence.
    pub fn initial_mode(&self) -> AgreementMode {
        self.agreement_sequence
            .first()
            .copied()
            .unwrap_or(AgreementMode::ConsensusFinalized)
    }

    /// Whether a mode is allowed by this policy.
    pub fn allows_mode(&self, mode: AgreementMode) -> bool {
        self.agreement_sequence.contains(&mode) || self.fallback.contains(&mode)
    }
}
