use aura_core::BoundedActorIngress;
use tokio::sync::{mpsc, oneshot, Mutex};

use super::traits::{ServiceError, ServiceErrorKind};
use crate::runtime::TaskGroup;

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
pub(crate) struct ServiceActorHandle<Domain, C> {
    name: &'static str,
    ingress: BoundedActorIngress<Domain, C>,
    cmd_tx: mpsc::Sender<C>,
}

#[derive(Debug)]
#[allow(dead_code)] // Ingress metadata is part of the actor boundary contract even when only tests inspect it directly today.
pub(crate) struct ServiceActorMailbox<Domain, C> {
    ingress: BoundedActorIngress<Domain, C>,
    cmd_rx: mpsc::Receiver<C>,
}

pub(crate) struct ActorOwnedServiceRoot<Domain, C, State> {
    lifecycle_state: Mutex<State>,
    tasks: Mutex<Option<TaskGroup>>,
    commands: Mutex<Option<ServiceActorHandle<Domain, C>>>,
    lifecycle: Mutex<()>,
}

impl<Domain, C, State> ActorOwnedServiceRoot<Domain, C, State> {
    pub(crate) fn new(initial_state: State) -> Self {
        Self {
            lifecycle_state: Mutex::new(initial_state),
            tasks: Mutex::new(None),
            commands: Mutex::new(None),
            lifecycle: Mutex::new(()),
        }
    }

    pub(crate) fn lifecycle(&self) -> &Mutex<()> {
        &self.lifecycle
    }

    pub(crate) async fn install_commands(&self, commands: ServiceActorHandle<Domain, C>) {
        *self.commands.lock().await = Some(commands);
    }

    pub(crate) async fn command_handle(
        &self,
        service: &'static str,
        unavailable_message: &'static str,
    ) -> Result<ServiceActorHandle<Domain, C>, ServiceError> {
        self.commands
            .lock()
            .await
            .clone()
            .ok_or_else(|| ServiceError::unavailable(service, unavailable_message))
    }

    pub(crate) async fn take_commands(&self) -> Option<ServiceActorHandle<Domain, C>> {
        self.commands.lock().await.take()
    }

    pub(crate) async fn has_commands(&self) -> bool {
        self.commands.lock().await.is_some()
    }

    pub(crate) async fn install_tasks(&self, tasks: TaskGroup) {
        *self.tasks.lock().await = Some(tasks);
    }

    #[cfg(test)]
    pub(crate) async fn task_group(&self) -> Option<TaskGroup> {
        self.tasks.lock().await.clone()
    }

    pub(crate) async fn take_tasks(&self) -> Option<TaskGroup> {
        self.tasks.lock().await.take()
    }

    pub(crate) async fn has_tasks(&self) -> bool {
        self.tasks.lock().await.is_some()
    }

    pub(crate) async fn state(&self) -> State
    where
        State: Copy,
    {
        *self.lifecycle_state.lock().await
    }

    pub(crate) async fn set_state(&self, state: State) {
        *self.lifecycle_state.lock().await = state;
    }
}

impl<Domain, C> Clone for ServiceActorHandle<Domain, C> {
    fn clone(&self) -> Self {
        Self {
            name: self.name,
            ingress: BoundedActorIngress::new(self.ingress.owner_name(), self.ingress.capacity()),
            cmd_tx: self.cmd_tx.clone(),
        }
    }
}

#[allow(dead_code)] // Shared handle methods are used incrementally as services migrate.
impl<Domain, C> ServiceActorHandle<Domain, C> {
    pub(crate) fn bounded(
        name: &'static str,
        capacity: u32,
    ) -> (Self, ServiceActorMailbox<Domain, C>) {
        let ingress = BoundedActorIngress::new(name, capacity);
        let (cmd_tx, cmd_rx) = mpsc::channel(ingress.capacity() as usize);
        let handle = Self::new(name, BoundedActorIngress::new(name, capacity), cmd_tx);
        let mailbox = ServiceActorMailbox { ingress, cmd_rx };
        (handle, mailbox)
    }

