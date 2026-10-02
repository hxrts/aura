use aura_app::views::home::{HomeState, HomesState};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};

fn main() {
    let mut homes = HomesState::new();
    let raw = HomeState::new(
        ChannelId::from_bytes([1; 32]),
        Some("partial".into()),
        AuthorityId::new_from_entropy([2; 32]),
        1,
        ContextId::new_from_entropy([3; 32]),
    );
    let _ = homes.add_home(raw);
}
