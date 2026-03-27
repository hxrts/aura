use super::*;
use crate::semantic_lifecycle::{
    UiLocalOperationOwner, UiOperationTransferScope, UiWorkflowHandoffOwner,
};
use aura_app::ui::workflows::slash_commands::{
    self as slash_command_workflows, SlashCommandTerminalSettlement, SlashCommandToastKind,
};
use aura_app::ui::workflows::strong_command::CommandResolver;
use aura_app::ui_contract::{
    OperationId, SemanticFailureCode, SemanticFailureDomain, SemanticOperationError,
    SemanticOperationKind,
};

fn command_failure(detail: impl Into<String>) -> SemanticOperationError {
    SemanticOperationError::new(
        SemanticFailureDomain::Command,
        SemanticFailureCode::InternalError,
    )
    .with_detail(detail.into())
}
pub(crate) fn selected_home_id_for_modal(
    runtime: &NeighborhoodRuntimeView,
    model: &UiModel,
) -> Option<String> {
    model
        .selected_home_id()
        .map(ToString::to_string)
        .filter(|id| !id.is_empty())
        .or_else(|| {
            runtime
                .homes
                .iter()
                .find(|home| Some(home.name.as_str()) == model.selected_home_name())
                .map(|home| home.id.clone())
                .filter(|id| !id.is_empty())
        })
        .or_else(|| (!runtime.active_home_id.is_empty()).then(|| runtime.active_home_id.clone()))
}

pub(crate) fn submit_runtime_chat_input(
    controller: Arc<UiController>,
    channel_name: String,
    input_text: String,
    rerender: Arc<dyn Fn() + Send + Sync>,
) -> bool {
    let trimmed = input_text.trim().to_string();
    if trimmed.is_empty() {
        return false;
    }

    if !trimmed.starts_with('/') {
        let app_core = controller.app_core().clone();
        let controller_for_task = controller.clone();
        let operation = UiWorkflowHandoffOwner::submit(
            controller.clone(),
            OperationId::send_message(),
            SemanticOperationKind::SendChatMessage,
        );
        let transfer = operation.handoff_to_app_workflow(UiOperationTransferScope::SendChatMessage);
        let content = trimmed.clone();

        controller.clear_input_buffer();
        spawn_ui(async move {
            let result = transfer
                .run_workflow(
                    controller_for_task.clone(),
                    "submit_runtime_chat_input send_chat_message",
                    messaging_workflows::handoff::send_chat_message(
                        &app_core,
                        messaging_workflows::handoff::SendChatMessageRequest {
                            target: messaging_workflows::handoff::SendChatTarget::ChannelName(
                                channel_name.clone(),
                            ),
                            content: content.clone(),
                            operation_instance_id: None,
                        },
                    ),
                )
                .await;

            match result {
                Ok(_) => controller_for_task.push_runtime_fact(RuntimeFact::MessageCommitted {
                    channel: ChannelFactKey::named(channel_name.clone()),
                    content: content.clone(),
                }),
                Err(error) => {
                    controller_for_task.push_log(&format!("chat_command: error {error}"));
                    controller_for_task.runtime_error_toast(error.to_string());
                }
            }
            rerender();
        });
        return true;
    }

    let app_core = controller.app_core().clone();
    let controller_for_task = controller.clone();
    spawn_ui(async move {
        let raw = trimmed.clone();
        let actor = {
            let core = app_core.read().await;
            core.runtime()
                .map(|runtime| runtime.authority_id())
                .or_else(|| core.authority().copied())
        };
        let resolver = CommandResolver::default();
        let report = slash_command_workflows::prepare_and_execute(
            &resolver,
            &app_core,
            &raw,
            Some(&channel_name),
            actor,
        )
        .await;
        if let Some(semantic) = report
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.semantic_operation.clone())
        {
            let owner = crate::semantic_lifecycle::UiLocalOperationOwner::submit(
                controller_for_task.clone(),
                semantic.operation_id,
                semantic.kind,
            );
            match report.feedback.terminal_settlement.clone() {
                Some(SlashCommandTerminalSettlement::Succeeded) => owner.succeed(None),
                Some(SlashCommandTerminalSettlement::Failed(error)) => owner.fail_with(error),
                None => {}
            }
        }
        let feedback = report.feedback;

        controller_for_task.clear_input_buffer();
        controller_for_task.push_log(&format!("chat_command: {}", feedback.message));
        match feedback.toast_kind {
            SlashCommandToastKind::Success => controller_for_task.info_toast(feedback.message),
            SlashCommandToastKind::Info => controller_for_task.info_toast(feedback.message),
            SlashCommandToastKind::Error => {
                controller_for_task.runtime_error_toast(feedback.message)
            }
        }
        rerender();
    });

    true
}

pub(crate) fn handle_runtime_character_shortcut(
    controller: Arc<UiController>,
    model: &UiModel,
    neighborhood_runtime: &NeighborhoodRuntimeView,
    key: &str,
    rerender: Arc<dyn Fn() + Send + Sync>,
) -> bool {
    if model.input_mode || model.modal_state().is_some() {
        return false;
    }

    match (model.screen, key) {
        (ScreenId::Neighborhood, "m") => {
            let app_core = controller.app_core().clone();
            let operation = UiLocalOperationOwner::submit(
                controller.clone(),
                OperationId::create_neighborhood(),
                SemanticOperationKind::CreateNeighborhood,
            );
            spawn_ui(async move {
                match context_workflows::create_neighborhood(&app_core, "Neighborhood".to_string())
                    .await
                {
                    Ok(_) => {
                        operation.succeed(None);
                        controller.info_toast("Neighborhood ready");
                    }
                    Err(error) => {
                        operation.fail_with(command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                rerender();
            });
            true
        }
        (ScreenId::Neighborhood, "v") => {
            let Some(home_id) = selected_home_id_for_modal(neighborhood_runtime, model) else {
                controller.runtime_error_toast("Select a home first");
                rerender();
                return true;
            };
            let app_core = controller.app_core().clone();
            let operation = UiLocalOperationOwner::submit(
                controller.clone(),
                OperationId::add_home_to_neighborhood(),
                SemanticOperationKind::AddHomeToNeighborhood,
            );
            spawn_ui(async move {
                match context_workflows::add_home_to_neighborhood(&app_core, &home_id).await {
                    Ok(_) => {
                        operation.succeed(None);
                        controller.info_toast("Home added to neighborhood");
                    }
                    Err(error) => {
                        operation.fail_with(command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                rerender();
            });
            true
        }
        (ScreenId::Neighborhood, "L") => {
            let Some(home_id) = selected_home_id_for_modal(neighborhood_runtime, model) else {
                controller.runtime_error_toast("Select a home first");
                rerender();
                return true;
            };
            let app_core = controller.app_core().clone();
            let operation = UiLocalOperationOwner::submit(
                controller.clone(),
                OperationId::link_home_one_hop_link(),
                SemanticOperationKind::LinkHomeOneHopLink,
            );
            spawn_ui(async move {
                match context_workflows::link_home_one_hop_link(&app_core, &home_id).await {
                    Ok(_) => {
                        operation.succeed(None);
                        controller.info_toast("Direct one-hop link created");
                    }
                    Err(error) => {
                        operation.fail_with(command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                rerender();
            });
            true
        }
        _ => false,
    }
}
