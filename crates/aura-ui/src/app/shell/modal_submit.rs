use super::*;
use crate::semantic_lifecycle::{
    UiCeremonySubmissionOwner, UiLocalOperationOwner, UiOperationTransferScope,
    UiWorkflowHandoffOwner,
};
use aura_app::frontend_primitives::SubmittedOperationWorkflowError;
use aura_app::ui_contract::{
    OperationId, SemanticFailureCode, SemanticFailureDomain, SemanticOperationError,
    SemanticOperationKind,
};

pub(crate) fn harness_log(line: &str) {
    tracing::info!("{line}");
}

fn invitation_command_failure(detail: impl Into<String>) -> SemanticOperationError {
    SemanticOperationError::new(
        SemanticFailureDomain::Command,
        SemanticFailureCode::InternalError,
    )
    .with_detail(detail.into())
}

pub(crate) fn selected_contact_for_modal(
    runtime: &ContactsRuntimeView,
    model: &UiModel,
) -> Option<ContactsRuntimeContact> {
    let selected = model.selected_contact_authority_id()?;
    runtime
        .contacts
        .iter()
        .find(|contact| contact.authority_id == selected)
        .cloned()
}

pub(crate) fn next_device_enrollment_invitee_authority_id(
    controller: &UiController,
    device_name: &str,
) -> AuthorityId {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let seed = format!(
        "ui-add-device:{}:{}:{}",
        controller.authority_id(),
        device_name,
        seq
    );
    AuthorityId::new_from_entropy(hash(seed.as_bytes()))
}

pub(crate) fn monitor_runtime_device_enrollment_ceremony(
    controller: Arc<UiController>,
    app_core: Arc<async_lock::RwLock<aura_app::AppCore>>,
    status_handle: ceremony_workflows::CeremonyStatusHandle,
    rerender: Arc<dyn Fn() + Send + Sync>,
) {
    spawn_ui(async move {
        let lifecycle = ceremony_workflows::monitor_key_rotation_ceremony_with_policy(
            &app_core,
            &status_handle,
            ceremony_workflows::CeremonyPollPolicy {
                interval: Duration::from_secs(1),
                refresh_settings_on_complete: false,
                ..Default::default()
            },
            |status| {
                controller.update_runtime_device_enrollment_status(
                    status.accepted_count,
                    status.total_count,
                    status.threshold,
                    status.is_complete,
                    status.has_failed,
                    status.error_message.clone(),
                );
                rerender();
            },
            |duration| {
                let app_core = app_core.clone();
                async move {
                    let sleep_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
                    let _ = time_workflows::sleep_ms(&app_core, sleep_ms).await;
                }
            },
        )
        .await;

        match lifecycle {
            Ok(lifecycle)
                if lifecycle.state == ceremony_workflows::CeremonyLifecycleState::TimedOut =>
            {
                controller.runtime_error_toast(
                    "Device enrollment status monitoring timed out; use Enter to refresh",
                );
                rerender();
            }
            Ok(_) => {}
            Err(error) => {
                controller.runtime_error_toast(error.to_string());
                rerender();
            }
        }
    });
}

pub(crate) fn monitor_runtime_key_rotation_ceremony(
    controller: Arc<UiController>,
    app_core: Arc<async_lock::RwLock<aura_app::AppCore>>,
    status_handle: ceremony_workflows::CeremonyStatusHandle,
    label: &'static str,
    rerender: Arc<dyn Fn() + Send + Sync>,
) {
    spawn_ui(async move {
        let lifecycle = ceremony_workflows::monitor_key_rotation_ceremony_with_policy(
            &app_core,
            &status_handle,
            ceremony_workflows::CeremonyPollPolicy::with_interval(Duration::from_secs(1)),
            |_| {},
            |duration| {
                let app_core = app_core.clone();
                async move {
                    let sleep_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
                    let _ = time_workflows::sleep_ms(&app_core, sleep_ms).await;
                }
            },
        )
        .await;

        match lifecycle {
            Ok(lifecycle)
                if lifecycle.state == ceremony_workflows::CeremonyLifecycleState::TimedOut =>
            {
                controller.runtime_error_toast(format!(
                    "{label} status monitoring timed out; reopen the wizard to inspect progress"
                ));
                rerender();
            }
            Ok(_) => {}
            Err(error) => {
                controller.runtime_error_toast(error.to_string());
                rerender();
            }
        }
    });
}

