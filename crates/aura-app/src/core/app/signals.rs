//! Reactive and callback signal surfaces for `AppCore`.
#![allow(missing_docs)]

use super::state::{AppCore, APP_RUNTIME_OPERATION_TIMEOUT, APP_RUNTIME_QUERY_TIMEOUT};
#[cfg(feature = "callbacks")]
use super::SubscriptionId;
use crate::core::IntentError;
use crate::runtime_bridge::{RuntimeBridgeError, RuntimeBridgeErrorKind};
use async_trait::async_trait;
use aura_core::effects::reactive::{
    ReactiveEffects, ReactiveError, Signal, SignalId, SignalStream,
};
use aura_core::query::{FactPredicate, Query};

fn signals_runtime_boundary(error: aura_core::AuraError) -> RuntimeBridgeError {
    use std::error::Error;
    let mut cause: Option<&(dyn Error + 'static)> = Some(&error);
    let mut timed_out = false;
    while let Some(source) = cause {
        if matches!(
            source.downcast_ref::<crate::workflows::error::WorkflowError>(),
            Some(crate::workflows::error::WorkflowError::TimedOut { .. })
        ) {
            timed_out = true;
            break;
        }
        cause = source.source();
    }
    let native = RuntimeBridgeError::with_source(
        IntentError::service_error("required signal initialization runtime boundary failed"),
        error,
    );
    if timed_out {
        native.with_kind(RuntimeBridgeErrorKind::TimedOut)
    } else {
        native
    }
}

impl AppCore {
    /// Initialize all application signals with default values.
    pub(super) async fn ensure_signals_registered(&mut self) -> Result<(), RuntimeBridgeError> {
        if let Some(runtime) = self.runtime.as_ref() {
            if crate::workflows::runtime::timeout_runtime_call(
                runtime,
                "ensure_signals_registered",
                "get_threshold_config",
                APP_RUNTIME_QUERY_TIMEOUT,
                || runtime.get_threshold_config(),
            )
            .await
            .map_err(signals_runtime_boundary)?
            .is_none()
            {
                let bootstrap = crate::workflows::runtime::timeout_runtime_call(
                    runtime,
                    "ensure_signals_registered",
                    "bootstrap_signing_keys",
                    APP_RUNTIME_OPERATION_TIMEOUT,
                    || runtime.bootstrap_signing_keys(),
                )
                .await
                .map_err(signals_runtime_boundary)?;
                match bootstrap {
                    Ok(_public_key) => {}
                    Err(error) if error.kind() == RuntimeBridgeErrorKind::NoAgent => {}
                    Err(error) => {
                        return Err(error);
                    }
                }
            }
        }

        crate::signal_defs::register_app_signals(&self.reactive)
            .await
            .map_err(|error| {
                RuntimeBridgeError::with_source(
                    IntentError::reactive_failure("app_signals", error.clone()),
                    error,
                )
            })?;

        // Registration can target a newly attached runtime graph. Mirror the
        // original app-owned history into that observed graph before hooks run;
        // this restores an existing snapshot and issues no terminal fact.
        if !self.authoritative_semantic_facts.is_empty() {
            self.reactive
                .emit(
                    &*crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
                    crate::ui_contract::AuthoritativeSemanticFactsSnapshot {
                        revision: crate::ui_contract::next_projection_revision(None),
                        facts: self.authoritative_semantic_facts.clone(),
                    },
                )
                .await
                .map_err(|source| {
                    RuntimeBridgeError::with_source(
                        IntentError::reactive_failure(
                            "bootstrap semantic history snapshot",
                            source.clone(),
                        ),
                        source,
                    )
                })?;
        }

        // The runtime may already have replayed its journal before these
        // signals existed; replay into the now-registered signals.
        if let Some(runtime) = self.runtime.as_ref() {
            let replay = crate::workflows::runtime::timeout_runtime_call(
                runtime,
                "ensure_signals_registered",
                "replay_committed_facts",
                APP_RUNTIME_OPERATION_TIMEOUT,
                || runtime.replay_committed_facts(),
            )
            .await
            .map_err(signals_runtime_boundary)?;
            if let Err(error) = replay {
                if error.kind() != RuntimeBridgeErrorKind::NoAgent {
                    return Err(error);
                }
            }
        }

        Ok(())
    }
}

#[cfg(feature = "callbacks")]
impl AppCore {
    pub fn subscribe(
        &mut self,
        observer: std::sync::Arc<dyn crate::bridge::callback::StateObserver>,
    ) -> SubscriptionId {
        let id = self.observer_registry.add(observer);
        SubscriptionId { id }
    }

    pub fn unsubscribe(&mut self, id: SubscriptionId) {
        self.observer_registry.remove(id.id);
    }

    pub fn notify_observers(&self) {
        let snapshot = self.snapshot();
        self.observer_registry.notify_chat(&snapshot.chat);
        self.observer_registry.notify_recovery(&snapshot.recovery);
        self.observer_registry
            .notify_invitations(&snapshot.invitations);
        self.observer_registry.notify_contacts(&snapshot.contacts);
        self.observer_registry.notify_homes(&snapshot.homes);
        self.observer_registry
            .notify_neighborhood(&snapshot.neighborhood);
    }

    #[cfg(test)]
    pub fn observer_registry(&self) -> &crate::bridge::callback::ObserverRegistry {
        &self.observer_registry
    }
}

#[cfg(feature = "signals")]
impl AppCore {
    pub fn chat_signal(
        &self,
    ) -> impl futures_signals::signal::Signal<Item = crate::views::ChatState> {
        self.views.chat_signal()
    }

    pub fn recovery_signal(
        &self,
    ) -> impl futures_signals::signal::Signal<Item = crate::views::RecoveryState> {
        self.views.recovery_signal()
    }

    pub fn invitations_signal(
        &self,
    ) -> impl futures_signals::signal::Signal<Item = crate::views::InvitationsState> {
        self.views.invitations_signal()
    }

    pub fn contacts_signal(
        &self,
    ) -> impl futures_signals::signal::Signal<Item = crate::views::ContactsState> {
        self.views.contacts_signal()
    }

    pub fn neighborhood_signal(
        &self,
    ) -> impl futures_signals::signal::Signal<Item = crate::views::NeighborhoodState> {
        self.views.neighborhood_signal()
    }
}

#[async_trait]
impl ReactiveEffects for AppCore {
    async fn read<T>(&self, signal: &Signal<T>) -> Result<T, ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.reactive.read(signal).await
    }

    async fn emit<T>(&self, signal: &Signal<T>, value: T) -> Result<(), ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.reactive.emit(signal, value).await
    }

    fn subscribe<T>(&self, signal: &Signal<T>) -> Result<SignalStream<T>, ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.reactive.subscribe(signal)
    }

    async fn register<T>(&self, signal: &Signal<T>, initial: T) -> Result<(), ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.reactive.register(signal, initial).await
    }

    async fn ensure_registered<T>(
        &self,
        signal: &Signal<T>,
        initial: T,
    ) -> Result<(), ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.reactive.ensure_registered(signal, initial).await
    }

    fn is_registered(&self, signal_id: &SignalId) -> bool {
        self.reactive.is_registered(signal_id)
    }

    async fn register_query<Q: Query>(
        &self,
        signal: &Signal<Q::Result>,
        query: Q,
    ) -> Result<(), ReactiveError> {
        self.reactive.register_query(signal, query).await
    }

    fn query_dependencies(&self, signal_id: &SignalId) -> Option<Vec<FactPredicate>> {
        self.reactive.query_dependencies(signal_id)
    }

    async fn invalidate_queries(&self, changed: &FactPredicate) {
        self.reactive.invalidate_queries(changed).await;
    }
}
