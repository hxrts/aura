use aura_core::ownership::{issue_operation_handle, LifecyclePublicationCapability};

struct Invite;

fn main() {
    let capability = LifecyclePublicationCapability::new("semantic:lifecycle");
    let _ = issue_operation_handle::<Invite, _, _>(&capability, "invitation_create", 1u64);
}