pub(crate) fn removable_device_for_modal(
    runtime: &SettingsRuntimeView,
    model: &UiModel,
) -> Option<SettingsRuntimeDevice> {
    runtime
        .devices
        .iter()
        .find(|device| {
            !device.is_current
                && device.name
                    == model
                        .secondary_device_name()
                        .or_else(|| {
                            model
                                .selected_device_modal()
                                .map(|state| state.candidate_name.as_str())
                        })
                        .unwrap_or("")
        })
        .cloned()
        .or_else(|| {
            runtime
                .devices
                .iter()
                .find(|device| !device.is_current)
                .cloned()
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservedModalSubmitAction {
    AdvanceAddDeviceShare,
    FinalizeAddDevice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SimpleModalSubmitAction {
    CreateHome,
    AcceptInvitation,
    CreateInvitation,
    SetChannelTopic,
    EditNickname,
    RemoveContact,
    RequestRecovery,
    ConfirmRemoveDevice,
    AssignModerator,
    SwitchAuthority,
    AccessOverride,
    CapabilityConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WizardModalSubmitAction {
    StartAddDevice,
    RefreshAddDeviceStatus,
    CreateChannel,
    GuardianSetup,
    MfaSetup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModalSubmitClass {
    ObservedOnly(ObservedModalSubmitAction),
    SimpleDispatch(SimpleModalSubmitAction),
    WizardOrCeremony(WizardModalSubmitAction),
}

fn classify_modal_submit(
    modal_state: Option<ModalState>,
    current_model: Option<&UiModel>,
    add_device_step: AddDeviceWizardStep,
    add_device_is_complete: bool,
    add_device_has_failed: bool,
) -> Option<ModalSubmitClass> {
    match modal_state {
        Some(ModalState::AddDeviceStep1) => match add_device_step {
            AddDeviceWizardStep::Name => Some(ModalSubmitClass::WizardOrCeremony(
                WizardModalSubmitAction::StartAddDevice,
            )),
            AddDeviceWizardStep::ShareCode => Some(ModalSubmitClass::ObservedOnly(
                ObservedModalSubmitAction::AdvanceAddDeviceShare,
            )),
            AddDeviceWizardStep::Confirm if add_device_is_complete || add_device_has_failed => {
                Some(ModalSubmitClass::ObservedOnly(
                    ObservedModalSubmitAction::FinalizeAddDevice,
                ))
            }
            AddDeviceWizardStep::Confirm => Some(ModalSubmitClass::WizardOrCeremony(
                WizardModalSubmitAction::RefreshAddDeviceStatus,
            )),
        },
        Some(ModalState::CreateChannel)
            if matches!(
                current_model
                    .and_then(|model| model.create_channel_modal().map(|state| state.step)),
                Some(CreateChannelWizardStep::Threshold)
            ) =>
        {
            Some(ModalSubmitClass::WizardOrCeremony(
                WizardModalSubmitAction::CreateChannel,
            ))
        }
        Some(ModalState::GuardianSetup)
            if matches!(
                current_model
                    .and_then(|model| model.guardian_setup_modal().map(|state| state.step)),
                Some(ThresholdWizardStep::Ceremony)
            ) =>
        {
            Some(ModalSubmitClass::WizardOrCeremony(
                WizardModalSubmitAction::GuardianSetup,
            ))
        }
        Some(ModalState::MfaSetup)
            if matches!(
                current_model.and_then(|model| model.mfa_setup_modal().map(|state| state.step)),
                Some(ThresholdWizardStep::Ceremony)
            ) =>
        {
            Some(ModalSubmitClass::WizardOrCeremony(
                WizardModalSubmitAction::MfaSetup,
            ))
        }
        Some(ModalState::CreateHome) => Some(ModalSubmitClass::SimpleDispatch(
            SimpleModalSubmitAction::CreateHome,
        )),
        Some(ModalState::AcceptInvitation) => Some(ModalSubmitClass::SimpleDispatch(
            SimpleModalSubmitAction::AcceptInvitation,
        )),
        Some(ModalState::ImportDeviceEnrollmentCode) => Some(ModalSubmitClass::SimpleDispatch(
            SimpleModalSubmitAction::AcceptInvitation,
        )),
        Some(ModalState::CreateInvitation) => Some(ModalSubmitClass::SimpleDispatch(
            SimpleModalSubmitAction::CreateInvitation,
        )),
        Some(ModalState::SetChannelTopic) => Some(ModalSubmitClass::SimpleDispatch(
            SimpleModalSubmitAction::SetChannelTopic,
        )),
        Some(ModalState::EditNickname) => Some(ModalSubmitClass::SimpleDispatch(
            SimpleModalSubmitAction::EditNickname,
        )),
        Some(ModalState::RemoveContact) => Some(ModalSubmitClass::SimpleDispatch(
            SimpleModalSubmitAction::RemoveContact,
        )),
        Some(ModalState::RequestRecovery) => Some(ModalSubmitClass::SimpleDispatch(
            SimpleModalSubmitAction::RequestRecovery,
        )),
        Some(ModalState::ConfirmRemoveDevice) => Some(ModalSubmitClass::SimpleDispatch(
            SimpleModalSubmitAction::ConfirmRemoveDevice,
        )),
        Some(ModalState::AssignModerator) => Some(ModalSubmitClass::SimpleDispatch(
            SimpleModalSubmitAction::AssignModerator,
        )),
        Some(ModalState::SwitchAuthority) => Some(ModalSubmitClass::SimpleDispatch(
            SimpleModalSubmitAction::SwitchAuthority,
        )),
        Some(ModalState::AccessOverride) => Some(ModalSubmitClass::SimpleDispatch(
            SimpleModalSubmitAction::AccessOverride,
        )),
        Some(ModalState::CapabilityConfig) => Some(ModalSubmitClass::SimpleDispatch(
            SimpleModalSubmitAction::CapabilityConfig,
        )),
        _ => None,
    }
}

fn submit_observed_modal_action(
    controller: Arc<UiController>,
    action: ObservedModalSubmitAction,
    rerender: Arc<dyn Fn() + Send + Sync>,
) -> bool {
    match action {
        ObservedModalSubmitAction::AdvanceAddDeviceShare => {
            controller.advance_runtime_device_enrollment_share();
            rerender();
            true
        }
        ObservedModalSubmitAction::FinalizeAddDevice => {
            controller.complete_runtime_device_enrollment_ready();
            rerender();
            true
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn submit_wizard_modal_action(
    controller: Arc<UiController>,
    action: WizardModalSubmitAction,
    current_model: Option<UiModel>,
    add_device_ceremony_id: Option<CeremonyId>,
    modal_buffer: String,
    contacts_runtime: ContactsRuntimeView,
    settings_runtime: SettingsRuntimeView,
    rerender: Arc<dyn Fn() + Send + Sync>,
) -> bool {
    match action {
        WizardModalSubmitAction::StartAddDevice => {
            let name = modal_buffer.trim().to_string();
            if name.is_empty() {
                controller.runtime_error_toast("Device name is required");
                rerender();
                return true;
            }

            let owner = UiCeremonySubmissionOwner::submit(
                controller.clone(),
                OperationId::device_enrollment(),
                SemanticOperationKind::StartDeviceEnrollment,
            );
            let app_core = controller.app_core().clone();
            let rerender_for_start = rerender.clone();
            let invitee_authority_id =
                next_device_enrollment_invitee_authority_id(&controller, &name);
            spawn_ui(async move {
                match ceremony_workflows::start_device_enrollment_ceremony(
                    &app_core,
                    name.clone(),
                    invitee_authority_id,
                )
                .await
                {
                    Ok(start) => {
                        let status_handle = start.status_handle.clone();
                        owner.monitor_started();
                        controller.set_runtime_device_enrollment_ceremony(start.handle);
                        controller
                            .set_runtime_device_enrollment_ceremony_id(start.ceremony_id.clone());
                        controller.complete_runtime_device_enrollment_started(
                            &name,
                            &start.enrollment_code,
                        );
                        monitor_runtime_device_enrollment_ceremony(
                            controller.clone(),
                            app_core.clone(),
                            status_handle,
                            rerender_for_start.clone(),
                        );
                    }
                    Err(error) => {
                        owner.fail_with(invitation_command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                rerender_for_start();
            });
            true
        }
        WizardModalSubmitAction::RefreshAddDeviceStatus => {
            let Some(_ceremony_id) = add_device_ceremony_id else {
                controller.runtime_error_toast("No active enrollment ceremony");
                rerender();
                return true;
            };

            let app_core = controller.app_core().clone();
            let rerender_for_status = rerender.clone();
            spawn_ui(async move {
                match controller.runtime_device_enrollment_status_handle() {
                    Some(status_handle) => {
                        match ceremony_workflows::get_key_rotation_ceremony_status(
                            &app_core,
                            &status_handle,
                        )
                        .await
                        {
                            Ok(status) => controller.update_runtime_device_enrollment_status(
                                status.accepted_count,
                                status.total_count,
                                status.threshold,
                                status.is_complete,
                                status.has_failed,
                                status.error_message,
                            ),
                            Err(error) => controller.runtime_error_toast(error.to_string()),
                        }
                    }
                    None => controller.runtime_error_toast("No active enrollment ceremony handle"),
                }
                rerender_for_status();
            });
            true
        }
        WizardModalSubmitAction::CreateChannel => {
            let Some(model) = current_model else {
                return false;
            };
            let (selected_members, channel_name, channel_topic, channel_threshold) =
                match model.active_modal.as_ref() {
                    Some(ActiveModal::CreateChannel(state)) => (
                        state.selected_members.clone(),
                        state.name.clone(),
                        state.topic.clone(),
                        state.threshold,
                    ),
                    _ => (Vec::new(), String::new(), String::new(), 1),
                };
            let operation = UiLocalOperationOwner::submit(
                controller.clone(),
                OperationId::create_channel(),
                SemanticOperationKind::CreateChannel,
            );
            let app_core = controller.app_core().clone();
            let rerender_for_create = rerender.clone();
            spawn_ui(async move {
                let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
                    Ok(value) => value,
                    Err(error) => {
                        operation.fail_with(invitation_command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                        rerender_for_create();
                        return;
                    }
                };
                let members: Vec<String> = model
                    .selected_contact_index()
                    .map(|_| ())
                    .map(|_| {
                        selected_members
                            .iter()
                            .filter_map(|idx| contacts_runtime.contacts.get(*idx))
                            .map(|contact| contact.authority_id.to_string())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_else(|| {
                        selected_members
                            .iter()
                            .filter_map(|idx| contacts_runtime.contacts.get(*idx))
                            .map(|contact| contact.authority_id.to_string())
                            .collect::<Vec<_>>()
                    });

                match messaging_workflows::create_channel(
                    &app_core,
                    channel_name.trim(),
                    (!channel_topic.trim().is_empty()).then(|| channel_topic.trim().to_string()),
                    &members,
                    channel_threshold,
                    timestamp_ms,
                )
                .await
                {
                    Ok(_) => {
                        operation.succeed(None);
                        controller.complete_runtime_modal_success(format!(
                            "Created '{}'",
                            channel_name.trim()
                        ));
                    }
                    Err(error) => {
                        operation.fail_with(invitation_command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                rerender_for_create();
            });
            true
        }
        WizardModalSubmitAction::GuardianSetup => {
            let Some(model) = current_model else {
                return false;
            };
            let (selected_indices, threshold_k) = match model.active_modal.as_ref() {
                Some(ActiveModal::GuardianSetup(state)) => {
                    (state.selected_indices.clone(), state.threshold_k)
                }
                _ => (Vec::new(), 1),
            };
            let owner = UiCeremonySubmissionOwner::submit(
                controller.clone(),
                OperationId::start_guardian_ceremony(),
                SemanticOperationKind::StartGuardianCeremony,
            );
            let app_core = controller.app_core().clone();
            let monitor_app_core = app_core.clone();
            let rerender_for_guardians = rerender.clone();
            let rerender_for_monitor = rerender.clone();
            let controller_for_monitor = controller.clone();
            spawn_ui(async move {
                let ids: Vec<AuthorityId> = selected_indices
                    .iter()
                    .filter_map(|idx| contacts_runtime.contacts.get(*idx))
                    .map(|contact| contact.authority_id)
                    .collect();
                let threshold = match aura_core::types::FrostThreshold::new(u16::from(threshold_k))
                {
                    Ok(value) => value,
                    Err(error) => {
                        owner.fail_with(invitation_command_failure(format!(
                            "Invalid threshold: {error}"
                        )));
                        controller.runtime_error_toast(format!("Invalid threshold: {error}"));
                        rerender_for_guardians();
                        return;
                    }
                };

                match ceremony_workflows::start_guardian_ceremony(
                    &app_core,
                    threshold,
                    ids.len() as u16,
                    ids,
                )
                .await
                {
                    Ok(ceremony_handle) => {
                        controller.set_runtime_guardian_ceremony_id(
                            ceremony_handle.ceremony_id().clone(),
                        );
                        owner.monitor_started();
                        monitor_runtime_key_rotation_ceremony(
                            controller_for_monitor,
                            monitor_app_core,
                            ceremony_handle.status_handle(),
                            "Guardian ceremony",
                            rerender_for_monitor,
                        );
                        controller.complete_runtime_modal_success("Guardian ceremony started");
                    }
                    Err(error) => {
                        owner.fail_with(invitation_command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                rerender_for_guardians();
            });
            true
        }
        WizardModalSubmitAction::MfaSetup => {
            let Some(model) = current_model else {
                return false;
            };
            let owner = UiCeremonySubmissionOwner::submit(
                controller.clone(),
                OperationId::start_multifactor_ceremony(),
                SemanticOperationKind::StartMultifactorCeremony,
            );
            let app_core = controller.app_core().clone();
            let monitor_app_core = app_core.clone();
            let rerender_for_mfa = rerender.clone();
            let rerender_for_monitor = rerender.clone();
            let controller_for_monitor = controller.clone();
            spawn_ui(async move {
                let Some(mfa_state) = model.mfa_setup_modal() else {
                    owner.fail_with(invitation_command_failure("Missing MFA setup modal state"));
                    rerender_for_mfa();
                    return;
                };
                let device_ids: Vec<String> = mfa_state
                    .selected_indices
                    .iter()
                    .filter_map(|idx| settings_runtime.devices.get(*idx))
                    .map(|device| device.id.clone())
                    .collect();
                let threshold =
                    match aura_core::types::FrostThreshold::new(u16::from(mfa_state.threshold_k)) {
                        Ok(value) => value,
                        Err(error) => {
                            owner.fail_with(invitation_command_failure(format!(
                                "Invalid threshold: {error}"
                            )));
                            controller.runtime_error_toast(format!("Invalid threshold: {error}"));
                            rerender_for_mfa();
                            return;
                        }
                    };

                match ceremony_workflows::start_device_threshold_ceremony(
                    &app_core,
                    threshold,
                    device_ids.len() as u16,
                    device_ids,
                )
                .await
                {
                    Ok(ceremony_handle) => {
                        owner.monitor_started();
                        monitor_runtime_key_rotation_ceremony(
                            controller_for_monitor,
                            monitor_app_core,
                            ceremony_handle.status_handle(),
                            "Multifactor ceremony",
                            rerender_for_monitor,
                        );
                        controller.complete_runtime_modal_success("Multifactor ceremony started");
                    }
                    Err(error) => {
                        owner.fail_with(invitation_command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                rerender_for_mfa();
            });
            true
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn submit_simple_modal_action(
    controller: Arc<UiController>,
    action: SimpleModalSubmitAction,
    current_model: Option<UiModel>,
    modal_text_value: String,
    neighborhood_runtime: NeighborhoodRuntimeView,
    chat_runtime: ChatRuntimeView,
    contacts_runtime: ContactsRuntimeView,
    settings_runtime: SettingsRuntimeView,
    selected_home_id: Option<String>,
    selected_member_key: Option<NeighborhoodMemberSelectionKey>,
    rerender: Arc<dyn Fn() + Send + Sync>,
) -> bool {
    match action {
        SimpleModalSubmitAction::CreateHome => {
            let name = modal_text_value.trim().to_string();
            if name.is_empty() {
                controller.runtime_error_toast("Home name is required");
                rerender();
                return true;
            }

            let operation = UiLocalOperationOwner::submit(
                controller.clone(),
                OperationId::create_home(),
                SemanticOperationKind::CreateHome,
            );
            let app_core = controller.app_core().clone();
            let rerender_for_create = rerender.clone();
            spawn_ui(async move {
                match context_workflows::create_home(&app_core, Some(name.clone()), None).await {
                    Ok(_) => {
                        operation.succeed(None);
                        controller.complete_runtime_home_created(&name);
                    }
                    Err(error) => {
                        operation.fail_with(invitation_command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                rerender_for_create();
            });
            true
        }
        SimpleModalSubmitAction::AcceptInvitation => {
            let code = modal_text_value.trim().to_string();
            if code.is_empty() {
                controller.runtime_error_toast("Invitation code is required");
                rerender();
                return true;
            }

            let submit_log = format!("accept_invitation submit start code_len={}", code.len());
            controller.push_log(&submit_log);
            harness_log(&submit_log);
            let device_import_modal = current_model.as_ref().is_some_and(|model| {
                matches!(
                    model.active_modal.as_ref(),
                    Some(ActiveModal::ImportDeviceEnrollmentCode(_))
                )
            });
            let operation_id = if device_import_modal {
                OperationId::device_enrollment()
            } else {
                OperationId::invitation_accept()
            };
            let base_kind = if device_import_modal {
                SemanticOperationKind::ImportDeviceEnrollmentCode
            } else {
                SemanticOperationKind::AcceptContactInvitation
            };
            let operation =
                UiWorkflowHandoffOwner::submit(controller.clone(), operation_id, base_kind);
            let app_core = controller.app_core().clone();
            let controller_for_import = controller;
            let rerender_for_import = rerender.clone();
            spawn_ui(async move {
                controller_for_import.push_log("accept_invitation import_details start");
                harness_log("accept_invitation import_details start");
                match invitation_workflows::import_invitation_details(&app_core, &code).await {
                    Ok(invitation) => {
                        let invitation_info = invitation.info().clone();
                        let invitation_kind = match &invitation_info.invitation_type {
                            InvitationBridgeType::DeviceEnrollment { .. } => "device_enrollment",
                            InvitationBridgeType::Contact { .. } => "contact",
                            InvitationBridgeType::Guardian { .. } => "guardian",
                            InvitationBridgeType::Channel { .. } => "channel",
                        };
                        let import_ok_log = format!(
                            "accept_invitation import_details ok invitation_id={} kind={}",
                            invitation.invitation_id(),
                            invitation_kind
                        );
                        controller_for_import.push_log(&import_ok_log);
                        harness_log(&import_ok_log);
                        controller_for_import.push_log("accept_invitation runtime_accept start");
                        harness_log("accept_invitation runtime_accept start");
                        let workflow_instance_id = operation.workflow_instance_id();
                        let transfer = operation
                            .handoff_to_app_workflow(UiOperationTransferScope::InvitationImport);
                        match transfer
                            .run_workflow(
                                controller_for_import.clone(),
                                "accept_imported_invitation",
                                invitation_workflows::handoff::accept_imported_invitation(
                                    &app_core,
                                    invitation_workflows::handoff::AcceptImportedInvitationRequest {
                                        invitation,
                                        operation_instance_id: workflow_instance_id,
                                    },
                                ),
                            )
                            .await
                        {
                            Ok(()) => {
                                controller_for_import
                                    .push_log("accept_invitation runtime_accept ok");
                                harness_log("accept_invitation runtime_accept ok");
                                if matches!(
                                    &invitation_info.invitation_type,
                                    InvitationBridgeType::DeviceEnrollment { .. }
                                ) {
                                    controller_for_import.complete_runtime_modal_success(
                                        "Device enrollment complete",
                                    );
                                    controller_for_import
                                        .push_log("accept_invitation complete device_enrollment");
                                    harness_log("accept_invitation complete device_enrollment");
                                } else {
                                    if let InvitationBridgeType::Contact { nickname } =
                                        &invitation_info.invitation_type
                                    {
                                        let display_name = nickname
                                            .clone()
                                            .filter(|value| !value.trim().is_empty())
                                            .unwrap_or_else(|| {
                                                invitation_info.sender_id.to_string()
                                            });
                                        controller_for_import
                                            .complete_runtime_contact_invitation_acceptance(
                                                invitation_info.sender_id,
                                                display_name,
                                            );
                                        controller_for_import
                                            .push_log("accept_invitation complete generic");
                                        harness_log("accept_invitation complete generic");
                                        return;
                                    }
                                    match &invitation_info.invitation_type {
                                        InvitationBridgeType::Guardian { .. }
                                        | InvitationBridgeType::Channel { .. } => {}
                                        InvitationBridgeType::DeviceEnrollment { .. }
                                        | InvitationBridgeType::Contact { .. } => {}
                                    }
                                    controller_for_import
                                        .complete_runtime_modal_success("Invitation accepted");
                                    controller_for_import
                                        .push_log("accept_invitation complete generic");
                                    harness_log("accept_invitation complete generic");
                                }
                            }
                            Err(SubmittedOperationWorkflowError::Workflow(error)) => {
                                let error_log =
                                    format!("accept_invitation runtime_accept error={error}");
                                controller_for_import.push_log(&error_log);
                                harness_log(&error_log);
                                controller_for_import.runtime_error_toast(error.to_string());
                            }
                            Err(
                                SubmittedOperationWorkflowError::Protocol(detail)
                                | SubmittedOperationWorkflowError::Panicked(detail),
                            ) => {
                                controller_for_import.runtime_error_toast(detail);
                            }
                        }
                    }
                    Err(error) => {
                        let error_log = format!("accept_invitation import_details error={error}");
                        controller_for_import.push_log(&error_log);
                        harness_log(&error_log);
                        operation.fail_with(invitation_command_failure(error.to_string()));
                        controller_for_import.runtime_error_toast(error.to_string());
                    }
                }
                rerender_for_import();
            });
            true
        }
        SimpleModalSubmitAction::CreateInvitation => {
            let Some(create_state) = current_model
                .as_ref()
                .and_then(UiModel::create_invitation_modal)
                .cloned()
            else {
                controller.runtime_error_toast("Invitation modal state is unavailable");
                rerender();
                return true;
            };
            let receiver = create_state.receiver_id.trim().to_string();
            if receiver.is_empty() {
                controller.runtime_error_toast("Receiver authority id is required");
                rerender();
                return true;
            }
            let message = (!create_state.message.trim().is_empty()).then_some(create_state.message);
            let ttl_ms = Some(create_state.ttl_hours.max(1).saturating_mul(60 * 60 * 1000));

            let app_core = controller.app_core().clone();
            spawn_ui(async move {
                tracing::info!("create_invitation submit start");
                let receiver_id = match receiver.parse::<AuthorityId>() {
                    Ok(value) => value,
                    Err(error) => {
                        tracing::warn!(error = %error, "create_invitation invalid receiver");
                        controller.runtime_error_toast(format!("Invalid authority id: {error}"));
                        rerender();
                        return;
                    }
                };

                let operation = UiWorkflowHandoffOwner::submit(
                    controller.clone(),
                    OperationId::invitation_create(),
                    SemanticOperationKind::CreateContactInvitation,
                );
                let workflow_instance_id = operation.workflow_instance_id();
                let transfer =
                    operation.handoff_to_app_workflow(UiOperationTransferScope::CreateInvitation);
                match transfer
                    .run_workflow(
                        controller.clone(),
                        "create_contact_invitation",
                        invitation_workflows::handoff::create_contact_invitation(
                            &app_core,
                            invitation_workflows::handoff::CreateContactInvitationRequest {
                                receiver: receiver_id,
                                nickname: None,
                                message,
                                ttl_ms,
                                operation_instance_id: workflow_instance_id,
                            },
                        ),
                    )
                    .await
                {
                    Ok(code) => {
                        tracing::info!("create_invitation export_invitation ok");
                        controller.write_clipboard(&code);
                        controller.remember_invitation_code(&code);
                        tracing::info!("create_invitation write_clipboard ok");
                        controller.push_runtime_fact(RuntimeFact::InvitationCodeReady {
                            receiver_authority_id: Some(receiver_id.to_string()),
                            source_operation: OperationId::invitation_create(),
                            code: Some(code),
                        });
                        controller
                            .complete_runtime_modal_success("Invitation code copied to clipboard");
                        tracing::info!("create_invitation operation succeeded");
                        tracing::info!("create_invitation complete");
                    }
                    Err(SubmittedOperationWorkflowError::Workflow(error)) => {
                        tracing::warn!(error = %error, "create_invitation workflow failed");
                        controller.runtime_error_toast(error.to_string());
                    }
                    Err(
                        SubmittedOperationWorkflowError::Protocol(detail)
                        | SubmittedOperationWorkflowError::Panicked(detail),
                    ) => {
                        tracing::warn!("create_invitation workflow panicked");
                        controller.runtime_error_toast(detail);
                    }
                }
                tracing::info!("create_invitation rerender");
            });
            true
        }
        SimpleModalSubmitAction::SetChannelTopic => {
            let channel_name = chat_runtime.active_channel.trim().to_string();
            let topic = modal_text_value.trim().to_string();
            if channel_name.is_empty() {
                controller.runtime_error_toast("Select a channel first");
                rerender();
                return true;
            }

            let operation = UiLocalOperationOwner::submit(
                controller.clone(),
                OperationId::set_channel_topic(),
                SemanticOperationKind::SetChannelTopic,
            );
            let app_core = controller.app_core().clone();
            let rerender_for_topic = rerender.clone();
            spawn_ui(async move {
                let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
                    Ok(value) => value,
                    Err(error) => {
                        operation.fail_with(invitation_command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                        rerender_for_topic();
                        return;
                    }
                };
                match messaging_workflows::set_topic_by_name(
                    &app_core,
                    &channel_name,
                    &topic,
                    timestamp_ms,
                )
                .await
                {
                    Ok(()) => {
                        operation.succeed(None);
                        controller.complete_runtime_modal_success("Topic updated");
                    }
                    Err(error) => {
                        operation.fail_with(invitation_command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                rerender_for_topic();
            });
            true
        }
        SimpleModalSubmitAction::EditNickname => {
            let value = modal_text_value.trim().to_string();
            if value.is_empty() {
                controller.runtime_error_toast("Nickname is required");
                rerender();
                return true;
            }

            let app_core = controller.app_core().clone();
            let rerender_for_nickname = rerender.clone();
            let selected_contact = current_model
                .as_ref()
                .and_then(|model| selected_contact_for_modal(&contacts_runtime, model));
            let is_settings_screen = current_model
                .as_ref()
                .map(|model| matches!(model.screen, ScreenId::Settings))
                .unwrap_or(false);
            if !is_settings_screen && selected_contact.is_none() {
                controller.runtime_error_toast("No contact selected");
                rerender();
                return true;
            }
            let operation = UiLocalOperationOwner::submit(
                controller.clone(),
                if is_settings_screen {
                    OperationId::update_nickname_suggestion()
                } else {
                    OperationId::update_contact_nickname()
                },
                if is_settings_screen {
                    SemanticOperationKind::UpdateNicknameSuggestion
                } else {
                    SemanticOperationKind::UpdateContactNickname
                },
            );
            spawn_ui(async move {
                let result = if is_settings_screen {
                    settings_workflows::update_nickname(&app_core, value.clone()).await
                } else if let Some(contact) = selected_contact {
                    let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
                        Ok(value) => value,
                        Err(error) => {
                            operation.fail_with(invitation_command_failure(error.to_string()));
                            controller.runtime_error_toast(error.to_string());
                            rerender_for_nickname();
                            return;
                        }
                    };
                    let authority_id = contact.authority_id.to_string();
                    contacts_workflows::update_contact_nickname(
                        &app_core,
                        &authority_id,
                        &value,
                        timestamp_ms,
                    )
                    .await
                } else {
                    Err(aura_core::AuraError::not_found("No contact selected"))
                };

                match result {
                    Ok(()) => {
                        operation.succeed(None);
                        controller.complete_runtime_modal_success("Nickname updated");
                    }
                    Err(error) => {
                        operation.fail_with(invitation_command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                rerender_for_nickname();
            });
            true
        }
        SimpleModalSubmitAction::RemoveContact => {
            let Some(contact) = current_model
                .as_ref()
                .and_then(|model| selected_contact_for_modal(&contacts_runtime, model))
            else {
                controller.runtime_error_toast("Select a contact first");
                rerender();
                return true;
            };
            let operation = UiLocalOperationOwner::submit(
                controller.clone(),
                OperationId::remove_contact(),
                SemanticOperationKind::RemoveContact,
            );
            let app_core = controller.app_core().clone();
            let rerender_for_remove = rerender.clone();
            spawn_ui(async move {
                let authority_id = contact.authority_id.to_string();
                let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
                    Ok(value) => value,
                    Err(error) => {
                        operation.fail_with(invitation_command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                        rerender_for_remove();
                        return;
                    }
                };
                match contacts_workflows::remove_contact(&app_core, &authority_id, timestamp_ms)
                    .await
                {
                    Ok(()) => {
                        operation.succeed(None);
                        controller.complete_runtime_modal_success("Contact removed");
                    }
                    Err(error) => {
                        operation.fail_with(invitation_command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                rerender_for_remove();
            });
            true
        }
        SimpleModalSubmitAction::RequestRecovery => {
            let operation = UiLocalOperationOwner::submit(
                controller.clone(),
                OperationId::start_recovery(),
                SemanticOperationKind::StartRecovery,
            );
            let app_core = controller.app_core().clone();
            let rerender_for_recovery = rerender.clone();
            spawn_ui(async move {
                match recovery_workflows::start_recovery_from_state(&app_core).await {
                    Ok(_) => {
                        operation.succeed(None);
                        controller.complete_runtime_modal_success("Recovery process started");
                    }
                    Err(error) => {
                        operation.fail_with(invitation_command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                rerender_for_recovery();
            });
            true
        }
        SimpleModalSubmitAction::ConfirmRemoveDevice => {
            let Some(contact_model) = current_model.as_ref() else {
                return false;
            };
            let Some(device) = removable_device_for_modal(&settings_runtime, contact_model) else {
                controller.runtime_error_toast("No removable device selected");
                rerender();
                return true;
            };
            let owner = UiCeremonySubmissionOwner::submit(
                controller.clone(),
                OperationId::remove_device(),
                SemanticOperationKind::RemoveDevice,
            );
            let app_core = controller.app_core().clone();
            let rerender_for_remove = rerender.clone();
            spawn_ui(async move {
                match ceremony_workflows::start_device_removal_ceremony(
                    &app_core,
                    device.id.clone(),
                )
                .await
                {
                    Ok(ceremony_handle) => {
                        owner.monitor_started();
                        let status_handle = ceremony_handle.status_handle();
                        match ceremony_workflows::get_key_rotation_ceremony_status(
                            &app_core,
                            &status_handle,
                        )
                        .await
                        {
                            Ok(status) if status.is_complete => {
                                let _ =
                                    settings_workflows::refresh_settings_from_runtime(&app_core)
                                        .await;
                                controller
                                    .complete_runtime_modal_success("Device removal complete");
                            }
                            Ok(_) => {
                                controller.complete_runtime_modal_success(
                                    "Device removal ceremony started",
                                );
                            }
                            Err(error) => controller.runtime_error_toast(error.to_string()),
                        }
                    }
                    Err(error) => {
                        owner.fail_with(invitation_command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                rerender_for_remove();
            });
            true
        }
        SimpleModalSubmitAction::AssignModerator => {
            let Some(selected_home_id) = selected_home_id else {
                controller.runtime_error_toast("Select an entered home first");
                rerender();
                return true;
            };
            let Some(member) = selected_member_key
                .as_ref()
                .and_then(|selected_key| {
                    neighborhood_runtime
                        .members
                        .iter()
                        .find(|member| neighborhood_member_selection_key(member) == *selected_key)
                })
                .cloned()
            else {
                controller.runtime_error_toast("Select a member first");
                rerender();
                return true;
            };
            if member.authority_id.is_empty() {
                controller.runtime_error_toast("Selected member cannot be resolved");
                rerender();
                return true;
            }

            let operation = UiLocalOperationOwner::submit(
                controller.clone(),
                if member.is_moderator {
                    OperationId::revoke_moderator()
                } else {
                    OperationId::grant_moderator()
                },
                if member.is_moderator {
                    SemanticOperationKind::RevokeModerator
                } else {
                    SemanticOperationKind::GrantModerator
                },
            );
            let app_core = controller.app_core().clone();
            let rerender_for_moderator = rerender.clone();
            spawn_ui(async move {
                let result = if member.is_moderator {
                    moderator_workflows::revoke_moderator(
                        &app_core,
                        Some(selected_home_id.as_str()),
                        &member.authority_id,
                    )
                    .await
                } else {
                    moderator_workflows::grant_moderator(
                        &app_core,
                        Some(selected_home_id.as_str()),
                        &member.authority_id,
                    )
                    .await
                };

                match result {
                    Ok(_) => {
                        operation.succeed(None);
                        controller.complete_runtime_modal_success(if member.is_moderator {
                            format!("Moderator revoked for {}", member.name)
                        } else {
                            format!("Moderator granted for {}", member.name)
                        });
                    }
                    Err(error) => {
                        operation.fail_with(invitation_command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                rerender_for_moderator();
            });
            true
        }
        SimpleModalSubmitAction::SwitchAuthority => {
            let Some(model) = current_model else {
                return false;
            };
            let Some(authority) = settings_runtime
                .authorities
                .get(model.selected_authority_index().unwrap_or_default())
                .cloned()
            else {
                controller.runtime_error_toast("Select an authority first");
                rerender();
                return true;
            };

            if authority.is_current {
                controller.complete_runtime_modal_success("Already using that authority");
                rerender();
                return true;
            }

            if !controller.request_authority_switch(authority.id) {
                controller.runtime_error_toast("Authority switching is not available");
                rerender();
                return true;
            }

            true
        }
        SimpleModalSubmitAction::AccessOverride => {
            let Some(model) = current_model else {
                return false;
            };
            let Some(selected_home_id) = selected_home_id else {
                controller.runtime_error_toast("Select an entered home first");
                rerender();
                return true;
            };
            let Some(contact) = contacts_runtime
                .contacts
                .get(model.selected_contact_index().unwrap_or_default())
                .cloned()
            else {
                controller.runtime_error_toast("Select a contact first");
                rerender();
                return true;
            };
            let authority_id = contact.authority_id;
            let selected_level = match model.active_modal.as_ref() {
                Some(ActiveModal::AccessOverride(state)) => state.level,
                _ => AccessOverrideLevel::Limited,
            };
            let access_level = match selected_level {
                AccessOverrideLevel::Partial => AccessLevel::Partial,
                AccessOverrideLevel::Limited => AccessLevel::Limited,
            };

            let app_core = controller.app_core().clone();
            let rerender_for_override = rerender.clone();
            spawn_ui(async move {
                match access_workflows::set_access_override(
                    &app_core,
                    Some(selected_home_id.as_str()),
                    authority_id,
                    access_level,
                )
                .await
                {
                    Ok(()) => controller.complete_runtime_modal_success(format!(
                        "Access override set for {} ({})",
                        contact.name,
                        match access_level {
                            AccessLevel::Partial => "Partial",
                            AccessLevel::Limited => "Limited",
                            AccessLevel::Full => "Full",
                        }
                    )),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_override();
            });
            true
        }
        SimpleModalSubmitAction::CapabilityConfig => {
            let Some(model) = current_model else {
                return false;
            };
            let Some(selected_home_id) = selected_home_id else {
                controller.runtime_error_toast("Select an entered home first");
                rerender();
                return true;
            };

            let (full_caps, partial_caps, limited_caps) = match model.active_modal.as_ref() {
                Some(ActiveModal::CapabilityConfig(state)) => (
                    state.full_caps.clone(),
                    state.partial_caps.clone(),
                    state.limited_caps.clone(),
                ),
                _ => (
                    DEFAULT_CAPABILITY_FULL.to_string(),
                    DEFAULT_CAPABILITY_PARTIAL.to_string(),
                    DEFAULT_CAPABILITY_LIMITED.to_string(),
                ),
            };

            let app_core = controller.app_core().clone();
            let rerender_for_caps = rerender.clone();
            spawn_ui(async move {
                match access_workflows::configure_home_capabilities(
                    &app_core,
                    Some(selected_home_id.as_str()),
                    &full_caps,
                    &partial_caps,
                    &limited_caps,
                )
                .await
                {
                    Ok(()) => controller.complete_runtime_modal_success("Capability config saved"),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_caps();
            });
            true
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_runtime_modal_action(
    controller: Arc<UiController>,
    modal_state: Option<ModalState>,
    add_device_step: AddDeviceWizardStep,
    add_device_ceremony_id: Option<CeremonyId>,
    add_device_is_complete: bool,
    add_device_has_failed: bool,
    modal_buffer: String,
    neighborhood_runtime: NeighborhoodRuntimeView,
    chat_runtime: ChatRuntimeView,
    contacts_runtime: ContactsRuntimeView,
    settings_runtime: SettingsRuntimeView,
    selected_home_id: Option<String>,
    selected_member_key: Option<NeighborhoodMemberSelectionKey>,
    rerender: Arc<dyn Fn() + Send + Sync>,
) -> bool {
    let current_model = controller.ui_model();
    let modal_text_value = current_model
        .as_ref()
        .and_then(|model| model.modal_text_value())
        .unwrap_or_else(|| modal_buffer.clone());

    match classify_modal_submit(
        modal_state,
        current_model.as_ref(),
        add_device_step,
        add_device_is_complete,
        add_device_has_failed,
    ) {
        Some(ModalSubmitClass::ObservedOnly(action)) => {
            submit_observed_modal_action(controller, action, rerender)
        }
        Some(ModalSubmitClass::SimpleDispatch(action)) => submit_simple_modal_action(
            controller,
            action,
            current_model,
            modal_text_value,
            neighborhood_runtime,
            chat_runtime,
            contacts_runtime,
            settings_runtime,
            selected_home_id,
            selected_member_key,
            rerender,
        ),
        Some(ModalSubmitClass::WizardOrCeremony(action)) => submit_wizard_modal_action(
            controller,
            action,
            current_model,
            add_device_ceremony_id,
            modal_buffer,
            contacts_runtime,
            settings_runtime,
            rerender,
        ),
        None => false,
    }
}

pub(crate) fn cancel_runtime_modal_action(
    controller: Arc<UiController>,
    modal_state: Option<ModalState>,
    add_device_step: AddDeviceWizardStep,
    add_device_ceremony_id: Option<CeremonyId>,
    add_device_is_complete: bool,
    add_device_has_failed: bool,
    rerender: Arc<dyn Fn() + Send + Sync>,
) -> bool {
    match modal_state {
        Some(ModalState::AddDeviceStep1) => {
            if matches!(add_device_step, AddDeviceWizardStep::Name)
                || add_device_is_complete
                || add_device_has_failed
            {
                return false;
            }

            let Some(_ceremony_id) = add_device_ceremony_id else {
                return false;
            };

            let app_core = controller.app_core().clone();
            let rerender_for_cancel = rerender.clone();
            let owner = UiCeremonySubmissionOwner::submit(
                controller.clone(),
                OperationId::cancel_key_rotation_ceremony(),
                SemanticOperationKind::CancelKeyRotationCeremony,
            );
            spawn_ui(async move {
                match controller.take_runtime_device_enrollment_ceremony() {
                    Some(handle) => {
                        match ceremony_workflows::cancel_key_rotation_ceremony(&app_core, handle)
                            .await
                        {
                            Ok(()) => {
                                owner.cancel();
                                controller
                                    .complete_runtime_modal_success("Device enrollment canceled");
                                controller.clear_runtime_device_enrollment_ceremony();
                            }
                            Err(error) => {
                                owner.fail_with(invitation_command_failure(error.to_string()));
                                controller.runtime_error_toast(error.to_string());
                            }
                        }
                    }
                    None => {
                        owner.fail_with(invitation_command_failure(
                            "No active enrollment ceremony handle",
                        ));
                        controller.runtime_error_toast("No active enrollment ceremony handle");
                    }
                }
                rerender_for_cancel();
            });
            true
        }
        Some(ModalState::GuardianSetup) => {
            let guardian_ceremony_id = controller.ui_model().and_then(|model| {
                model
                    .guardian_setup_modal()
                    .and_then(|state| state.ceremony_id.clone())
            });
            let Some(ceremony_id) = guardian_ceremony_id else {
                return false;
            };

            let app_core = controller.app_core().clone();
            let rerender_for_cancel = rerender.clone();
            let owner = UiCeremonySubmissionOwner::submit(
                controller.clone(),
                OperationId::cancel_guardian_ceremony(),
                SemanticOperationKind::CancelGuardianCeremony,
            );
            spawn_ui(async move {
                match ceremony_workflows::cancel_key_rotation_ceremony_by_id(&app_core, ceremony_id)
                    .await
                {
                    Ok(()) => {
                        owner.cancel();
                        controller.complete_runtime_modal_success("Guardian ceremony canceled");
                        controller.clear_runtime_guardian_ceremony_id();
                    }
                    Err(error) => {
                        owner.fail_with(invitation_command_failure(error.to_string()));
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                rerender_for_cancel();
            });
            true
        }
        _ => false,
    }
}
