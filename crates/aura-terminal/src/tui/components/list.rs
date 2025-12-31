//! # List Components
//!
//! Navigable list with selection state.

use iocraft::prelude::*;

use crate::tui::theme::{focus_border_color, list_item_colors_with_muted, Borders, Spacing, Theme};

// =============================================================================
// List Item
// =============================================================================

/// Props for ListItem
#[derive(Default, Props)]
pub struct ListItemProps {
    /// Primary label text
    pub label: String,
    /// Optional secondary/description text
    pub description: String,
    /// Whether this item is currently selected
    pub selected: bool,
    /// Whether this item is highlighted (hovered)
    pub highlighted: bool,
    /// Optional icon/prefix character
    pub icon: String,
}

/// A single item in a selectable list
#[component]
pub fn ListItem(props: &ListItemProps) -> impl Into<AnyElement<'static>> {
    // Use consistent list item colors for all scrollable components
    let (bg, label_color, desc_color) = if props.highlighted && !props.selected {
        (
            Theme::BG_HOVER,
            Theme::LIST_TEXT_NORMAL,
            Theme::LIST_TEXT_MUTED,
        )
    } else {
        list_item_colors_with_muted(props.selected)
    };

    let icon_color = Theme::SECONDARY;

    let label = props.label.clone();
    let description = props.description.clone();
    let icon = props.icon.clone();
    let has_icon = !icon.is_empty();
    let has_desc = !description.is_empty();

    element! {
        View(
            flex_direction: FlexDirection::Row,
            background_color: bg,
            padding_left: Spacing::LIST_ITEM_PADDING,
            padding_right: Spacing::LIST_ITEM_PADDING,
            gap: Spacing::XS,
        ) {
            #(if has_icon {
                Some(element! {
                    Text(content: icon, color: icon_color)
                })
            } else {
                None
            })
            View(flex_direction: FlexDirection::Column, flex_grow: 1.0) {
                Text(content: label, color: label_color, weight: Weight::Bold)
                #(if has_desc {
                    Some(element! {
                        Text(content: description, color: desc_color)
                    })
                } else {
                    None
                })
            }
            #(if props.selected {
                Some(element! {
                    Text(content: "→", color: Theme::LIST_TEXT_SELECTED)
                })
            } else {
                None
            })
        }
    }
}

// =============================================================================
// List
// =============================================================================

/// A list item for the List component
#[derive(Clone, Debug, Default)]
pub struct ListEntry {
    /// Unique identifier
    pub id: String,
    /// Display label
    pub label: String,
    /// Optional description
    pub description: String,
    /// Optional icon
    pub icon: String,
}

impl ListEntry {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: String::new(),
            icon: String::new(),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = icon.into();
        self
    }
}

/// Props for List
#[derive(Default, Props)]
pub struct ListProps {
    /// Items to display
    pub items: Vec<ListEntry>,
    /// Currently selected index
    pub selected_index: usize,
    /// Whether the list has focus
    pub focused: bool,
    /// Optional title for the list
    pub title: String,
    /// Show border around the list
    pub bordered: bool,
}

/// A navigable list with keyboard selection
///
/// State management handled by parent component.
#[component]
pub fn List(props: &ListProps) -> impl Into<AnyElement<'static>> {
    let items = props.items.clone();
    let selected = props.selected_index;
    let focused = props.focused;
    let title = props.title.clone();
    let bordered = props.bordered;
    let has_title = !title.is_empty();

    let content = element! {
        View(flex_direction: FlexDirection::Column) {
            #(if has_title {
                Some(element! {
                    View(
                        padding_bottom: Spacing::XS,
                        border_style: BorderStyle::Single,
                        border_edges: Edges::Bottom,
                        border_color: Theme::BORDER,
                    ) {
                        Text(content: title, weight: Weight::Bold, color: Theme::PRIMARY)
                    }
                })
            } else {
                None
            })
            View(flex_direction: FlexDirection::Column, gap: 0) {
                #(items.into_iter().enumerate().map(|(idx, entry)| {
                    let is_selected = idx == selected;
                    element! {
                        ListItem(
                            label: entry.label,
                            description: entry.description,
                            icon: entry.icon,
                            selected: is_selected,
                            highlighted: false,
                        )
                    }
                }))
            }
        }
    };

    if bordered {
        element! {
            View(
                border_style: Borders::LIST,
                border_color: focus_border_color(focused),
                padding: Spacing::PANEL_PADDING,
            ) {
                #(content)
            }
        }
    } else {
        content
    }
}

/// Helper to navigate list selection
#[must_use]
pub fn navigate_list(current: usize, total: usize, direction: ListNavigation) -> usize {
    if total == 0 {
        return 0;
    }
    match direction {
        ListNavigation::Up => {
            if current == 0 {
                total - 1 // wrap to bottom
            } else {
                current - 1
            }
        }
        ListNavigation::Down => {
            if current + 1 >= total {
                0 // wrap to top
            } else {
                current + 1
            }
        }
        ListNavigation::First => 0,
        ListNavigation::Last => total - 1,
    }
}

/// Navigation direction for list
#[derive(Clone, Copy, Debug)]
pub enum ListNavigation {
    Up,
    Down,
    First,
    Last,
}
