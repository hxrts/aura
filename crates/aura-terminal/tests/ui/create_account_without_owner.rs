use std::sync::Arc;

use aura_terminal::tui::callbacks::CreateAccountCallback;

fn main() {
    let _: CreateAccountCallback = Arc::new(|_name: String| {});
}
