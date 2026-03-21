use std::sync::Arc;

use aura_terminal::tui::callbacks::SendOwnedCallback;

fn main() {
    let _: SendOwnedCallback = Arc::new(|_channel_id: String, _content: String| {});
}
