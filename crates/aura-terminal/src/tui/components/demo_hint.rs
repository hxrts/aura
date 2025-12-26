//! # Demo Hint Component
//!
//! Displays contextual hints for demo mode, including contact invite codes for Alice and Carol.
//! These codes add Alice/Carol as contacts. Guardian requests are sent in-band from the
//! Settings > Recovery Requests section after someone is a contact.

use iocraft::prelude::*;

use crate::tui::theme::{Spacing, Theme};

/// Props for DemoHintBar
#[derive(Default, Props)]
pub struct DemoHintBarProps {
    /// The hint message to display
    pub hint: String,
    /// Optional invite code to highlight (will be styled for easy copying)
    pub invite_code: Option<String>,
}

/// A bar showing demo mode hints with optional invite code
#[component]
pub fn DemoHintBar(props: &DemoHintBarProps) -> impl Into<AnyElement<'static>> {
    let hint = props.hint.clone();
    let code = props.invite_code.clone();

    element! {
        View(
            flex_direction: FlexDirection::Row,
            width: 100pct,
            padding: Spacing::XS,

            border_style: BorderStyle::Round,
            border_color: Theme::WARNING,
        ) {
            // Demo indicator
            Text(
                content: "[DEMO] ",
                color: Theme::WARNING,
                weight: Weight::Bold,
            )
            // Hint message
            Text(
                content: hint,
                color: Theme::TEXT_MUTED,
            )
            // Invite code if present (styled for visibility)
            #(code.map(|c| element! {
                View(flex_direction: FlexDirection::Row, margin_left: Spacing::SM) {
                    Text(content: " Code: ", color: Theme::TEXT_MUTED)
                    Text(content: c, color: Theme::PRIMARY, weight: Weight::Bold)
                }
            }))
        }
    }
}

/// Props for the full demo hint panel that shows both Alice and Carol codes
#[derive(Default, Props)]
pub struct DemoInviteCodesProps {
    /// Alice's invite code
    pub alice_code: String,
    /// Carol's invite code
    pub carol_code: String,
    /// Whether to show the panel (only in demo mode)
    pub visible: bool,
}

/// Format a long code into multiple lines for display
fn format_code_lines(code: &str, line_width: usize) -> Vec<String> {
    code.chars()
        .collect::<Vec<_>>()
        .chunks(line_width)
        .map(|c| c.iter().collect::<String>())
        .collect()
}

/// A modal showing both Alice and Carol invite codes
#[component]
pub fn DemoInviteCodes(props: &DemoInviteCodesProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! { View() };
    }

    // Break long codes into chunks for display (50 chars per line to fit in modal)
    let alice_lines = format_code_lines(&props.alice_code, 50);
    let carol_lines = format_code_lines(&props.carol_code, 50);

    element! {
        View(
            position: Position::Absolute,
            width: 100pct,
            height: 100pct,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,

        ) {
            View(
                flex_direction: FlexDirection::Column,
                width: Percent(70.0),
                background_color: Theme::BG_MODAL,
                border_style: BorderStyle::Round,
                border_color: Theme::WARNING,
                padding: Spacing::MD,
            ) {
                // Header
                Text(
                    content: "DEMO MODE - Contact Invite Codes",
                    color: Theme::WARNING,
                    weight: Weight::Bold,
                )
                View(margin_top: Spacing::XS) {
                    Text(
                        content: "Go to Contacts (4) and Accept (a) to add contacts:",
                        color: Theme::TEXT_MUTED,
                    )
                }

                // Alice section
                View(flex_direction: FlexDirection::Column, margin_top: Spacing::MD) {
                    Text(content: "Alice:", color: Theme::TEXT, weight: Weight::Bold)
                    View(
                        margin_top: Spacing::XS,
                        padding: Spacing::XS,

                        border_style: BorderStyle::Round,
                        border_color: Theme::BORDER,
                    ) {
                        View(flex_direction: FlexDirection::Column) {
                            #(alice_lines.iter().map(|line| {
                                let line_content = line.clone();
                                element! {
                                    Text(content: line_content, color: Theme::PRIMARY)
                                }
                            }))
                        }
                    }
                }

                // Carol section
                View(flex_direction: FlexDirection::Column, margin_top: Spacing::MD) {
                    Text(content: "Carol:", color: Theme::TEXT, weight: Weight::Bold)
                    View(
                        margin_top: Spacing::XS,
                        padding: Spacing::XS,

                        border_style: BorderStyle::Round,
                        border_color: Theme::BORDER,
                    ) {
                        View(flex_direction: FlexDirection::Column) {
                            #(carol_lines.iter().map(|line| {
                                let line_content = line.clone();
                                element! {
                                    Text(content: line_content, color: Theme::PRIMARY)
                                }
                            }))
                        }
                    }
                }

                // Footer with close hint
                View(
                    margin_top: Spacing::MD,
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::Center,
                ) {
                    Text(content: "Press ", color: Theme::TEXT_MUTED)
                    Text(content: "Esc", color: Theme::SECONDARY, weight: Weight::Bold)
                    Text(content: " or ", color: Theme::TEXT_MUTED)
                    Text(content: "d", color: Theme::SECONDARY, weight: Weight::Bold)
                    Text(content: " to close", color: Theme::TEXT_MUTED)
                }
            }
        }
    }
}
