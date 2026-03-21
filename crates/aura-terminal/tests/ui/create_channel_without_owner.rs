use std::sync::Arc;

use aura_terminal::tui::callbacks::CreateChannelCallback;

fn main() {
    let _: CreateChannelCallback = Arc::new(
        |_name: String, _topic: Option<String>, _members: Vec<String>, _threshold_k: u8| {},
    );
}
