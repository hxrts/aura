use super::*;
use crate::semantic_lifecycle::{
    UiLocalOperationOwner, UiOperationTransferScope, UiWorkflowHandoffOwner,
};
use aura_app::frontend_primitives::SubmittedOperationWorkflowError;
use aura_app::ui_contract::{
    ControlId, OperationId, SemanticFailureCode, SemanticFailureDomain, SemanticOperationError,
    SemanticOperationKind,
};

fn command_failure(detail: impl Into<String>) -> SemanticOperationError {
    SemanticOperationError::new(
        SemanticFailureDomain::Command,
        SemanticFailureCode::InternalError,
    )
    .with_detail(detail.into())
}

fn handle_workflow_error(controller: &UiController, error: SubmittedOperationWorkflowError) {
    match error {
        SubmittedOperationWorkflowError::Workflow(error) => {
            controller.runtime_error_toast(error.to_string());
        }
        SubmittedOperationWorkflowError::Protocol(detail)
        | SubmittedOperationWorkflowError::Panicked(detail) => {
            controller.runtime_error_toast(detail);
        }
    }
}

fn control_dom_id(control_id: ControlId) -> Option<String> {
    control_id.web_dom_id().map(str::to_string)
}

fn amp_observed_action(
    controller: Arc<UiController>,
    render_tick: Signal<u64>,
    operation_id: OperationId,
    kind: SemanticOperationKind,
    success_message: &'static str,
    unsupported_detail: Option<&'static str>,
) {
    let operation = UiLocalOperationOwner::submit(controller.clone(), operation_id, kind);
    if let Some(detail) = unsupported_detail {
        operation.fail_with(
            SemanticOperationError::new(
                SemanticFailureDomain::Command,
                SemanticFailureCode::UnsupportedCommand,
            )
            .with_detail(detail),
        );
        controller.runtime_error_toast(detail);
    } else {
        operation.succeed(None);
        controller.info_toast(success_message);
    }
    let mut tick = render_tick;
    tick.set(tick() + 1);
}

pub(in crate::app) fn accept_invitation_action(
    controller: Arc<UiController>,
    render_tick: Signal<u64>,
    invitation_id: String,
) {
    let app_core = controller.app_core().clone();
    let mut tick = render_tick;
    spawn_ui(async move {
        let operation = UiWorkflowHandoffOwner::submit(
            controller.clone(),
            OperationId::invitation_accept_contact(),
            SemanticOperationKind::AcceptContactInvitation,
        );
        let instance_id = operation.workflow_instance_id();
        let transfer =
            operation.handoff_to_app_workflow(UiOperationTransferScope::AcceptInvitation);
        match transfer
            .run_workflow(
                controller.clone(),
                "accept_invitation_by_id",
                invitation_workflows::handoff::accept_invitation_by_id(
                    &app_core,
                    invitation_workflows::handoff::InvitationByIdRequest {
                        invitation_id,
                        operation_instance_id: instance_id,
                    },
                ),
            )
            .await
        {
            Ok(_) => controller.complete_runtime_modal_success("Invitation accepted"),
            Err(error) => handle_workflow_error(&controller, error),
        }
        tick.set(tick() + 1);
    });
}

pub(in crate::app) fn decline_invitation_action(
    controller: Arc<UiController>,
    render_tick: Signal<u64>,
    invitation_id: String,
) {
    let app_core = controller.app_core().clone();
    let mut tick = render_tick;
    spawn_ui(async move {
        let operation = UiWorkflowHandoffOwner::submit(
            controller.clone(),
            OperationId::invitation_decline(),
            SemanticOperationKind::DeclineInvitation,
        );
        let instance_id = operation.workflow_instance_id();
        let transfer =
            operation.handoff_to_app_workflow(UiOperationTransferScope::DeclineInvitation);
        match transfer
            .run_workflow(
                controller.clone(),
                "decline_invitation_by_id",
                invitation_workflows::handoff::decline_invitation_by_id(
                    &app_core,
                    invitation_workflows::handoff::InvitationByIdRequest {
                        invitation_id,
                        operation_instance_id: instance_id,
                    },
                ),
            )
            .await
        {
            Ok(()) => controller.complete_runtime_modal_success("Invitation declined"),
            Err(error) => handle_workflow_error(&controller, error),
        }
        tick.set(tick() + 1);
    });
}

