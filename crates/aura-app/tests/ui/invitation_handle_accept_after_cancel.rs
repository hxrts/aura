use async_lock::RwLock;
use aura_app::core::AppCore;
use aura_app::ui::workflows::invitation::{accept_invitation, cancel_invitation, InvitationHandle};
use std::sync::Arc;

fn never<T>() -> T {
    loop {}
}

async fn consume_twice(app_core: Arc<RwLock<AppCore>>, handle: InvitationHandle) {
    let _ = cancel_invitation(&app_core, handle).await;
    let _ = accept_invitation(&app_core, handle).await;
}

fn main() {
    let app_core: Arc<RwLock<AppCore>> = never();
    let handle: InvitationHandle = never();
    let _future = consume_twice(app_core, handle);
}
