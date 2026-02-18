//! Unified Neighborhood Screen.

use iocraft::prelude::*;

use aura_app::ui::signals::{CHAT_SIGNAL, CONTACTS_SIGNAL, HOMES_SIGNAL, NEIGHBORHOOD_SIGNAL};

use crate::tui::hooks::{subscribe_signal_with_retry, AppCoreContext};
use crate::tui::layout::dim;
use crate::tui::props::NeighborhoodViewProps;
use crate::tui::state_machine::NeighborhoodMode;
use crate::tui::theme::Theme;
use crate::tui::types::{short_id, Contact, HomeBudget, HomeSummary, Resident, TraversalDepth};

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
    enter_depth: TraversalDepth,
}

#[component]
fn HomeMap(props: &HomeMapProps) -> impl Into<AnyElement<'static>> {
    let homes = props.homes.clone();
    let selected = props.selected_index;
    let enter_depth = props.enter_depth;

    let (depth_icon, depth_label) = (enter_depth.icon().to_string(), enter_depth.label());

    let can_enter_interior = homes.get(selected).map(|b| b.can_enter).unwrap_or(false);

    let can_enter_line = format!(
        "Can enter: Street ✔ Frontage ✔ Interior {}",
        if can_enter_interior { "✔" } else { "✖" }
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
                                Text(content: "No homes yet", color: Theme::TEXT_MUTED)
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
                        let residents = format!("{}/{}", home_entry.resident_count, home_entry.max_residents);
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
                                Text(content: residents, color: Theme::TEXT_MUTED)
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
    resident_count: usize,
    storage_text: String,
    steward_label: String,
}

