use std::sync::Arc;

use aura_terminal::tui::callbacks::ImportInvitationOwnedCallback;

fn main() {
    let _: ImportInvitationOwnedCallback = Arc::new(|_code: String| {});
}
