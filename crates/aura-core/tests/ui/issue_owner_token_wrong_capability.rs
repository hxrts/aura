use aura_core::ownership::{issue_owner_token, ActorIngressMutationCapability};

fn main() {
    let capability = ActorIngressMutationCapability::new("actor:ingress");
    let _ = issue_owner_token(&capability, "token-1", "session");
}
