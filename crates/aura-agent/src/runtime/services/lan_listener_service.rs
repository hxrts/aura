use super::service_actor::{validate_actor_transition, ActorLifecyclePhase};
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use super::LanTransportService;
#[cfg(not(target_arch = "wasm32"))]
use crate::runtime::system::spawn_lan_transport_listener_tasks;
use crate::runtime::AuraEffectSystem;
use crate::runtime::TaskGroup;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LanListenerServiceState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

impl LanListenerServiceState {
    fn phase(self) -> ActorLifecyclePhase {
        match self {
            Self::Stopped => ActorLifecyclePhase::Stopped,
            Self::Starting => ActorLifecyclePhase::Starting,
            Self::Running => ActorLifecyclePhase::Running,
            Self::Stopping => ActorLifecyclePhase::Stopping,
            Self::Failed => ActorLifecyclePhase::Failed,
        }
    }
}

struct LanListenerShared {
    tasks: RwLock<Option<TaskGroup>>,
    state: RwLock<LanListenerServiceState>,
    lifecycle: Mutex<()>,
}

#[derive(Clone)]
#[aura_macros::actor_root(
    owner = "lan_transport_listener_service",
    domain = "lan_transport_listener",
    supervision = "lan_transport_listener_task_root",
    category = "actor_owned"
)]
pub struct LanTransportListenerService {
    #[cfg(not(target_arch = "wasm32"))]
    effects: Arc<AuraEffectSystem>,
    #[cfg(not(target_arch = "wasm32"))]
    lan_transport: Arc<LanTransportService>,
    shared: Arc<LanListenerShared>,
}

impl LanTransportListenerService {
    pub fn new(effects: Arc<AuraEffectSystem>, lan_transport: Arc<LanTransportService>) -> Self {
        #[cfg(target_arch = "wasm32")]
        let _ = (&effects, &lan_transport);

        Self {
            #[cfg(not(target_arch = "wasm32"))]
            effects,
            #[cfg(not(target_arch = "wasm32"))]
            lan_transport,
            shared: Arc::new(LanListenerShared {
                tasks: RwLock::new(None),
                state: RwLock::new(LanListenerServiceState::Stopped),
                lifecycle: Mutex::new(()),
            }),
        }
    }

    async fn mark_state(&self, next: LanListenerServiceState) {
        *self.shared.state.write().await = next;
    }

    async fn start_managed(&self, context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        let _guard = self.shared.lifecycle.lock().await;
        let current = *self.shared.state.read().await;
        if current == LanListenerServiceState::Running {
            return Ok(());
        }
        validate_actor_transition(self.name(), current.phase(), ActorLifecyclePhase::Starting)?;
        self.mark_state(LanListenerServiceState::Starting).await;

        #[cfg(target_arch = "wasm32")]
        {
            let _ = context;
            self.mark_state(LanListenerServiceState::Running).await;
            return Ok(());
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let tasks = context.tasks().group(self.name());
            spawn_lan_transport_listener_tasks(
                tasks.clone(),
                self.effects.clone(),
                self.lan_transport.clone(),
            );
            *self.shared.tasks.write().await = Some(tasks);
            self.mark_state(LanListenerServiceState::Running).await;
            Ok(())
        }
    }

    async fn stop_managed(&self) -> Result<(), ServiceError> {
        let _guard = self.shared.lifecycle.lock().await;
        let current = *self.shared.state.read().await;
        if current == LanListenerServiceState::Stopped {
            return Ok(());
        }
        validate_actor_transition(self.name(), current.phase(), ActorLifecyclePhase::Stopping)?;
        self.mark_state(LanListenerServiceState::Stopping).await;

        let shutdown_error = if let Some(tasks) = self.shared.tasks.write().await.take() {
            tasks
                .shutdown_with_timeout(Duration::from_secs(2))
                .await
                .err()
                .map(|error| {
                    ServiceError::shutdown_failed(
                        self.name(),
                        format!("failed to stop LAN listener task group: {error}"),
                    )
                })
        } else {
            None
        };

        match shutdown_error {
            Some(error) => {
                self.mark_state(LanListenerServiceState::Failed).await;
                Err(error)
            }
            None => {
                self.mark_state(LanListenerServiceState::Stopped).await;
                Ok(())
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RuntimeService for LanTransportListenerService {
    fn name(&self) -> &'static str {
        "lan_transport_listener"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["reactive_pipeline"]
    }

    async fn start(&self, context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        self.start_managed(context).await
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        self.stop_managed().await
    }

    async fn health(&self) -> ServiceHealth {
        match *self.shared.state.read().await {
            LanListenerServiceState::Stopped => ServiceHealth::Stopped,
            LanListenerServiceState::Starting => ServiceHealth::Starting,
            LanListenerServiceState::Stopping => ServiceHealth::Stopping,
            LanListenerServiceState::Failed => ServiceHealth::Unhealthy {
                reason: "LAN listener entered failed lifecycle state".to_string(),
            },
            LanListenerServiceState::Running => {
                #[cfg(target_arch = "wasm32")]
                {
                    ServiceHealth::Healthy
                }

                #[cfg(not(target_arch = "wasm32"))]
                {
                    if self.shared.tasks.read().await.is_some() {
                        ServiceHealth::Healthy
                    } else {
                        ServiceHealth::Unhealthy {
                            reason: "LAN listener missing task group".to_string(),
                        }
                    }
                }
            }
        }
    }
}
