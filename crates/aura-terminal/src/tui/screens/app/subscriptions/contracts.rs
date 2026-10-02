use super::*;

#[derive(Clone)]
pub(super) struct StructuralDegradationSink {
    tasks: Arc<UiTaskOwner>,
    update_tx: Option<UiUpdateSender>,
    health_gate: Arc<crate::tui::updates::OrderedUiUpdateGate>,
}

pub(super) fn report_subscription_health(
    sink: &StructuralDegradationSink,
    signal_id: String,
    health: crate::tui::hooks::SubscriptionHealth,
) {
    let Some(tx) = sink.update_tx.as_ref() else {
        return;
    };
    let update = crate::tui::hooks::subscription_health_update(signal_id, health);
    crate::tui::updates::spawn_ordered_ui_updates(&sink.tasks, tx, &sink.health_gate, vec![update]);
}

impl StructuralDegradationSink {
    pub(super) fn new(tasks: Arc<UiTaskOwner>, update_tx: Option<UiUpdateSender>) -> Self {
        Self {
            tasks,
            update_tx,
            health_gate: Arc::new(crate::tui::updates::OrderedUiUpdateGate::new()),
        }
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
    let signal_id = format!("shell/{}", signal.id());
    subscribe_signal_with_retry_report(app_core, signal, on_value, move |health| {
        report_subscription_health(&degradation, signal_id.clone(), health);
    })
    .await;
}

pub(super) async fn subscribe_observed_projection_signal<T, F>(
    app_ctx: AppCoreContext,
    signal: &'static aura_core::effects::reactive::Signal<T>,
    on_value: F,
) where
    T: Clone + Send + Sync + 'static,
    F: FnMut(T) + Send + 'static,
{
    subscribe_signal_with_retry(app_ctx, signal, on_value).await;
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
