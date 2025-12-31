//! # Scrollable Content Area
//!
//! A container that enables vertical scrolling for overflow content.

use iocraft::prelude::*;

/// Props for Scrollable
#[derive(Default, Props)]
pub struct ScrollableProps {
    /// Content items to render
    pub items: Vec<String>,
    /// Current scroll offset (first visible item index)
    pub scroll_offset: usize,
    /// Number of visible items (viewport height)
    pub visible_count: usize,
    /// Whether scrolling is enabled
    pub scrollable: bool,
}

/// A scrollable content area
///
/// Displays a window of items based on scroll_offset.
/// Parent component manages scroll state.
#[component]
pub fn Scrollable(props: &ScrollableProps) -> impl Into<AnyElement<'static>> {
    let items = props.items.clone();
    let offset = props.scroll_offset;
    let visible = if props.visible_count == 0 {
        items.len()
    } else {
        props.visible_count
    };

    // Calculate visible window
    let end = (offset + visible).min(items.len());
    let visible_items: Vec<String> = items.into_iter().skip(offset).take(end - offset).collect();

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            overflow: Overflow::Scroll,
        ) {
            #(visible_items.into_iter().map(|content| {
                element! {
                    Text(content: content)
                }
            }))
        }
    }
}

/// Helper to calculate scroll bounds
#[must_use]
pub fn calculate_scroll(
    current_offset: usize,
    total_items: usize,
    visible_count: usize,
    direction: ScrollDirection,
) -> usize {
    match direction {
        ScrollDirection::Up => current_offset.saturating_sub(1),
        ScrollDirection::Down => {
            let max_offset = total_items.saturating_sub(visible_count);
            (current_offset + 1).min(max_offset)
        }
        ScrollDirection::PageUp => current_offset.saturating_sub(visible_count),
        ScrollDirection::PageDown => {
            let max_offset = total_items.saturating_sub(visible_count);
            (current_offset + visible_count).min(max_offset)
        }
        ScrollDirection::Home => 0,
        ScrollDirection::End => total_items.saturating_sub(visible_count),
    }
}

/// Scroll direction for helper function
#[derive(Clone, Copy, Debug)]
pub enum ScrollDirection {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
}