pub(in crate::app) fn accept_channel_invitation_action(
    controller: Arc<UiController>,
    render_tick: Signal<u64>,
) {
    let app_core = controller.app_core().clone();
    let mut tick = render_tick;
    spawn_ui(async move {
        let operation = UiWorkflowHandoffOwner::submit(
            controller.clone(),
            OperationId::invitation_accept_channel(),
            SemanticOperationKind::AcceptPendingChannelInvitation,
        );
        let instance_id = operation.workflow_instance_id();
        let transfer = operation
            .handoff_to_app_workflow(UiOperationTransferScope::AcceptPendingChannelInvitation);
        match transfer
            .run_workflow(
                controller.clone(),
                "accept_pending_channel_invitation",
                invitation_workflows::handoff::accept_pending_channel_invitation(
                    &app_core,
                    invitation_workflows::handoff::PendingChannelInvitationRequest {
                        operation_instance_id: instance_id,
                    },
                ),
            )
            .await
        {
            Ok(_) => controller.complete_runtime_modal_success("Channel invitation accepted"),
            Err(error) => handle_workflow_error(&controller, error),
        }
        tick.set(tick() + 1);
    });
}

pub(in crate::app) fn export_invitation_action(
    controller: Arc<UiController>,
    render_tick: Signal<u64>,
    invitation_id: String,
) {
    let app_core = controller.app_core().clone();
    let mut tick = render_tick;
    spawn_ui(async move {
        let operation = UiWorkflowHandoffOwner::submit(
            controller.clone(),
            OperationId::invitation_export(),
            SemanticOperationKind::ExportInvitation,
        );
        let instance_id = operation.workflow_instance_id();
        let transfer =
            operation.handoff_to_app_workflow(UiOperationTransferScope::ExportInvitation);
        match transfer
            .run_workflow(
                controller.clone(),
                "export_invitation_by_id",
                invitation_workflows::handoff::export_invitation_by_id(
                    &app_core,
                    invitation_workflows::handoff::InvitationByIdRequest {
                        invitation_id,
                        operation_instance_id: instance_id,
                    },
                ),
            )
            .await
        {
            Ok(code) => {
                controller.write_clipboard(&code);
                controller.complete_runtime_modal_success("Invitation code copied to clipboard");
            }
            Err(error) => handle_workflow_error(&controller, error),
        }
        tick.set(tick() + 1);
    });
}

pub(in crate::app) fn revoke_invitation_action(
    controller: Arc<UiController>,
    render_tick: Signal<u64>,
    invitation_id: String,
) {
    let app_core = controller.app_core().clone();
    let mut tick = render_tick;
    spawn_ui(async move {
        let operation = UiWorkflowHandoffOwner::submit(
            controller.clone(),
            OperationId::invitation_revoke(),
            SemanticOperationKind::RevokeInvitation,
        );
        let instance_id = operation.workflow_instance_id();
        let transfer =
            operation.handoff_to_app_workflow(UiOperationTransferScope::RevokeInvitation);
        match transfer
            .run_workflow(
                controller.clone(),
                "cancel_invitation_by_id",
                invitation_workflows::handoff::cancel_invitation_by_id(
                    &app_core,
                    invitation_workflows::handoff::InvitationByIdRequest {
                        invitation_id,
                        operation_instance_id: instance_id,
                    },
                ),
            )
            .await
        {
            Ok(()) => controller.complete_runtime_modal_success("Invitation revoked"),
            Err(error) => handle_workflow_error(&controller, error),
        }
        tick.set(tick() + 1);
    });
}

