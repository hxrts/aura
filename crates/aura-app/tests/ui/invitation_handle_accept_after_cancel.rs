use async_lock::RwLock;
use aura_app::core::AppCore;
use aura_app::ui::workflows::invitation::{
    accept_invitation_with_terminal_status, cancel_invitation, InvitationAcceptanceRequest,
    InvitationHandle,
};
use std::sync::Arc;

fn never<T>() -> T {
    loop {}
}

async fn consume_twice(app_core: Arc<RwLock<AppCore>>, handle: InvitationHandle) {
    let _ = cancel_invitation(&app_core, handle).await;
    let _ = accept_invitation_with_terminal_status(
        &app_core,
        InvitationAcceptanceRequest::RetainedHandle {
            invitation: Box::new(handle),
            operation_instance_id: None,
        },
    )
    .await;
}

fn main() {
    let app_core: Arc<RwLock<AppCore>> = never();
    let handle: InvitationHandle = never();
    let _future = consume_twice(app_core, handle);
}
