//! # Neighborhood Screen
//!
//! Block traversal and neighborhood navigation

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::components::KeyHintsBar;
use crate::tui::theme::Theme;
use crate::tui::types::{BlockSummary, KeyHint, TraversalDepth};

/// Callback type for navigation actions (block_id, depth)
pub type NavigationCallback = Arc<dyn Fn(String, TraversalDepth) + Send + Sync>;

/// Callback type for go home action (no args, navigates to home block)
pub type GoHomeCallback = Arc<dyn Fn() + Send + Sync>;

/// Props for BlockCard
#[derive(Default, Props)]
pub struct BlockCardProps {
    pub block: BlockSummary,
    pub is_selected: bool,
}

/// A block card in the neighborhood view
#[component]
pub fn BlockCard(props: &BlockCardProps) -> impl Into<AnyElement<'static>> {
    let b = &props.block;
    let is_selected = props.is_selected;

    let border_color = if is_selected {
        Theme::BORDER_FOCUS
    } else if b.is_home {
        Theme::PRIMARY
    } else if b.can_enter {
        Theme::SECONDARY
    } else {
        Theme::BORDER
    };

    let name = b
        .name
        .clone()
        .unwrap_or_else(|| "Unnamed Block".to_string());
    let residents_text = format!("{}/{} residents", b.resident_count, b.max_residents);
    let home_badge = if b.is_home { " ⌂" } else { "" }.to_string();
    let access_text = if b.can_enter {
        "Enter ↵"
    } else {
        "Locked ⊡"
    }
    .to_string();
    let access_color = if b.can_enter {
        Theme::SUCCESS
    } else {
        Theme::TEXT_MUTED
    };

    element! {
        View(
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Round,
            border_color: border_color,
            padding: 1,
            min_width: 20,
        ) {
            View(flex_direction: FlexDirection::Row) {
                Text(content: name, weight: Weight::Bold, color: Theme::TEXT)
                Text(content: home_badge, color: Theme::PRIMARY)
            }
            Text(content: residents_text, color: Theme::TEXT_MUTED)
            View(height: 1)
            Text(content: access_text, color: access_color)
        }
    }
}

/// Props for BlockGrid
#[derive(Default, Props)]
pub struct BlockGridProps {
    pub blocks: Vec<BlockSummary>,
    pub selected_index: usize,
}

/// Grid of blocks in the neighborhood
#[component]
pub fn BlockGrid(props: &BlockGridProps) -> impl Into<AnyElement<'static>> {
    let blocks = props.blocks.clone();
    let selected = props.selected_index;

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
        ) {
            View(padding_left: 1) {
                Text(content: "Blocks", weight: Weight::Bold, color: Theme::PRIMARY)
            }
            View(
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                gap: 1,
                padding: 1,
            ) {
                #(if blocks.is_empty() {
                    vec![element! {
                        View {
                            Text(content: "No blocks in this neighborhood", color: Theme::TEXT_MUTED)
                        }
                    }]
                } else {
                    blocks.iter().enumerate().map(|(idx, block)| {
                        let is_selected = idx == selected;
                        element! {
                            View {
                                BlockCard(block: block.clone(), is_selected: is_selected)
                            }
                        }
                    }).collect()
                })
            }
        }
    }
}

/// Props for TraversalInfo
#[derive(Default, Props)]
pub struct TraversalInfoProps {
    pub depth: TraversalDepth,
    pub neighborhood_name: String,
}

/// Traversal info panel
#[component]
pub fn TraversalInfo(props: &TraversalInfoProps) -> impl Into<AnyElement<'static>> {
    let depth_label = props.depth.label().to_string();
    let depth_icon = props.depth.icon().to_string();
    let neighborhood = props.neighborhood_name.clone();

    let depth_description = match props.depth {
        TraversalDepth::Street => "Passing by - can see block frontage only",
        TraversalDepth::Frontage => "At the door - can view public info",
        TraversalDepth::Interior => "Inside - full access as resident",
    }
    .to_string();

    element! {
        View(
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
            padding: 1,
        ) {
            View(flex_direction: FlexDirection::Row, gap: 1) {
                Text(content: "Neighborhood:", color: Theme::TEXT_MUTED)
                Text(content: neighborhood, color: Theme::TEXT)
            }
            View(flex_direction: FlexDirection::Row, gap: 1) {
                Text(content: "Position:", color: Theme::TEXT_MUTED)
                Text(content: depth_icon, color: Theme::TEXT)
                Text(content: depth_label, color: Theme::SECONDARY)
            }
            Text(content: depth_description, color: Theme::TEXT_MUTED)
        }
    }
}

