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
use crate::tui::theme::{Borders, Icons, Spacing, Theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardianSetupKind {
    Guardian,
    Mfa,
}

impl Default for GuardianSetupKind {
    fn default() -> Self {
        GuardianSetupKind::Guardian
    }
}

struct GuardianSetupCopy {
    title: &'static str,
    select_prompt: &'static str,
    empty_title: &'static str,
    empty_subtitle: &'static str,
    threshold_prompt: &'static str,
    decline_message: &'static str,
    step1: &'static str,
    step2: &'static str,
    step3: &'static str,
    low_hint: &'static str,
}

impl GuardianSetupKind {
    fn copy(self) -> GuardianSetupCopy {
        match self {
            GuardianSetupKind::Guardian => GuardianSetupCopy {
                title: "Guardian Setup",
                select_prompt: "Select guardians (can help recover your account):",
                empty_title: "No contacts available.",
                empty_subtitle: "Add contacts first to set up guardians.",
                threshold_prompt: "How many guardians must approve recovery?",
                decline_message: "Failed: guardian declined",
                step1: "1.Select",
                step2: "2.Threshold",
                step3: "3.Ceremony",
                low_hint: "Low: any 1 can recover",
            },
            GuardianSetupKind::Mfa => GuardianSetupCopy {
                title: "Multifactor Setup",
                select_prompt: "Select devices to authorize multifactor signing:",
                empty_title: "No devices available.",
                empty_subtitle: "Add devices first to enable multifactor.",
                threshold_prompt: "How many devices must sign?",
                decline_message: "Failed: signer declined",
                step1: "1.Devices",
                step2: "2.Threshold",
                step3: "3.Ceremony",
                low_hint: "Low: any 1 can sign",
            },
        }
    }
}

/// Props for GuardianSetupModal
#[derive(Default, Props)]
pub struct GuardianSetupModalProps {
    /// Whether the modal is visible
    pub visible: bool,
    /// What this setup flow is for
    pub kind: GuardianSetupKind,
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
    let copy = props.kind.copy();
    let error = props.error.clone();

    element! {
        View(
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            flex_direction: FlexDirection::Column,
            background_color: Theme::BG_MODAL,
            border_style: Borders::PRIMARY,
            border_color: Theme::BORDER_FOCUS,
            overflow: Overflow::Hidden,
        ) {
            // Title bar
            View(
                width: 100pct,
                padding: Spacing::PANEL_PADDING,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
            ) {
                Text(
                    content: copy.title,
                    weight: Weight::Bold,
                    color: Theme::PRIMARY,
                )
                // Step indicator
                #(render_step_indicator(&step, props.kind))
            }

            // Error message if any
            #(if !error.is_empty() {
                Some(element! {
                    View(width: 100pct, padding: Spacing::PANEL_PADDING, background_color: Theme::ERROR) {
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
                padding: Spacing::PANEL_PADDING,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Center,
                gap: Spacing::LG,
            ) {
                #(render_key_hints(&step))
            }
        }
    }
}

fn render_step_indicator(step: &GuardianSetupStep, kind: GuardianSetupKind) -> AnyElement<'static> {
    let copy = kind.copy();
    let (step1, step2, step3) = match step {
        GuardianSetupStep::SelectContacts => (Theme::PRIMARY, Theme::TEXT_MUTED, Theme::TEXT_MUTED),
        GuardianSetupStep::ChooseThreshold => (Theme::SUCCESS, Theme::PRIMARY, Theme::TEXT_MUTED),
        GuardianSetupStep::CeremonyInProgress => (Theme::SUCCESS, Theme::SUCCESS, Theme::PRIMARY),
    };

    element! {
        View(flex_direction: FlexDirection::Row, gap: 1) {
            Text(content: copy.step1, color: step1)
            Text(content: Icons::ARROW_RIGHT, color: Theme::TEXT_MUTED)
            Text(content: copy.step2, color: step2)
            Text(content: Icons::ARROW_RIGHT, color: Theme::TEXT_MUTED)
            Text(content: copy.step3, color: step3)
        }
    }
    .into_any()
}

fn render_select_contacts(props: &GuardianSetupModalProps) -> AnyElement<'static> {
    let contacts = props.contacts.clone();
    let selected = props.selected_indices.clone();
    let focused = props.focused_index;
    let copy = props.kind.copy();

    // Empty state when no contacts
    if contacts.is_empty() {
        return element! {
            View(
                padding: Spacing::MD,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
            ) {
                Text(
                    content: copy.empty_title,
                    color: Theme::TEXT_MUTED,
                )
                Text(
                    content: copy.empty_subtitle,
                    color: Theme::TEXT_MUTED,
                )
                View(margin_top: Spacing::SM) {
                    Text(
                        content: "Press Esc to close",
                        color: Theme::TEXT_MUTED,
                    )
                }
            }
        }
        .into_any();
    }

    element! {
        View(
            padding_left: Spacing::SM,
            padding_right: Spacing::SM,
            padding_top: Spacing::XS,
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
        ) {
            Text(
                content: copy.select_prompt,
                color: Theme::TEXT_MUTED,
            )

            // Contact list with checkboxes - compact
            View(
                margin_top: Spacing::XS,
                flex_direction: FlexDirection::Column,
                border_style: BorderStyle::Round,
                border_color: Theme::BORDER,
                max_height: 10,
                overflow: Overflow::Hidden,
            ) {
                #(contacts.iter().enumerate().map(|(i, contact)| {
                    let is_selected = selected.contains(&i);
                    let is_focused = i == focused;
                    let pointer = if is_focused { "▸" } else { " " };
                    let checkbox = if is_selected { "[x]" } else { "[ ]" };
                    let guardian_badge = if contact.is_current_guardian { " (current)" } else { "" };

                    let bg = if is_focused { Theme::BG_SELECTED } else { Color::Reset };
                    let fg = if is_focused { Theme::TEXT } else { Theme::TEXT_MUTED };
                    let pointer_color = if is_focused { Theme::PRIMARY } else { Color::Reset };

                    element! {
                        View(
                            flex_direction: FlexDirection::Row,
                            gap: 1,
                            padding_left: Spacing::XS,
                            background_color: bg,
                        ) {
                            Text(content: pointer.to_string(), color: pointer_color)
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

            // Selection count - inline
            Text(
                content: format!("{} selected (min 2)", selected.len()),
                color: if selected.len() >= 2 { Theme::SUCCESS } else { Theme::WARNING },
            )

            // Key hints footer
            View(margin_top: Spacing::XS) {
                Text(
                    content: "↑↓/jk Navigate  Space Select  Enter Confirm  Esc Cancel",
                    color: Theme::TEXT_MUTED,
                )
            }
        }
    }
    .into_any()
}

fn render_choose_threshold(props: &GuardianSetupModalProps) -> AnyElement<'static> {
    let k = props.threshold_k;
    let n = props.threshold_n;
    let copy = props.kind.copy();

    // Security level hint - compact
    let security_hint = if k == 1 {
        copy.low_hint
    } else if k == n {
        "Max: all must agree"
    } else {
        "Balanced"
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
                content: copy.threshold_prompt,
                color: Theme::TEXT_MUTED,
            )

            // Threshold selector - vertical layout for up/down controls
            View(
                margin_top: Spacing::SM,
                margin_bottom: Spacing::SM,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
            ) {
                Text(
                    content: Icons::ARROW_UP,
                    color: if k < n { Theme::PRIMARY } else { Theme::TEXT_MUTED },
                    weight: Weight::Bold,
                )
                View(
                    border_style: BorderStyle::Round,
                    border_color: Theme::PRIMARY,
                    padding_left: 2,
                    padding_right: 2,
                ) {
                    Text(
                        content: format!("{} of {}", k, n),
                        color: Theme::PRIMARY,
                        weight: Weight::Bold,
                    )
                }
                Text(
                    content: Icons::ARROW_DOWN,
                    color: if k > 1 { Theme::PRIMARY } else { Theme::TEXT_MUTED },
                    weight: Weight::Bold,
                )
            }

            // Security hint - inline
            Text(content: security_hint.to_string(), color: Theme::SECONDARY)
        }
    }
    .into_any()
}

fn render_ceremony_progress(props: &GuardianSetupModalProps) -> AnyElement<'static> {
    let responses = props.ceremony_responses.clone();
    let copy = props.kind.copy();

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
            // Header with counts inline
            View(flex_direction: FlexDirection::Row, gap: 2) {
                Text(content: "Waiting...", color: Theme::TEXT, weight: Weight::Bold)
                Text(
                    content: format!("{}✓ {}⏳ {}✗", accepted, pending, declined),
                    color: Theme::TEXT_MUTED,
                )
            }

            // Response list - compact
            View(
                margin_top: Spacing::XS,
                flex_direction: FlexDirection::Column,
                border_style: BorderStyle::Round,
                border_color: Theme::BORDER,
                max_height: 8,
                overflow: Overflow::Hidden,
            ) {
                #(responses.iter().map(|(_, name, response)| {
                    let (icon, color) = match response {
                        GuardianCeremonyResponse::Pending => (Icons::PENDING, Theme::WARNING),
                        GuardianCeremonyResponse::Accepted => (Icons::CHECK, Theme::SUCCESS),
                        GuardianCeremonyResponse::Declined => (Icons::CROSS, Theme::ERROR),
                    };

                    element! {
                        View(flex_direction: FlexDirection::Row, gap: 1, padding_left: Spacing::XS) {
                            Text(content: icon.to_string(), color: color)
                            Text(content: name.clone(), color: Theme::TEXT)
                        }
                    }
                }))
            }

            // Status message - compact
            #(if declined > 0 {
                Some(element! {
                    Text(content: copy.decline_message, color: Theme::ERROR, weight: Weight::Bold)
                })
            } else if accepted == total && total > 0 {
                Some(element! {
                    Text(content: "All accepted! Completing...", color: Theme::SUCCESS, weight: Weight::Bold)
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
            View(flex_direction: FlexDirection::Row, gap: Spacing::LG) {
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "↑/↓", weight: Weight::Bold, color: Theme::SECONDARY)
                    Text(content: "Navigate", color: Theme::TEXT_MUTED)
                }
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "Space", weight: Weight::Bold, color: Theme::SECONDARY)
                    Text(content: "Toggle", color: Theme::TEXT_MUTED)
                }
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "Enter", weight: Weight::Bold, color: Theme::SECONDARY)
                    Text(content: "Next", color: Theme::TEXT_MUTED)
                }
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "Esc", weight: Weight::Bold, color: Theme::SECONDARY)
                    Text(content: "Cancel", color: Theme::TEXT_MUTED)
                }
            }
        }
        .into_any(),
        GuardianSetupStep::ChooseThreshold => element! {
            View(flex_direction: FlexDirection::Row, gap: Spacing::LG) {
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "↑/↓", weight: Weight::Bold, color: Theme::SECONDARY)
                    Text(content: "Adjust", color: Theme::TEXT_MUTED)
                }
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "Enter", weight: Weight::Bold, color: Theme::SECONDARY)
                    Text(content: "Confirm", color: Theme::TEXT_MUTED)
                }
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "Esc", weight: Weight::Bold, color: Theme::SECONDARY)
                    Text(content: "Back", color: Theme::TEXT_MUTED)
                }
            }
        }
        .into_any(),
        GuardianSetupStep::CeremonyInProgress => element! {
            View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                Text(content: "Esc", weight: Weight::Bold, color: Theme::SECONDARY)
                Text(content: "Cancel", color: Theme::TEXT_MUTED)
            }
        }
        .into_any(),
    }
}
