use async_lock::RwLock;
use aura_app::ui::workflows::ceremonies::{cancel_key_rotation_ceremony, CeremonyHandle};
use aura_app::core::AppCore;
use std::sync::Arc;

fn never<T>() -> T {
    loop {}
}

async fn consume_twice(app_core: Arc<RwLock<AppCore>>, handle: CeremonyHandle) {
    let _ = cancel_key_rotation_ceremony(&app_core, handle).await;
    let _ = cancel_key_rotation_ceremony(&app_core, handle).await;
}

fn main() {
    let app_core: Arc<RwLock<AppCore>> = never();
    let handle: CeremonyHandle = never();
    let _future = consume_twice(app_core, handle);
}
