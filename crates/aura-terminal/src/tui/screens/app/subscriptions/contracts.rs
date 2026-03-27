use super::*;

#[derive(Clone)]
pub(super) struct StructuralDegradationSink {
    tasks: Arc<UiTaskOwner>,
    update_tx: Option<UiUpdateSender>,
}

pub(super) fn report_subscription_degradation(
    sink: &StructuralDegradationSink,
    signal_id: impl Into<String>,
    reason: impl Into<String>,
) {
    publish_structural_degradation(
        sink,
        SubscriptionDegradationNotice::structural_exhaustion(signal_id.into(), reason.into()),
    );
}

impl StructuralDegradationSink {
    pub(super) fn new(tasks: Arc<UiTaskOwner>, update_tx: Option<UiUpdateSender>) -> Self {
        Self { tasks, update_tx }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SubscriptionDegradationReason {
    StructuralExhaustion(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SubscriptionDegradationNotice {
    signal_id: String,
    reason: SubscriptionDegradationReason,
}

impl SubscriptionDegradationNotice {
    fn structural_exhaustion(signal_id: String, reason: String) -> Self {
        Self {
            signal_id,
            reason: SubscriptionDegradationReason::StructuralExhaustion(reason),
        }
    }

    fn into_update(self) -> UiUpdate {
        let SubscriptionDegradationReason::StructuralExhaustion(reason) = self.reason;
        UiUpdate::SubscriptionDegraded {
            signal_id: self.signal_id,
            reason,
        }
    }
}

fn publish_structural_degradation(
    sink: &StructuralDegradationSink,
    notice: SubscriptionDegradationNotice,
) {
    if let Some(tx) = sink.update_tx.as_ref() {
        spawn_ui_update(
            &sink.tasks,
            tx,
            notice.into_update(),
            UiUpdatePublication::RequiredUnordered,
        );
    }
}

async fn subscribe_with_structural_degradation<T, F>(
    app_core: InitializedAppCore,
    signal: &'static aura_core::effects::reactive::Signal<T>,
    on_value: F,
    degradation: StructuralDegradationSink,
) where
    T: Clone + Send + Sync + 'static,
    F: FnMut(T) + Send + 'static,
{
    let signal_id = signal.id().to_string();
    subscribe_signal_with_retry_report(app_core, signal, on_value, move |reason| {
        report_subscription_degradation(&degradation, signal_id.clone(), reason);
    })
    .await;
}

pub(super) async fn subscribe_observed_projection_signal<T, F>(
    app_core: InitializedAppCore,
    signal: &'static aura_core::effects::reactive::Signal<T>,
    on_value: F,
) where
    T: Clone + Send + Sync + 'static,
    F: FnMut(T) + Send + 'static,
{
    subscribe_signal_with_retry(app_core, signal, on_value).await;
}

pub(super) async fn subscribe_update_bridge_signal<T, F>(
    app_core: InitializedAppCore,
    signal: &'static aura_core::effects::reactive::Signal<T>,
    on_value: F,
    degradation: StructuralDegradationSink,
) where
    T: Clone + Send + Sync + 'static,
    F: FnMut(T) + Send + 'static,
{
    subscribe_with_structural_degradation(app_core, signal, on_value, degradation).await;
}

pub(super) async fn subscribe_lifecycle_signal<T, F>(
    app_core: InitializedAppCore,
    signal: &'static aura_core::effects::reactive::Signal<T>,
    on_value: F,
    degradation: StructuralDegradationSink,
) where
    T: Clone + Send + Sync + 'static,
    F: FnMut(T) + Send + 'static,
{
    subscribe_with_structural_degradation(app_core, signal, on_value, degradation).await;
}
