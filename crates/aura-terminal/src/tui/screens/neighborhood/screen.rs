//! Unified Neighborhood Screen.

use iocraft::prelude::*;
use std::sync::Arc;

use aura_app::ui::signals::{CHAT_SIGNAL, CONTACTS_SIGNAL, HOMES_SIGNAL, NEIGHBORHOOD_SIGNAL};

use crate::tui::chat_scope::{active_home_scope_id, scoped_channels};
use crate::tui::hooks::{subscribe_signal_with_retry, AppCoreContext};
use crate::tui::layout::dim;
use crate::tui::props::NeighborhoodViewProps;
use crate::tui::state_machine::NeighborhoodMode;
use crate::tui::theme::Theme;
use crate::tui::types::{short_id, AccessLevel, Contact, HomeBudget, HomeMember, HomeSummary};

pub async fn run_neighborhood_screen() -> std::io::Result<()> {
    element! {
        NeighborhoodScreen(
            view: NeighborhoodViewProps::default(),
        )
    }
    .fullscreen()
    .await
}

use crate::tui::updates::UiUpdateSender;

#[derive(Default, Props)]
pub struct NeighborhoodScreenProps {
    pub view: NeighborhoodViewProps,
    /// UI update sender (reserved for future neighborhood-side reactive updates)
    pub update_tx: Option<UiUpdateSender>,
}

#[derive(Clone, Debug, Default)]
struct ChannelSummary {
    id: String,
    name: String,
}

#[derive(Default, Props)]
struct HomeMapProps {
    homes: Vec<HomeSummary>,
    selected_index: usize,
    enter_depth: AccessLevel,
}

#[component]
fn HomeMap(props: &HomeMapProps) -> impl Into<AnyElement<'static>> {
    let homes = props.homes.clone();
    let selected = props.selected_index;
    let enter_depth = props.enter_depth;

    let (depth_icon, depth_label) = (enter_depth.icon().to_string(), enter_depth.label());

    let can_enter_full = homes.get(selected).map(|b| b.can_enter).unwrap_or(false);

    let can_enter_line = format!(
        "Can enter: Limited ✔ Partial ✔ Full {}",
        if can_enter_full { "✔" } else { "✖" }
    );

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
            padding_left: 1,
            padding_right: 1,
            padding_bottom: 1,
        ) {
            View(flex_direction: FlexDirection::Row, gap: 1) {
                Text(content: "Map", weight: Weight::Bold, color: Theme::PRIMARY)
                Text(content: depth_icon, color: Theme::SECONDARY)
                Text(content: depth_label, color: Theme::SECONDARY)
            }
            View(height: 1)
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                gap: 1,
                overflow: Overflow::Scroll,
            ) {
                #(if homes.is_empty() {
                    vec![
                        element! {
                            View(flex_direction: FlexDirection::Column, gap: 1) {
                                Text(content: "No home yet", color: Theme::TEXT_MUTED)
                                View(height: 1)
                                Text(content: "To get started:", color: Theme::TEXT)
                                View(flex_direction: FlexDirection::Column, padding_left: 1) {
                                    Text(content: "• Create a new home", color: Theme::TEXT_MUTED)
                                    Text(content: "• Accept an invitation to join", color: Theme::TEXT_MUTED)
                                    Text(content: "  an existing home", color: Theme::TEXT_MUTED)
                                }
                                View(height: 1)
                                Text(content: "[n] Create home", color: Theme::SECONDARY)
                                Text(content: "[i] Import invitation", color: Theme::SECONDARY)
                            }
                        }
                    ]
                } else {
                    homes.iter().enumerate().map(|(idx, home_entry)| {
                        let is_selected = idx == selected;
                        let border_color = if is_selected { Theme::PRIMARY } else { Theme::BORDER };
                        let name = home_entry
                            .name
                            .clone()
                            .unwrap_or_else(|| "Unnamed Home".to_string());
                        let home_badge = if home_entry.is_home { " ⌂" } else { "" };
                        let members = format!("{}/{}", home_entry.member_count, home_entry.max_members);
                        element! {
                            View(
                                key: home_entry.id.clone(),
                                flex_direction: FlexDirection::Column,
                                border_style: BorderStyle::Round,
                                border_color: border_color,
                                padding_left: 1,
                                padding_right: 1,
                            ) {
                                View(flex_direction: FlexDirection::Row) {
                                    Text(content: name, color: Theme::TEXT)
                                    Text(content: home_badge, color: Theme::PRIMARY)
                                }
                                Text(content: members, color: Theme::TEXT_MUTED)
                            }
                        }
                    }).collect()
                })
            }
            View(height: 1)
            Text(content: can_enter_line, color: Theme::TEXT_MUTED)
        }
    }
}

