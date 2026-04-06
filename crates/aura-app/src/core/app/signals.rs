//! Reactive and callback signal surfaces for `AppCore`.

#[cfg(feature = "callbacks")]
use super::state::SubscriptionId;
use super::state::{AppCore, APP_RUNTIME_OPERATION_TIMEOUT, APP_RUNTIME_QUERY_TIMEOUT};
use crate::core::IntentError;
use async_trait::async_trait;
use aura_core::effects::reactive::{
    ReactiveEffects, ReactiveError, Signal, SignalId, SignalStream,
};
use aura_core::query::{FactPredicate, Query};

impl AppCore {
    /// Initialize all application signals with default values.
    pub async fn init_signals(&mut self) -> Result<(), IntentError> {
        if let Some(runtime) = self.runtime.as_ref() {
            if crate::workflows::runtime::timeout_runtime_call(
                runtime,
                "init_signals",
                "get_threshold_config",
                APP_RUNTIME_QUERY_TIMEOUT,
                || runtime.get_threshold_config(),
            )
            .await
            .map_err(|error| IntentError::internal_error(error.to_string()))?
            .is_none()
            {
                let bootstrap = crate::workflows::runtime::timeout_runtime_call(
                    runtime,
                    "init_signals",
                    "bootstrap_signing_keys",
                    APP_RUNTIME_OPERATION_TIMEOUT,
                    || runtime.bootstrap_signing_keys(),
                )
                .await
                .map_err(|error| IntentError::internal_error(error.to_string()))?;
                match bootstrap {
                    Ok(_public_key) => {}
                    Err(IntentError::NoAgent { .. }) => {}
                    Err(error) => {
                        return Err(IntentError::internal_error(format!(
                            "Failed to bootstrap signing keys: {error}"
                        )));
                    }
                }
            }
        }

        let sentinel_id = (*crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).id();
        if self.reactive.is_registered(sentinel_id) {
            return Ok(());
        }

        crate::signal_defs::register_app_signals(&self.reactive)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to initialize signals: {e}"))
            })?;

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
