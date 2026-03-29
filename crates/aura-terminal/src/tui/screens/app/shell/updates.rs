use super::*;
use std::collections::HashSet;

use crate::tui::screens::app::subscriptions::{SharedChannels, SharedContacts, SharedMessages};

use super::update_handlers::process_ui_update_match;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UiUpdateLoopAction {
    ContinueLoop,
    Handled,
}

pub(super) struct UiUpdateContext {
    pub show_setup: bool,
    pub nickname_suggestion_state: State<String>,
    pub should_exit: State<bool>,
    pub app_ctx: AppCoreContext,
    pub bootstrap_handoff_tx: Option<Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>>,
    pub bg_shutdown: Ref<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub tui: TuiStateHandle,
    pub tasks_for_updates: Arc<crate::tui::tasks::UiTaskOwner>,
    pub shared_contacts_for_updates: SharedContacts,
    pub shared_channels_for_updates: SharedChannels,
    pub shared_devices_for_updates: SharedDevices,
    pub shared_messages_for_updates: SharedMessages,
    pub tui_selected_for_updates: SharedCommittedChannelSelection,
    pub ready_join_channel_instances_for_updates: Arc<Mutex<HashSet<String>>>,
}

pub(super) async fn process_ui_update(
    update: UiUpdate,
    ctx: &mut UiUpdateContext,
) -> UiUpdateLoopAction {
    process_ui_update_match(update, ctx).await
}
