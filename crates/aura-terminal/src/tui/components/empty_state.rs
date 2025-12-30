//! # Empty State Component
//!
//! Placeholder for empty lists and screens.

use iocraft::prelude::*;

use crate::tui::theme::{Icons, Spacing, Theme};

/// Props for EmptyState
#[derive(Default, Props)]
pub struct EmptyStateProps {
    /// Icon to display (unicode character)
    pub icon: String,
    /// Title text
    pub title: String,
    /// Description text (optional)
    pub description: String,
    /// Action hint (e.g., "Press Enter to create")
    pub action_hint: String,
}

/// A polished empty state display
#[component]
pub fn EmptyState(props: &EmptyStateProps) -> impl Into<AnyElement<'static>> {
    let icon = props.icon.clone();
    let has_icon = !icon.is_empty();
    let title = props.title.clone();
    let description = props.description.clone();
    let action_hint = props.action_hint.clone();
    let has_description = !description.is_empty();
    let has_action = !action_hint.is_empty();

    element! {
        View(
            flex_grow: 1.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            flex_direction: FlexDirection::Column,
            padding: Spacing::LG,
        ) {
            // Icon (only if explicitly provided)
            #(if has_icon {
                Some(element! {
                    View {
                        Text(content: icon, color: Theme::TEXT_MUTED)
                    }
                })
            } else {
                None
            })
            // Spacer (only if icon shown)
            #(if has_icon {
                Some(element! { View(height: Spacing::XS) })
            } else {
                None
            })
            // Title
            Text(content: title, weight: Weight::Bold, color: Theme::TEXT_MUTED)
            // Description
            #(if has_description {
                Some(element! {
                    View(margin_top: Spacing::XS, max_width: 40u32) {
                        Text(content: description, color: Theme::TEXT_MUTED, align: TextAlign::Center)
                    }
                })
            } else {
                None
            })
            // Action hint
            #(if has_action {
                Some(element! {
                    View(margin_top: Spacing::SM) {
                        Text(content: action_hint, color: Theme::PRIMARY)
                    }
                })
            } else {
                None
            })
        }
    }
}

/// Props for NoResults
#[derive(Default, Props)]
pub struct NoResultsProps {
    /// Search query that yielded no results
    pub query: String,
}

/// Specialized empty state for search with no results
#[component]
pub fn NoResults(props: &NoResultsProps) -> impl Into<AnyElement<'static>> {
    let query = props.query.clone();
    let title = if query.is_empty() {
        "No results".to_string()
    } else {
        format!("No results for \"{query}\"")
    };

    element! {
        EmptyState(
            icon: Icons::CROSS.to_string(),
            title: title,
            description: "Try a different search term".to_string(),
        )
    }
}

/// Props for LoadingState
#[derive(Default, Props)]
pub struct LoadingStateProps {
    /// Loading message
    pub message: String,
    /// Spinner frame index (0-3)
    pub frame: usize,
}

/// A loading state with spinner
#[component]
pub fn LoadingState(props: &LoadingStateProps) -> impl Into<AnyElement<'static>> {
    let frame = props.frame % 4;
    let spinner = Icons::SPINNER_FRAMES[frame];
    let message = if props.message.is_empty() {
        "Loading...".to_string()
    } else {
        props.message.clone()
    };

    element! {
        View(
            flex_grow: 1.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            flex_direction: FlexDirection::Row,
            gap: Spacing::SM,
        ) {
            Text(content: spinner, color: Theme::PRIMARY)
            Text(content: message, color: Theme::TEXT_MUTED)
        }
    }
}
