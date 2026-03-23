use std::sync::Arc;

use aura_terminal::tui::callbacks::AddDeviceCallback;

fn main() {
    let _: AddDeviceCallback =
        Arc::new(|_nickname: String, _authority_id: aura_core::AuthorityId| {});
}
