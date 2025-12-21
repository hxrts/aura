//! Footer component with fixed 3-row layout and 6-column grid.
//!
//! The footer occupies the bottom 3 rows of the TUI and contains:
//! - Row 1: Top border/separator
//! - Row 2-3: Key hints (columns 1-5) + sync status (column 6)
//!
//! The 6-column grid aligns with the nav bar's 6 screen tabs.

use crate::tui::layout::dim;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::KeyHint;
use iocraft::prelude::*;

/// Width of each column (6 columns across 80 chars)
const COL_WIDTH: u16 = dim::TOTAL_WIDTH / 6; // 13 chars each
/// Width for hints area (columns 1-5)
const HINTS_WIDTH: u16 = COL_WIDTH * 5; // 65 chars
/// Width for the divider
const DIVIDER_WIDTH: u16 = 1;
/// Width for the status column (column 6, minus divider)
const STATUS_COL_WIDTH: u16 = dim::TOTAL_WIDTH - HINTS_WIDTH - DIVIDER_WIDTH;

/// Props for Footer
#[derive(Default, Props)]
pub struct FooterProps {
    /// Screen-specific key hints (top row)
    pub hints: Vec<KeyHint>,
    /// Global key hints including navigation (bottom row)
    pub global_hints: Vec<KeyHint>,
    /// Whether the footer is disabled (darkened, indicating hotkeys are inactive)
    pub disabled: bool,
    /// Whether sync is in progress
    pub syncing: bool,
    /// Last sync time (ms since epoch), None if never synced
    pub last_sync_time: Option<u64>,
    /// Number of known peers
    pub peer_count: usize,
}

/// Format a timestamp as relative time (e.g., "2m ago", "1h ago")
fn format_relative_time(ts_ms: u64) -> String {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let elapsed_ms = now_ms.saturating_sub(ts_ms);
    let elapsed_secs = elapsed_ms / 1000;

    if elapsed_secs < 60 {
        "just now".to_string()
    } else if elapsed_secs < 3600 {
        format!("{}m ago", elapsed_secs / 60)
    } else if elapsed_secs < 86400 {
        format!("{}h ago", elapsed_secs / 3600)
    } else {
        format!("{}d ago", elapsed_secs / 86400)
    }
}