    pub(crate) fn new(
        name: &'static str,
        ingress: BoundedActorIngress<Domain, C>,
        cmd_tx: mpsc::Sender<C>,
    ) -> Self {
        Self {
            name,
            ingress,
            cmd_tx,
        }
    }

    pub(crate) fn ingress(&self) -> &BoundedActorIngress<Domain, C> {
        &self.ingress
    }

    pub(crate) async fn request<R>(
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

#[allow(dead_code)] // Mailbox metadata/read helpers are kept with the boundary type.
impl<Domain, C> ServiceActorMailbox<Domain, C> {
    pub(crate) fn ingress(&self) -> &BoundedActorIngress<Domain, C> {
        &self.ingress
    }

    pub(crate) async fn recv(&mut self) -> Option<C> {
        self.cmd_rx.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::BoundedActorIngress;
    use std::collections::VecDeque;
    use tokio::sync::mpsc;

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

    #[test]
    fn service_actor_handle_preserves_bounded_ingress_metadata() {
        let ingress = BoundedActorIngress::<(), u8>::new("svc", 8);
        let (tx, _rx) = mpsc::channel(ingress.capacity() as usize);
        let handle = ServiceActorHandle::new("svc", ingress.clone(), tx);

        assert_eq!(handle.ingress().owner_name(), "svc");
        assert_eq!(handle.ingress().capacity(), 8);
        assert_eq!(handle.ingress(), &ingress);
    }

    #[tokio::test]
    async fn bounded_service_actor_channel_uses_declared_capacity() {
        let (handle, mut mailbox) = ServiceActorHandle::<(), u8>::bounded("svc", 2);

        assert_eq!(handle.ingress().capacity(), 2);
        assert_eq!(mailbox.ingress().capacity(), 2);

        handle
            .cmd_tx
            .send(7)
            .await
            .expect("send into bounded mailbox");
        assert_eq!(mailbox.recv().await, Some(7));
    }

    #[test]
    fn loom_bounded_mailbox_send_recv_under_contention() {
        loom::model(|| {
            use loom::sync::atomic::{AtomicBool, Ordering};
            use loom::sync::{Arc as LoomArc, Mutex as LoomMutex};
            use loom::thread;

            struct LoomBoundedMailbox<T> {
                queue: LoomMutex<VecDeque<T>>,
                capacity: usize,
            }

            impl<T> LoomBoundedMailbox<T> {
                fn new(capacity: usize) -> Self {
                    Self {
                        queue: LoomMutex::new(VecDeque::new()),
                        capacity,
                    }
                }

                fn try_send(&self, value: T) -> Result<(), T> {
                    let mut queue = self.queue.lock().expect("mailbox lock");
                    if queue.len() >= self.capacity {
                        return Err(value);
                    }
                    queue.push_back(value);
                    Ok(())
                }

                fn recv(&self) -> Option<T> {
                    self.queue.lock().expect("mailbox lock").pop_front()
                }
            }

            let mailbox = LoomArc::new(LoomBoundedMailbox::new(1));
            let sender_one_admitted = LoomArc::new(AtomicBool::new(false));
            let sender_two_admitted = LoomArc::new(AtomicBool::new(false));

            let sender_one = {
                let mailbox = mailbox.clone();
                let sender_one_admitted = sender_one_admitted.clone();
                thread::spawn(move || {
                    if mailbox.try_send(1u8).is_ok() {
                        sender_one_admitted.store(true, Ordering::SeqCst);
                    }
                })
            };

            let sender_two = {
                let mailbox = mailbox.clone();
                let sender_two_admitted = sender_two_admitted.clone();
                thread::spawn(move || {
                    if mailbox.try_send(2u8).is_ok() {
                        sender_two_admitted.store(true, Ordering::SeqCst);
                    }
                })
            };

            sender_one.join().expect("sender one joins");
            sender_two.join().expect("sender two joins");

            let admitted_count = sender_one_admitted.load(Ordering::SeqCst) as u8
                + sender_two_admitted.load(Ordering::SeqCst) as u8;
            assert_eq!(admitted_count, 1);

            let first = mailbox.recv().expect("one sender must be admitted");
            let retry_value = if sender_one_admitted.load(Ordering::SeqCst) {
                2u8
            } else {
                1u8
            };
            mailbox
                .try_send(retry_value)
                .expect("the non-admitted sender must succeed after a receive");
            let second = mailbox.recv().expect("retry send must be receivable");

            let received = LoomMutex::new(vec![first, second]);
            let mut values = received.lock().expect("received lock").clone();
            values.sort_unstable();
            assert_eq!(values, vec![1, 2]);
        });
    }

    #[test]
    fn loom_actor_command_ingress_reports_backpressure() {
        loom::model(|| {
            use loom::sync::atomic::{AtomicBool, Ordering};
            use loom::sync::{Arc as LoomArc, Mutex as LoomMutex};
            use loom::thread;

            struct LoomBoundedMailbox<T> {
                queue: LoomMutex<VecDeque<T>>,
                capacity: usize,
            }

            impl<T> LoomBoundedMailbox<T> {
                fn new(capacity: usize) -> Self {
                    Self {
                        queue: LoomMutex::new(VecDeque::new()),
                        capacity,
                    }
                }

                fn try_send(&self, value: T) -> Result<(), T> {
                    let mut queue = self.queue.lock().expect("mailbox lock");
                    if queue.len() >= self.capacity {
                        return Err(value);
                    }
                    queue.push_back(value);
                    Ok(())
                }

                fn recv(&self) -> Option<T> {
                    self.queue.lock().expect("mailbox lock").pop_front()
                }
            }

            let mailbox = LoomArc::new(LoomBoundedMailbox::new(1));
            mailbox.try_send(1u8).expect("prefill mailbox");

            let backpressure_seen = LoomArc::new(AtomicBool::new(false));
            let sender_attempted = LoomArc::new(AtomicBool::new(false));
            let receiver_drained = LoomArc::new(AtomicBool::new(false));
            let received = LoomArc::new(LoomMutex::new(Vec::new()));

            let sender = {
                let mailbox = mailbox.clone();
                let backpressure_seen = backpressure_seen.clone();
                let sender_attempted = sender_attempted.clone();
                let receiver_drained = receiver_drained.clone();
                thread::spawn(move || match mailbox.try_send(2u8) {
                    Ok(()) => panic!("full mailbox must reject the first retry-free send"),
                    Err(value) => {
                        backpressure_seen.store(true, Ordering::SeqCst);
                        sender_attempted.store(true, Ordering::SeqCst);
                        while !receiver_drained.load(Ordering::SeqCst) {
                            thread::yield_now();
                        }
                        mailbox
                            .try_send(value)
                            .expect("mailbox admits command after receiver drains");
                    }
                })
            };

            let receiver = {
                let mailbox = mailbox.clone();
                let sender_attempted = sender_attempted.clone();
                let receiver_drained = receiver_drained.clone();
                let received = received.clone();
                thread::spawn(move || {
                    while !sender_attempted.load(Ordering::SeqCst) {
                        thread::yield_now();
                    }

                    let first = mailbox.recv().expect("prefilled command available");
                    received.lock().expect("received lock").push(first);
                    receiver_drained.store(true, Ordering::SeqCst);

                    while received.lock().expect("received lock").len() < 2 {
                        if let Some(value) = mailbox.recv() {
                            received.lock().expect("received lock").push(value);
                            break;
                        }
                        thread::yield_now();
                    }
                })
            };

            sender.join().expect("sender joins");
            receiver.join().expect("receiver joins");

            let values = received.lock().expect("received lock").clone();
            assert_eq!(values.len(), 2);
            assert!(backpressure_seen.load(Ordering::SeqCst));
            assert_eq!(values[0], 1);
            assert_eq!(values[1], 2);
        });
    }
}
