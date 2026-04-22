use aura_authentication::guardian_auth_relational::{
    verify_guardian_proof, GuardianAuthProof, GuardianAuthRequest,
};
use aura_core::effects::PhysicalTimeEffects;
use aura_relational::RelationalContext;

fn value<T>() -> T {
    panic!("compile-fail fixture")
}

async fn verify_with_caller_supplied_key<T: PhysicalTimeEffects>(time_effects: &T) {
    let context: RelationalContext = value();
    let request: GuardianAuthRequest = value();
    let proof: GuardianAuthProof = value();
    let self_supplied_key = [0_u8; 32];

    let _ = verify_guardian_proof(
        &context,
        &request,
        &proof,
        time_effects,
        &self_supplied_key,
    )
    .await;
}

fn main() {}