/// Fixed 3-row footer with 6-column grid for the TUI.
///
/// Layout:
/// ```text
/// ├─────────────────────────────────────────────────────────────────┬────────────┤ Row 1: Border
/// │ [Esc] back  [Enter] select  [Tab] next  [↑↓] navigate           │ Synced 2m  │ Row 2: Hints + status
/// │ [1-6] screen  [?] help                                          │ 3 peers    │ Row 3: Hints + peers
/// ```
#[component]
pub fn Footer(props: &FooterProps) -> impl Into<AnyElement<'static>> {
    // Format screen-specific hints (top row)
    let screen_hints_text: Vec<String> = props
        .hints
        .iter()
        .map(|h| format!("[{}] {}", h.key, h.description))
        .collect();

    // Format global hints (bottom row)
    let global_hints_text: Vec<String> = props
        .global_hints
        .iter()
        .map(|h| format!("[{}] {}", h.key, h.description))
        .collect();

    // Use darker colors when disabled (insert mode active)
    let border_color = if props.disabled {
        Theme::BG_DARK
    } else {
        Theme::BORDER
    };
    let text_color = if props.disabled {
        Theme::TEXT_DISABLED
    } else {
        Theme::TEXT_MUTED
    };

    // Build sync status text
    let sync_status = if props.syncing {
        "Syncing...".to_string()
    } else if let Some(ts) = props.last_sync_time {
        format!("Synced {}", format_relative_time(ts))
    } else {
        "Not synced".to_string()
    };

    let sync_color = if props.disabled {
        Theme::TEXT_DISABLED
    } else if props.syncing {
        Theme::WARNING
    } else if props.last_sync_time.is_some() {
        Theme::SUCCESS
    } else {
        Theme::TEXT_MUTED
    };

    let peer_status = format!("{} peers", props.peer_count);

    element! {
        View(
            width: dim::TOTAL_WIDTH,
            height: dim::FOOTER_HEIGHT,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::Hidden,
        ) {
            // Row 1: Top border (1 row)
            View(
                width: 100pct,
                height: dim::FOOTER_BORDER_HEIGHT,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: border_color,
            )

            // Row 2: Screen-specific hints (cols 1-5) + divider + sync status (col 6)
            View(
                width: 100pct,
                height: 1u16,
                flex_direction: FlexDirection::Row,
                overflow: Overflow::Hidden,
            ) {
                // Columns 1-5: Screen hints in fixed-width columns, left-justified
                View(
                    width: HINTS_WIDTH,
                    height: 1u16,
                    flex_direction: FlexDirection::Row,
                    padding_left: Spacing::SM,
                    overflow: Overflow::Hidden,
                ) {
                    #(screen_hints_text.iter().map(|hint| {
                        let color = text_color;
                        element! {
                            View(
                                width: COL_WIDTH,
                                height: 1u16,
                            ) {
                                Text(content: hint.clone(), color: color)
                            }
                        }
                    }))
                }

                // Divider
                Text(content: "│", color: border_color)

                // Column 6: Sync status (left-justified with padding)
                View(
                    width: STATUS_COL_WIDTH,
                    height: 1u16,
                    flex_direction: FlexDirection::Row,
                    padding_left: Spacing::XS,
                    overflow: Overflow::Hidden,
                ) {
                    Text(content: sync_status, color: sync_color)
                }
            }

            // Row 3: Global hints (cols 1-5) + divider + peer count (col 6)
            View(
                width: 100pct,
                height: 1u16,
                flex_direction: FlexDirection::Row,
                overflow: Overflow::Hidden,
            ) {
                // Columns 1-5: Global hints in fixed-width columns, left-justified
                View(
                    width: HINTS_WIDTH,
                    height: 1u16,
                    flex_direction: FlexDirection::Row,
                    padding_left: Spacing::SM,
                    overflow: Overflow::Hidden,
                ) {
                    #(global_hints_text.iter().map(|hint| {
                        let color = text_color;
                        element! {
                            View(
                                width: COL_WIDTH,
                                height: 1u16,
                            ) {
                                Text(content: hint.clone(), color: color)
                            }
                        }
                    }))
                }

                // Divider
                Text(content: "│", color: border_color)

                // Column 6: Peer count (left-justified with padding)
                View(
                    width: STATUS_COL_WIDTH,
                    height: 1u16,
                    flex_direction: FlexDirection::Row,
                    padding_left: Spacing::XS,
                    overflow: Overflow::Hidden,
                ) {
                    Text(content: peer_status, color: text_color)
                }
            }
        }
    }
}

/// Footer with no hints (empty/minimal)
#[component]
pub fn EmptyFooter() -> impl Into<AnyElement<'static>> {
    element! {
        View(
            width: dim::TOTAL_WIDTH,
            height: dim::FOOTER_HEIGHT,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::Hidden,
        ) {
            // Row 1: Top border
            View(
                width: 100pct,
                height: dim::FOOTER_BORDER_HEIGHT,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            )

            // Row 2-3: Empty space
            View(
                width: 100pct,
                height: dim::KEY_HINTS_HEIGHT,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_footer_dimensions() {
        // Footer should be exactly 3 rows (1 border + 2 hints)
        assert_eq!(dim::FOOTER_HEIGHT, 3);
        assert_eq!(dim::FOOTER_BORDER_HEIGHT, 1);
        assert_eq!(dim::KEY_HINTS_HEIGHT, 2);
    }

    #[test]
    fn test_key_hint_formatting() {
        let hints = [KeyHint::new("Esc", "back"), KeyHint::new("Enter", "select")];

        let formatted: Vec<String> = hints
            .iter()
            .map(|h| format!("[{}] {}", h.key, h.description))
            .collect();

        assert_eq!(formatted[0], "[Esc] back");
        assert_eq!(formatted[1], "[Enter] select");
    }

    #[test]
    fn test_column_widths() {
        // Verify widths add up to total
        assert_eq!(
            HINTS_WIDTH + DIVIDER_WIDTH + STATUS_COL_WIDTH,
            dim::TOTAL_WIDTH
        );
    }

    #[test]
    fn test_format_relative_time() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // Test "just now"
        assert_eq!(format_relative_time(now_ms), "just now");

        // Test minutes ago
        let two_min_ago = now_ms - 120_000;
        assert_eq!(format_relative_time(two_min_ago), "2m ago");

        // Test hours ago
        let two_hr_ago = now_ms - 7_200_000;
        assert_eq!(format_relative_time(two_hr_ago), "2h ago");

        // Test days ago
        let two_days_ago = now_ms - 172_800_000;
        assert_eq!(format_relative_time(two_days_ago), "2d ago");
    }
}
