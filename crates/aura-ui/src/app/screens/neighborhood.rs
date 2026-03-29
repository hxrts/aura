use super::*;
use crate::semantic_lifecycle::UiLocalOperationOwner;
use aura_app::ui_contract::{
    OperationId, SemanticFailureCode, SemanticFailureDomain, SemanticOperationError,
    SemanticOperationKind,
};

#[allow(non_snake_case)]
pub(super) fn NeighborhoodScreen(
    model: &UiModel,
    runtime: &NeighborhoodRuntimeView,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
) -> Element {
    let is_detail = matches!(model.neighborhood_mode, NeighborhoodMode::Detail);
    let selected_home = model
        .selected_home_name()
        .map(str::to_string)
        .or_else(|| {
            if !runtime.active_home_name.is_empty() {
                Some(runtime.active_home_name.clone())
            } else if !runtime.neighborhood_name.is_empty() {
                Some(runtime.neighborhood_name.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "Neighborhood".to_string());
    let show_detail_lists = is_detail && matches!(model.access_depth, AccessDepth::Full);
    let mut home_rows = runtime.homes.clone();
    if home_rows.is_empty() {
        if let Some(home) = model.selected_home.as_ref() {
            home_rows.push(NeighborhoodRuntimeHome {
                id: home.id.clone(),
                name: home.name.clone(),
                member_count: None,
                can_enter: true,
                is_local: false,
            });
        }
    }
    let should_materialize_selected_home =
        model.selected_home.is_some() || !runtime.homes.is_empty();
    if should_materialize_selected_home && !home_rows.iter().any(|home| home.name == selected_home)
    {
        home_rows.push(NeighborhoodRuntimeHome {
            id: model
                .selected_home_id()
                .map(str::to_string)
                .unwrap_or_else(|| {
                    if selected_home == "Neighborhood" {
                        model.authority_id.clone()
                    } else {
                        format!("home-{}", selected_home.to_lowercase().replace(' ', "-"))
                    }
                }),
            name: selected_home.clone(),
            member_count: None,
            can_enter: true,
            is_local: selected_home == "Neighborhood",
        });
    }
    home_rows.sort_by(|left, right| {
        right
            .is_local
            .cmp(&left.is_local)
            .then_with(|| left.name.cmp(&right.name))
    });
    home_rows.dedup_by(|left, right| left.name == right.name);

    let display_members = if !runtime.members.is_empty() {
        runtime.members.clone()
    } else {
        let mut members = vec![NeighborhoodRuntimeMember {
            authority_id: model.authority_id.clone(),
            name: model.profile_nickname.clone(),
            role_label: "Member".to_string(),
            is_self: true,
            is_online: true,
            is_moderator: false,
        }];
        members.extend(
            model
                .contacts
                .iter()
                .map(|contact| NeighborhoodRuntimeMember {
                    authority_id: String::new(),
                    name: contact.name.clone(),
                    role_label: "Participant".to_string(),
                    is_self: false,
                    is_online: false,
                    is_moderator: false,
                }),
        );
        members
    };
    let member_count = display_members.len();
    let selected_member_index = model
        .selected_neighborhood_member_key
        .as_ref()
        .and_then(|selected| {
            display_members
                .iter()
                .position(|member| neighborhood_member_selection_key(member) == *selected)
        })
        .unwrap_or(0)
        .min(display_members.len().saturating_sub(1));
    let selected_runtime_member = display_members.get(selected_member_index).cloned();

    let selected_channel_id = model.selected_channel_id().unwrap_or("none").to_string();
    let display_channels = if !runtime.channels.is_empty() {
        let has_selected = runtime
            .channels
            .iter()
            .any(|channel| channel.id == selected_channel_id);
        runtime
            .channels
            .iter()
            .enumerate()
            .map(|(idx, channel)| {
                let is_selected = if has_selected {
                    channel.id == selected_channel_id
                } else {
                    idx == 0
                };
                (
                    channel.id.clone(),
                    channel.name.clone(),
                    channel.topic.clone(),
                    is_selected,
                )
            })
            .collect::<Vec<_>>()
    } else {
        model
            .channels
            .iter()
            .map(|channel| {
                (
                    channel.id.clone(),
                    channel.name.clone(),
                    channel.topic.clone(),
                    channel.selected,
                )
            })
            .collect::<Vec<_>>()
    };
    let selected_channel_name = display_channels
        .iter()
        .find(|(_, _, _, selected)| *selected)
        .map(|(_, name, _, _)| name.clone())
        .unwrap_or_else(|| "none".to_string());
    let social_mode_label = if is_detail { "Entered" } else { "Browsing" };
    let selected_home_id = home_rows
        .iter()
        .find(|home| home.name == selected_home)
        .map(|home| home.id.clone())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| {
            if selected_home == "Neighborhood" {
                model.authority_id.clone()
            } else if !runtime.active_home_id.is_empty()
                && selected_home == runtime.active_home_name
            {
                runtime.active_home_id.clone()
            } else {
                format!("home-{}", selected_home.to_lowercase().replace(' ', "-"))
            }
        });
    let display_neighborhood_name = if !runtime.neighborhood_name.is_empty() {
        runtime.neighborhood_name.clone()
    } else {
        "Neighborhood".to_string()
    };
    let selected_runtime_home = runtime.homes.iter().find(|home| home.name == selected_home);
    let enter_target_home_id = selected_runtime_home
        .map(|home| home.id.clone())
        .or_else(|| {
            if !runtime.active_home_id.is_empty() && runtime.active_home_name == selected_home {
                Some(runtime.active_home_id.clone())
            } else {
                None
            }
        });
    let can_enter_selected_home = selected_runtime_home
        .map(|home| home.can_enter)
        .unwrap_or_else(|| enter_target_home_id.is_some());
    let strongest_access_label = if can_enter_selected_home {
        "Full"
    } else {
        "Partial"
    };
    let strongest_access_tone = if can_enter_selected_home {
        PillTone::Success
    } else {
        PillTone::Info
    };
    let detail_back_controller = controller.clone();
    let detail_moderator_controller = controller.clone();
    let detail_access_override_controller = controller.clone();
    let detail_capability_controller = controller.clone();
    let map_enter_controller = controller.clone();
    let map_new_home_controller = controller.clone();
    let map_accept_invitation_controller = controller.clone();

    rsx! {
        div {
            class: "grid w-full gap-3 lg:grid-cols-12 lg:h-full lg:min-h-0 lg:[grid-template-rows:minmax(0,1fr)]",
            UiCard {
                title: if is_detail { "Home".to_string() } else { "Map".to_string() },
                subtitle: Some(if is_detail {
                    format!("Entry access: {strongest_access_label}")
                } else {
                    "Explore your neighboring network".to_string()
                }),
                extra_class: Some("lg:col-span-4".to_string()),
                if is_detail {
                    UiCardBody {
                        extra_class: Some("gap-3".to_string()),
                        div {
                            class: "rounded-sm bg-background/60 px-3 py-3",
                            p { class: "m-0 text-xs font-sans font-semibold uppercase tracking-[0.08em] text-muted-foreground", "Home" }
                            p { class: "m-0 mt-1 text-sm text-foreground", "Name: {selected_home}" }
                            p {
                                class: "m-0 mt-1 text-xs text-muted-foreground",
                                "Members/Participants: {member_count} • Entry access: {strongest_access_label} • Mode: {social_mode_label}"
                            }
                        }
                        if show_detail_lists {
                            div {
                                class: "grid flex-1 gap-4 lg:min-h-0 md:grid-cols-2",
                                div {
                                    class: "flex lg:min-h-0 min-w-0 flex-col overflow-hidden rounded-sm bg-background/60 px-3 py-3",
                                    p { class: "m-0 text-xs font-sans font-semibold uppercase tracking-[0.08em] text-muted-foreground", "Channels" }
                                    div {
                                        class: "mt-3 flex-1 lg:min-h-0 min-w-0 overflow-y-auto pr-1",
                                        if display_channels.is_empty() {
                                            p { class: "m-0 text-sm text-muted-foreground", "No channels" }
                                        } else {
                                            div { class: "aura-list space-y-2 min-w-0",
                                                for (channel_id, channel_name, channel_topic, is_selected) in &display_channels {
                                                    button {
                                                        r#type: "button",
                                                        id: list_item_dom_id(ListId::Channels, channel_id),
                                                        class: "block w-full min-w-0 text-left",
                                                        onclick: {
                                                            let controller = controller.clone();
                                                            let channel_id = channel_id.clone();
                                                            move |_| {
                                                                controller.select_channel_by_id(&channel_id);
                                                                render_tick.set(render_tick() + 1);
                                                            }
                                                        },
                                                        UiListItem {
                                                            label: format!("# {}", channel_name),
                                                            secondary: Some(if channel_topic.is_empty() {
                                                                "\u{00A0}".to_string()
                                                            } else {
                                                                channel_topic.clone()
                                                            }),
                                                            active: *is_selected,
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                div {
                                    class: "flex lg:min-h-0 flex-col rounded-sm bg-background/60 px-3 py-3",
                                    p { class: "m-0 text-xs font-sans font-semibold uppercase tracking-[0.08em] text-muted-foreground", "Members & Participants" }
                                    div {
                                        class: "mt-3 flex-1 lg:min-h-0 overflow-y-auto pr-1",
                                        div { class: "aura-list space-y-2",
                                            for (idx, member) in display_members.iter().enumerate() {
                                                button {
                                                    r#type: "button",
                                                    id: list_item_dom_id(
                                                        ListId::NeighborhoodMembers,
                                                        &neighborhood_member_selection_key(member).0,
                                                    ),
                                                    class: "block w-full text-left",
                                                    onclick: {
                                                        let controller = controller.clone();
                                                        let member_key = neighborhood_member_selection_key(member);
                                                        move |_| {
                                                            controller.set_selected_neighborhood_member_key(Some(member_key.clone()));
                                                            render_tick.set(render_tick() + 1);
                                                        }
                                                    },
                                                    UiListItem {
                                                        label: if member.is_self {
                                                            format!("{} (you)", member.name)
                                                        } else {
                                                            member.name.clone()
                                                        },
                                                        secondary: Some(if member.is_online {
                                                            format!("{} • online", member.role_label)
                                                        } else {
                                                            member.role_label.clone()
                                                        }),
                                                        active: idx == selected_member_index,
                                                    }
                                                }
                                            }
                                            if display_members.is_empty() {
                                                UiListItem {
                                                    label: "No other members or participants".to_string(),
                                                    secondary: Some("Invite or join another home to populate this view.".to_string()),
                                                    active: false,
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            div {
                                class: "flex flex-1 items-center justify-center rounded-sm bg-background/40 px-4 py-6 text-center",
                                p {
                                    class: "m-0 text-sm text-muted-foreground",
                                    "Partial/Limited view: full channel and membership details are hidden until Full access is active."
                                }
                            }
                        }
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                                UiButton {
                                    label: "Back To Map".to_string(),
                                    variant: ButtonVariant::Secondary,
                                    onclick: move |_| {
                                        detail_back_controller.send_key_named("esc", 1);
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                                if show_detail_lists {
                                    UiButton {
                                        label: if selected_runtime_member.as_ref().map(|member| member.is_moderator).unwrap_or(false) {
                                            "Revoke Moderator".to_string()
                                        } else {
                                            "Assign Moderator".to_string()
                                        },
                                        variant: ButtonVariant::Secondary,
                                        width_class: Some("w-[10rem]".to_string()),
                                        onclick: move |_| {
                                            detail_moderator_controller.send_action_keys("o");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                    UiButton {
                                        label: "Access Override".to_string(),
                                        variant: ButtonVariant::Secondary,
                                        onclick: move |_| {
                                            detail_access_override_controller.send_action_keys("x");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                    UiButton {
                                        label: "Capability Config".to_string(),
                                        variant: ButtonVariant::Secondary,
                                        onclick: move |_| {
                                            detail_capability_controller.send_action_keys("p");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else {
                    UiCardBody {
                        extra_class: Some("gap-3".to_string()),
                        div { class: "flex flex-wrap gap-2",
                            UiPill {
                                label: if home_rows.is_empty() {
                                    "Known Homes: 0".to_string()
                                } else {
                                    format!("Known Homes: {}", home_rows.len())
                                },
                                tone: PillTone::Neutral
                            }
                        }
                        if home_rows.is_empty() {
                            Empty {
                                class: Some("flex-1 min-h-[16rem] border-0 bg-background/40".to_string()),
                                EmptyHeader {
                                    EmptyTitle { "No home yet" }
                                    EmptyDescription { "Create a new home or accept an invitation to join." }
                                }
                            }
                        } else {
                            div {
                                class: "flex-1 lg:min-h-0 overflow-y-auto pr-1",
                                div { class: "aura-list space-y-2",
                                    for home in &home_rows {
                                        button {
                                            r#type: "button",
                                            id: list_item_dom_id(ListId::Homes, &home.id),
                                            class: "block w-full text-left",
                                            onclick: {
                                                let controller = controller.clone();
                                                let home_id = home.id.clone();
                                                let home_name = home.name.clone();
                                                move |_| {
                                                    controller.select_home(home_id.clone(), home_name.clone());
                                                    render_tick.set(render_tick() + 1);
                                                }
                                            },
                                            UiListItem {
                                                label: home.name.clone(),
                                                secondary: Some(if home.is_local {
                                                    "Local home".to_string()
                                                } else if let Some(member_count) = home.member_count {
                                                    format!(
                                                        "Members/Participants: {}{}",
                                                        member_count,
                                                        if home.can_enter { "" } else { " • traversal unavailable" }
                                                    )
                                                } else if home.can_enter {
                                                    "Neighbor home".to_string()
                                                } else {
                                                    "Neighbor home • traversal unavailable".to_string()
                                                }),
                                                active: selected_home == home.name,
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        div {
                            class: "rounded-sm bg-background/60 px-3 py-3",
                            p { class: "m-0 text-xs font-sans font-semibold uppercase tracking-[0.08em] text-muted-foreground", "Access" }
                            p {
                                class: "m-0 mt-1 text-sm text-foreground",
                                "Entry access: {strongest_access_label}"
                            }
                            p {
                                class: "m-0 mt-1 text-xs text-muted-foreground",
                                "Select a home, then enter it. Aura uses the strongest available access automatically."
                            }
                        }
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                                UiButton {
                                    id: Some(ControlId::NeighborhoodNewHomeButton.web_dom_id().required_dom_id("ControlId::NeighborhoodNewHomeButton must define a web DOM id").to_string()),
                                    label: "New Home".to_string(),
                                    variant: ButtonVariant::Primary,
                                    onclick: move |_| {
                                        map_new_home_controller.send_action_keys("n");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                                UiButton {
                                    id: Some(
                                        ControlId::NeighborhoodAcceptInvitationButton
                                            .web_dom_id()
                                            .required_dom_id("ControlId::NeighborhoodAcceptInvitationButton must define a web DOM id")
                                            .to_string(),
                                    ),
                                    label: "Accept Invitation".to_string(),
                                    variant: ButtonVariant::Primary,
                                    onclick: move |_| {
                                        map_accept_invitation_controller.send_action_keys("a");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                                if can_enter_selected_home {
                                    UiButton {
                                        label: "Enter Home".to_string(),
                                        variant: ButtonVariant::Primary,
                                        onclick: {
                                            let controller = map_enter_controller;
                                            let home_name = selected_home.clone();
                                            let depth = AccessDepth::Full;
                                            let target_home_id = enter_target_home_id;
                                            move |_| {
                                                let Some(target_home_id) = target_home_id.clone() else {
                                                    controller.runtime_error_toast("No runtime home selected");
                                                    render_tick.set(render_tick() + 1);
                                                    return;
                                                };
                                                let controller = controller.clone();
                                                let app_core = controller.app_core().clone();
                                                let operation = UiLocalOperationOwner::submit(
                                                    controller.clone(),
                                                    OperationId::move_position(),
                                                    SemanticOperationKind::MovePosition,
                                                );
                                                let mut tick = render_tick;
                                                let home_name = home_name.clone();
                                                spawn_ui(async move {
                                                    match context_workflows::move_position(
                                                        &app_core,
                                                        &target_home_id,
                                                        depth.label(),
                                                    )
                                                    .await
                                                    {
                                                        Ok(_) => {
                                                            operation.succeed(None);
                                                            controller.complete_runtime_enter_home(
                                                                &target_home_id,
                                                                &home_name,
                                                                depth,
                                                            );
                                                        }
                                                        Err(error) => {
                                                            operation.fail_with(
                                                                SemanticOperationError::new(
                                                                    SemanticFailureDomain::Command,
                                                                    SemanticFailureCode::InternalError,
                                                                )
                                                                .with_detail(error.to_string()),
                                                            );
                                                            controller.runtime_error_toast(
                                                                error.to_string(),
                                                            );
                                                        }
                                                    }
                                                    tick.set(tick() + 1);
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            UiCard {
                title: "Social View".to_string(),
                subtitle: Some("Neighborhood status and scope".to_string()),
                extra_class: Some("lg:col-span-8".to_string()),
                div {
                    class: "aura-list grid gap-2 md:grid-cols-2 md:gap-x-5",
                    UiListItem {
                        label: format!("Neighborhood: {display_neighborhood_name}"),
                        secondary: Some(format!("Selected home: {selected_home}")),
                        active: false,
                    }
                    UiListItem {
                        label: format!("Home ID: {selected_home_id}"),
                        secondary: Some("Authority-scoped identifier".to_string()),
                        active: false,
                    }
                    UiListItem {
                        label: format!("Entry access: {strongest_access_label}"),
                        secondary: Some(format!(
                            "{social_mode_label} view"
                        )),
                        active: false,
                    }
                    UiListItem {
                        label: format!("Known Homes: {}", home_rows.len()),
                        secondary: Some("Neighborhood graph currently in view".to_string()),
                        active: false,
                    }
                    UiListItem {
                        label: format!("Channels: {}", display_channels.len()),
                        secondary: Some(format!("Focus: #{selected_channel_name}")),
                        active: false,
                    }
                    UiListItem {
                        label: format!("Members/Participants: {member_count}"),
                        secondary: Some(if show_detail_lists {
                            "Full detail available".to_string()
                        } else {
                            "Detail lists hidden outside Full access".to_string()
                        }),
                        active: false,
                    }
                    UiListItem {
                        label: format!("Authority: {}", model.authority_id),
                        secondary: Some("Local identity".to_string()),
                        active: false,
                    }
                    UiListItem {
                        label: "Moderator Actions".to_string(),
                        secondary: Some(if show_detail_lists {
                            "Available in detail view".to_string()
                        } else {
                            "Unavailable".to_string()
                        }),
                        active: false,
                    }
                }
            }
        }
    }
}

pub(crate) fn neighborhood_member_selection_key(
    member: &NeighborhoodRuntimeMember,
) -> NeighborhoodMemberSelectionKey {
    if !member.authority_id.is_empty() {
        NeighborhoodMemberSelectionKey(format!("authority:{}", member.authority_id))
    } else {
        NeighborhoodMemberSelectionKey(format!("name:{}", member.name))
    }
}
