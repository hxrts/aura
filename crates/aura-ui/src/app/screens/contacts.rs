use super::*;
use crate::channel_selection::selected_authoritative_channel;
use crate::semantic_lifecycle::{
    UiLocalOperationOwner, UiOperationTransferScope, UiWorkflowHandoffOwner,
};
use aura_app::frontend_primitives::SubmittedOperationWorkflowError;
use aura_app::ui::contract::contacts_friend_action_controls;
use aura_app::ui::types::ContactRelationshipState;
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

fn contact_relationship_label(state: ContactRelationshipState) -> &'static str {
    match state {
        ContactRelationshipState::Contact => "Contact",
        ContactRelationshipState::PendingOutbound => "Pending outbound",
        ContactRelationshipState::PendingInbound => "Pending inbound",
        ContactRelationshipState::Friend => "Friend",
    }
}

fn contact_secondary_label(contact: &ContactsRuntimeContact) -> String {
    match contact.relationship_state {
        ContactRelationshipState::Friend => "Friend".to_string(),
        ContactRelationshipState::PendingOutbound => "Friend request sent".to_string(),
        ContactRelationshipState::PendingInbound => "Friend request received".to_string(),
        ContactRelationshipState::Contact if contact.is_guardian => "Guardian".to_string(),
        ContactRelationshipState::Contact if contact.is_member => "Member".to_string(),
        ContactRelationshipState::Contact if contact.is_online => "Online".to_string(),
        ContactRelationshipState::Contact => "\u{00A0}".to_string(),
    }
}