#[derive(Default, Props)]
struct HomeHeaderProps {
    home_name: String,
    member_count: usize,
    storage_text: String,
    moderator_label: String,
}

#[component]
fn HomeHeader(props: &HomeHeaderProps) -> impl Into<AnyElement<'static>> {
    let status_line = format!(
        "Members/Participants: {} • {} • {}",
        props.member_count, props.storage_text, props.moderator_label
    );
    let name_line = format!("Name: {}", props.home_name);

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
            padding_left: 1,
            padding_right: 1,
        ) {
            View(flex_direction: FlexDirection::Column, gap: 0) {
                Text(content: "Home", weight: Weight::Bold, color: Theme::PRIMARY)
                Text(content: name_line, color: Theme::TEXT_MUTED)
                Text(content: status_line, color: Theme::TEXT_MUTED)
            }
        }
    }
}

#[derive(Default, Props)]
struct ChannelListProps {
    channels: Vec<ChannelSummary>,
    selected_index: usize,
}

#[component]
fn ChannelList(props: &ChannelListProps) -> impl Into<AnyElement<'static>> {
    let channels = props.channels.clone();
    let selected = props.selected_index;

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
            padding_left: 1,
            padding_right: 1,
        ) {
            Text(content: "Channels", weight: Weight::Bold, color: Theme::PRIMARY)
            View(flex_direction: FlexDirection::Column, gap: 0) {
                #(if channels.is_empty() {
                    vec![element! {
                        View { Text(content: "No channels", color: Theme::TEXT_MUTED) }
                    }]
                } else {
                    channels.iter().enumerate().map(|(idx, channel)| {
                        let is_selected = idx == selected;
                        let prefix = if is_selected { "▸ " } else { "  " };
                        element! {
                            View(key: channel.id.clone(), flex_direction: FlexDirection::Row) {
                                Text(content: format!("{}# {}", prefix, channel.name), color: if is_selected { Theme::TEXT } else { Theme::TEXT_MUTED })
                            }
                        }
                    }).collect()
                })
            }
        }
    }
}

#[derive(Default, Props)]
struct MemberListProps {
    members: Vec<HomeMember>,
    selected_index: usize,
    moderator_actions_enabled: bool,
}

#[component]
fn MemberList(props: &MemberListProps) -> impl Into<AnyElement<'static>> {
    let members = props.members.clone();
    let selected = props.selected_index;

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
            padding_left: 1,
            padding_right: 1,
        ) {
            View(flex_direction: FlexDirection::Row, gap: 1) {
                Text(content: "Members & Participants", weight: Weight::Bold, color: Theme::PRIMARY)
                #(if props.moderator_actions_enabled {
                    Some(element! { Text(content: "⚖︎ Moderator", color: Theme::WARNING) })
                } else {
                    None
                })
            }
            View(flex_direction: FlexDirection::Column, gap: 0) {
                #(if members.is_empty() {
                    vec![element! {
                        View { Text(content: "No members or participants", color: Theme::TEXT_MUTED) }
                    }]
                } else {
                    members.iter().enumerate().map(|(idx, r)| {
                        let is_selected = idx == selected;
                        let id_hint = short_id(&r.id, 4);
                        let name = format!("{} · #{}", r.name, id_hint);
                        let moderator_badge = if r.is_moderator { " ⚖︎" } else { "" };
                        let self_badge = if r.is_self { " (you)" } else { "" };
                        element! {
                            View(
                                key: r.id.clone(),
                                flex_direction: FlexDirection::Row,
                                background_color: if is_selected { Theme::LIST_BG_SELECTED } else { Theme::LIST_BG_NORMAL },
                                padding_left: 1,
                            ) {
                                Text(content: name, color: if is_selected { Theme::LIST_TEXT_SELECTED } else { Theme::LIST_TEXT_NORMAL })
                                Text(content: moderator_badge, color: Theme::WARNING)
                                Text(content: self_badge, color: if is_selected { Theme::LIST_TEXT_SELECTED } else { Theme::LIST_TEXT_MUTED })
                            }
                        }
                    }).collect()
                })
            }
        }
    }
}

