//! # Generic TabBar Component
//!
//! Reusable tab/filter bar for navigation within screens.

use iocraft::prelude::*;

use crate::tui::theme::{Spacing, Theme};

/// Configuration for a single tab
#[derive(Clone, Default)]
pub struct TabItem {
    /// Display label for the tab
    pub label: String,
    /// Optional badge count (e.g., pending items)
    pub badge: Option<usize>,
}

impl TabItem {
    /// Create a new tab with just a label
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            badge: None,
        }
    }

    /// Create a new tab with a label and badge count
    pub fn with_badge(label: impl Into<String>, count: usize) -> Self {
        Self {
            label: label.into(),
            badge: Some(count),
        }
    }
}

/// Props for TabBar
#[derive(Default, Props)]
pub struct TabBarProps {
    /// The tab items to display
    pub tabs: Vec<TabItem>,
    /// Index of the currently active tab
    pub active_index: usize,
    /// Gap between tabs (defaults to MD)
    pub gap: Option<u32>,
}

/// Generic tab bar component
///
/// Displays a horizontal row of tabs with active highlighting.
///
/// ## Example
///
/// ```rust,ignore
/// let tabs = vec![
///     TabItem::new("All"),
///     TabItem::new("Sent"),
///     TabItem::with_badge("Pending", pending_count),
/// ];
///
/// element! {
///     TabBar(tabs: tabs, active_index: 0)
/// }
/// ```
#[component]
pub fn TabBar(props: &TabBarProps) -> impl Into<AnyElement<'static>> {
    let tabs = props.tabs.clone();
    let active = props.active_index;
    let gap = props.gap.unwrap_or(Spacing::MD);

    element! {
        View(
            flex_direction: FlexDirection::Row,
            width: 100pct,
            overflow: Overflow::Hidden,
            gap: gap,
            padding: Spacing::PANEL_PADDING,
            border_style: BorderStyle::Single,
            border_edges: Edges::Bottom,
            border_color: Theme::BORDER,
        ) {
            #(tabs.iter().enumerate().map(|(idx, tab)| {
                let is_active = idx == active;
                let color = if is_active { Theme::PRIMARY } else { Theme::TEXT_MUTED };
                let weight = if is_active { Weight::Bold } else { Weight::Normal };

                // Format label with optional badge
                let display_text = match tab.badge {
                    Some(count) if count > 0 => format!("{} ({})", tab.label, count),
                    _ => tab.label.clone(),
                };

                element! {
                    Text(content: display_text, color: color, weight: weight)
                }
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_item_new() {
        let tab = TabItem::new("Test");
        assert_eq!(tab.label, "Test");
        assert!(tab.badge.is_none());
    }

    #[test]
    fn test_tab_item_with_badge() {
        let tab = TabItem::with_badge("Pending", 5);
        assert_eq!(tab.label, "Pending");
        assert_eq!(tab.badge, Some(5));
    }

    #[test]
    fn test_tab_item_with_zero_badge() {
        let tab = TabItem::with_badge("Empty", 0);
        assert_eq!(tab.label, "Empty");
        assert_eq!(tab.badge, Some(0));
    }
}