#[component]
fn HomeHeader(props: &HomeHeaderProps) -> impl Into<AnyElement<'static>> {
    let status_line = format!(
        "Residents: {} • {} • {}",
        props.resident_count, props.storage_text, props.steward_label
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
struct ResidentListProps {
    residents: Vec<Resident>,
    selected_index: usize,
    steward_actions_enabled: bool,
}

#[component]
fn ResidentList(props: &ResidentListProps) -> impl Into<AnyElement<'static>> {
    let residents = props.residents.clone();
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
                Text(content: "Residents", weight: Weight::Bold, color: Theme::PRIMARY)
                #(if props.steward_actions_enabled {
                    Some(element! { Text(content: "⚖︎ Steward", color: Theme::WARNING) })
                } else {
                    None
                })
            }
            View(flex_direction: FlexDirection::Column, gap: 0) {
                #(if residents.is_empty() {
                    vec![element! {
                        View { Text(content: "No residents", color: Theme::TEXT_MUTED) }
                    }]
                } else {
                    residents.iter().enumerate().map(|(idx, r)| {
                        let is_selected = idx == selected;
                        let id_hint = short_id(&r.id, 4);
                        let name = format!("{} · #{}", r.name, id_hint);
                        let steward_badge = if r.is_steward { " ⚖︎" } else { "" };
                        let self_badge = if r.is_self { " (you)" } else { "" };
                        element! {
                            View(
                                key: r.id.clone(),
                                flex_direction: FlexDirection::Row,
                                background_color: if is_selected { Theme::LIST_BG_SELECTED } else { Theme::LIST_BG_NORMAL },
                                padding_left: 1,
                            ) {
                                Text(content: name, color: if is_selected { Theme::LIST_TEXT_SELECTED } else { Theme::LIST_TEXT_NORMAL })
                                Text(content: steward_badge, color: Theme::WARNING)
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
    enter_depth: TraversalDepth,
    entered_home: bool,
    homes_count: usize,
    channel_count: usize,
    selected_channel_name: String,
    resident_count: usize,
    steward_actions_enabled: bool,
}

#[component]
fn SocialStatusPanel(props: &SocialStatusProps) -> impl Into<AnyElement<'static>> {
    let entered_text = if props.entered_home {
        "Entered"
    } else {
        "Browsing"
    };
    let steward_text = if props.steward_actions_enabled {
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
            Text(content: format!("Selected block: {}", props.selected_home_name), color: Theme::TEXT)
            Text(content: format!("Traversal: {} • {}", props.enter_depth.label(), entered_text), color: Theme::TEXT_MUTED)
            Text(content: format!("Known homes: {}", props.homes_count), color: Theme::TEXT_MUTED)
            Text(content: format!("Channels: {} • Focus: #{}", props.channel_count, props.selected_channel_name), color: Theme::TEXT_MUTED)
            Text(content: format!("Residents in view: {}", props.resident_count), color: Theme::TEXT_MUTED)
            Text(content: format!("Steward actions: {}", steward_text), color: Theme::TEXT_MUTED)
            View(height: 1)
            Text(content: "Messaging is available on Chat.", color: Theme::SECONDARY)
        }
    }
}

fn is_steward_role(role: aura_app::ui::types::home::ResidentRole) -> bool {
    matches!(
        role,
        aura_app::ui::types::home::ResidentRole::Admin
            | aura_app::ui::types::home::ResidentRole::Owner
    )
}

fn convert_resident(r: &aura_app::ui::types::home::Resident) -> Resident {
    Resident {
        id: r.id.to_string(),
        name: r.name.clone(),
        is_steward: is_steward_role(r.role),
        is_self: false,
    }
}

fn convert_budget(
    storage: &aura_app::ui::types::HomeFlowBudget,
    resident_count: u32,
) -> HomeBudget {
    HomeBudget {
        total: storage.total_allocation(),
        used: storage.total_used(),
        resident_count: resident_count as u8,
        max_residents: aura_app::ui::types::MAX_RESIDENTS,
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
        resident_count: n.resident_count.unwrap_or(0) as u8,
        max_residents: 8,
        is_home: n.id == *home_home_id,
        can_enter: n.can_traverse,
    }
}

fn convert_traversal_depth(depth: u32) -> TraversalDepth {
    match depth {
        0 => TraversalDepth::Interior,
        1 => TraversalDepth::Frontage,
        _ => TraversalDepth::Street,
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
    let reactive_depth = hooks.use_state(TraversalDepth::default);

    let reactive_residents = hooks.use_state(Vec::new);
    let reactive_budget = hooks.use_state(HomeBudget::default);
    let reactive_channels = hooks.use_state(Vec::new);
    let reactive_contacts = hooks.use_state(Vec::new);

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut reactive_neighborhood_name = reactive_neighborhood_name.clone();
        let mut reactive_homes = reactive_homes.clone();
        let mut reactive_depth = reactive_depth.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*NEIGHBORHOOD_SIGNAL, move |n| {
                let home_id = &n.home_home_id;
                let mut homes: Vec<HomeSummary> = Vec::with_capacity(n.neighbor_count() + 1);
                homes.push(HomeSummary {
                    id: n.home_home_id.to_string(),
                    name: Some(n.home_name.clone()),
                    resident_count: 0,
                    max_residents: 8,
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
                    .map(|p| convert_traversal_depth(p.depth))
                    .unwrap_or(TraversalDepth::Interior);
                reactive_neighborhood_name.set(n.home_name.clone());
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
        let mut reactive_residents = reactive_residents.clone();
        let mut reactive_budget = reactive_budget.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*HOMES_SIGNAL, move |home_state| {
                if let Some(current_home) = home_state.current_home() {
                    let residents: Vec<Resident> = current_home
                        .residents
                        .iter()
                        .map(convert_resident)
                        .collect();
                    let budget = convert_budget(&current_home.storage, current_home.resident_count);
                    reactive_residents.set(residents);
                    reactive_budget.set(budget);
                } else {
                    reactive_residents.set(Vec::new());
                    reactive_budget.set(HomeBudget {
                        total: 0,
                        used: 0,
                        resident_count: 0,
                        max_residents: aura_app::ui::types::MAX_RESIDENTS,
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
                let channel_list: Vec<ChannelSummary> = chat_state
                    .all_channels()
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
    let residents = reactive_residents.read().clone();
    let budget = reactive_budget.read().clone();
    let channels = reactive_channels.read().clone();

    let is_detail = props.view.mode == NeighborhoodMode::Detail;
    let is_entered = props.view.entered_home_id.is_some();

    let current_home_name = homes
        .get(props.view.selected_home)
        .and_then(|b| b.name.clone())
        .unwrap_or_else(|| neighborhood_name.clone());
    let selected_channel_name = channels
        .get(props.view.selected_channel)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| "none".to_string());
    let homes_count = homes.len();
    let channel_count = channels.len();
    let resident_count = residents.len();

    let storage_text = format!(
        "Storage: {}/{}MB",
        (budget.used as f64 / (1024.0 * 1024.0)).round() as u64,
        (budget.total as f64 / (1024.0 * 1024.0)).round() as u64
    );

    let steward_label = if props.view.steward_actions_enabled {
        "Steward: Yes".to_string()
    } else {
        "Steward: No".to_string()
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
                                    resident_count: resident_count,
                                    storage_text: storage_text,
                                    steward_label: steward_label,
                                )
                                ChannelList(channels: channels.clone(), selected_index: props.view.selected_channel)
                                ResidentList(
                                    residents: residents.clone(),
                                    selected_index: props.view.selected_resident,
                                    steward_actions_enabled: props.view.steward_actions_enabled,
                                )
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
                        enter_depth: props.view.enter_depth,
                        entered_home: is_entered,
                        homes_count: homes_count,
                        channel_count: channel_count,
                        selected_channel_name: selected_channel_name,
                        resident_count: resident_count,
                        steward_actions_enabled: props.view.steward_actions_enabled,
                    )
                }
            }
        }
    }
}
