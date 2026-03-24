use super::*;
use std::collections::HashSet;

use aura_app::ui_contract::SemanticOperationKind;

use crate::tui::components::copy_to_clipboard;
use crate::tui::screens::app::subscriptions::{SharedChannels, SharedContacts, SharedMessages};
use crate::tui::screens::app::shell::dispatch::format_ui_operation_failure;

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
    pub selected_channel_binding_for_updates:
        std::sync::Arc<parking_lot::RwLock<Option<ChannelBindingWitness>>>,
    pub ready_join_channel_instances_for_updates: Arc<Mutex<HashSet<String>>>,
}

pub(super) async fn process_ui_update(
    update: UiUpdate,
    ctx: &mut UiUpdateContext,
) -> UiUpdateLoopAction {
    let show_setup = ctx.show_setup;
    let nickname_suggestion_state = &mut ctx.nickname_suggestion_state;
    let should_exit = &mut ctx.should_exit;
    let app_core = ctx.app_ctx.app_core.clone();
    let app_ctx_for_updates = ctx.app_ctx.clone();
    let bootstrap_handoff_tx = &ctx.bootstrap_handoff_tx;
    let io_ctx = ctx.app_ctx.io_context();
    let bg_shutdown = &ctx.bg_shutdown;
    let tui = &mut ctx.tui;
    let tasks_for_updates = ctx.tasks_for_updates.clone();
    let shared_contacts_for_updates = &ctx.shared_contacts_for_updates;
    let shared_channels_for_updates = &ctx.shared_channels_for_updates;
    let shared_devices_for_updates = &ctx.shared_devices_for_updates;
    let shared_messages_for_updates = &ctx.shared_messages_for_updates;
    let tui_selected_for_updates = &ctx.tui_selected_for_updates;
    let selected_channel_binding_for_updates = &ctx.selected_channel_binding_for_updates;
    let ready_join_channel_instances_for_updates = &ctx.ready_join_channel_instances_for_updates;

    macro_rules! enqueue_toast {
        ($msg:expr, $level:expr) => {{
            tui.with_mut(|state| {
                let toast_id = state.next_toast_id;
                state.next_toast_id += 1;
                let toast = crate::tui::state::QueuedToast::new(toast_id, $msg, $level);
                state.toast_queue.enqueue(toast);
            });
        }};
    }

    include!("updates_match.inc");
    UiUpdateLoopAction::Handled
}
