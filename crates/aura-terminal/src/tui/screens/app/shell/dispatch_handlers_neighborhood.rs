use super::dispatch::*;
use super::*;

use aura_app::ui::workflows::access as access_workflows;
use aura_app::ui_contract::SemanticOperationKind;
use crate::tui::types::AccessLevel;

pub(super) fn handle_neighborhood_dispatch(
    dispatch_cmd: DispatchCommand,
    new_state: &mut TuiState,
    event_ctx: &EventDispatchContext<'_>,
) -> EventCommandLoopAction {
    let cb = event_ctx.callbacks;
    let app_core_for_events = event_ctx.app_ctx.app_core.raw().clone();
    let app_core_for_ceremony = event_ctx.app_ctx.app_core.clone();
    let update_tx_for_events = event_ctx.update_tx_for_events.clone();
    let update_tx_for_ceremony = event_ctx.update_tx_for_ceremony.clone();
    let shared_contacts_for_dispatch = event_ctx.shared_contacts_for_dispatch;
    let shared_neighborhood_homes_for_dispatch = event_ctx.shared_neighborhood_homes_for_dispatch;
    let tasks_for_events = event_ctx.tasks_for_events.clone();

    match dispatch_cmd {
        DispatchCommand::EnterHome => {
            let idx = new_state.neighborhood.grid.current();
            {
                let guard = shared_neighborhood_homes_for_dispatch.read();
                if let Some(home_id) = guard.get(idx) {
                    // Keep entered_home_id authoritative as a real home ID.
                    // The state-machine layer sets an index sentinel first.
                    new_state.neighborhood.entered_home_id = Some(home_id.clone());
                    new_state.neighborhood.enter_depth = AccessLevel::Full;
                    (cb.neighborhood.on_enter_home)(home_id.clone(), AccessLevel::Full);
                } else {
                    new_state.toast_error("No home selected");
                }
            }
        }
        DispatchCommand::GoHome => {
            new_state.neighborhood.enter_depth = AccessLevel::Full;
            (cb.neighborhood.on_go_home)();
        }
        DispatchCommand::BackToLimited => {
            new_state.neighborhood.enter_depth = AccessLevel::Limited;
            (cb.neighborhood.on_back_to_limited)();
        }
        DispatchCommand::OpenHomeCreate => {
            new_state
                .modal_queue
                .enqueue(crate::tui::state::QueuedModal::NeighborhoodHomeCreate(
                    crate::tui::state::HomeCreateModalState::new(),
                ));
        }
        DispatchCommand::OpenModeratorAssignmentModal => {
            let contacts = shared_contacts_for_dispatch.read().clone();
            new_state.modal_queue.enqueue(
                crate::tui::state::QueuedModal::NeighborhoodModeratorAssignment(
                    crate::tui::state::ModeratorAssignmentModalState::new(contacts),
                ),
            );
        }
        DispatchCommand::SubmitModeratorAssignment { target_id, assign } => {
            let Some(update_tx) = update_tx_for_events else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = submit_local_terminal_operation(
                app_core_for_events,
                tasks_for_events,
                update_tx,
                if assign {
                    OperationId::grant_moderator()
                } else {
                    OperationId::revoke_moderator()
                },
                if assign {
                    SemanticOperationKind::GrantModerator
                } else {
                    SemanticOperationKind::RevokeModerator
                },
            );
            (cb.neighborhood.on_set_moderator)(
                new_state.neighborhood.entered_home_id.clone(),
                target_id.to_string(),
                assign,
                operation,
            );
            new_state.modal_queue.dismiss();
        }
        DispatchCommand::OpenAccessOverrideModal => {
            let contacts = shared_contacts_for_dispatch.read().clone();
            new_state.modal_queue.enqueue(
                crate::tui::state::QueuedModal::NeighborhoodAccessOverride(
                    crate::tui::state::AccessOverrideModalState::new(contacts),
                ),
            );
        }
        DispatchCommand::SubmitAccessOverride {
            target_id,
            access_level,
        } => {
            new_state.modal_queue.dismiss();
            let app_core = app_core_for_ceremony;
            let update_tx = update_tx_for_ceremony;
            let home_id = new_state.neighborhood.entered_home_id.clone();
            let target_for_toast = target_id.clone();
            let tasks = tasks_for_events;
            tasks.spawn(async move {
                match access_workflows::set_access_override(
                    app_core.raw(),
                    home_id.as_deref(),
                    target_id,
                    access_level.into(),
                )
                .await
                {
                    Ok(()) => {
                        send_optional_ui_update_required(
                            &update_tx,
                            UiUpdate::ToastAdded(ToastMessage::success(
                                "access-override",
                                format!(
                                    "Access override set for {}: {}",
                                    target_for_toast,
                                    access_level.label()
                                ),
                            )),
                        )
                        .await;
                    }
                    Err(error) => {
                        send_optional_ui_update_required(
                            &update_tx,
                            UiUpdate::ToastAdded(ToastMessage::error(
                                "access-override",
                                format!("Failed to set access override: {error}"),
                            )),
                        )
                        .await;
                    }
                }
            });
        }
        DispatchCommand::OpenHomeCapabilityConfigModal => {
            new_state.modal_queue.enqueue(
                crate::tui::state::QueuedModal::NeighborhoodCapabilityConfig(
                    crate::tui::state::HomeCapabilityConfigModalState::default(),
                ),
            );
        }
        DispatchCommand::SubmitHomeCapabilityConfig { config } => {
            new_state.modal_queue.dismiss();
            let app_core = app_core_for_ceremony;
            let update_tx = update_tx_for_ceremony;
            let home_id = new_state.neighborhood.entered_home_id.clone();
            let tasks = tasks_for_events;
            tasks.spawn(async move {
                match access_workflows::configure_home_capabilities(
                    app_core.raw(),
                    home_id.as_deref(),
                    &config.full_csv(),
                    &config.partial_csv(),
                    &config.limited_csv(),
                )
                .await
                {
                    Ok(()) => {
                        send_optional_ui_update_required(
                            &update_tx,
                            UiUpdate::ToastAdded(ToastMessage::success(
                                "capability-config",
                                "Capability config saved",
                            )),
                        )
                        .await;
                    }
                    Err(error) => {
                        send_optional_ui_update_required(
                            &update_tx,
                            UiUpdate::ToastAdded(ToastMessage::error(
                                "capability-config",
                                format!("Failed to save capability config: {error}"),
                            )),
                        )
                        .await;
                    }
                }
            });
        }
        DispatchCommand::CreateHome { name, description } => {
            let Some(update_tx) = update_tx_for_events else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = submit_local_terminal_operation(
                app_core_for_events,
                tasks_for_events,
                update_tx,
                OperationId::create_home(),
                SemanticOperationKind::CreateHome,
            );
            (cb.neighborhood.on_create_home)(name, description, operation);
            new_state.modal_queue.dismiss();
        }
        DispatchCommand::CreateNeighborhood { name } => {
            let Some(update_tx) = update_tx_for_events else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = submit_local_terminal_operation(
                app_core_for_events,
                tasks_for_events,
                update_tx,
                OperationId::create_neighborhood(),
                SemanticOperationKind::CreateNeighborhood,
            );
            (cb.neighborhood.on_create_neighborhood)(name, operation);
        }
        DispatchCommand::AddSelectedHomeToNeighborhood => {
            let idx = new_state.neighborhood.grid.current();
            {
                let guard = shared_neighborhood_homes_for_dispatch.read();
                if let Some(home_id) = guard.get(idx) {
                    let Some(update_tx) = update_tx_for_events else {
                        new_state.toast_error("UI update sender is unavailable");
                        return EventCommandLoopAction::ContinueCommand;
                    };
                    let operation = submit_local_terminal_operation(
                        app_core_for_events,
                        tasks_for_events,
                        update_tx,
                        OperationId::add_home_to_neighborhood(),
                        SemanticOperationKind::AddHomeToNeighborhood,
                    );
                    (cb.neighborhood.on_add_home_to_neighborhood)(home_id.clone(), operation);
                } else {
                    new_state.toast_error("No home selected");
                }
            }
        }
        DispatchCommand::AddHomeToNeighborhood { target } => {
            let Some(update_tx) = update_tx_for_events else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = submit_local_terminal_operation(
                app_core_for_events,
                tasks_for_events,
                update_tx,
                OperationId::add_home_to_neighborhood(),
                SemanticOperationKind::AddHomeToNeighborhood,
            );
            (cb.neighborhood.on_add_home_to_neighborhood)(target.as_command_arg(), operation);
        }
        DispatchCommand::LinkSelectedHomeOneHopLink => {
            let idx = new_state.neighborhood.grid.current();
            {
                let guard = shared_neighborhood_homes_for_dispatch.read();
                if let Some(home_id) = guard.get(idx) {
                    let Some(update_tx) = update_tx_for_events else {
                        new_state.toast_error("UI update sender is unavailable");
                        return EventCommandLoopAction::ContinueCommand;
                    };
                    let operation = submit_local_terminal_operation(
                        app_core_for_events,
                        tasks_for_events,
                        update_tx,
                        OperationId::link_home_one_hop_link(),
                        SemanticOperationKind::LinkHomeOneHopLink,
                    );
                    (cb.neighborhood.on_link_home_one_hop_link)(home_id.clone(), operation);
                } else {
                    new_state.toast_error("No home selected");
                }
            }
        }
        DispatchCommand::LinkHomeOneHopLink { target } => {
            let Some(update_tx) = update_tx_for_events else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = submit_local_terminal_operation(
                app_core_for_events,
                tasks_for_events,
                update_tx,
                OperationId::link_home_one_hop_link(),
                SemanticOperationKind::LinkHomeOneHopLink,
            );
            (cb.neighborhood.on_link_home_one_hop_link)(target.as_command_arg(), operation);
        }
        _ => {
            debug_assert!(
                false,
                "non-neighborhood command routed to neighborhood dispatcher"
            );
        }
    }

    EventCommandLoopAction::Handled
}
