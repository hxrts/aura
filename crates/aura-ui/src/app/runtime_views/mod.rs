mod chat;
mod contacts;
mod neighborhood;
mod notifications;
mod settings;

fn observed_snapshot_or_report<T>(
    result: Result<T, aura_core::effects::reactive::ReactiveError>,
    controller: &crate::model::UiController,
    signal: &aura_core::effects::reactive::SignalId,
) -> Option<T> {
    match result {
        Ok(snapshot) => Some(snapshot),
        Err(error) => {
            controller.set_subscription_health(
                &signal.to_string(),
                aura_app::ui_contract::SubscriptionHealthState::Degraded {
                    reason: aura_app::ui_contract::SubscriptionFailureCode::SnapshotReadFailed,
                },
            );
            controller.push_log(&format!(
                "canonical snapshot read failed for {signal}: {error}"
            ));
            None
        }
    }
}

pub(super) use chat::{
    load_chat_runtime_view, ChatRuntimeChannel, ChatRuntimeMessage, ChatRuntimeView,
};
pub(super) use contacts::{
    load_contacts_runtime_view, ContactsRuntimeContact, ContactsRuntimeView,
};
pub(super) use neighborhood::{
    load_neighborhood_runtime_view, NeighborhoodRuntimeHome, NeighborhoodRuntimeMember,
    NeighborhoodRuntimeView,
};
pub(super) use notifications::{
    load_notifications_runtime_view, NotificationRuntimeAction, NotificationsRuntimeView,
};
#[cfg(test)]
pub(super) use settings::SettingsRuntimeAuthority;
pub(super) use settings::{load_settings_runtime_view, SettingsRuntimeDevice, SettingsRuntimeView};

#[cfg(test)]
mod observation_failure_tests {
    #[test]
    fn failed_snapshot_is_degraded_without_empty_projection_observation() {
        let controller = crate::model::UiController::new(
            std::sync::Arc::new(async_lock::RwLock::new(
                aura_app::AppCore::new(aura_app::AppConfig::default())
                    .unwrap_or_else(|error| panic!("{error}")),
            )),
            std::sync::Arc::new(crate::MemoryClipboard::default()),
        );
        let signal = aura_core::effects::reactive::SignalId::from("missing-chat");
        let snapshot: Option<aura_app::ui::types::ChatState> = super::observed_snapshot_or_report(
            Err(
                aura_core::effects::reactive::ReactiveError::SignalNotFound {
                    id: signal.to_string(),
                },
            ),
            &controller,
            &signal,
        );
        assert!(snapshot.is_none());
        let exported = controller.ui_snapshot();
        assert!(exported.runtime_events.is_empty());
        assert!(matches!(
            exported.subscription_health[0].state,
            aura_app::ui_contract::SubscriptionHealthState::Degraded {
                reason: aura_app::ui_contract::SubscriptionFailureCode::SnapshotReadFailed
            }
        ));
    }
}
