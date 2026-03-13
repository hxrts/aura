use tokio::sync::{mpsc, oneshot};

use super::traits::{ServiceError, ServiceErrorKind};

#[allow(dead_code)] // `New` and some helpers are staged for later actor conversions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorLifecyclePhase {
    New,
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
}

pub fn validate_actor_transition(
    service: &'static str,
    from: ActorLifecyclePhase,
    to: ActorLifecyclePhase,
) -> Result<(), ServiceError> {
    let allowed = matches!(
        (from, to),
        (ActorLifecyclePhase::New, ActorLifecyclePhase::Starting)
            | (ActorLifecyclePhase::Starting, ActorLifecyclePhase::Running)
            | (ActorLifecyclePhase::Starting, ActorLifecyclePhase::Stopped)
            | (ActorLifecyclePhase::Starting, ActorLifecyclePhase::Failed)
            | (ActorLifecyclePhase::Running, ActorLifecyclePhase::Stopping)
            | (ActorLifecyclePhase::Running, ActorLifecyclePhase::Failed)
            | (ActorLifecyclePhase::Stopping, ActorLifecyclePhase::Stopped)
            | (ActorLifecyclePhase::Stopping, ActorLifecyclePhase::Failed)
            | (ActorLifecyclePhase::Stopped, ActorLifecyclePhase::Starting)
            | (ActorLifecyclePhase::Failed, ActorLifecyclePhase::Starting)
            | (ActorLifecyclePhase::Failed, ActorLifecyclePhase::Stopped)
    );

    if allowed {
        Ok(())
    } else {
        Err(ServiceError::new(
            service,
            ServiceErrorKind::Internal,
            format!("illegal lifecycle transition {from:?} -> {to:?}"),
        ))
    }
}

#[allow(dead_code)] // Shared handle type is introduced before all services migrate to command actors.
#[derive(Debug)]
pub struct ServiceActorHandle<C> {
    name: &'static str,
    cmd_tx: mpsc::Sender<C>,
}

impl<C> Clone for ServiceActorHandle<C> {
    fn clone(&self) -> Self {
        Self {
            name: self.name,
            cmd_tx: self.cmd_tx.clone(),
        }
    }
}

#[allow(dead_code)] // Shared handle methods are used incrementally as services migrate.
impl<C> ServiceActorHandle<C> {
    pub fn new(name: &'static str, cmd_tx: mpsc::Sender<C>) -> Self {
        Self { name, cmd_tx }
    }

    pub fn sender(&self) -> mpsc::Sender<C> {
        self.cmd_tx.clone()
    }

    pub async fn request<R>(
        &self,
        build: impl FnOnce(oneshot::Sender<R>) -> C,
    ) -> Result<R, ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx.send(build(reply_tx)).await.map_err(|_| {
            ServiceError::unavailable(
                self.name,
                "service actor command channel closed before request dispatch",
            )
        })?;
        reply_rx.await.map_err(|_| {
            ServiceError::internal(
                self.name,
                "service actor dropped reply channel before completing request",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_lifecycle_transition_rules_are_explicit() {
        assert!(validate_actor_transition(
            "svc",
            ActorLifecyclePhase::Stopped,
            ActorLifecyclePhase::Starting
        )
        .is_ok());
        assert!(validate_actor_transition(
            "svc",
            ActorLifecyclePhase::Running,
            ActorLifecyclePhase::Stopping
        )
        .is_ok());
        assert!(validate_actor_transition(
            "svc",
            ActorLifecyclePhase::Running,
            ActorLifecyclePhase::Starting
        )
        .is_err());
    }
}
