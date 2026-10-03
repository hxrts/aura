use std::sync::Arc;

use async_lock::RwLock;

use crate::runtime_bridge::OfflineRuntimeBridge;
use crate::{AppConfig, AppCore, RuntimeBridge};
use aura_core::{AuthorityId, OwnedShutdownToken, OwnedTaskSpawner};

pub(crate) fn running_offline_runtime(authority: AuthorityId) -> Arc<OfflineRuntimeBridge> {
    let mut runtime = OfflineRuntimeBridge::new(authority);
    runtime.use_test_task_spawner(OwnedTaskSpawner::new(
        Arc::new(aura_effects::reactive::CountingTestTaskSpawner::default()),
        OwnedShutdownToken::detached(),
    ));
    Arc::new(runtime)
}

#[track_caller]
pub(crate) fn test_app_core(config: AppConfig) -> Arc<RwLock<AppCore>> {
    Arc::new(RwLock::new(
        AppCore::new(config).unwrap_or_else(|error| panic!("{error}")),
    ))
}

#[track_caller]
pub(crate) fn default_test_app_core() -> Arc<RwLock<AppCore>> {
    test_app_core(AppConfig::default())
}

#[track_caller]
pub(crate) fn test_app_core_with_runtime<R>(
    config: AppConfig,
    runtime: Arc<R>,
) -> Arc<RwLock<AppCore>>
where
    R: RuntimeBridge + 'static,
{
    let runtime: Arc<dyn RuntimeBridge> = runtime;
    Arc::new(RwLock::new(
        AppCore::with_runtime(config, runtime).unwrap_or_else(|error| panic!("{error}")),
    ))
}
