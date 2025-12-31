//! # Threshold Configuration Modal
//!
//! Modal for configuring recovery threshold (k of n).

use iocraft::prelude::*;

use aura_app::ui::types::normalize_recovery_threshold;

use crate::tui::components::{modal_header, status_message, ModalHeaderProps, ModalStatus};
use crate::tui::layout::dim;
use crate::tui::theme::{Borders, Spacing, Theme};

/// State for threshold configuration modal
#[derive(Clone, Debug, Default)]
pub struct ThresholdState {
    /// Whether the modal is visible
    pub visible: bool,
    /// Current threshold k value (required signatures)
    pub threshold_k: u8,
    /// Total guardians (n)
    pub threshold_n: u8,
    /// Original k value (for cancel restoration)
    original_k: u8,
    /// Error message if any
    pub error: Option<String>,
    /// Whether submission is in progress
    pub submitting: bool,
}

impl ThresholdState {
    /// Create new state with initial values
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the modal with current threshold values
    pub fn show(&mut self, k: u8, n: u8) {
        self.visible = true;
        self.threshold_k = k;
        self.threshold_n = n;
        self.original_k = k;
        self.error = None;
        self.submitting = false;
    }

    /// Hide the modal (cancel)
    pub fn hide(&mut self) {
        self.visible = false;
        self.threshold_k = self.original_k;
        self.error = None;
        self.submitting = false;
    }

    /// Increment threshold k (up to n)
    pub fn increment(&mut self) {
        let next = self.threshold_k.saturating_add(1);
        self.threshold_k = normalize_recovery_threshold(next, self.threshold_n);
        self.error = None;
    }

    /// Decrement threshold k (down to 1)
    pub fn decrement(&mut self) {
        let next = self.threshold_k.saturating_sub(1);
        self.threshold_k = normalize_recovery_threshold(next, self.threshold_n);
        self.error = None;
    }

    /// Check if value has changed from original
    pub fn has_changed(&self) -> bool {
        self.threshold_k != self.original_k
    }

    /// Check if can submit (value changed and valid)
    pub fn can_submit(&self) -> bool {
        self.has_changed()
            && self.threshold_k == normalize_recovery_threshold(self.threshold_k, self.threshold_n)
            && !self.submitting
    }

    /// Mark as submitting
    pub fn start_submitting(&mut self) {
        self.submitting = true;
    }

    /// Set error message
    pub fn set_error(&mut self, error: &str) {
        self.error = Some(error.to_string());
        self.submitting = false;
    }

    /// Get current threshold value
    pub fn get_threshold(&self) -> u8 {
        self.threshold_k
    }
}

/// Props for ThresholdModal
#[derive(Default, Props)]
pub struct ThresholdModalProps {
    /// Whether the modal is visible
    pub visible: bool,
    /// Whether the modal is focused
    pub focused: bool,
    /// Current threshold k (required signatures)
    pub threshold_k: u8,
    /// Total guardians n
    pub threshold_n: u8,
    /// Whether value has changed
    pub has_changed: bool,
    /// Error message
    pub error: String,
    /// Whether submitting
    pub submitting: bool,
}

/// Modal for threshold configuration
#[component]
pub fn ThresholdModal(props: &ThresholdModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        };
    }

    let k = props.threshold_k;
    let n = props.threshold_n;
    let error = props.error.clone();
    let can_decrement = k > 1;
    let can_increment = k < n;
    let can_submit = props.has_changed && !props.submitting;

    // Header props
    let header_props = ModalHeaderProps::new("Configure Recovery Threshold");

    // Status for error display
    let status = if !error.is_empty() {
        ModalStatus::Error(error)
    } else {
        ModalStatus::Idle
    };

    // Build threshold display text
    let threshold_text = format!("{k} of {n} guardians required");

    // Security level hint based on threshold (uses portable function)
    let security_hint = aura_app::ui::types::security_level_hint(k as u32, n as u32);

    element! {
        View(
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            flex_direction: FlexDirection::Column,
            background_color: Theme::BG_MODAL,
            border_style: Borders::PRIMARY,
            border_color: if props.focused { Theme::BORDER_FOCUS } else { Theme::BORDER },
            overflow: Overflow::Hidden,
        ) {
            // Title bar
            #(Some(modal_header(&header_props).into()))

            // Content area - fills available space
            View(
                width: 100pct,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                padding: Spacing::MODAL_PADDING,
                gap: Spacing::XS,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                overflow: Overflow::Hidden,
            ) {
                // Threshold selector
                View(
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    gap: Spacing::SM,
                ) {
                    // Decrement button
                    View(
                        padding_left: Spacing::XS,
                        padding_right: Spacing::XS,
                        border_style: Borders::INPUT,
                        border_color: if can_decrement { Theme::SECONDARY } else { Theme::BORDER },
                    ) {
                        Text(
                            content: "◄",
                            color: if can_decrement { Theme::SECONDARY } else { Theme::TEXT_MUTED },
                        )
                    }

                    // Current value display
                    View(
                        padding_left: Spacing::SM,
                        padding_right: Spacing::SM,
                        border_style: Borders::INPUT,
                        border_color: Theme::PRIMARY,
                    ) {
                        Text(
                            content: format!("{}", k),
                            weight: Weight::Bold,
                            color: Theme::PRIMARY,
                        )
                    }

                    Text(content: "of", color: Theme::TEXT_MUTED)

                    View(
                        padding_left: Spacing::SM,
                        padding_right: Spacing::SM,
                        border_style: Borders::INPUT,
                        border_color: Theme::BORDER,
                    ) {
                        Text(content: format!("{}", n), color: Theme::TEXT)
                    }

                    // Increment button
                    View(
                        padding_left: Spacing::XS,
                        padding_right: Spacing::XS,
                        border_style: Borders::INPUT,
                        border_color: if can_increment { Theme::SECONDARY } else { Theme::BORDER },
                    ) {
                        Text(
                            content: "►",
                            color: if can_increment { Theme::SECONDARY } else { Theme::TEXT_MUTED },
                        )
                    }
                }

                // Threshold description
                View(
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                ) {
                    Text(content: threshold_text, color: Theme::TEXT)
                    Text(content: security_hint, color: Theme::TEXT_MUTED)
                }

                // Error display
                #(Some(status_message(&status).into()))

                // Help text
                View(
                    flex_direction: FlexDirection::Column,
                    padding_top: Spacing::XS,
                ) {
                    Text(
                        content: "The threshold determines how many guardians",
                        color: Theme::TEXT_MUTED,
                    )
                    Text(
                        content: "must approve to recover your account.",
                        color: Theme::TEXT_MUTED,
                    )
                }
            }

            // Key hints
            View(
                width: 100pct,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Center,
                padding: Spacing::PANEL_PADDING,
                gap: Spacing::LG,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            ) {
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "←/→", weight: Weight::Bold, color: Theme::SECONDARY)
                    Text(content: "Adjust", color: Theme::TEXT_MUTED)
                }
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "Esc", weight: Weight::Bold, color: Theme::SECONDARY)
                    Text(content: "Cancel", color: Theme::TEXT_MUTED)
                }
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(
                        content: "Enter",
                        weight: Weight::Bold,
                        color: if can_submit { Theme::PRIMARY } else { Theme::TEXT_MUTED },
                    )
                    Text(
                        content: "Save",
                        color: if can_submit { Theme::TEXT } else { Theme::TEXT_MUTED },
                    )
                }
            }
        }
    }
}