#[derive(Default, Props)]
struct SocialStatusProps {
    neighborhood_name: String,
    selected_home_name: String,
    selected_home_id: String,
    enter_depth: AccessLevel,
    entered_home: bool,
    homes_count: usize,
    channel_count: usize,
    selected_channel_name: String,
    member_count: usize,
    moderator_actions_enabled: bool,
}

#[component]
fn SocialStatusPanel(props: &SocialStatusProps) -> impl Into<AnyElement<'static>> {
    let entered_text = if props.entered_home {
        "Entered"
    } else {
        "Browsing"
    };
    let moderator_text = if props.moderator_actions_enabled {
        "Enabled"
    } else {
        "Disabled"
    };

    element! {
        View(
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
            padding_left: 1,
            padding_right: 1,
            flex_grow: 1.0,
            gap: 1,
        ) {
            Text(content: "Social View", weight: Weight::Bold, color: Theme::PRIMARY)
            Text(content: format!("Neighborhood: {}", props.neighborhood_name), color: Theme::TEXT)
            Text(content: format!("Selected home: {}", props.selected_home_name), color: Theme::TEXT)
            Text(content: format!("Home ID: {}", props.selected_home_id), color: Theme::TEXT_MUTED)
            Text(content: format!("Access: {} ({}) • {}", props.enter_depth.label(), hop_distance_hint(props.enter_depth), entered_text), color: Theme::TEXT_MUTED)
            Text(content: format!("Known homes: {}", props.homes_count), color: Theme::TEXT_MUTED)
            Text(content: format!("Channels: {} • Focus: #{}", props.channel_count, props.selected_channel_name), color: Theme::TEXT_MUTED)
            Text(content: format!("Members/participants in view: {}", props.member_count), color: Theme::TEXT_MUTED)
            Text(content: format!("Moderator actions: {}", moderator_text), color: Theme::TEXT_MUTED)
        }
    }
}

fn hop_distance_hint(access_level: AccessLevel) -> &'static str {
    match access_level {
        AccessLevel::Full => "0-hop",
        AccessLevel::Partial => "1-hop",
        AccessLevel::Limited => "2+ hops/disconnected",
    }
}

fn convert_member(r: &aura_app::ui::types::home::HomeMember) -> HomeMember {
    let role_label = match r.role {
        aura_app::ui::types::home::HomeRole::Member => "Member",
        aura_app::ui::types::home::HomeRole::Moderator => "Member + Moderator",
        aura_app::ui::types::home::HomeRole::Participant => "Participant",
    };
    HomeMember {
        id: r.id.to_string(),
        name: format!("{} ({role_label})", r.name),
        is_moderator: r.is_moderator(),
        is_self: false,
    }
}

