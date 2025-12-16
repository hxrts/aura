//! # Guardian Setup Modal
//!
//! Multi-step wizard for setting up guardians with threshold selection.
//!
//! Steps:
//! 1. SelectContacts - Multi-select contacts to become guardians (checkboxes)
//! 2. ChooseThreshold - Select k-of-n threshold
//! 3. CeremonyInProgress - Wait for guardian responses

use iocraft::prelude::*;

use crate::tui::layout::dim;
use crate::tui::state_machine::{GuardianCeremonyResponse, GuardianSetupStep};
use crate::tui::theme::{Icons, Spacing, Theme};

/// Props for GuardianSetupModal
#[derive(Default, Props)]
pub struct GuardianSetupModalProps {
    /// Whether the modal is visible
    pub visible: bool,
    /// Current step in the wizard
    pub step: GuardianSetupStep,
    /// Contact candidates for guardian selection
    pub contacts: Vec<GuardianCandidateProps>,
    /// Indices of selected contacts
    pub selected_indices: Vec<usize>,
    /// Currently focused contact index
    pub focused_index: usize,
    /// Selected threshold k (required signers)
    pub threshold_k: u8,
    /// Total selected guardians (n)
    pub threshold_n: u8,
    /// Ceremony responses (id, name, response)
    pub ceremony_responses: Vec<(String, String, GuardianCeremonyResponse)>,
    /// Error message if any
    pub error: String,
}

/// Props for a guardian candidate
#[derive(Clone, Debug, Default)]
pub struct GuardianCandidateProps {
    pub id: String,
    pub name: String,
    pub is_current_guardian: bool,
}

/// Guardian Setup Modal Component
#[component]
pub fn GuardianSetupModal(props: &GuardianSetupModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! { View {} };
    }

    let step = props.step.clone();
    let error = props.error.clone();

    element! {
        View(
            position: Position::Absolute,
            top: 0u16,
            left: 0u16,
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            flex_direction: FlexDirection::Column,
            background_color: Theme::BG_MODAL,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER_FOCUS,
            overflow: Overflow::Hidden,
        ) {
            // Title bar
            View(
                width: 100pct,
                padding: Spacing::SM,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
            ) {
                Text(
                    content: "Guardian Setup",
                    weight: Weight::Bold,
                    color: Theme::PRIMARY,
                )
                // Step indicator
                #(render_step_indicator(&step))
            }

            // Error message if any
            #(if !error.is_empty() {
                Some(element! {
                    View(width: 100pct, padding: Spacing::SM, background_color: Theme::ERROR) {
                        Text(content: error.clone(), color: Theme::TEXT)
                    }
                })
            } else {
                None
            })

            // Content based on step - fills available space
            View(
                width: 100pct,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                overflow: Overflow::Hidden,
            ) {
                #(match step {
                    GuardianSetupStep::SelectContacts => render_select_contacts(props),
                    GuardianSetupStep::ChooseThreshold => render_choose_threshold(props),
                    GuardianSetupStep::CeremonyInProgress => render_ceremony_progress(props),
                })
            }

            // Footer with key hints
            View(
                width: 100pct,
                padding: Spacing::SM,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Center,
                gap: 2,
            ) {
                #(render_key_hints(&step))
            }
        }
    }
}

fn render_step_indicator(step: &GuardianSetupStep) -> AnyElement<'static> {
    let (step1, step2, step3) = match step {
        GuardianSetupStep::SelectContacts => (Theme::PRIMARY, Theme::TEXT_MUTED, Theme::TEXT_MUTED),
        GuardianSetupStep::ChooseThreshold => (Theme::SUCCESS, Theme::PRIMARY, Theme::TEXT_MUTED),
        GuardianSetupStep::CeremonyInProgress => (Theme::SUCCESS, Theme::SUCCESS, Theme::PRIMARY),
    };

    element! {
        View(flex_direction: FlexDirection::Row, gap: 1) {
            Text(content: "1.Select", color: step1)
            Text(content: Icons::ARROW_RIGHT, color: Theme::TEXT_MUTED)
            Text(content: "2.Threshold", color: step2)
            Text(content: Icons::ARROW_RIGHT, color: Theme::TEXT_MUTED)
            Text(content: "3.Ceremony", color: step3)
        }
    }
    .into_any()
}

