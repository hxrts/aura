use aura_core::OpaqueOperationHandle;

struct Invite;

fn main() {
    let _ = OpaqueOperationHandle::<Invite, _, _>::new("invitation_create", 7u64);
}
