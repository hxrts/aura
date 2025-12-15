//! # Conflict Resolution Modal
//!
//! Modal component for displaying and resolving operation conflicts.
//!
//! When an optimistic operation (Category B) is rolled back due to a conflict
//! with another admin's concurrent action, this modal provides UI for the user
//! to understand what happened and optionally choose a resolution.

use iocraft::prelude::*;

use crate::tui::theme::{Icons, Spacing, Theme};
use crate::tui::types::{ConflictResolution, OperationConflict};

/// Props for ConflictModal
#[derive(Default, Props)]
pub struct ConflictModalProps {
    /// Whether the modal is visible
    pub visible: bool,
    /// The conflict to display
    pub conflict: Option<OperationConflict>,
    /// Currently focused resolution option index
    pub focused_index: usize,
}

/// Modal for displaying and resolving operation conflicts
#[component]
pub fn ConflictModal(props: &ConflictModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! { View {} };
    }

    let conflict = match &props.conflict {
        Some(c) => c.clone(),
        None => return element! { View {} },
    };

    let title = format!("{} Conflict Detected", conflict.operation_type.icon());
    let summary = conflict.summary();
    let local_action = format!("You: {}", conflict.local_action);
    let remote_action = format!("{}: {}", conflict.remote_actor, conflict.remote_action);

    element! {
        // Modal overlay
        View(
            position: Position::Absolute,
            width: 100pct,
            height: 100pct,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            background_color: Theme::OVERLAY,
        ) {
            // Modal content
            View(
                flex_direction: FlexDirection::Column,
                min_width: 60,
                max_width: 80,
                background_color: Theme::BG_MODAL,
                border_style: BorderStyle::Round,
                border_color: Theme::WARNING,
                padding: Spacing::PANEL_PADDING,
            ) {
                // Header
                View(
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    margin_bottom: Spacing::SM,
                ) {
                    Text(content: title, weight: Weight::Bold, color: Theme::WARNING)
                    Text(content: "[Esc] close", color: Theme::TEXT_MUTED)
                }

                // Divider
                View(
                    width: 100pct,
                    height: 1,
                    background_color: Theme::BORDER,
                    margin_bottom: Spacing::SM,
                ) {}

                // Conflict summary
                View(margin_bottom: Spacing::SM) {
                    Text(content: summary, color: Theme::TEXT, wrap: TextWrap::Wrap)
                }

                // What happened section
                View(
                    flex_direction: FlexDirection::Column,
                    margin_bottom: Spacing::MD,
                    padding: Spacing::XS,
                    border_style: BorderStyle::Round,
                    border_color: Theme::BORDER,
                ) {
                    View(margin_bottom: Spacing::XS) {
                        Text(content: "What happened:", weight: Weight::Bold, color: Theme::TEXT_HIGHLIGHT)
                    }
                    View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                        Text(content: Icons::ARROW_RIGHT, color: Theme::PRIMARY)
                        Text(content: local_action, color: Theme::TEXT, wrap: TextWrap::Wrap)
                    }
                    View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                        Text(content: Icons::CROSS, color: Theme::WARNING)
                        Text(content: remote_action, color: Theme::TEXT, wrap: TextWrap::Wrap)
                    }
                }

                // Resolution options
                View(
                    flex_direction: FlexDirection::Column,
                    margin_bottom: Spacing::SM,
                ) {
                    View(margin_bottom: Spacing::XS) {
                        Text(content: "Resolution options:", weight: Weight::Bold, color: Theme::TEXT_HIGHLIGHT)
                    }
                    #(conflict.available_resolutions.iter().enumerate().map(|(i, resolution)| {
                        let is_focused = i == props.focused_index;
                        let indicator = if is_focused { Icons::ARROW_RIGHT } else { " " };
                        let bg = if is_focused { Theme::BG_SELECTED } else { Color::Reset };
                        let label = resolution.label().to_string();
                        let desc = resolution.description().to_string();

                        element! {
                            View(
                                flex_direction: FlexDirection::Row,
                                background_color: bg,
                                padding_left: Spacing::XS,
                                padding_right: Spacing::XS,
                                gap: Spacing::XS,
                            ) {
                                Text(content: indicator, color: Theme::PRIMARY)
                                Text(content: label, weight: Weight::Bold, color: Theme::TEXT)
                                Text(content: "-", color: Theme::TEXT_MUTED)
                                Text(content: desc, color: Theme::TEXT_MUTED, wrap: TextWrap::Wrap)
                            }
                        }
                    }))
                }

                // Key hints
                View(
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::Center,
                    gap: Spacing::MD,
                    margin_top: Spacing::XS,
                ) {
                    Text(content: "[j/k] Navigate", color: Theme::TEXT_MUTED)
                    Text(content: "[Enter] Apply", color: Theme::TEXT_MUTED)
                    Text(content: "[Esc] Dismiss", color: Theme::TEXT_MUTED)
                }
            }
        }
    }
}

/// Props for ConflictBanner
#[derive(Default, Props)]
pub struct ConflictBannerProps {
    /// Number of pending conflicts
    pub conflict_count: usize,
    /// Whether to show the banner
    pub visible: bool,
}

