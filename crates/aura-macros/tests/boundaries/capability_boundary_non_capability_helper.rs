#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof"
)]
fn issue_pending_invitation_consumed_proof(invitation_id: &str) -> String {
    invitation_id.to_string()
}

fn main() {}
