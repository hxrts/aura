use super::super::shell::submit_runtime_chat_input;
use super::*;
use crate::semantic_lifecycle::{
    UiLocalOperationOwner, UiOperationTransferScope, UiWorkflowHandoffOwner,
};
use aura_app::frontend_primitives::SubmittedOperationWorkflowError;
use aura_app::ui::workflows::messaging::handoff::{RetryChatMessageRequest, SendChatTarget};
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

#[allow(non_snake_case)]
pub(super) fn ChatScreen(
    model: &UiModel,
    runtime: &ChatRuntimeView,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
) -> Element {
    let active_channel = model
        .selected_channel_name()
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            (!runtime.active_channel.trim().is_empty()).then(|| runtime.active_channel.clone())
        })
        .unwrap_or_else(|| NOTE_TO_SELF_CHANNEL_NAME.to_string());
    let topic = runtime
        .channels
        .iter()
        .find(|channel| channel.name.eq_ignore_ascii_case(&active_channel))
        .map(|channel| channel.topic.clone())
        .unwrap_or_else(|| model.selected_channel_topic().to_string());
    let is_input_mode = model.input_mode;
    let composer_text = model.input_buffer.clone();
    let new_group_controller = controller.clone();
    let composer_container_focus_controller = controller.clone();
    let composer_field_focus_controller = controller.clone();
    let composer_input_controller = controller.clone();
    let composer_keydown_controller = controller.clone();
    let send_message_controller = controller.clone();
    let retry_message_controller = controller.clone();
    let close_channel_controller = controller.clone();
    let exit_insert_mode_controller = controller.clone();
    let composer_value = composer_text.clone();
    let composer_active_channel = active_channel.clone();
    let composer_submit_text = composer_text.clone();
    let retryable_message = runtime
        .messages
        .iter()
        .rev()
        .find(|message| message.is_own && message.can_retry)
        .cloned();
    let can_close_channel = !NOTE_TO_SELF_CHANNEL_NAME.eq_ignore_ascii_case(&active_channel);
    let runtime_channels = if runtime.loaded {
        runtime.channels.clone()
    } else {
        model
            .channels
            .iter()
            .map(|channel| ChatRuntimeChannel {
                id: channel.name.clone(),
                name: channel.name.clone(),
                topic: channel.topic.clone(),
                unread_count: 0,
                last_message: None,
                member_count: 0,
                is_dm: false,
            })
            .collect()
    };

    rsx! {
        div {
            class: "grid w-full gap-3 lg:grid-cols-12 lg:h-full lg:min-h-0 lg:[grid-template-rows:minmax(0,1fr)]",
            onclick: move |_| {
                if is_input_mode {
                    exit_insert_mode_controller.exit_input_mode();
                    render_tick.set(render_tick() + 1);
                }
            },
            UiCard {
                title: "Channels".to_string(),
                subtitle: Some("E2EE and forward secure".to_string()),
                extra_class: Some("lg:col-span-4".to_string()),
                UiCardBody {
                    extra_class: Some("gap-2".to_string()),
                    ScrollArea {
                        class: Some("flex-1 lg:min-h-0 pr-1".to_string()),
                        ScrollAreaViewport {
                            class: Some("flex flex-col gap-2".to_string()),
                            for channel in &runtime_channels {
                                UiListButton {
                                    id: Some(list_item_dom_id(ListId::Channels, &channel.id)),
                                    label: channel.name.clone(),
                                    active: channel.name.eq_ignore_ascii_case(&active_channel),
                                    extra_class: Some("pt-px pb-0".to_string()),
                                    onclick: {
                                        let controller = controller.clone();
                                        let channel_id = channel.id.clone();
                                        move |_| {
                                            controller.select_channel_by_id(&channel_id);
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    UiCardFooter {
                        extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                                if can_close_channel {
                                    UiButton {
                                        id: Some(
                                            ControlId::ChatEditChannelButton
                                                .web_dom_id()
                                                .required_dom_id("ControlId::ChatEditChannelButton must define a web DOM id")
                                                .to_string(),
                                        ),
                                        label: "Edit".to_string(),
                                        variant: ButtonVariant::Secondary,
                                        onclick: {
                                            let edit_controller = controller.clone();
                                            move |_| {
                                                edit_controller.send_action_keys("e");
                                                render_tick.set(render_tick() + 1);
                                            }
                                        }
                                    }
                                    UiButton {
                                        id: Some(
                                            ControlId::ChatCloseChannelButton
                                                .web_dom_id()
                                                .required_dom_id("ControlId::ChatCloseChannelButton must define a web DOM id")
                                                .to_string(),
                                        ),
                                        label: "Close Channel".to_string(),
                                        variant: ButtonVariant::Secondary,
                                        onclick: {
                                            let channel_name = active_channel.clone();
                                            move |_| {
                                                let controller_for_click = close_channel_controller.clone();
                                                let channel_name_for_click = channel_name.clone();
                                                let app_core = controller_for_click.app_core().clone();
                                                let operation = UiLocalOperationOwner::submit(
                                                    controller_for_click.clone(),
                                                    OperationId::close_channel(),
                                                    SemanticOperationKind::CloseChannel,
                                                );
                                                spawn_ui(async move {
                                                    let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
                                                        Ok(value) => value,
                                                        Err(error) => {
                                                            operation.fail_with(command_failure(error.to_string()));
                                                            controller_for_click.runtime_error_toast(error.to_string());
                                                            return;
                                                        }
                                                    };
                                                    match messaging_workflows::close_channel_by_name(
                                                        &app_core,
                                                        &channel_name_for_click,
                                                        timestamp_ms,
                                                    ).await {
                                                        Ok(()) => {
                                                            operation.succeed(None);
                                                            controller_for_click.info_toast("Channel closed");
                                                        }
                                                        Err(error) => {
                                                            operation.fail_with(command_failure(error.to_string()));
                                                            controller_for_click.runtime_error_toast(error.to_string());
                                                        }
                                                    }
                                                });
                                                render_tick.set(render_tick() + 1);
                                            }
                                        }
                                    }
                                }
                                UiButton {
                                    id: Some(
                                        ControlId::ChatNewGroupButton
                                        .web_dom_id()
                                        .required_dom_id("ControlId::ChatNewGroupButton must define a web DOM id")
                                        .to_string(),
                                ),
                                label: "New Group".to_string(),
                                variant: ButtonVariant::Primary,
                                onclick: move |_| {
                                    new_group_controller.send_action_keys("n");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                    }
                }
            }

            div {
                class: "lg:col-span-8 h-full min-h-0",
                onclick: move |event| event.stop_propagation(),
                UiCard {
                    title: active_channel.clone(),
                    subtitle: Some(if topic.is_empty() { "No topic set".to_string() } else { topic }),
                    extra_class: None,
                    UiCardBody {
                        extra_class: Some("!-mt-6".to_string()),
                        div {
                            class: "flex-1 lg:min-h-0 overflow-y-auto pr-1",
                            div {
                                class: "flex min-h-full flex-col justify-end gap-3",
                                if runtime.messages.is_empty() {
                                    Empty {
                                        class: Some("h-full flex-1 border-0 bg-background/40".to_string()),
                                        EmptyHeader {
                                            EmptyTitle { "No messages yet" }
                                            EmptyDescription { "Send one from input mode." }
                                        }
                                    }
                                } else {
                                    for message in &runtime.messages {
                                        {render_chat_message_bubble(message.clone())}
                                    }
                                }
                            }
                        }
                        UiCardFooter {
                            extra_class: Some("!px-3".to_string()),
                            div {
                                class: "grid h-full w-full grid-cols-[minmax(0,1fr)_auto] items-stretch gap-2",
                                div {
                                    class: "flex h-full min-w-0 items-center rounded-sm bg-background/80 px-3",
                                    onclick: move |_| {
                                        if !is_input_mode {
                                            composer_container_focus_controller.send_action_keys("i");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    },
                                    textarea {
                                        id: FieldId::ChatInput
                                            .web_dom_id()
                                            .required_dom_id("FieldId::ChatInput must define a web DOM id"),
                                        class: "h-full w-full resize-none overflow-hidden border-0 bg-transparent py-2 text-sm text-foreground outline-none placeholder:text-muted-foreground",
                                        value: "{composer_value}",
                                        readonly: !is_input_mode,
                                        placeholder: if is_input_mode {
                                            "Type a message and press Enter to send"
                                        } else {
                                            "Click here or press 𝒊 to start typing"
                                        },
                                        onfocus: move |_| {
                                            if !is_input_mode {
                                                composer_field_focus_controller.send_action_keys("i");
                                                render_tick.set(render_tick() + 1);
                                            }
                                        },
                                        oninput: move |event| {
                                            composer_input_controller.set_input_buffer(event.value());
                                        },
                                        onkeydown: move |event| {
                                            event.stop_propagation();
                                            if matches!(event.data().key(), Key::Enter)
                                                && !event.data().modifiers().contains(Modifiers::SHIFT)
                                            {
                                                event.prevent_default();
                                                let _ = submit_runtime_chat_input(
                                                    composer_keydown_controller.clone(),
                                                    composer_active_channel.clone(),
                                                    composer_submit_text.clone(),
                                                    schedule_update(),
                                                );
                                                render_tick.set(render_tick() + 1);
                                                return;
                                            }
                                            if matches!(event.data().key(), Key::Escape) {
                                                event.prevent_default();
                                                composer_keydown_controller.send_key_named("esc", 1);
                                                render_tick.set(render_tick() + 1);
                                            }
                                        },
                                    }
                                }
                                div {
                                    class: "flex h-full min-w-[4.5rem] flex-col items-end justify-end gap-1",
                                    if let Some(retryable_message) = retryable_message {
                                        UiButton {
                                            id: Some(
                                                ControlId::ChatRetryMessageButton
                                                    .web_dom_id()
                                                    .required_dom_id("ControlId::ChatRetryMessageButton must define a web DOM id")
                                                    .to_string()
                                            ),
                                            label: "Retry".to_string(),
                                            variant: ButtonVariant::Secondary,
                                            onclick: {
                                                let active_channel = active_channel.clone();
                                                move |_| {
                                                    let controller_for_click = retry_message_controller.clone();
                                                    let active_channel_for_click = active_channel.clone();
                                                    let retry_message_for_click = retryable_message.clone();
                                                    let app_core = controller_for_click.app_core().clone();
                                                    let operation = UiWorkflowHandoffOwner::submit(
                                                        controller_for_click.clone(),
                                                        OperationId::retry_message(),
                                                        SemanticOperationKind::RetryChatMessage,
                                                    );
                                                    let workflow_instance_id = operation.workflow_instance_id();
                                                    let transfer = operation.handoff_to_app_workflow(
                                                        UiOperationTransferScope::SendChatMessage,
                                                    );
                                                    spawn_ui(async move {
                                                        let target = retry_message_for_click
                                                            .channel_id
                                                            .parse::<aura_core::ChannelId>()
                                                            .map(SendChatTarget::ChannelId)
                                                            .unwrap_or_else(|_| SendChatTarget::ChannelName(active_channel_for_click));
                                                        match transfer
                                                            .run_workflow(
                                                                controller_for_click.clone(),
                                                                "retry_chat_message",
                                                                messaging_workflows::handoff::retry_chat_message(
                                                                    &app_core,
                                                                    RetryChatMessageRequest {
                                                                        target,
                                                                        content: retry_message_for_click.content.clone(),
                                                                        operation_instance_id: workflow_instance_id,
                                                                    },
                                                                ),
                                                            )
                                                            .await
                                                        {
                                                            Ok(_) => controller_for_click.info_toast("Message retry queued"),
                                                            Err(SubmittedOperationWorkflowError::Workflow(error)) => {
                                                                controller_for_click.runtime_error_toast(error.to_string());
                                                            }
                                                            Err(
                                                                SubmittedOperationWorkflowError::Protocol(detail)
                                                                | SubmittedOperationWorkflowError::Panicked(detail),
                                                            ) => {
                                                                controller_for_click.runtime_error_toast(detail);
                                                            }
                                                        }
                                                    });
                                                    render_tick.set(render_tick() + 1);
                                                }
                                            }
                                        }
                                    }
                                    UiButton {
                                        id: Some(
                                            ControlId::ChatSendMessageButton
                                                .web_dom_id()
                                                .required_dom_id("ControlId::ChatSendMessageButton must define a web DOM id")
                                                .to_string()
                                        ),
                                        label: "Send".to_string(),
                                        variant: if is_input_mode { ButtonVariant::Primary } else { ButtonVariant::Secondary },
                                        onclick: move |_| {
                                            if is_input_mode {
                                                let _ = submit_runtime_chat_input(
                                                    send_message_controller.clone(),
                                                    active_channel.clone(),
                                                    composer_text.clone(),
                                                    schedule_update(),
                                                );
                                            } else {
                                                send_message_controller.send_action_keys("i");
                                            }
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn render_chat_message_bubble(message: ChatRuntimeMessage) -> Element {
    rsx! {
        div {
            class: if message.is_own {
                "ml-auto flex w-full justify-end"
            } else {
                "mr-auto flex w-full justify-start"
            },
            div {
                class: if message.is_own {
                    "flex max-w-[78%] flex-col items-end gap-1"
                } else {
                    "flex max-w-[78%] flex-col items-start gap-1"
                },
                span {
                    class: "text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground",
                    if message.is_own { "You" } else { {message.sender_name.clone()} }
                }
                div {
                    class: if message.is_own {
                        "rounded-[1.75rem] bg-primary px-4 py-1.5 text-sm text-primary-foreground shadow-sm"
                    } else {
                        "rounded-[1.75rem] border border-border bg-muted px-4 py-1.5 text-sm text-foreground shadow-sm"
                    },
                    p {
                        class: "m-0 whitespace-pre-wrap break-words leading-relaxed",
                        {message.content.clone()}
                    }
                }
                if message.is_own {
                    span {
                        class: "text-[0.68rem] text-muted-foreground",
                        {message.delivery_status.clone()}
                    }
                }
            }
        }
    }
}