/// A banner showing pending conflicts (appears at top of screen)
#[component]
pub fn ConflictBanner(props: &ConflictBannerProps) -> impl Into<AnyElement<'static>> {
    if !props.visible || props.conflict_count == 0 {
        return element! { View {} };
    }

    let message = if props.conflict_count == 1 {
        "1 operation was rolled back due to a conflict".to_string()
    } else {
        format!(
            "{} operations were rolled back due to conflicts",
            props.conflict_count
        )
    };

    element! {
        View(
            flex_direction: FlexDirection::Row,
            width: 100pct,
            background_color: Theme::WARNING,
            padding_left: Spacing::SM,
            padding_right: Spacing::SM,
            gap: Spacing::XS,
        ) {
            Text(content: Icons::WARNING, color: Theme::BG_MODAL, weight: Weight::Bold)
            Text(content: message, color: Theme::BG_MODAL)
            Text(content: "[c] View conflicts", color: Theme::BG_MODAL, weight: Weight::Bold)
        }
    }
}

/// State for conflict resolution modal
#[derive(Clone, Debug, Default)]
pub struct ConflictModalState {
    /// Whether the modal is visible
    pub visible: bool,
    /// List of pending conflicts
    pub conflicts: Vec<OperationConflict>,
    /// Currently viewing conflict index
    pub current_conflict_index: usize,
    /// Focused resolution option index
    pub focused_resolution_index: usize,
}

impl ConflictModalState {
    /// Create a new conflict modal state
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a conflict to the list
    pub fn add_conflict(&mut self, conflict: OperationConflict) {
        self.conflicts.push(conflict);
    }

    /// Get the current conflict being viewed
    pub fn current_conflict(&self) -> Option<&OperationConflict> {
        self.conflicts.get(self.current_conflict_index)
    }

    /// Show the modal
    pub fn show(&mut self) {
        self.visible = true;
        self.focused_resolution_index = 0;
    }

    /// Hide the modal
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Navigate to next resolution option
    pub fn next_resolution(&mut self) {
        if let Some(conflict) = self.current_conflict() {
            let max = conflict.available_resolutions.len().saturating_sub(1);
            self.focused_resolution_index = (self.focused_resolution_index + 1).min(max);
        }
    }

    /// Navigate to previous resolution option
    pub fn prev_resolution(&mut self) {
        self.focused_resolution_index = self.focused_resolution_index.saturating_sub(1);
    }

    /// Get the currently selected resolution
    pub fn selected_resolution(&self) -> Option<ConflictResolution> {
        self.current_conflict()
            .and_then(|c| c.available_resolutions.get(self.focused_resolution_index))
            .copied()
    }

    /// Mark current conflict as resolved and move to next (or close if done)
    pub fn resolve_current(&mut self) -> Option<(String, ConflictResolution)> {
        let resolution = self.selected_resolution()?;
        let conflict_id = self.current_conflict()?.id.clone();

        // Mark as resolved
        if let Some(conflict) = self.conflicts.get_mut(self.current_conflict_index) {
            conflict.resolved = true;
            conflict.selected_resolution = Some(resolution);
        }

        // Move to next unresolved conflict or close
        let unresolved: Vec<_> = self
            .conflicts
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.resolved)
            .collect();

        if unresolved.is_empty() {
            self.hide();
        } else {
            self.current_conflict_index = unresolved[0].0;
            self.focused_resolution_index = 0;
        }

        Some((conflict_id, resolution))
    }

    /// Number of unresolved conflicts
    pub fn unresolved_count(&self) -> usize {
        self.conflicts.iter().filter(|c| !c.resolved).count()
    }

    /// Clear all resolved conflicts
    pub fn clear_resolved(&mut self) {
        self.conflicts.retain(|c| !c.resolved);
        self.current_conflict_index = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::types::ConflictOperationType;

    #[test]
    fn test_conflict_modal_state() {
        let mut state = ConflictModalState::new();
        assert!(!state.visible);
        assert_eq!(state.unresolved_count(), 0);

        // Add a conflict
        let conflict = OperationConflict::new("conflict-1", ConflictOperationType::ChannelRename)
            .with_local_action("renamed to #general")
            .with_remote_action("renamed to #main", "Alice")
            .with_target("chat");

        state.add_conflict(conflict);
        assert_eq!(state.unresolved_count(), 1);

        // Show modal
        state.show();
        assert!(state.visible);
        assert!(state.current_conflict().is_some());

        // Navigate resolutions
        state.next_resolution();
        assert_eq!(state.focused_resolution_index, 1);
        state.prev_resolution();
        assert_eq!(state.focused_resolution_index, 0);

        // Resolve
        let result = state.resolve_current();
        assert!(result.is_some());
        let (id, resolution) = result.unwrap();
        assert_eq!(id, "conflict-1");
        assert_eq!(resolution, ConflictResolution::KeepLocal);

        // Should auto-hide when all resolved
        assert!(!state.visible);
        assert_eq!(state.unresolved_count(), 0);
    }

    #[test]
    fn test_conflict_notification_message() {
        let conflict = OperationConflict::new("c1", ConflictOperationType::MemberRemoval)
            .with_local_action("kicked Bob")
            .with_remote_action("kicked Bob", "Alice")
            .with_target("general");

        let msg = conflict.notification_message();
        assert!(msg.contains("Member removal"));
        assert!(msg.contains("Alice"));
        assert!(msg.contains("general"));
    }
}