/// Props for NeighborhoodScreen
#[derive(Default, Props)]
pub struct NeighborhoodScreenProps {
    pub neighborhood_name: String,
    pub blocks: Vec<BlockSummary>,
    pub depth: TraversalDepth,
    /// Callback when entering a block (block_id, depth)
    pub on_enter_block: Option<NavigationCallback>,
    /// Callback when going home
    pub on_go_home: Option<GoHomeCallback>,
    /// Callback when going back to street view
    pub on_back_to_street: Option<GoHomeCallback>,
}

/// The neighborhood screen
#[component]
pub fn NeighborhoodScreen(
    props: &NeighborhoodScreenProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let selected = hooks.use_state(|| 0usize);

    let hints = vec![
        KeyHint::new("←→↑↓", "Navigate"),
        KeyHint::new("Enter", "Enter block"),
        KeyHint::new("h", "Go home"),
        KeyHint::new("b", "Back to street"),
        KeyHint::new("Esc", "Back"),
    ];

    let neighborhood_name = props.neighborhood_name.clone();
    let blocks = props.blocks.clone();
    let depth = props.depth;
    let current_selected = selected.get();

    // Clone callbacks for event handler
    let on_enter_block = props.on_enter_block.clone();
    let on_go_home = props.on_go_home.clone();
    let on_back_to_street = props.on_back_to_street.clone();

    hooks.use_terminal_events({
        let mut selected = selected.clone();
        let blocks_for_handler = blocks.clone();
        let count = blocks_for_handler.len();
        move |event| match event {
            TerminalEvent::Key(KeyEvent { code, .. }) => match code {
                // Navigation - arrow keys and vim j/k/l (not h, as h is used for "go home")
                KeyCode::Left => {
                    let current = selected.get();
                    if current > 0 {
                        selected.set(current - 1);
                    }
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    let current = selected.get();
                    if current + 1 < count {
                        selected.set(current + 1);
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let current = selected.get();
                    // Move up a "row" (assume 3 per row)
                    if current >= 3 {
                        selected.set(current - 3);
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let current = selected.get();
                    // Move down a "row"
                    if current + 3 < count {
                        selected.set(current + 3);
                    }
                }
                // Enter block - navigate into the selected block
                KeyCode::Enter => {
                    if let Some(ref callback) = on_enter_block {
                        if let Some(block) = blocks_for_handler.get(selected.get()) {
                            if block.can_enter {
                                callback(block.id.clone(), TraversalDepth::Interior);
                            }
                        }
                    }
                }
                // Go home - navigate to home block
                KeyCode::Char('h') => {
                    if let Some(ref callback) = on_go_home {
                        callback();
                    }
                }
                // Back to street - reset to street view
                KeyCode::Char('b') => {
                    if let Some(ref callback) = on_back_to_street {
                        callback();
                    }
                }
                _ => {}
            },
            _ => {}
        }
    });

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: 100pct,
            height: 100pct,
        ) {
            // Header
            View(
                padding: 1,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
            ) {
                Text(content: "Neighborhood", weight: Weight::Bold, color: Theme::PRIMARY)
            }

            // Main content
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                gap: 1,
                padding: 1,
            ) {
                // Traversal info
                TraversalInfo(depth: depth, neighborhood_name: neighborhood_name)

                // Block grid
                BlockGrid(blocks: blocks, selected_index: current_selected)
            }

            // Key hints
            KeyHintsBar(hints: hints)
        }
    }
}

/// Run the neighborhood screen with sample data
pub async fn run_neighborhood_screen() -> std::io::Result<()> {
    let blocks = vec![
        BlockSummary::new("b1")
            .with_name("My Block")
            .with_residents(3)
            .home(),
        BlockSummary::new("b2")
            .with_name("Alice's Block")
            .with_residents(5)
            .accessible(),
        BlockSummary::new("b3")
            .with_name("Bob's Block")
            .with_residents(2)
            .accessible(),
        BlockSummary::new("b4").with_residents(8), // Full, locked
        BlockSummary::new("b5")
            .with_name("Community")
            .with_residents(4)
            .accessible(),
    ];

    element! {
        NeighborhoodScreen(
            neighborhood_name: "Downtown".to_string(),
            blocks: blocks,
            depth: TraversalDepth::Street,
        )
    }
    .fullscreen()
    .await
}