fn convert_budget(storage: &aura_app::ui::types::HomeFlowBudget, member_count: u32) -> HomeBudget {
    HomeBudget {
        total: storage.total_allocation(),
        used: storage.total_used(),
        member_count: member_count as u8,
        max_members: aura_app::ui::types::MAX_MEMBERS,
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // ChannelId is at the 32-byte limit boundary
fn convert_neighbor_home(
    n: &aura_app::ui::types::NeighborHome,
    home_home_id: &aura_core::identifiers::ChannelId,
) -> HomeSummary {
    HomeSummary {
        id: n.id.to_string(),
        name: Some(n.name.clone()),
        member_count: n.member_count.unwrap_or(0) as u8,
        max_members: 8,
        is_home: n.id == *home_home_id,
        can_enter: n.can_traverse,
    }
}

fn convert_access_level_value(depth: u32) -> AccessLevel {
    match depth {
        0 => AccessLevel::Limited,
        1 => AccessLevel::Partial,
        _ => AccessLevel::Full,
    }
}

#[component]
pub fn NeighborhoodScreen(
    props: &NeighborhoodScreenProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let app_ctx = hooks.use_context::<AppCoreContext>();

    let reactive_neighborhood_name = hooks.use_state(String::new);
    let reactive_homes = hooks.use_state(Vec::new);
    let reactive_depth = hooks.use_state(AccessLevel::default);
    let active_scope_ref = hooks.use_ref(|| Arc::new(std::sync::RwLock::new(String::new())));
    let active_scope: Arc<std::sync::RwLock<String>> = active_scope_ref.read().clone();

    let reactive_members = hooks.use_state(Vec::new);
    let reactive_budget = hooks.use_state(HomeBudget::default);
    let reactive_channels = hooks.use_state(Vec::new);
    let reactive_contacts = hooks.use_state(Vec::new);

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut reactive_neighborhood_name = reactive_neighborhood_name.clone();
        let mut reactive_homes = reactive_homes.clone();
        let mut reactive_depth = reactive_depth.clone();
        let active_scope = active_scope.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*NEIGHBORHOOD_SIGNAL, move |n| {
                let home_id = &n.home_home_id;
                let mut homes: Vec<HomeSummary> = Vec::with_capacity(n.neighbor_count() + 1);
                homes.push(HomeSummary {
                    id: n.home_home_id.to_string(),
                    name: Some(n.home_name.clone()),
                    member_count: 0,
                    max_members: 8,
                    is_home: true,
                    can_enter: true,
                });
                homes.extend(
                    n.all_neighbors()
                        .filter(|b| b.id != n.home_home_id)
                        .map(|b| convert_neighbor_home(b, home_id)),
                );
                let depth = n
                    .position
                    .as_ref()
                    .map(|p| convert_access_level_value(p.depth))
                    .unwrap_or(AccessLevel::Full);
                if let Ok(mut guard) = active_scope.write() {
                    *guard = active_home_scope_id(&n);
                }
                reactive_neighborhood_name.set(
                    n.neighborhood_name
                        .clone()
                        .unwrap_or_else(|| n.home_name.clone()),
                );
                reactive_homes.set(homes);
                reactive_depth.set(depth);
            })
            .await;
        }
    });

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut reactive_contacts = reactive_contacts.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CONTACTS_SIGNAL, move |contacts_state| {
                let contacts: Vec<Contact> =
                    contacts_state.all_contacts().map(Contact::from).collect();
                reactive_contacts.set(contacts);
            })
            .await;
        }
    });

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut reactive_members = reactive_members.clone();
        let mut reactive_budget = reactive_budget.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*HOMES_SIGNAL, move |home_state| {
                if let Some(current_home) = home_state.current_home() {
                    let members: Vec<HomeMember> =
                        current_home.members.iter().map(convert_member).collect();
                    let budget = convert_budget(&current_home.storage, current_home.member_count);
                    reactive_members.set(members);
                    reactive_budget.set(budget);
                } else {
                    reactive_members.set(Vec::new());
                    reactive_budget.set(HomeBudget {
                        total: 0,
                        used: 0,
                        member_count: 0,
                        max_members: aura_app::ui::types::MAX_MEMBERS,
                    });
                }
            })
            .await;
        }
    });

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut reactive_channels = reactive_channels.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CHAT_SIGNAL, move |chat_state| {
                let scope: Option<String> = active_scope.read().ok().map(|guard| guard.clone());
                let channel_list: Vec<ChannelSummary> =
                    scoped_channels(&chat_state, scope.as_deref())
                        .into_iter()
                        .map(|c| ChannelSummary {
                            id: c.id.to_string(),
                            name: c.name.clone(),
                        })
                        .collect();
                reactive_channels.set(channel_list);
            })
            .await;
        }
    });

    let neighborhood_name = reactive_neighborhood_name.read().clone();
    let homes = reactive_homes.read().clone();
    let members = reactive_members.read().clone();
    let budget = reactive_budget.read().clone();
    let channels = reactive_channels.read().clone();

    let is_detail = props.view.mode == NeighborhoodMode::Detail;
    let is_entered = props.view.entered_home_id.is_some();

    let current_home_name = homes
        .get(props.view.selected_home)
        .and_then(|b| b.name.clone())
        .unwrap_or_else(|| neighborhood_name.clone());
    let selected_home_id = homes
        .get(props.view.selected_home)
        .map(|b| b.id.clone())
        .unwrap_or_default();
    // Only expose channel/member detail when full access is active.
    // This keeps Limited/Partial traversal views from leaking full-only data.
    let full_entered =
        is_detail && is_entered && matches!(props.view.enter_depth, AccessLevel::Full);
    let show_detail_lists = !is_detail || full_entered;
    let display_channels = if show_detail_lists {
        channels
    } else {
        Vec::new()
    };
    let display_members = if show_detail_lists {
        members
    } else {
        Vec::new()
    };

    let selected_channel_name = display_channels
        .get(props.view.selected_channel)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| "none".to_string());
    let homes_count = homes.len();
    let channel_count = display_channels.len();
    let member_count = display_members.len();
    let display_moderator_actions_enabled =
        props.view.moderator_actions_enabled && show_detail_lists;

    let storage_text = if budget.total > 0 {
        format!(
            "Storage: {}/{}MB",
            (budget.used as f64 / (1024.0 * 1024.0)).round() as u64,
            (budget.total as f64 / (1024.0 * 1024.0)).round() as u64
        )
    } else {
        "Storage: --/--MB".to_string()
    };

    let moderator_label = if display_moderator_actions_enabled {
        "Moderator: Yes".to_string()
    } else {
        "Moderator: No".to_string()
    };

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            overflow: Overflow::Hidden,
        ) {
            View(
                flex_direction: FlexDirection::Row,
                height: dim::MIDDLE_HEIGHT,
                overflow: Overflow::Hidden,
                gap: dim::TWO_PANEL_GAP,
            ) {
                View(width: dim::TWO_PANEL_LEFT_WIDTH, height: dim::MIDDLE_HEIGHT) {
                    #(if is_detail {
                        vec![element! {
                            View(flex_direction: FlexDirection::Column, gap: 0) {
                                HomeHeader(
                                    home_name: current_home_name.clone(),
                                    member_count: member_count,
                                    storage_text: storage_text,
                                    moderator_label: moderator_label,
                                )
                                #(if show_detail_lists {
                                    vec![element! {
                                        View(flex_direction: FlexDirection::Column, gap: 0) {
                                            ChannelList(channels: display_channels.clone(), selected_index: props.view.selected_channel)
                                            MemberList(
                                                members: display_members.clone(),
                                                selected_index: props.view.selected_member,
                                                moderator_actions_enabled: display_moderator_actions_enabled,
                                            )
                                        }
                                    }]
                                } else {
                                    vec![
                                        element! {
                                            View(
                                                border_style: BorderStyle::Round,
                                                border_color: Theme::BORDER,
                                                padding_left: 1,
                                                padding_right: 1,
                                                padding_top: 1,
                                                padding_bottom: 1,
                                            ) {
                                                Text(
                                                    content: "Partial/Limited view: full channel and membership details are hidden",
                                                    color: Theme::TEXT_MUTED,
                                                )
                                            }
                                        }
                                    ]
                                })
                            }
                        }]
                    } else {
                        vec![element! {
                            View {
                                HomeMap(homes: homes.clone(), selected_index: props.view.selected_home, enter_depth: props.view.enter_depth)
                            }
                        }]
                    })
                }
                View(width: dim::TWO_PANEL_RIGHT_WIDTH, height: dim::MIDDLE_HEIGHT) {
                    SocialStatusPanel(
                        neighborhood_name: neighborhood_name,
                        selected_home_name: current_home_name,
                        selected_home_id: selected_home_id,
                        enter_depth: props.view.enter_depth,
                        entered_home: is_entered,
                        homes_count: homes_count,
                        channel_count: channel_count,
                        selected_channel_name: selected_channel_name,
                        member_count: member_count,
                        moderator_actions_enabled: display_moderator_actions_enabled,
                    )
                }
            }
        }
    }
}