pub(in crate::app) fn approve_recovery_action(
    controller: Arc<UiController>,
    render_tick: Signal<u64>,
    ceremony_id: String,
) {
    let app_core = controller.app_core().clone();
    let operation = UiLocalOperationOwner::submit(
        controller.clone(),
        OperationId::submit_guardian_approval(),
        SemanticOperationKind::SubmitGuardianApproval,
    );
    let mut tick = render_tick;
    spawn_ui(async move {
        match recovery_workflows::approve_recovery(&app_core, &CeremonyId::new(ceremony_id)).await {
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

pub(in crate::app) fn accept_friend_request_action(
    controller: Arc<UiController>,
    render_tick: Signal<u64>,
    authority_id: String,
) {
    let app_core = controller.app_core().clone();
    let operation = UiLocalOperationOwner::submit(
        controller.clone(),
        OperationId::accept_friend_request(),
        SemanticOperationKind::AcceptFriendRequest,
    );
    let mut tick = render_tick;
    spawn_ui(async move {
        let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
            Ok(value) => value,
            Err(error) => {
                operation.fail_with(command_failure(error.to_string()));
                controller.runtime_error_toast(error.to_string());
                return;
            }
        };
        match contacts_workflows::accept_friend_request(&app_core, &authority_id, timestamp_ms)
            .await
        {
            Ok(()) => {
                operation.succeed(None);
                controller.complete_runtime_modal_success("Friend request accepted");
            }
            Err(error) => {
                operation.fail_with(command_failure(error.to_string()));
                controller.runtime_error_toast(error.to_string());
            }
        }
        tick.set(tick() + 1);
    });
}

pub(in crate::app) fn decline_friend_request_action(
    controller: Arc<UiController>,
    render_tick: Signal<u64>,
    authority_id: String,
) {
    let app_core = controller.app_core().clone();
    let operation = UiLocalOperationOwner::submit(
        controller.clone(),
        OperationId::decline_friend_request(),
        SemanticOperationKind::DeclineFriendRequest,
    );
    let mut tick = render_tick;
    spawn_ui(async move {
        let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
            Ok(value) => value,
            Err(error) => {
                operation.fail_with(command_failure(error.to_string()));
                controller.runtime_error_toast(error.to_string());
                return;
            }
        };
        match contacts_workflows::decline_friend_request(&app_core, &authority_id, timestamp_ms)
            .await
        {
            Ok(()) => {
                operation.succeed(None);
                controller.complete_runtime_modal_success("Friend request declined");
            }
            Err(error) => {
                operation.fail_with(command_failure(error.to_string()));
                controller.runtime_error_toast(error.to_string());
            }
        }
        tick.set(tick() + 1);
    });
}

/// Shared action bar for notification detail panel.
/// Renders accept/decline/approve buttons at the bottom right based on action type.
#[component]
pub(in crate::app) fn NotificationActionBar(
    action: NotificationRuntimeAction,
    item_id: String,
    controller: Arc<UiController>,
    render_tick: Signal<u64>,
) -> Element {
    match action {
        NotificationRuntimeAction::ReceivedInvitation => {
            let accept_controller = controller.clone();
            let accept_id = item_id.clone();
            let decline_id = item_id;
            rsx! {
                UiButton {
                    label: "Accept".to_string(),
                    variant: ButtonVariant::Primary,
                    onclick: move |_| {
                        accept_invitation_action(
                            accept_controller.clone(),
                            render_tick,
                            accept_id.clone(),
                        );
                    },
                }
                UiButton {
                    label: "Decline".to_string(),
                    variant: ButtonVariant::Secondary,
                    onclick: move |_| {
                        decline_invitation_action(
                            controller.clone(),
                            render_tick,
                            decline_id.clone(),
                        );
                    },
                }
            }
        }
        NotificationRuntimeAction::PendingChannelInvitation => {
            let accept_controller = controller.clone();
            let decline_id = item_id;
            rsx! {
                UiButton {
                    label: "Accept".to_string(),
                    variant: ButtonVariant::Primary,
                    onclick: move |_| {
                        accept_channel_invitation_action(accept_controller.clone(), render_tick);
                    },
                }
                UiButton {
                    label: "Decline".to_string(),
                    variant: ButtonVariant::Secondary,
                    onclick: move |_| {
                        decline_invitation_action(
                            controller.clone(),
                            render_tick,
                            decline_id.clone(),
                        );
                    },
                }
            }
        }
        NotificationRuntimeAction::SentInvitation => {
            let export_controller = controller.clone();
            let export_id = item_id.clone();
            let revoke_id = item_id;
            rsx! {
                UiButton {
                    label: "Copy Code".to_string(),
                    variant: ButtonVariant::Primary,
                    onclick: move |_| {
                        export_invitation_action(
                            export_controller.clone(),
                            render_tick,
                            export_id.clone(),
                        );
                    },
                }
                UiButton {
                    label: "Revoke".to_string(),
                    variant: ButtonVariant::Secondary,
                    onclick: move |_| {
                        revoke_invitation_action(
                            controller.clone(),
                            render_tick,
                            revoke_id.clone(),
                        );
                    },
                }
            }
        }
        NotificationRuntimeAction::RecoveryApproval => {
            let ceremony_id = item_id;
            rsx! {
                UiButton {
                    label: "Approve Recovery".to_string(),
                    variant: ButtonVariant::Primary,
                    onclick: move |_| {
                        approve_recovery_action(
                            controller.clone(),
                            render_tick,
                            ceremony_id.clone(),
                        );
                    },
                }
            }
        }
        NotificationRuntimeAction::FriendRequest => {
            let accept_controller = controller.clone();
            let accept_id = item_id.clone();
            let decline_id = item_id;
            rsx! {
                UiButton {
                    label: "Accept".to_string(),
                    variant: ButtonVariant::Primary,
                    onclick: move |_| {
                        accept_friend_request_action(
                            accept_controller.clone(),
                            render_tick,
                            accept_id.clone(),
                        );
                    },
                }
                UiButton {
                    label: "Decline".to_string(),
                    variant: ButtonVariant::Secondary,
                    onclick: move |_| {
                        decline_friend_request_action(
                            controller.clone(),
                            render_tick,
                            decline_id.clone(),
                        );
                    },
                }
            }
        }
        NotificationRuntimeAction::AmpRaiseEmergencyAlarm => rsx! {
            UiButton {
                id: control_dom_id(ControlId::AmpRaiseEmergencyAlarmButton),
                label: "Raise Alarm".to_string(),
                variant: ButtonVariant::Secondary,
                onclick: move |_| {
                    amp_observed_action(
                        controller.clone(),
                        render_tick,
                        OperationId::amp_raise_emergency_alarm(),
                        SemanticOperationKind::AmpRaiseEmergencyAlarm,
                        "Emergency alarm status opened",
                        Some("AMP emergency alarm submission is not available from this frontend yet"),
                    );
                },
            }
        },
        NotificationRuntimeAction::AmpApproveQuarantine => rsx! {
            UiButton {
                id: control_dom_id(ControlId::AmpApproveQuarantineButton),
                label: "Approve Quarantine".to_string(),
                variant: ButtonVariant::Primary,
                onclick: move |_| {
                    amp_observed_action(
                        controller.clone(),
                        render_tick,
                        OperationId::amp_approve_quarantine(),
                        SemanticOperationKind::AmpApproveQuarantine,
                        "Quarantine approval status opened",
                        Some("AMP quarantine approval submission is not available from this frontend yet"),
                    );
                },
            }
        },
        NotificationRuntimeAction::AmpApproveCryptoshred => rsx! {
            UiButton {
                id: control_dom_id(ControlId::AmpApproveCryptoshredButton),
                label: "Confirm Cryptoshred".to_string(),
                variant: ButtonVariant::Primary,
                onclick: move |_| {
                    amp_observed_action(
                        controller.clone(),
                        render_tick,
                        OperationId::amp_approve_cryptoshred(),
                        SemanticOperationKind::AmpApproveCryptoshred,
                        "Cryptoshred confirmation status opened",
                        Some("AMP cryptoshred approval is destructive and not available from this frontend yet; pre-emergency readability may be lost once approved"),
                    );
                },
            }
        },
        NotificationRuntimeAction::AmpViewConflictEvidence => rsx! {
            UiButton {
                id: control_dom_id(ControlId::AmpViewConflictEvidenceButton),
                label: "View Evidence".to_string(),
                variant: ButtonVariant::Secondary,
                onclick: move |_| {
                    amp_observed_action(
                        controller.clone(),
                        render_tick,
                        OperationId::amp_view_conflict_evidence(),
                        SemanticOperationKind::AmpViewConflictEvidence,
                        "Conflict evidence is shown in the transition notification details",
                        None,
                    );
                },
            }
        },
        NotificationRuntimeAction::AmpViewFinalizationStatus => rsx! {
            UiButton {
                id: control_dom_id(ControlId::AmpViewFinalizationStatusButton),
                label: "Finalization Status".to_string(),
                variant: ButtonVariant::Secondary,
                onclick: move |_| {
                    amp_observed_action(
                        controller.clone(),
                        render_tick,
                        OperationId::amp_view_finalization_status(),
                        SemanticOperationKind::AmpViewFinalizationStatus,
                        "Finalization status is shown in the transition notification details",
                        None,
                    );
                },
            }
        },
        NotificationRuntimeAction::None => rsx! {},
    }
}
