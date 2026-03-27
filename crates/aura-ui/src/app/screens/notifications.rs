use super::*;
use crate::semantic_lifecycle::{
    UiLocalOperationOwner, UiOperationTransferScope, UiWorkflowHandoffOwner,
};
use aura_app::frontend_primitives::SubmittedOperationWorkflowError;
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
pub(super) fn NotificationsScreen(
    model: &UiModel,
    runtime: &NotificationsRuntimeView,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
) -> Element {
    let selected = runtime
        .items
        .get(model.selected_notification_index().unwrap_or_default())
        .cloned();
    rsx! {
        div {
            class: "grid w-full gap-3 lg:grid-cols-12 lg:h-full lg:min-h-0 lg:[grid-template-rows:minmax(0,1fr)]",
            UiCard {
                title: "Notifications".to_string(),
                subtitle: Some("Runtime events".to_string()),
                extra_class: Some("lg:col-span-4".to_string()),
                if runtime.items.is_empty() {
                    Empty {
                        class: Some("h-full border-0 bg-background".to_string()),
                        EmptyHeader {
                            EmptyTitle { "No notifications" }
                            EmptyDescription { "Runtime events will appear here." }
                        }
                    }
                } else {
                    ScrollArea {
                        class: Some("flex-1 lg:min-h-0 pr-1".to_string()),
                        ScrollAreaViewport {
                            class: Some("aura-list space-y-2".to_string()),
                            for (idx, entry) in runtime.items.iter().enumerate() {
                                button {
                                    r#type: "button",
                                    id: list_item_dom_id(ListId::Notifications, &entry.id),
                                    class: "block w-full text-left",
                                    onclick: {
                                        let controller = controller.clone();
                                        let item_count = runtime.items.len();
                                        move |_| {
                                            controller.set_selected_notification_index(idx, item_count);
                                            render_tick.set(render_tick() + 1);
                                        }
                                    },
                                    UiListItem {
                                        label: entry.title.clone(),
                                        secondary: Some(entry.kind_label.clone()),
                                        active: model.selected_notification_index() == Some(idx),
                                    }
                                }
                            }
                        }
                    }
                }
            }
            UiCard {
                title: "Details".to_string(),
                subtitle: Some("Selected notification".to_string()),
                extra_class: Some("lg:col-span-8".to_string()),
                if let Some(item) = selected {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        UiListItem {
                            label: item.kind_label,
                            secondary: Some(item.title),
                            active: false,
                        }
                        UiListItem {
                            label: item.subtitle,
                            secondary: Some(item.detail),
                            active: false,
                        }
                        UiCardFooter {
                            extra_class: None,
                            div {
                                class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                                match item.action {
                                    NotificationRuntimeAction::ReceivedInvitation => {
                                        let accept_controller = controller.clone();
                                        let accept_invitation_id = item.id.clone();
                                        let decline_invitation_id = item.id;
                                        rsx! {
                                            UiButton {
                                                label: "Accept".to_string(),
                                                variant: ButtonVariant::Primary,
                                                onclick: {
                                                    move |_| {
                                                        let controller = accept_controller.clone();
                                                        let app_core = controller.app_core().clone();
                                                        let mut tick = render_tick;
                                                        let invitation_id = accept_invitation_id.clone();
                                                        spawn_ui(async move {
                                                            let operation = UiWorkflowHandoffOwner::submit(
                                                                controller.clone(),
                                                                OperationId::invitation_accept(),
                                                                SemanticOperationKind::AcceptContactInvitation,
                                                            );
                                                            let workflow_instance_id = operation.workflow_instance_id();
                                                            let transfer = operation.handoff_to_app_workflow(
                                                                UiOperationTransferScope::AcceptInvitation,
                                                            );
                                                            match transfer
                                                                .run_workflow(
                                                                    controller.clone(),
                                                                    "accept_invitation_by_str",
                                                                    invitation_workflows::handoff::accept_invitation_by_id(
                                                                        &app_core,
                                                                        invitation_workflows::handoff::InvitationByIdRequest {
                                                                            invitation_id: invitation_id.clone(),
                                                                            operation_instance_id: workflow_instance_id,
                                                                        },
                                                                    ),
                                                                )
                                                                .await
                                                            {
                                                                Ok(_) => controller.complete_runtime_modal_success("Invitation accepted"),
                                                                Err(SubmittedOperationWorkflowError::Workflow(error)) => controller.runtime_error_toast(error.to_string()),
                                                                Err(
                                                                    SubmittedOperationWorkflowError::Protocol(detail)
                                                                    | SubmittedOperationWorkflowError::Panicked(detail),
                                                                ) => controller.runtime_error_toast(detail),
                                                            }
                                                            tick.set(tick() + 1);
                                                        });
                                                    }
                                                }
                                            }
                                            UiButton {
                                                label: "Decline".to_string(),
                                                variant: ButtonVariant::Secondary,
                                                onclick: {
                                                    move |_| {
                                                        let controller = controller.clone();
                                                        let app_core = controller.app_core().clone();
                                                        let mut tick = render_tick;
                                                        let invitation_id = decline_invitation_id.clone();
                                                        spawn_ui(async move {
                                                            let operation = UiWorkflowHandoffOwner::submit(
                                                                controller.clone(),
                                                                OperationId::invitation_decline(),
                                                                SemanticOperationKind::DeclineInvitation,
                                                            );
                                                            let workflow_instance_id = operation.workflow_instance_id();
                                                            let transfer = operation.handoff_to_app_workflow(
                                                                UiOperationTransferScope::DeclineInvitation,
                                                            );
                                                            match transfer
                                                                .run_workflow(
                                                                    controller.clone(),
                                                                    "decline_invitation_by_str",
                                                                    invitation_workflows::handoff::decline_invitation_by_id(
                                                                        &app_core,
                                                                        invitation_workflows::handoff::InvitationByIdRequest {
                                                                            invitation_id: invitation_id.clone(),
                                                                            operation_instance_id: workflow_instance_id,
                                                                        },
                                                                    ),
                                                                )
                                                                .await
                                                            {
                                                                Ok(()) => controller.complete_runtime_modal_success("Invitation declined"),
                                                                Err(SubmittedOperationWorkflowError::Workflow(error)) => controller.runtime_error_toast(error.to_string()),
                                                                Err(
                                                                    SubmittedOperationWorkflowError::Protocol(detail)
                                                                    | SubmittedOperationWorkflowError::Panicked(detail),
                                                                ) => controller.runtime_error_toast(detail),
                                                            }
                                                            tick.set(tick() + 1);
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    },
                                    NotificationRuntimeAction::PendingChannelInvitation => {
                                        let accept_controller = controller.clone();
                                        let decline_invitation_id = item.id;
                                        rsx! {
                                            UiButton {
                                                label: "Accept".to_string(),
                                                variant: ButtonVariant::Primary,
                                                onclick: {
                                                    move |_| {
                                                        let controller = accept_controller.clone();
                                                        let app_core = controller.app_core().clone();
                                                        let mut tick = render_tick;
                                                        spawn_ui(async move {
                                                            let operation = UiWorkflowHandoffOwner::submit(
                                                                controller.clone(),
                                                                OperationId::invitation_accept(),
                                                                SemanticOperationKind::AcceptPendingChannelInvitation,
                                                            );
                                                            let workflow_instance_id = operation.workflow_instance_id();
                                                            let transfer = operation.handoff_to_app_workflow(
                                                                UiOperationTransferScope::AcceptPendingChannelInvitation,
                                                            );
                                                            match transfer
                                                                .run_workflow(
                                                                    controller.clone(),
                                                                    "accept_pending_home_invitation",
                                                                    invitation_workflows::handoff::accept_pending_home_invitation(
                                                                        &app_core,
                                                                        invitation_workflows::handoff::PendingHomeInvitationRequest {
                                                                            operation_instance_id: workflow_instance_id,
                                                                        },
                                                                    ),
                                                                )
                                                                .await
                                                            {
                                                                Ok(_) => controller.complete_runtime_modal_success("Home invitation accepted"),
                                                                Err(SubmittedOperationWorkflowError::Workflow(error)) => controller.runtime_error_toast(error.to_string()),
                                                                Err(
                                                                    SubmittedOperationWorkflowError::Protocol(detail)
                                                                    | SubmittedOperationWorkflowError::Panicked(detail),
                                                                ) => controller.runtime_error_toast(detail),
                                                            }
                                                            tick.set(tick() + 1);
                                                        });
                                                    }
                                                }
                                            }
                                            UiButton {
                                                label: "Decline".to_string(),
                                                variant: ButtonVariant::Secondary,
                                                onclick: {
                                                    move |_| {
                                                        let controller = controller.clone();
                                                        let app_core = controller.app_core().clone();
                                                        let mut tick = render_tick;
                                                        let invitation_id = decline_invitation_id.clone();
                                                        spawn_ui(async move {
                                                            let operation = UiWorkflowHandoffOwner::submit(
                                                                controller.clone(),
                                                                OperationId::invitation_decline(),
                                                                SemanticOperationKind::DeclineInvitation,
                                                            );
                                                            let workflow_instance_id = operation.workflow_instance_id();
                                                            let transfer = operation.handoff_to_app_workflow(
                                                                UiOperationTransferScope::DeclineInvitation,
                                                            );
                                                            match transfer
                                                                .run_workflow(
                                                                    controller.clone(),
                                                                    "decline_invitation_by_str",
                                                                    invitation_workflows::handoff::decline_invitation_by_id(
                                                                        &app_core,
                                                                        invitation_workflows::handoff::InvitationByIdRequest {
                                                                            invitation_id: invitation_id.clone(),
                                                                            operation_instance_id: workflow_instance_id,
                                                                        },
                                                                    ),
                                                                )
                                                                .await
                                                            {
                                                                Ok(()) => controller.complete_runtime_modal_success("Invitation declined"),
                                                                Err(SubmittedOperationWorkflowError::Workflow(error)) => controller.runtime_error_toast(error.to_string()),
                                                                Err(
                                                                    SubmittedOperationWorkflowError::Protocol(detail)
                                                                    | SubmittedOperationWorkflowError::Panicked(detail),
                                                                ) => controller.runtime_error_toast(detail),
                                                            }
                                                            tick.set(tick() + 1);
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    },
                                    NotificationRuntimeAction::SentInvitation => {
                                        let export_controller = controller.clone();
                                        let revoke_controller = controller.clone();
                                        rsx! {
                                        UiButton {
                                            label: "Copy Code".to_string(),
                                            variant: ButtonVariant::Primary,
                                            onclick: {
                                                let invitation_id = item.id.clone();
                                                move |_| {
                                                    let controller = export_controller.clone();
                                                    let app_core = controller.app_core().clone();
                                                    let mut tick = render_tick;
                                                    let invitation_id = invitation_id.clone();
                                                    spawn_ui(async move {
                                                        let operation = UiWorkflowHandoffOwner::submit(
                                                            controller.clone(),
                                                            OperationId::invitation_export(),
                                                            SemanticOperationKind::ExportInvitation,
                                                        );
                                                        let workflow_instance_id = operation.workflow_instance_id();
                                                        let transfer = operation.handoff_to_app_workflow(
                                                            UiOperationTransferScope::ExportInvitation,
                                                        );
                                                        match transfer
                                                            .run_workflow(
                                                                controller.clone(),
                                                                "export_invitation_by_str",
                                                                invitation_workflows::handoff::export_invitation_by_id(
                                                                    &app_core,
                                                                    invitation_workflows::handoff::InvitationByIdRequest {
                                                                        invitation_id: invitation_id.clone(),
                                                                        operation_instance_id: workflow_instance_id,
                                                                    },
                                                                ),
                                                            )
                                                            .await
                                                        {
                                                            Ok(code) => {
                                                                controller.write_clipboard(&code);
                                                                controller.remember_invitation_code(&code);
                                                                controller.complete_runtime_modal_success("Invitation code copied to clipboard");
                                                            }
                                                            Err(SubmittedOperationWorkflowError::Workflow(error)) => {
                                                                controller.runtime_error_toast(error.to_string())
                                                            }
                                                            Err(
                                                                SubmittedOperationWorkflowError::Protocol(detail)
                                                                | SubmittedOperationWorkflowError::Panicked(detail),
                                                            ) => {
                                                                controller.runtime_error_toast(detail);
                                                            }
                                                        }
                                                        tick.set(tick() + 1);
                                                    });
                                                }
                                            }
                                        }
                                        UiButton {
                                            label: "Revoke".to_string(),
                                            variant: ButtonVariant::Secondary,
                                            onclick: {
                                                let invitation_id = item.id;
                                                move |_| {
                                                    let controller = revoke_controller.clone();
                                                    let app_core = controller.app_core().clone();
                                                    let mut tick = render_tick;
                                                    let invitation_id = invitation_id.clone();
                                                    spawn_ui(async move {
                                                        let operation = UiWorkflowHandoffOwner::submit(
                                                            controller.clone(),
                                                            OperationId::invitation_revoke(),
                                                            SemanticOperationKind::RevokeInvitation,
                                                        );
                                                        let workflow_instance_id = operation.workflow_instance_id();
                                                        let transfer = operation.handoff_to_app_workflow(
                                                            UiOperationTransferScope::RevokeInvitation,
                                                        );
                                                        match transfer
                                                            .run_workflow(
                                                                controller.clone(),
                                                                "cancel_invitation_by_str",
                                                                invitation_workflows::handoff::cancel_invitation_by_id(
                                                                    &app_core,
                                                                    invitation_workflows::handoff::InvitationByIdRequest {
                                                                        invitation_id: invitation_id.clone(),
                                                                        operation_instance_id: workflow_instance_id,
                                                                    },
                                                                ),
                                                            )
                                                            .await
                                                        {
                                                            Ok(()) => controller.complete_runtime_modal_success("Invitation revoked"),
                                                            Err(SubmittedOperationWorkflowError::Workflow(error)) => {
                                                                controller.runtime_error_toast(error.to_string())
                                                            }
                                                            Err(
                                                                SubmittedOperationWorkflowError::Protocol(detail)
                                                                | SubmittedOperationWorkflowError::Panicked(detail),
                                                            ) => {
                                                                controller.runtime_error_toast(detail);
                                                            }
                                                        }
                                                        tick.set(tick() + 1);
                                                    });
                                                }
                                            }
                                        }
                                    }},
                                    NotificationRuntimeAction::RecoveryApproval => rsx! {
                                        UiButton {
                                            label: "Approve Recovery".to_string(),
                                            variant: ButtonVariant::Primary,
                                            onclick: {
                                                let ceremony_id = item.id;
                                                move |_| {
                                                    let controller = controller.clone();
                                                    let app_core = controller.app_core().clone();
                                                    let operation = UiLocalOperationOwner::submit(
                                                        controller.clone(),
                                                        OperationId::submit_guardian_approval(),
                                                        SemanticOperationKind::SubmitGuardianApproval,
                                                    );
                                                    let mut tick = render_tick;
                                                    let ceremony_id = ceremony_id.clone();
                                                    spawn_ui(async move {
                                                        match recovery_workflows::approve_recovery(
                                                            &app_core,
                                                            &CeremonyId::new(ceremony_id),
                                                        ).await {
                                                            Ok(()) => {
                                                                operation.succeed(None);
                                                                controller.complete_runtime_modal_success("Recovery approved");
                                                            }
                                                            Err(error) => {
                                                                operation.fail_with(command_failure(error.to_string()));
                                                                controller.runtime_error_toast(error.to_string());
                                                            }
                                                        }
                                                        tick.set(tick() + 1);
                                                    });
                                                }
                                            }
                                        }
                                    },
                                    NotificationRuntimeAction::None => rsx! {},
                                }
                            }
                            if let Some(code) = model.last_invite_code.as_ref() {
                                div {
                                    class: "w-full rounded-md border border-border bg-background/60 px-3 py-2 text-xs",
                                    p { class: "m-0 text-[0.7rem] uppercase tracking-[0.06em] text-muted-foreground", "Last Invitation Code" }
                                    p { class: "m-0 mt-1 break-all font-mono text-foreground", "{code}" }
                                }
                            }
                        }
                    }
                } else {
                    Empty {
                        class: Some("h-full border-0 bg-background".to_string()),
                        EmptyHeader {
                            EmptyTitle { "No notification selected" }
                            EmptyDescription { "Select an item from the list to inspect the latest invitation or recovery activity." }
                        }
                        UiButton {
                            label: "Import Invitation".to_string(),
                            variant: ButtonVariant::Primary,
                            onclick: move |_| {
                                controller.send_action_keys("a");
                                render_tick.set(render_tick() + 1);
                            }
                        }
                    }
                }
            }
        }
    }
}
