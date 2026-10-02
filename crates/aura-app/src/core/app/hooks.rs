//! Hook-installation and runtime-detach responsibilities for `AppCore`.

use super::state::AppCore;
use crate::core::IntentError;
use crate::workflows::system::hooks::{HookGroup, HookInstallError};
use async_lock::RwLock;
use std::sync::Arc;

pub(super) enum HookInstallState {
    Stopped,
    Installing,
    Ready(HookGroup),
}

impl AppCore {
    /// Initialize signals and attach one complete runtime-backed hook group.
    pub async fn init_signals_with_hooks(
        app_core: &Arc<RwLock<AppCore>>,
    ) -> Result<(), IntentError> {
        let gate = {
            let core = app_core.read().await;
            Arc::clone(&core.hook_install_gate)
        };
        let _install = gate.lock().await;

        {
            let mut core = app_core.write().await;
            core.ensure_signals_registered().await?;
            if core.runtime().is_none()
                || matches!(&core.hook_install_state, HookInstallState::Ready(group) if group.is_active())
            {
                return Ok(());
            }
            core.hook_install_state = HookInstallState::Installing;
        }

        let installed =
            crate::workflows::system::hooks::install_system_refresh_hooks(app_core).await;
        let mut core = app_core.write().await;
        match installed {
            Ok(group) => {
                core.hook_install_state = HookInstallState::Ready(group);
                Ok(())
            }
            Err(error) => {
                core.hook_install_state = HookInstallState::Stopped;
                Err(match error {
                    HookInstallError::Reactive { signal_id, source } => {
                        IntentError::reactive_failure(signal_id, source)
                    }
                    HookInstallError::RuntimeUnavailable => {
                        IntentError::no_agent("refresh hooks require an attached runtime")
                    }
                    other => IntentError::service_error(other.to_string()),
                })
            }
        }
    }