fn render_select_contacts(props: &GuardianSetupModalProps) -> AnyElement<'static> {
    let contacts = props.contacts.clone();
    let selected = props.selected_indices.clone();
    let focused = props.focused_index;

    element! {
        View(
            padding: Spacing::SM,
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
        ) {
            Text(
                content: "Select contacts to become your guardians:",
                color: Theme::TEXT_MUTED,
                wrap: TextWrap::Wrap,
            )
            View(margin_top: Spacing::SM, margin_bottom: Spacing::SM) {
                Text(
                    content: "Guardians can help recover your account if you lose access.",
                    color: Theme::TEXT_MUTED,
                    wrap: TextWrap::Wrap,
                )
            }

            // Contact list with checkboxes
            View(
                flex_direction: FlexDirection::Column,
                border_style: BorderStyle::Round,
                border_color: Theme::BORDER,
                padding: Spacing::XS,
                max_height: 15,
            ) {
                #(contacts.iter().enumerate().map(|(i, contact)| {
                    let is_selected = selected.contains(&i);
                    let is_focused = i == focused;
                    let checkbox = if is_selected { "[x]" } else { "[ ]" };
                    let guardian_badge = if contact.is_current_guardian { " (current)" } else { "" };

                    let bg = if is_focused { Theme::BG_SELECTED } else { Color::Reset };
                    let fg = if is_focused { Theme::TEXT } else { Theme::TEXT_MUTED };

                    element! {
                        View(
                            flex_direction: FlexDirection::Row,
                            gap: 1,
                            padding_left: Spacing::XS,
                            padding_right: Spacing::XS,
                            background_color: bg,
                        ) {
                            Text(content: checkbox.to_string(), color: if is_selected { Theme::SUCCESS } else { fg })
                            Text(content: contact.name.clone(), color: fg)
                            #(if contact.is_current_guardian {
                                Some(element! {
                                    Text(content: guardian_badge.to_string(), color: Theme::WARNING)
                                })
                            } else {
                                None
                            })
                        }
                    }
                }))
            }

            // Selection count
            View(margin_top: Spacing::SM) {
                Text(
                    content: format!("{} contacts selected", selected.len()),
                    color: if selected.len() >= 2 { Theme::SUCCESS } else { Theme::WARNING },
                )
            }
        }
    }
    .into_any()
}

fn render_choose_threshold(props: &GuardianSetupModalProps) -> AnyElement<'static> {
    let k = props.threshold_k;
    let n = props.threshold_n;

    // Security level hint
    let security_hint = if k == 1 {
        "Low security: Any single guardian can recover".to_string()
    } else if k == n {
        "Maximum security: All guardians must agree".to_string()
    } else {
        format!("Balanced: {} of {} guardians must agree", k, n)
    };

    element! {
        View(
            padding: Spacing::SM,
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
        ) {
            Text(
                content: "Choose recovery threshold:",
                color: Theme::TEXT,
                weight: Weight::Bold,
            )
            View(margin_top: Spacing::MD) {
                Text(
                    content: "How many guardians must approve a recovery request?",
                    color: Theme::TEXT_MUTED,
                    wrap: TextWrap::Wrap,
                )
            }

            // Threshold selector
            View(
                margin_top: Spacing::LG,
                margin_bottom: Spacing::LG,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                gap: 2,
            ) {
                // Left arrow
                Text(
                    content: Icons::ARROW_DOUBLE_LEFT,
                    color: if k > 1 { Theme::PRIMARY } else { Theme::TEXT_MUTED },
                    weight: Weight::Bold,
                )

                // Threshold display
                View(
                    border_style: BorderStyle::Round,
                    border_color: Theme::PRIMARY,
                    padding_left: 3,
                    padding_right: 3,
                    padding_top: 1,
                    padding_bottom: 1,
                ) {
                    Text(
                        content: format!("{} of {}", k, n),
                        color: Theme::PRIMARY,
                        weight: Weight::Bold,
                    )
                }

                // Right arrow
                Text(
                    content: Icons::ARROW_DOUBLE_RIGHT,
                    color: if k < n { Theme::PRIMARY } else { Theme::TEXT_MUTED },
                    weight: Weight::Bold,
                )
            }

            // Security hint
            View(
                padding: Spacing::SM,
                border_style: BorderStyle::Round,
                border_color: Theme::SECONDARY,
            ) {
                Text(content: security_hint, color: Theme::SECONDARY)
            }
        }
    }
    .into_any()
}

