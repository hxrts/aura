//! # Code Display Modal
//!
//! Shared modal component for displaying codes that need to be shared out-of-band.
//! Used by both device enrollment and invitation creation flows.
//!
//! Supports:
//! - Code display with automatic line wrapping
//! - Status indicator (success, pending, error)
//! - Optional progress tracking
//! - Press 'c' to copy code to clipboard

use iocraft::prelude::*;

use super::{modal_footer, status_message, ModalFooterProps, ModalStatus};
use crate::tui::layout::dim;
use crate::tui::theme::{Borders, Icons, Spacing, Theme};
use crate::tui::types::KeyHint;

/// Status of the code display operation
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CodeDisplayStatus {
    /// Operation successful (green check)
    Success,
    /// Waiting for acceptance (yellow pending)
    #[default]
    Pending,
    /// Operation failed (red cross)
    Error,
}

impl CodeDisplayStatus {
    fn icon(self) -> &'static str {
        match self {
            Self::Success => Icons::CHECK,
            Self::Pending => Icons::PENDING,
            Self::Error => Icons::CROSS,
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Success => Theme::SUCCESS,
            Self::Pending => Theme::WARNING,
            Self::Error => Theme::ERROR,
        }
    }

    fn border_color(self) -> Color {
        match self {
            Self::Success => Theme::SUCCESS,
            Self::Pending => Theme::PRIMARY,
            Self::Error => Theme::ERROR,
        }
    }
}

/// Props for CodeDisplayModal
#[derive(Default, Props)]
pub struct CodeDisplayModalProps {
    /// Whether the modal is visible
    pub visible: bool,
    /// Main title (e.g., "Enroll device: MyPhone" or "Invitation Created")
    pub title: String,
    /// Optional subtitle (e.g., "Type: Contact")
    pub subtitle: String,
    /// Current status
    pub status: CodeDisplayStatus,
    /// Status text (e.g., "Waiting for acceptance…" or "Enrollment complete")
    pub status_text: String,
    /// Optional progress text (e.g., "2/3 accepted (need 2)")
    pub progress_text: String,
    /// Instruction text above the code (e.g., "Share this code with the recipient:")
    pub instruction: String,
    /// The code to display
    pub code: String,
    /// Optional help text below the code
    pub help_text: String,
    /// Optional error message
    pub error_message: String,
    /// Whether code was copied to clipboard (shows feedback)
    pub copied: bool,
}

