use aura_app::ui::workflows::messaging::AuthoritativeChannelRef;
use aura_core::crypto::hash::hash;
use aura_core::types::identifiers::{ChannelId, ContextId};

fn main() {
    let channel_id = ChannelId::from_bytes(hash(b"compile-fail-authoritative-channel"));
    let context_id = ContextId::new_from_entropy([7u8; 32]);
    let _ = AuthoritativeChannelRef::new(channel_id, context_id);
}