fn render_ceremony_progress(props: &GuardianSetupModalProps) -> AnyElement<'static> {
    let responses = props.ceremony_responses.clone();

    // Count responses
    let total = responses.len();
    let accepted = responses
        .iter()
        .filter(|(_, _, r)| *r == GuardianCeremonyResponse::Accepted)
        .count();
    let declined = responses
        .iter()
        .filter(|(_, _, r)| *r == GuardianCeremonyResponse::Declined)
        .count();
    let pending = total - accepted - declined;

    element! {
        View(
            padding: Spacing::SM,
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
        ) {
            Text(
                content: "Waiting for guardian responses...",
                color: Theme::TEXT,
                weight: Weight::Bold,
            )
            View(margin_top: Spacing::SM) {
                Text(
                    content: format!("{} accepted, {} pending, {} declined", accepted, pending, declined),
                    color: Theme::TEXT_MUTED,
                )
            }

            // Response list
            View(
                margin_top: Spacing::MD,
                flex_direction: FlexDirection::Column,
                border_style: BorderStyle::Round,
                border_color: Theme::BORDER,
                padding: Spacing::SM,
            ) {
                #(responses.iter().map(|(_, name, response)| {
                    let (icon, color) = match response {
                        GuardianCeremonyResponse::Pending => (Icons::PENDING, Theme::WARNING),
                        GuardianCeremonyResponse::Accepted => (Icons::CHECK, Theme::SUCCESS),
                        GuardianCeremonyResponse::Declined => (Icons::CROSS, Theme::ERROR),
                    };

                    element! {
                        View(flex_direction: FlexDirection::Row, gap: 1, padding: Spacing::XS) {
                            Text(content: icon.to_string(), color: color)
                            Text(content: name.clone(), color: Theme::TEXT)
                            Text(
                                content: format!("({:?})", response),
                                color: color,
                            )
                        }
                    }
                }))
            }

            // Status message
            #(if declined > 0 {
                Some(element! {
                    View(margin_top: Spacing::MD, padding: Spacing::SM, background_color: Theme::ERROR) {
                        Text(
                            content: "Ceremony failed: A guardian declined",
                            color: Theme::TEXT,
                            weight: Weight::Bold,
                        )
                    }
                })
            } else if accepted == total && total > 0 {
                Some(element! {
                    View(margin_top: Spacing::MD, padding: Spacing::SM, background_color: Theme::SUCCESS) {
                        Text(
                            content: "All guardians accepted! Completing ceremony...",
                            color: Theme::TEXT,
                            weight: Weight::Bold,
                        )
                    }
                })
            } else {
                None
            })
        }
    }
    .into_any()
}

fn render_key_hints(step: &GuardianSetupStep) -> AnyElement<'static> {
    match step {
        GuardianSetupStep::SelectContacts => element! {
            View(flex_direction: FlexDirection::Row, gap: 2) {
                Text(content: "j/k", color: Theme::SECONDARY, weight: Weight::Bold)
                Text(content: "navigate", color: Theme::TEXT_MUTED)
                Text(content: "Space", color: Theme::SECONDARY, weight: Weight::Bold)
                Text(content: "toggle", color: Theme::TEXT_MUTED)
                Text(content: "Tab/Enter", color: Theme::SECONDARY, weight: Weight::Bold)
                Text(content: "next", color: Theme::TEXT_MUTED)
                Text(content: "Esc", color: Theme::SECONDARY, weight: Weight::Bold)
                Text(content: "cancel", color: Theme::TEXT_MUTED)
            }
        }
        .into_any(),
        GuardianSetupStep::ChooseThreshold => element! {
            View(flex_direction: FlexDirection::Row, gap: 2) {
                Text(content: "h/l", color: Theme::SECONDARY, weight: Weight::Bold)
                Text(content: "adjust", color: Theme::TEXT_MUTED)
                Text(content: "Enter", color: Theme::SECONDARY, weight: Weight::Bold)
                Text(content: "start ceremony", color: Theme::TEXT_MUTED)
                Text(content: "Esc", color: Theme::SECONDARY, weight: Weight::Bold)
                Text(content: "back", color: Theme::TEXT_MUTED)
            }
        }
        .into_any(),
        GuardianSetupStep::CeremonyInProgress => element! {
            View(flex_direction: FlexDirection::Row, gap: 2) {
                Text(content: "Esc", color: Theme::SECONDARY, weight: Weight::Bold)
                Text(content: "cancel ceremony", color: Theme::TEXT_MUTED)
            }
        }
        .into_any(),
    }
}
