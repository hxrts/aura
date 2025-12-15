//! Nav bar component with fixed 3-row layout.
//!
//! The nav bar occupies the top 3 rows of the TUI and contains:
//! - Row 1: Screen tabs
//! - Row 2: Status indicators (sync, peers)
//! - Row 3: Bottom border/separator

use crate::tui::layout::dim;
use crate::tui::screens::Screen;
use crate::tui::theme::{Spacing, Theme};
use iocraft::prelude::*;

/// Props for NavBar
#[derive(Default, Props)]
pub struct NavBarProps {
    /// Currently active screen
    pub active_screen: Screen,
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

/// Fixed 3-row nav bar for the TUI.
///
/// Layout:
/// ```text
/// ┌──────────────────────────────────────────────────────────────────────────────┐
/// │ Block  Chat  Contacts  Recovery  Settings  Help                              │ Row 1: Tabs
/// │                                                   Synced 2m ago │ 3 peers    │ Row 2: Status
/// ├──────────────────────────────────────────────────────────────────────────────┤ Row 3: Border
/// ```
#[component]
pub fn NavBar(props: &NavBarProps) -> impl Into<AnyElement<'static>> {
    let active = props.active_screen;

    // Build sync status text
    let sync_status = if props.syncing {
        "Syncing...".to_string()
    } else if let Some(ts) = props.last_sync_time {
        format!("Synced {}", format_relative_time(ts))
    } else {
        "Not synced".to_string()
    };

    let sync_color = if props.syncing {
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
            height: dim::NAV_HEIGHT,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::Hidden,
        ) {
            // Row 1: Screen tabs (1 row)
            View(
                width: 100pct,
                height: 1,
                flex_direction: FlexDirection::Row,
                gap: Spacing::SM,
                padding_left: Spacing::SM,
                padding_right: Spacing::SM,
            ) {
                #(Screen::all().iter().map(|&screen| {
                    let is_active = screen == active;
                    let color = if is_active { Theme::PRIMARY } else { Theme::TEXT_MUTED };
                    let weight = if is_active { Weight::Bold } else { Weight::Normal };
                    let title = screen.name().to_string();
                    element! {
                        Text(content: title, color: color, weight: weight)
                    }
                }))
            }

            // Row 2: Status info (1 row)
            View(
                width: 100pct,
                height: 1,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::End,
                gap: Spacing::MD,
                padding_left: Spacing::SM,
                padding_right: Spacing::SM,
            ) {
                Text(content: sync_status, color: sync_color)
                Text(content: " │ ", color: Theme::BORDER)
                Text(content: peer_status, color: Theme::TEXT_MUTED)
            }

            // Row 3: Bottom border (1 row)
            View(
                width: 100pct,
                height: 1,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nav_bar_dimensions() {
        // Nav bar should be exactly 3 rows
        assert_eq!(dim::NAV_HEIGHT, 3);
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
