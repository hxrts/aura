use aura_core::{issue_owner_token, OwnershipTransferCapability};

fn main() {
    let capability = OwnershipTransferCapability::new("ownership:transfer");
    let token = issue_owner_token(&capability, "token-1", "session");
    let _transfer = token.handoff("owner-b");
    let _ = token.scope();
}
