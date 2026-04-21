use aura_core::{types::identifiers::AuthorityId, Hash32};
use aura_recovery::{guardian_ceremony::GuardianRotationOp, types::GuardianProfile};

pub fn authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

pub fn hash(seed: u8) -> Hash32 {
    Hash32([seed; 32])
}

pub fn guardian_profile(seed: u8) -> GuardianProfile {
    GuardianProfile::new(authority(seed))
}

pub fn guardian_profile_with_label(seed: u8, label: &str) -> GuardianProfile {
    GuardianProfile::with_label(authority(seed), label.to_string())
}

pub fn guardian_ids(seeds: &[u8]) -> Vec<AuthorityId> {
    seeds.iter().copied().map(authority).collect()
}

pub fn guardian_rotation_op(threshold_k: u16, seeds: &[u8], new_epoch: u64) -> GuardianRotationOp {
    let guardian_ids = guardian_ids(seeds);
    GuardianRotationOp {
        threshold_k,
        total_n: guardian_ids.len() as u16,
        guardian_ids,
        new_epoch,
    }
}
