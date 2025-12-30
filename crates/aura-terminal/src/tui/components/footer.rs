//! Footer component with fixed 3-row layout and 6-column grid.
//!
//! The footer occupies the bottom 3 rows of the TUI and contains:
//! - Row 1: Top border/separator
//! - Row 2: Screen-specific key hints (columns 1-5) + sync status (column 6)
//! - Row 3: Global key hints (columns 1-5) + peer count (column 6)
//!
//! The 6-column grid uses the same column width (13 chars) and padding as the
//! nav bar, ensuring perfect vertical alignment between nav tabs and footer hints.

use aura_app::ui::signals::NetworkStatus;

use crate::tui::layout::dim;
use crate::tui::theme::Theme;
use crate::tui::types::KeyHint;
use iocraft::prelude::*;

/// Width of each hint column (5 columns for hints)
const COL_WIDTH: u16 = dim::TOTAL_WIDTH / 6; // 13 chars each

/// Width of status column (remaining space after 5 hint columns)
const STATUS_COL_WIDTH: u16 = dim::TOTAL_WIDTH - (5 * COL_WIDTH); // 15 chars

/// Props for Footer
#[derive(Default, Props)]
pub struct FooterProps {
    /// Screen-specific key hints (top row)
    pub hints: Vec<KeyHint>,
    /// Global key hints including navigation (bottom row)
    pub global_hints: Vec<KeyHint>,
    /// Whether the footer is disabled (darkened, indicating hotkeys are inactive)
    pub disabled: bool,
    /// Unified network status (disconnected, no peers, syncing, synced)
    pub network_status: NetworkStatus,
    /// Current time (ms since epoch) from runtime, for relative formatting
    pub now_ms: Option<u64>,
    /// Transport-level peers (active network connections)
    pub transport_peers: usize,
    /// Online contacts (people you know who are currently online)
    pub known_online: usize,
}

/// Format a timestamp as relative time (e.g., "2m ago", "1h ago")
fn format_relative_time(now_ms: u64, ts_ms: u64) -> String {
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
/// Layout (columns align with nav bar tabs):
#[component]
pub fn Footer(props: &FooterProps) -> impl Into<AnyElement<'static>> {
    // Format screen-specific hints (top row), padded to 5 columns
    let mut screen_hints_text: Vec<String> = props
        .hints
        .iter()
        .take(5)
        .map(|h| format!("[{}] {}", h.key, h.description))
        .collect();
    screen_hints_text.resize(5, String::new());

    // Format global hints (bottom row), padded to 5 columns
    let mut global_hints_text: Vec<String> = props
        .global_hints
        .iter()
        .take(5)
        .map(|h| format!("[{}] {}", h.key, h.description))
        .collect();
    global_hints_text.resize(5, String::new());

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

    // Build network status text and color based on unified NetworkStatus
    let (sync_status, sync_color) = if props.disabled {
        // Disabled state uses muted colors regardless of network status
        let status = match &props.network_status {
            NetworkStatus::Disconnected => "Disconnected".to_string(),
            NetworkStatus::NoPeers => "No contacts".to_string(),
            NetworkStatus::Syncing => "Syncing...".to_string(),
            NetworkStatus::Synced { last_sync_ms } => {
                if let Some(now_ms) = props.now_ms {
                    format!("Synced {}", format_relative_time(now_ms, *last_sync_ms))
                } else {
                    "Synced".to_string()
                }
            }
        };
        (status, Theme::TEXT_DISABLED)
    } else {
        match &props.network_status {
            NetworkStatus::Disconnected => ("Disconnected".to_string(), Theme::ERROR),
            NetworkStatus::NoPeers => ("No contacts".to_string(), Theme::WARNING),
            NetworkStatus::Syncing => ("Syncing...".to_string(), Theme::WARNING),
            NetworkStatus::Synced { last_sync_ms } => {
                let status = if let Some(now_ms) = props.now_ms {
                    format!("Synced {}", format_relative_time(now_ms, *last_sync_ms))
                } else {
                    "Synced".to_string()
                };
                (status, Theme::SUCCESS)
            }
        }
    };

    // Format: "123 P, 45 On" - must fit in STATUS_COL_WIDTH (15 chars)
    // Max realistic: "999 P, 99 On" = 12 chars
    let peer_status = format!("{} P, {} On", props.transport_peers, props.known_online);

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

            // Row 2: Screen-specific hints (cols 1-5) + sync status (col 6)
            // Uses same 6-column layout as nav bar for perfect alignment
            View(
                width: 100pct,
                height: 1u16,
                flex_direction: FlexDirection::Row,
                overflow: Overflow::Hidden,
            ) {
                // Columns 1-5: Screen hints in fixed-width columns
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

                // Column 6: Sync status (wider to fit "Synced Xm ago")
                View(
                    width: STATUS_COL_WIDTH,
                    height: 1u16,
                ) {
                    Text(content: sync_status.clone(), color: sync_color)
                }
            }

            // Row 3: Global hints (cols 1-5) + peer count (col 6)
            // Uses same 6-column layout as nav bar for perfect alignment
            View(
                width: 100pct,
                height: 1u16,
                flex_direction: FlexDirection::Row,
                overflow: Overflow::Hidden,
            ) {
                // Columns 1-5: Global hints in fixed-width columns
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

                // Column 6: Peer count (wider to fit "123 Peers | 45 On")
                View(
                    width: STATUS_COL_WIDTH,
                    height: 1u16,
                ) {
                    Text(content: peer_status.clone(), color: text_color)
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
        // Footer uses same 6-column layout as nav bar (13 chars each)
        assert_eq!(COL_WIDTH, dim::TOTAL_WIDTH / 6);
        assert_eq!(COL_WIDTH, 13);
    }

    #[test]
    fn test_format_relative_time() {
        // Use a large enough value to avoid underflow when subtracting days
        let now_ms = 1_000_000_000; // ~11.5 days in ms

        // Test "just now"
        assert_eq!(format_relative_time(now_ms, now_ms), "just now");

        // Test minutes ago
        let two_min_ago = now_ms - 120_000;
        assert_eq!(format_relative_time(now_ms, two_min_ago), "2m ago");

        // Test hours ago
        let two_hr_ago = now_ms - 7_200_000;
        assert_eq!(format_relative_time(now_ms, two_hr_ago), "2h ago");

        // Test days ago
        let two_days_ago = now_ms - 172_800_000;
        assert_eq!(format_relative_time(now_ms, two_days_ago), "2d ago");
    }
}