    /// Detach the runtime bridge and cancel its refresh-hook group.
    pub async fn detach_runtime(app_core: &Arc<RwLock<AppCore>>) -> bool {
        let gate = {
            let core = app_core.read().await;
            Arc::clone(&core.hook_install_gate)
        };
        let _install = gate.lock().await;
        let mut core = app_core.write().await;
        let had_runtime = core.runtime.take().is_some();
        core.hook_install_state = HookInstallState::Stopped;
        had_runtime
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AppConfig;
    use crate::runtime_bridge::OfflineRuntimeBridge;
    use crate::signal_defs::{CONTACTS_SIGNAL, SYNC_STATUS_SIGNAL};
    use aura_core::AuthorityId;
    use futures::FutureExt;

    fn test_runtime(seed: u8) -> Arc<OfflineRuntimeBridge> {
        let runtime =
            crate::testing::running_offline_runtime(AuthorityId::new_from_entropy([seed; 32]));
        runtime.set_pending_invitations(Vec::new());
        runtime
    }

    #[tokio::test]
    async fn hook_group_is_idempotent_and_cancels_on_detach() {
        let runtime = test_runtime(31);
        let app = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime.clone()).unwrap(),
        ));

        AppCore::init_signals_with_hooks(&app).await.unwrap();
        let reactive = { app.read().await.reactive().clone() };
        #[cfg(feature = "signals")]
        let ids = [
            CONTACTS_SIGNAL.id(),
            SYNC_STATUS_SIGNAL.id(),
            crate::signal_defs::CHAT_SIGNAL.id(),
            crate::signal_defs::HOMES_SIGNAL.id(),
            crate::signal_defs::TRANSPORT_PEERS_SIGNAL.id(),
            crate::signal_defs::INVITATIONS_SIGNAL.id(),
        ];
        #[cfg(not(feature = "signals"))]
        let ids = [CONTACTS_SIGNAL.id(), SYNC_STATUS_SIGNAL.id()];
        for id in ids {
            assert_eq!(reactive.graph().subscriber_count(id).await, 1);
        }
        let cancellation = {
            let core = app.read().await;
            match &core.hook_install_state {
                HookInstallState::Ready(group) => {
                    assert!(group.is_active());
                    group.cancellation_receiver()
                }
                _ => panic!("hook group must be ready"),
            }
        };
        AppCore::init_signals_with_hooks(&app).await.unwrap();
        assert!(cancellation.clone().now_or_never().is_none());
        for id in ids {
            assert_eq!(reactive.graph().subscriber_count(id).await, 1);
        }

        assert!(AppCore::detach_runtime(&app).await);
        cancellation.await;
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let mut attached = 0;
                for id in ids {
                    attached += reactive.graph().subscriber_count(id).await;
                }
                if attached == 0 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("detached hook receivers should close");
        {
            let mut core = app.write().await;
            assert!(matches!(core.hook_install_state, HookInstallState::Stopped));
            core.runtime = Some(runtime);
        }
        AppCore::init_signals_with_hooks(&app).await.unwrap();
        let core = app.read().await;
        assert!(
            matches!(&core.hook_install_state, HookInstallState::Ready(group) if group.is_active())
        );
        drop(core);
        for id in ids {
            assert_eq!(reactive.graph().subscriber_count(id).await, 1);
        }
    }

    #[tokio::test]
    async fn partially_attached_hook_group_rolls_back_and_retries() {
        #[cfg(feature = "signals")]
        use crate::signal_defs::{
            CHAT_SIGNAL, HOMES_SIGNAL, INVITATIONS_SIGNAL, TRANSPORT_PEERS_SIGNAL,
        };

        #[cfg(feature = "signals")]
        let failure_steps = 6;
        #[cfg(not(feature = "signals"))]
        let failure_steps = 2;

        for failed_step in 0..failure_steps {
            let runtime = test_runtime(32 + failed_step as u8);
            let app = Arc::new(RwLock::new(
                AppCore::with_runtime(AppConfig::default(), runtime).unwrap(),
            ));
            let reactive = { app.read().await.reactive().clone() };
            app.write().await.ensure_signals_registered().await.unwrap();
            assert!(
                crate::workflows::system::hooks::install_system_refresh_hooks_with_fault(
                    &app,
                    Some(failed_step),
                )
                .await
                .is_err(),
                "attachment step {failed_step} must fail"
            );
            #[cfg(feature = "signals")]
            let ids = [
                CONTACTS_SIGNAL.id(),
                SYNC_STATUS_SIGNAL.id(),
                CHAT_SIGNAL.id(),
                HOMES_SIGNAL.id(),
                TRANSPORT_PEERS_SIGNAL.id(),
                INVITATIONS_SIGNAL.id(),
            ];
            #[cfg(not(feature = "signals"))]
            let ids = [CONTACTS_SIGNAL.id(), SYNC_STATUS_SIGNAL.id()];
            tokio::time::timeout(std::time::Duration::from_secs(1), async {
                loop {
                    let mut attached = 0;
                    for id in ids {
                        attached += reactive.graph().subscriber_count(id).await;
                    }
                    if attached == 0 {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("failed group must release every attached receiver");

            AppCore::init_signals_with_hooks(&app).await.unwrap();
            assert!(
                matches!(&app.read().await.hook_install_state, HookInstallState::Ready(group) if group.is_active())
            );
            for id in ids {
                assert_eq!(reactive.graph().subscriber_count(id).await, 1);
            }
        }
    }

    #[tokio::test]
    async fn registration_type_conflict_returns_typed_reactive_failure() {
        let app = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), test_runtime(61)).unwrap(),
        ));
        let reactive = { app.read().await.reactive().clone() };
        reactive
            .graph()
            .ensure_registered(CONTACTS_SIGNAL.id().clone(), 4u32)
            .await
            .unwrap();

        let error = AppCore::init_signals_with_hooks(&app).await.unwrap_err();
        assert!(matches!(
            error,
            IntentError::ReactiveFailure {
                kind: crate::core::error::ReactiveFailureKind::TypeMismatch,
                signal_id,
                ..
            } if signal_id == CONTACTS_SIGNAL.id().to_string()
        ));
        assert!(matches!(
            app.read().await.hook_install_state,
            HookInstallState::Stopped
        ));
    }

    #[tokio::test]
    async fn task_dropping_spawner_cannot_report_hook_readiness() {
        let runtime = Arc::new(OfflineRuntimeBridge::new(AuthorityId::new_from_entropy(
            [62; 32],
        )));
        runtime.set_pending_invitations(Vec::new());
        let app = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime).unwrap(),
        ));

        let error = AppCore::init_signals_with_hooks(&app).await.unwrap_err();
        assert!(matches!(error, IntentError::ServiceError { .. }));
        assert!(matches!(
            app.read().await.hook_install_state,
            HookInstallState::Stopped
        ));
    }
}
