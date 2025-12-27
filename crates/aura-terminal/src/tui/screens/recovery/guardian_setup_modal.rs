//! # Guardian Setup Modal
//!
//! Multi-step wizard for setting up guardians with threshold selection.
//!
//! Steps:
//! 1. SelectContacts - Multi-select contacts to become guardians (checkboxes)
//! 2. ChooseThreshold - Select k-of-n threshold
//! 3. CeremonyInProgress - Wait for guardian responses

use iocraft::prelude::*;

use crate::tui::components::{
    contact_multi_select, modal_footer, modal_header, threshold_selector, ContactMultiSelectItem,
    ContactMultiSelectProps, ModalFooterProps, ModalHeaderProps, ThresholdSelectorProps,
};
use crate::tui::layout::dim;
use crate::tui::state_machine::{GuardianCeremonyResponse, GuardianSetupStep};
use crate::tui::theme::{Borders, Icons, Spacing, Theme};
use crate::tui::types::KeyHint;

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
                low_hint: "Low: any 1 can recover",
            },
            GuardianSetupKind::Mfa => GuardianSetupCopy {
                title: "Multifactor Setup",
                select_prompt: "Select devices to authorize multifactor signing:",
                empty_title: "No devices available.",
                empty_subtitle: "Add devices first to enable multifactor.",
                threshold_prompt: "How many devices must sign?",
                decline_message: "Failed: signer declined",
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

    // Step indicator: 1-indexed step number, 3 total steps
    let step_num = match step {
        GuardianSetupStep::SelectContacts => 1,
        GuardianSetupStep::ChooseThreshold => 2,
        GuardianSetupStep::CeremonyInProgress => 3,
    };
    let header_props = ModalHeaderProps::new(copy.title).with_step(step_num, 3);

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
            // Header with step indicator
            #(Some(modal_header(&header_props).into()))

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
            #(Some(modal_footer(&get_footer_hints(&step)).into()))
        }
    }
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

    let items = contacts
        .iter()
        .map(|contact| ContactMultiSelectItem {
            name: contact.name.clone(),
            badge: contact
                .is_current_guardian
                .then_some(" (current)".to_string()),
        })
        .collect::<Vec<_>>();

    let selector = ContactMultiSelectProps {
        prompt: copy.select_prompt.to_string(),
        items,
        selected: selected.clone(),
        focused,
        min_selected: Some(2),
        footer_hint: Some("↑↓/jk Navigate  Space Select  Enter Confirm  Esc Cancel".to_string()),
    };

    contact_multi_select(&selector).into()
}

fn render_choose_threshold(props: &GuardianSetupModalProps) -> AnyElement<'static> {
    let copy = props.kind.copy();
    let selector = ThresholdSelectorProps {
        prompt: copy.threshold_prompt.to_string(),
        subtext: None,
        k: props.threshold_k,
        n: props.threshold_n,
        low_hint: Some(copy.low_hint.to_string()),
        show_hint: true,
    };

    threshold_selector(&selector).into()
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
                    content: format!(
                        "Accepted: {}  Pending: {}  Declined: {}",
                        accepted, pending, declined
                    ),
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

fn get_footer_hints(step: &GuardianSetupStep) -> ModalFooterProps {
    let hints = match step {
        GuardianSetupStep::SelectContacts => vec![
            KeyHint::new("↑/↓", "Navigate"),
            KeyHint::new("Space", "Toggle"),
            KeyHint::new("Enter", "Next"),
            KeyHint::new("Esc", "Cancel"),
        ],
        GuardianSetupStep::ChooseThreshold => vec![
            KeyHint::new("↑/↓", "Adjust"),
            KeyHint::new("Enter", "Confirm"),
            KeyHint::new("Esc", "Back"),
        ],
        GuardianSetupStep::CeremonyInProgress => vec![KeyHint::new("Esc", "Cancel")],
    };
    ModalFooterProps::new(hints)
}