#[allow(non_snake_case)]
pub(super) fn ContactsScreen(
    model: &UiModel,
    runtime: &ContactsRuntimeView,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
) -> Element {
    let contacts = runtime.contacts.clone();
    let selected_contact_id = model.selected_contact_authority_id();
    let selected_contact = selected_contact_id
        .and_then(|authority_id| {
            contacts
                .iter()
                .find(|contact| contact.authority_id == authority_id)
                .cloned()
        })
        .or_else(|| contacts.first().cloned());
    let selected_name = selected_contact
        .as_ref()
        .map(|contact| contact.name.clone())
        .unwrap_or_else(|| "none".to_string());
    let invite_controller = controller.clone();
    let accept_invitation_controller = controller.clone();
    let start_chat_controller = controller.clone();
    let invite_to_channel_controller = controller.clone();
    let add_guardian_controller = controller.clone();
    let send_friend_request_controller = controller.clone();
    let remove_friend_controller = controller.clone();
    let edit_controller = controller.clone();
    let remove_controller = controller.clone();
    rsx! {
        div {
            class: "grid w-full gap-3 lg:grid-cols-12 lg:h-full lg:min-h-0 lg:[grid-template-rows:minmax(0,1fr)]",
            UiCard {
                title: format!("Contacts ({})", contacts.len()),
                subtitle: Some("People you know".to_string()),
                extra_class: Some("lg:col-span-4".to_string()),
                UiCardBody {
                    extra_class: Some("gap-3".to_string()),
                    div {
                        class: "rounded-sm bg-background/60 px-3 py-3",
                        div {
                            class: "flex items-center gap-3",
                            p { class: "m-0 text-xs font-sans font-semibold uppercase tracking-[0.08em] text-muted-foreground", "Bootstrap Candidates" }
                            p {
                                class: "m-0 text-xs text-muted-foreground",
                                "updates automatically"
                            }
                        }
                        if runtime.lan_peers.is_empty() {
                            p { class: "m-0 mt-3 text-sm text-muted-foreground", "No bootstrap candidates discovered yet." }
                        } else {
                            div { class: "mt-3 space-y-2",
                                for peer in &runtime.lan_peers {
                                    div {
                                        class: "flex items-center gap-2",
                                        div { class: "min-w-0 flex-1",
                                            UiListItem {
                                                label: peer.authority_id.to_string(),
                                                secondary: Some(if peer.invited {
                                                    format!("{} • invitation pending", peer.address)
                                                } else {
                                                    peer.address.clone()
                                                }),
                                                active: false,
                                            }
                                        }
                                        UiButton {
                                            label: if peer.invited {
                                                "Pending".to_string()
                                            } else {
                                                "Invite".to_string()
                                            },
                                            variant: if peer.invited {
                                                ButtonVariant::Secondary
                                            } else {
                                                ButtonVariant::Primary
                                            },
                                            width_class: Some("w-[6.5rem]".to_string()),
                                            onclick: {
                                                let controller = controller.clone();
                                                let app_core = controller.app_core().clone();
                                                let authority_id = peer.authority_id;
                                                move |_| {
                                                    let controller = controller.clone();
                                                    let app_core = app_core.clone();
                                                    spawn_ui(async move {
                                                        let operation = UiWorkflowHandoffOwner::submit(
                                                            controller.clone(),
                                                            OperationId::invitation_create(),
                                                            SemanticOperationKind::CreateContactInvitation,
                                                        );
                                                        let workflow_instance_id = operation.workflow_instance_id();
                                                        let transfer = operation.handoff_to_app_workflow(
                                                            UiOperationTransferScope::CreateInvitation,
                                                        );
                                                        match transfer
                                                            .run_workflow(
                                                                controller.clone(),
                                                                "create_contact_invitation_code",
                                                                invitation_workflows::handoff::create_contact_invitation(
                                                                    &app_core,
                                                                    invitation_workflows::handoff::CreateContactInvitationRequest {
                                                                        receiver: authority_id,
                                                                        nickname: None,
                                                                        message: None,
                                                                        ttl_ms: None,
                                                                        operation_instance_id: workflow_instance_id,
                                                                    },
                                                                ),
                                                            )
                                                            .await
                                                        {
                                                            Ok(code) => {
                                                                controller.write_clipboard(&code);
                                                                controller.remember_invitation_code(&code);
                                                                controller.push_runtime_fact(
                                                                    RuntimeFact::InvitationCodeReady {
                                                                        receiver_authority_id: Some(authority_id.to_string()),
                                                                        source_operation: OperationId::invitation_create(),
                                                                        code: Some(code),
                                                                    },
                                                                );
                                                                controller.info_toast(
                                                                    "Invitation code copied to clipboard",
                                                                );
                                                            }
                                                            Err(SubmittedOperationWorkflowError::Workflow(error)) => {
                                                                controller.runtime_error_toast(error.to_string());
                                                            }
                                                            Err(
                                                                SubmittedOperationWorkflowError::Protocol(detail)
                                                                | SubmittedOperationWorkflowError::Panicked(detail),
                                                            ) => {
                                                                controller.runtime_error_toast(detail);
                                                            }
                                                        }
                                                    });
                                                    render_tick.set(render_tick() + 1);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div {
                        class: "flex-1 lg:min-h-0",
                        if contacts.is_empty() {
                            Empty {
                                class: Some("h-full border-0 bg-background".to_string()),
                                EmptyHeader {
                                    EmptyTitle { "No contacts yet" }
                                    EmptyDescription { "Use the invitation flow to add contacts." }
                                }
                            }
                        } else {
                            ScrollArea {
                                class: Some("h-full pr-1".to_string()),
                                ScrollAreaViewport {
                                    class: Some("aura-list space-y-2".to_string()),
                                    for contact in contacts.iter() {
                                        button {
                                            r#type: "button",
                                            id: list_item_dom_id(
                                                ListId::Contacts,
                                                &contact.authority_id.to_string(),
                                            ),
                                            class: "block w-full text-left",
                                            onclick: {
                                                let controller = controller.clone();
                                                let authority_id = contact.authority_id;
                                                move |_| {
                                                    controller.set_selected_contact_authority_id(authority_id);
                                                    render_tick.set(render_tick() + 1);
                                                }
                                            },
                                            UiListItem {
                                                label: contact.name.clone(),
                                                secondary: Some(
                                                    contact_secondary_label(contact)
                                                ),
                                                active: selected_contact_id == Some(contact.authority_id),
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    UiCardFooter {
                        extra_class: None,
                        div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                            UiButton {
                                id: Some(
                                    ControlId::ContactsAcceptInvitationButton
                                        .web_dom_id()
                                        .required_dom_id("ControlId::ContactsAcceptInvitationButton must define a web DOM id")
                                        .to_string(),
                                ),
                                label: "Accept Invitation".to_string(),
                                variant: ButtonVariant::Primary,
                                onclick: move |_| {
                                    accept_invitation_controller.send_action_keys("a");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                            UiButton {
                                id: Some(
                                    ControlId::ContactsCreateInvitationButton
                                        .web_dom_id()
                                        .required_dom_id("ControlId::ContactsCreateInvitationButton must define a web DOM id")
                                        .to_string(),
                                ),
                                label: "Create Invitation".to_string(),
                                variant: ButtonVariant::Primary,
                                onclick: {
                                    let controller = invite_controller;
                                    let selected_contact = selected_contact.clone();
                                    move |_| {
                                        if let Some(contact) = &selected_contact {
                                            let controller = controller.clone();
                                            let app_core = controller.app_core().clone();
                                            let authority_id = contact.authority_id;
                                            spawn_ui(async move {
                                                let operation = UiWorkflowHandoffOwner::submit(
                                                    controller.clone(),
                                                    OperationId::invitation_create(),
                                                    SemanticOperationKind::CreateContactInvitation,
                                                );
                                                let workflow_instance_id = operation.workflow_instance_id();
                                                let transfer = operation.handoff_to_app_workflow(
                                                    UiOperationTransferScope::CreateInvitation,
                                                );
                                                match transfer
                                                    .run_workflow(
                                                        controller.clone(),
                                                        "create_contact_invitation_code",
                                                        invitation_workflows::handoff::create_contact_invitation(
                                                            &app_core,
                                                            invitation_workflows::handoff::CreateContactInvitationRequest {
                                                                receiver: authority_id,
                                                                nickname: None,
                                                                message: None,
                                                                ttl_ms: None,
                                                                operation_instance_id: workflow_instance_id,
                                                            },
                                                        ),
                                                    )
                                                    .await
                                                {
                                                    Ok(code) => {
                                                        controller.write_clipboard(&code);
                                                        controller.remember_invitation_code(&code);
                                                        controller.push_runtime_fact(
                                                            RuntimeFact::InvitationCodeReady {
                                                                receiver_authority_id: Some(authority_id.to_string()),
                                                                source_operation: OperationId::invitation_create(),
                                                                code: Some(code),
                                                            },
                                                        );
                                                        controller.info_toast(
                                                            "Invitation code copied to clipboard",
                                                        );
                                                    }
                                                    Err(SubmittedOperationWorkflowError::Workflow(error)) => {
                                                        controller.runtime_error_toast(error.to_string());
                                                    }
                                                    Err(
                                                        SubmittedOperationWorkflowError::Protocol(detail)
                                                        | SubmittedOperationWorkflowError::Panicked(detail),
                                                    ) => {
                                                        controller.runtime_error_toast(detail);
                                                    }
                                                }
                                            });
                                        } else {
                                            controller.open_create_invitation_modal(None, Some("New contact"));
                                        }
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            UiCard {
                title: "Details".to_string(),
                subtitle: Some(format!("Selected: {selected_name}")),
                extra_class: Some("lg:col-span-8".to_string()),
                if let Some(contact) = selected_contact {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        id: format!("aura-contact-selected-{}", dom_slug(&contact.name)),
                        UiListItem {
                            label: format!("Authority: {}", contact.authority_id),
                            secondary: Some("Relational identity".to_string()),
                            active: false,
                        }
                        UiListItem {
                            label: format!("Name: {}", contact.name),
                            secondary: contact.nickname_hint.clone().or_else(|| Some("No shared nickname suggestion".to_string())),
                            active: false,
                        }
                        UiListItem {
                            label: if contact.is_online { "Status: Online".to_string() } else { "Status: Offline".to_string() },
                            secondary: Some(if contact.is_guardian {
                                "Guardian contact".to_string()
                            } else if contact.is_member {
                                "Home member".to_string()
                            } else {
                                "Direct contact".to_string()
                            }),
                            active: false,
                        }
                        UiListItem {
                            label: format!(
                                "Relationship: {}",
                                contact_relationship_label(contact.relationship_state)
                            ),
                            secondary: Some(match contact.relationship_state {
                                ContactRelationshipState::Contact => {
                                    "Unilateral reachability".to_string()
                                }
                                ContactRelationshipState::PendingOutbound => {
                                    "Awaiting the other authority".to_string()
                                }
                                ContactRelationshipState::PendingInbound => {
                                    "Accept or decline the inbound request".to_string()
                                }
                                ContactRelationshipState::Friend => {
                                    "Bilateral accepted trust".to_string()
                                }
                            }),
                            active: false,
                        }
                        UiCardFooter {
                            extra_class: None,
                            if let Some(code) = model.last_invite_code.as_ref() {
                                div {
                                    class: "w-full rounded-md border border-border bg-background/60 px-3 py-2 text-xs",
                                    p { class: "m-0 text-[0.7rem] uppercase tracking-[0.06em] text-muted-foreground", "Last Invitation Code" }
                                    p { class: "m-0 mt-1 break-all font-mono text-foreground", "{code}" }
                                }
                            }
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                                if contacts_friend_action_controls(contact.relationship_state)
                                    .contains(&ControlId::ContactsSendFriendRequestButton) {
                                    UiButton {
                                        id: Some(
                                            ControlId::ContactsSendFriendRequestButton
                                                .web_dom_id()
                                                .required_dom_id(
                                                    "ControlId::ContactsSendFriendRequestButton must define a web DOM id",
                                                )
                                                .to_string(),
                                        ),
                                        label: "Send Friend Request".to_string(),
                                        variant: ButtonVariant::Primary,
                                        onclick: {
                                            let authority_id = contact.authority_id;
                                            move |_| {
                                                let controller = send_friend_request_controller.clone();
                                                let app_core = controller.app_core().clone();
                                                let operation = UiLocalOperationOwner::submit(
                                                    controller.clone(),
                                                    OperationId::send_friend_request(),
                                                    SemanticOperationKind::SendFriendRequest,
                                                );
                                                spawn_ui(async move {
                                                    let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
                                                        Ok(value) => value,
                                                        Err(error) => {
                                                            operation.fail_with(command_failure(error.to_string()));
                                                            controller.runtime_error_toast(error.to_string());
                                                            return;
                                                        }
                                                    };
                                                    match contacts_workflows::send_friend_request(
                                                        &app_core,
                                                        &authority_id.to_string(),
                                                        timestamp_ms,
                                                    ).await {
                                                        Ok(()) => {
                                                            operation.succeed(None);
                                                            controller.info_toast("friend request sent");
                                                        }
                                                        Err(error) => {
                                                            operation.fail_with(command_failure(error.to_string()));
                                                            controller.runtime_error_toast(error.to_string());
                                                        }
                                                    }
                                                });
                                                render_tick.set(render_tick() + 1);
                                            }
                                        }
                                    }
                                }
                                if contacts_friend_action_controls(contact.relationship_state)
                                    .contains(&ControlId::ContactsRemoveFriendButton) {
                                    UiButton {
                                        id: Some(
                                            ControlId::ContactsRemoveFriendButton
                                                .web_dom_id()
                                                .required_dom_id(
                                                    "ControlId::ContactsRemoveFriendButton must define a web DOM id",
                                                )
                                                .to_string(),
                                        ),
                                        label: if matches!(contact.relationship_state, ContactRelationshipState::PendingOutbound) {
                                            "Cancel Friend Request".to_string()
                                        } else {
                                            "Remove Friend".to_string()
                                        },
                                        variant: ButtonVariant::Secondary,
                                        onclick: {
                                            let authority_id = contact.authority_id;
                                            let success_message = if matches!(contact.relationship_state, ContactRelationshipState::PendingOutbound) {
                                                "friend request cancelled".to_string()
                                            } else {
                                                "friend removed".to_string()
                                            };
                                            move |_| {
                                                let controller = remove_friend_controller.clone();
                                                let app_core = controller.app_core().clone();
                                                let operation = UiLocalOperationOwner::submit(
                                                    controller.clone(),
                                                    OperationId::revoke_friendship(),
                                                    SemanticOperationKind::RevokeFriendship,
                                                );
                                                let success_message = success_message.clone();
                                                spawn_ui(async move {
                                                    let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
                                                        Ok(value) => value,
                                                        Err(error) => {
                                                            operation.fail_with(command_failure(error.to_string()));
                                                            controller.runtime_error_toast(error.to_string());
                                                            return;
                                                        }
                                                    };
                                                    match contacts_workflows::revoke_friendship(
                                                        &app_core,
                                                        &authority_id.to_string(),
                                                        timestamp_ms,
                                                    ).await {
                                                        Ok(()) => {
                                                            operation.succeed(None);
                                                            controller.info_toast(success_message);
                                                        }
                                                        Err(error) => {
                                                            operation.fail_with(command_failure(error.to_string()));
                                                            controller.runtime_error_toast(error.to_string());
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
                                        ControlId::ContactsStartChatButton
                                            .web_dom_id()
                                            .required_dom_id("ControlId::ContactsStartChatButton must define a web DOM id")
                                            .to_string(),
                                    ),
                                    label: "Start Chat".to_string(),
                                    variant: ButtonVariant::Primary,
                                    onclick: {
                                        let authority_id = contact.authority_id;
                                        move |_| {
                                            let controller = start_chat_controller.clone();
                                            let app_core = controller.app_core().clone();
                                            let operation = UiLocalOperationOwner::submit(
                                                controller.clone(),
                                                OperationId::start_direct_chat(),
                                                SemanticOperationKind::StartDirectChat,
                                            );
                                            spawn_ui(async move {
                                                let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
                                                    Ok(value) => value,
                                                    Err(error) => {
                                                        operation.fail_with(command_failure(error.to_string()));
                                                        controller.runtime_error_toast(error.to_string());
                                                        return;
                                                    }
                                                };
                                                match messaging_workflows::start_direct_chat(
                                                    &app_core,
                                                    &authority_id.to_string(),
                                                    timestamp_ms,
                                                ).await {
                                                    Ok(channel_id) => {
                                                        operation.succeed(None);
                                                        controller.set_screen(ScreenId::Chat);
                                                        controller.select_channel_by_id(&channel_id);
                                                    }
                                                    Err(error) => {
                                                        operation.fail_with(command_failure(error.to_string()));
                                                        controller.runtime_error_toast(error.to_string());
                                                    }
                                                }
                                            });
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                                UiButton {
                                    id: Some(
                                        ControlId::ContactsInviteToChannelButton
                                            .web_dom_id()
                                            .required_dom_id(
                                                "ControlId::ContactsInviteToChannelButton must define a web DOM id",
                                            )
                                            .to_string(),
                                    ),
                                    label: "Invite to Channel".to_string(),
                                    variant: ButtonVariant::Primary,
                                    onclick: {
                                        let authority_id = contact.authority_id;
                                        move |_| {
                                            let controller = invite_to_channel_controller.clone();
                                            let app_core = controller.app_core().clone();
                                            spawn_ui(async move {
                                                let selected_channel = match selected_authoritative_channel(
                                                    controller.as_ref(),
                                                )
                                                .await
                                                {
                                                    Ok(channel) => channel,
                                                    Err(detail) => {
                                                        controller.runtime_error_toast(detail);
                                                        return;
                                                    }
                                                };
                                                let operation = UiWorkflowHandoffOwner::submit(
                                                    controller.clone(),
                                                    OperationId::invitation_create(),
                                                    SemanticOperationKind::InviteActorToChannel,
                                                );
                                                let workflow_instance_id = operation.workflow_instance_id();
                                                let transfer = operation.handoff_to_app_workflow(
                                                    UiOperationTransferScope::InviteActorToChannel,
                                                );
                                                match transfer
                                                    .run_workflow(
                                                        controller.clone(),
                                                        "invite_authority_to_channel_with_context",
                                                        messaging_workflows::handoff::invite_authority_to_channel(
                                                            &app_core,
                                                            messaging_workflows::handoff::InviteAuthorityToChannelRequest {
                                                                receiver: authority_id,
                                                                channel_id: selected_channel.channel_id,
                                                                context_id: Some(selected_channel.context_id),
                                                                channel_name_hint: Some(selected_channel.channel_name),
                                                                operation_instance_id: workflow_instance_id,
                                                                message: None,
                                                                ttl_ms: None,
                                                            },
                                                        ),
                                                    )
                                                    .await
                                                {
                                                    Ok(_) => {
                                                        controller.info_toast("channel invitation sent");
                                                    }
                                                    Err(SubmittedOperationWorkflowError::Workflow(error)) => {
                                                        controller.runtime_error_toast(error.to_string());
                                                    }
                                                    Err(
                                                        SubmittedOperationWorkflowError::Protocol(detail)
                                                        | SubmittedOperationWorkflowError::Panicked(detail),
                                                    ) => {
                                                        controller.runtime_error_toast(detail);
                                                    }
                                                }
                                            });
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                                if !contact.is_guardian {
                                    UiButton {
                                        id: Some(
                                            ControlId::ContactsAddGuardianButton
                                                .web_dom_id()
                                                .required_dom_id(
                                                    "ControlId::ContactsAddGuardianButton must define a web DOM id",
                                                )
                                                .to_string(),
                                        ),
                                        label: "Add Guardian".to_string(),
                                        variant: ButtonVariant::Primary,
                                        onclick: {
                                            let authority_id = contact.authority_id;
                                            move |_| {
                                                let controller = add_guardian_controller.clone();
                                                let app_core = controller.app_core().clone();
                                                spawn_ui(async move {
                                                    let subject = {
                                                        let core = app_core.read().await;
                                                        core.runtime()
                                                            .map(|runtime| runtime.authority_id())
                                                            .or_else(|| core.authority().copied())
                                                    };
                                                    let Some(subject) = subject else {
                                                        controller.runtime_error_toast("Authority not set");
                                                        return;
                                                    };
                                                    let operation = UiWorkflowHandoffOwner::submit(
                                                        controller.clone(),
                                                        OperationId::invitation_create(),
                                                        SemanticOperationKind::CreateGuardianInvitation,
                                                    );
                                                    let workflow_instance_id = operation.workflow_instance_id();
                                                    let transfer = operation.handoff_to_app_workflow(
                                                        UiOperationTransferScope::CreateInvitation,
                                                    );
                                                    match transfer
                                                        .run_workflow(
                                                            controller.clone(),
                                                            "create_guardian_invitation",
                                                            invitation_workflows::handoff::create_guardian_invitation(
                                                                &app_core,
                                                                invitation_workflows::handoff::CreateGuardianInvitationRequest {
                                                                    receiver: authority_id,
                                                                    subject,
                                                                    message: None,
                                                                    ttl_ms: None,
                                                                    operation_instance_id: workflow_instance_id,
                                                                },
                                                            ),
                                                        )
                                                        .await
                                                    {
                                                        Ok(_) => controller.info_toast("Guardian invitation sent"),
                                                        Err(SubmittedOperationWorkflowError::Workflow(error)) => {
                                                            controller.runtime_error_toast(error.to_string());
                                                        }
                                                        Err(
                                                            SubmittedOperationWorkflowError::Protocol(detail)
                                                            | SubmittedOperationWorkflowError::Panicked(detail),
                                                        ) => {
                                                            controller.runtime_error_toast(detail);
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
                                        ControlId::ContactsEditNicknameButton
                                            .web_dom_id()
                                            .required_dom_id(
                                                "ControlId::ContactsEditNicknameButton must define a web DOM id",
                                            )
                                            .to_string(),
                                    ),
                                    label: "Edit Nickname".to_string(),
                                    variant: ButtonVariant::Secondary,
                                    onclick: move |_| {
                                        edit_controller.send_action_keys("e");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                                UiButton {
                                    id: Some(
                                        ControlId::ContactsRemoveContactButton
                                            .web_dom_id()
                                            .required_dom_id(
                                                "ControlId::ContactsRemoveContactButton must define a web DOM id",
                                            )
                                            .to_string(),
                                    ),
                                    label: "Remove Contact".to_string(),
                                    variant: ButtonVariant::Secondary,
                                    onclick: move |_| {
                                        remove_controller.send_action_keys("r");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                        }
                    }
                } else {
                    Empty {
                        class: Some("h-full border-0 bg-background".to_string()),
                        EmptyHeader {
                            EmptyTitle { "No contact selected" }
                            EmptyDescription { "Select a contact to inspect identity and relationship details." }
                        }
                    }
                }
            }
        }
    }
}

fn dom_slug(value: &str) -> String {
    let mut slug = String::with_capacity(value.len());
    let mut previous_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            slug.push('-');
            previous_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}