/// Modal for displaying shareable codes with status tracking
#[component]
pub fn CodeDisplayModal(props: &CodeDisplayModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! { View {} };
    }

    // Format long codes into multiple lines for readability.
    // Available width: 80 (total) - 2 (border) - 4 (modal padding) - 2 (box border) - 2 (box padding) = 70
    let chunk_width = 68;
    let formatted_code = if props.code.len() > chunk_width {
        props
            .code
            .chars()
            .collect::<Vec<_>>()
            .chunks(chunk_width)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        props.code.clone()
    };

    let status_icon = props.status.icon().to_string();
    let status_color = props.status.color();
    let border_color = props.status.border_color();
    let status_text = props.status_text.clone();
    let has_progress = !props.progress_text.is_empty();
    let has_subtitle = !props.subtitle.is_empty();
    let has_help_text = !props.help_text.is_empty();
    let has_error = !props.error_message.is_empty();

    let footer_close_text = if props.status == CodeDisplayStatus::Pending {
        "Cancel"
    } else {
        "Close"
    };

    // Footer props
    let footer_props = ModalFooterProps::new(vec![
        KeyHint::new("c", "Copy"),
        KeyHint::new("Esc", footer_close_text),
    ]);

    // Error status for display
    let error_status = if has_error {
        ModalStatus::Error(props.error_message.clone())
    } else {
        ModalStatus::Idle
    };

    element! {
        View(
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            flex_direction: FlexDirection::Column,
            background_color: Theme::BG_MODAL,
            border_style: Borders::PRIMARY,
            border_color: border_color,
            overflow: Overflow::Hidden,
        ) {
            // Header
            View(
                width: 100pct,
                padding_left: Spacing::PANEL_PADDING,
                padding_right: Spacing::PANEL_PADDING,
                padding_top: Spacing::XS,
                padding_bottom: Spacing::XS,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
            ) {
                // Title row (with optional icon for success status)
                View(flex_direction: FlexDirection::Row, gap: 1) {
                    #(if props.status == CodeDisplayStatus::Success {
                        Some(element! {
                            Text(content: status_icon.clone(), color: status_color, weight: Weight::Bold)
                        })
                    } else {
                        None
                    })
                    Text(
                        content: props.title.clone(),
                        weight: Weight::Bold,
                        color: if props.status == CodeDisplayStatus::Success { status_color } else { Theme::TEXT },
                    )
                }
                // Status row (for pending/error) or subtitle (for success)
                #(if props.status == CodeDisplayStatus::Success && has_subtitle {
                    Some(element! {
                        View(margin_top: Spacing::XS) {
                            Text(content: props.subtitle.clone(), color: Theme::TEXT_MUTED)
                        }
                    })
                } else if props.status != CodeDisplayStatus::Success {
                    Some(element! {
                        View(flex_direction: FlexDirection::Row, gap: 1) {
                            Text(content: status_icon, color: status_color)
                            Text(content: status_text, color: status_color, weight: Weight::Bold)
                            #(if has_progress {
                                Some(element! {
                                    View(flex_direction: FlexDirection::Row) {
                                        Text(content: " — ", color: Theme::TEXT_MUTED)
                                        Text(content: props.progress_text.clone(), color: Theme::TEXT_MUTED)
                                    }
                                })
                            } else {
                                None
                            })
                        }
                    })
                } else {
                    None
                })
            }

            // Body
            View(
                width: 100pct,
                padding_left: Spacing::MODAL_PADDING,
                padding_right: Spacing::MODAL_PADDING,
                padding_top: Spacing::XS,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                overflow: Overflow::Hidden,
            ) {
                // Instruction
                #(if !props.instruction.is_empty() {
                    Some(element! {
                        View(margin_bottom: Spacing::XS) {
                            Text(content: props.instruction.clone(), color: Theme::TEXT)
                        }
                    })
                } else {
                    None
                })

                // Code box
                View(
                    width: 100pct,
                    flex_direction: FlexDirection::Column,
                    border_style: Borders::INPUT,
                    border_color: Theme::PRIMARY,
                    padding_left: Spacing::PANEL_PADDING,
                    padding_right: Spacing::PANEL_PADDING,
                ) {
                    Text(
                        content: formatted_code,
                        color: Theme::PRIMARY,
                        wrap: TextWrap::Wrap,
                    )
                }

                // Help text
                #(if has_help_text {
                    Some(element! {
                        View(margin_top: Spacing::SM) {
                            Text(content: props.help_text.clone(), color: Theme::TEXT_MUTED)
                        }
                    })
                } else {
                    None
                })

                // Error message
                #(Some(status_message(&error_status).into()))

                // Copy feedback
                #(if props.copied {
                    Some(element! {
                        View(margin_top: Spacing::XS, flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                            Text(content: Icons::CHECK, color: Theme::SUCCESS)
                            Text(content: "Copied to clipboard", color: Theme::SUCCESS)
                        }
                    })
                } else {
                    None
                })
            }

            // Footer
            #(Some(modal_footer(&footer_props).into()))
        }
    }
}

/// Copy text to the system clipboard
///
/// Returns Ok(()) on success, Err with message on failure.
/// Silently succeeds if clipboard is unavailable (e.g., headless environment).
pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    use arboard::Clipboard;

    match Clipboard::new() {
        Ok(mut clipboard) => clipboard
            .set_text(text)
            .map_err(|e| format!("Failed to copy: {e}")),
        Err(e) => {
            // Log but don't fail - clipboard may not be available in all environments
            tracing::debug!("Clipboard unavailable: {}", e);
            Ok(())
        }
    }
}
