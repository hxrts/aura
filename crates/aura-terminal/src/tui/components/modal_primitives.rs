//! # Modal Primitives
//!
//! Reusable building blocks for modal dialogs.
//!
//! These components provide consistent styling and reduce boilerplate
//! across all modal implementations.
//!
//! ## Components
//!
//! - [`modal_header`] - Standard modal header with title and optional step indicator
//! - [`modal_footer`] - Footer with key hints
//! - [`labeled_input`] - Label + input field combination
//! - [`status_message`] - Loading/error/success status display
//! - [`multi_select_list`] - Checkbox list with selection
//! - [`key_hint_group`] - Single key hint (key + action)

use iocraft::prelude::*;

use crate::tui::theme::{Borders, Spacing, Theme};
use crate::tui::types::KeyHint;

// =============================================================================
// ModalHeader
// =============================================================================

/// Props for modal_header function
#[derive(Clone, Debug, Default)]
pub struct ModalHeaderProps {
    /// Main title text
    pub title: String,
    /// Optional subtitle (displayed below title)
    pub subtitle: Option<String>,
    /// Optional step indicator (current, total) - displays "Step X of Y"
    pub step: Option<(usize, usize)>,
}

impl ModalHeaderProps {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            step: None,
        }
    }

    pub fn with_step(mut self, current: usize, total: usize) -> Self {
        self.step = Some((current, total));
        self
    }

    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }
}

/// Standard modal header with title, optional subtitle, and step indicator
///
/// # Example
/// ```ignore
/// #(modal_header(&ModalHeaderProps::new("Create New Chat").with_step(1, 3)))
/// ```
pub fn modal_header(props: &ModalHeaderProps) -> impl Into<AnyElement<'static>> {
    let title = props.title.clone();
    let subtitle = props.subtitle.clone();
    let step = props.step;

    element! {
        View(
            width: 100pct,
            padding: Spacing::PANEL_PADDING,
            flex_direction: FlexDirection::Row,
            justify_content: if step.is_some() { JustifyContent::SpaceBetween } else { JustifyContent::Center },
            align_items: AlignItems::Center,
            border_style: BorderStyle::Single,
            border_edges: Edges::Bottom,
            border_color: Theme::BORDER,
        ) {
            // Title (and optional subtitle)
            View(flex_direction: FlexDirection::Column) {
                Text(
                    content: title,
                    weight: Weight::Bold,
                    color: Theme::PRIMARY,
                )
                #(subtitle.map(|sub| {
                    element! {
                        Text(
                            content: sub,
                            color: Theme::TEXT_MUTED,
                        )
                    }
                }))
            }

            // Step indicator
            #(step.map(|(current, total)| {
                element! {
                    Text(
                        content: format!("Step {} of {}", current, total),
                        color: Theme::TEXT_MUTED,
                    )
                }
            }))
        }
    }
}

// =============================================================================
// ModalFooter
// =============================================================================

/// Props for modal_footer function
#[derive(Clone, Debug, Default)]
pub struct ModalFooterProps {
    /// Key hints to display
    pub hints: Vec<KeyHint>,
}

impl ModalFooterProps {
    pub fn new(hints: Vec<KeyHint>) -> Self {
        Self { hints }
    }
}

/// Standard modal footer with key hints
///
/// # Example
/// ```ignore
/// #(modal_footer(&ModalFooterProps::new(vec![
///     KeyHint::new("Esc", "Cancel"),
///     KeyHint::new("Tab", "Next"),
///     KeyHint::new("Enter", "Submit"),
/// ])))
/// ```
pub fn modal_footer(props: &ModalFooterProps) -> impl Into<AnyElement<'static>> {
    let hints = props.hints.clone();

    element! {
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
            #(hints.into_iter().map(|hint| {
                element! {
                    View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                        Text(content: hint.key, weight: Weight::Bold, color: Theme::SECONDARY)
                        Text(content: hint.description, color: Theme::TEXT_MUTED)
                    }
                }
            }))
        }
    }
}

// =============================================================================
// KeyHintGroup
// =============================================================================

/// Single key hint displaying key and action
///
/// # Example
/// ```ignore
/// #(key_hint_group("Enter", "Submit"))
/// ```
pub fn key_hint_group(key: &str, action: &str) -> impl Into<AnyElement<'static>> {
    let key = key.to_string();
    let action = action.to_string();

    element! {
        View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
            Text(content: key, weight: Weight::Bold, color: Theme::SECONDARY)
            Text(content: action, color: Theme::TEXT_MUTED)
        }
    }
}

// =============================================================================
// LabeledInput
// =============================================================================

/// Props for labeled_input function
#[derive(Clone, Debug, Default)]
pub struct LabeledInputProps {
    /// Label text displayed above the input
    pub label: String,
    /// Current input value
    pub value: String,
    /// Placeholder text when value is empty
    pub placeholder: String,
    /// Whether this input is focused
    pub focused: bool,
    /// Whether this field is required (adds " *" to label)
    pub required: bool,
    /// Optional error message
    pub error: Option<String>,
}

impl LabeledInputProps {
    pub fn new(label: impl Into<String>, placeholder: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: String::new(),
            placeholder: placeholder.into(),
            focused: false,
            required: false,
            error: None,
        }
    }

    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn with_error(mut self, error: Option<String>) -> Self {
        self.error = error;
        self
    }

    pub fn with_required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
}

/// Label + input field combination with consistent styling
///
/// # Example
/// ```ignore
/// #(labeled_input(&LabeledInputProps {
///     label: "Group Name:".to_string(),
///     value: state.name.clone(),
///     placeholder: "Enter name...".to_string(),
///     focused: state.active_field == 0,
///     error: None,
/// }))
/// ```
pub fn labeled_input(props: &LabeledInputProps) -> impl Into<AnyElement<'static>> {
    let label = if props.required {
        format!("{}*", props.label)
    } else {
        props.label.clone()
    };
    let value = props.value.clone();
    let placeholder = props.placeholder.clone();
    let focused = props.focused;
    let error = props.error.clone();

    // Display text: show placeholder if empty
    let display_text = if value.is_empty() {
        placeholder
    } else {
        value.clone()
    };

    // Text color: muted for placeholder, normal for value
    let text_color = if value.is_empty() {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
    };

    // Border color: focused, error, or default
    let border_color = if error.is_some() {
        Theme::ERROR
    } else if focused {
        Theme::PRIMARY
    } else {
        Theme::BORDER
    };

    element! {
        View(flex_direction: FlexDirection::Column, width: 100pct) {
            // Label
            Text(content: label, color: Theme::TEXT_MUTED)

            // Input box (tight styling - no gap between label and input)
            View(
                width: 100pct,
                border_style: Borders::INPUT,
                border_color: border_color,
                padding_left: Spacing::XS,
                padding_right: Spacing::XS,
                padding_top: 0,
                padding_bottom: 0,
            ) {
                Text(content: display_text, color: text_color)
            }

            // Error message (if any)
            #(error.map(|err| {
                element! {
                    View(margin_top: Spacing::XS) {
                        Text(content: err, color: Theme::ERROR)
                    }
                }
            }))
        }
    }
}

// =============================================================================
// StatusMessage
// =============================================================================

/// Status types for modal operations
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ModalStatus {
    /// No status to display
    #[default]
    Idle,
    /// Loading/in-progress operation
    Loading(String),
    /// Error occurred
    Error(String),
    /// Operation succeeded
    Success(String),
}

/// Status message display for loading/error/success states
///
/// # Example
/// ```ignore
/// #(status_message(&ModalStatus::Loading("Creating...".to_string())))
/// ```
pub fn status_message(status: &ModalStatus) -> impl Into<AnyElement<'static>> {
    match status.clone() {
        ModalStatus::Idle => element! { View {} },
        ModalStatus::Loading(msg) => element! {
            View(margin_top: Spacing::XS) {
                Text(content: msg, color: Theme::WARNING)
            }
        },
        ModalStatus::Error(msg) => element! {
            View(margin_top: Spacing::XS) {
                Text(content: msg, color: Theme::ERROR)
            }
        },
        ModalStatus::Success(msg) => element! {
            View(margin_top: Spacing::XS) {
                Text(content: msg, color: Theme::SUCCESS)
            }
        },
    }
}

// =============================================================================
// MultiSelectList
// =============================================================================

/// A single item in a multi-select list
#[derive(Clone, Debug, Default)]
pub struct SelectableItem {
    /// Unique identifier
    pub id: String,
    /// Display name
    pub name: String,
}

impl SelectableItem {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
        }
    }
}

/// Props for multi_select_list function
#[derive(Clone, Debug, Default)]
pub struct MultiSelectListProps {
    /// Items to display
    pub items: Vec<SelectableItem>,
    /// Indices of selected items
    pub selected_indices: Vec<usize>,
    /// Currently focused index
    pub focused_index: usize,
    /// Maximum height (number of visible items)
    pub max_height: Option<u32>,
}

impl MultiSelectListProps {
    pub fn new(items: Vec<SelectableItem>) -> Self {
        Self {
            items,
            selected_indices: Vec::new(),
            focused_index: 0,
            max_height: None,
        }
    }

    pub fn with_selected(mut self, indices: Vec<usize>) -> Self {
        self.selected_indices = indices;
        self
    }

    pub fn with_focused(mut self, index: usize) -> Self {
        self.focused_index = index;
        self
    }

    pub fn with_max_height(mut self, height: u32) -> Self {
        self.max_height = Some(height);
        self
    }
}

/// Multi-select list with checkboxes
///
/// # Example
/// ```ignore
/// #(multi_select_list(&MultiSelectListProps {
///     items: contacts.iter().map(|(id, name)| SelectableItem::new(id, name)).collect(),
///     selected_indices: state.selected.clone(),
///     focused_index: state.focused,
///     max_height: Some(10),
/// }))
/// ```
pub fn multi_select_list(props: &MultiSelectListProps) -> impl Into<AnyElement<'static>> {
    let items = props.items.clone();
    let selected = props.selected_indices.clone();
    let focused = props.focused_index;
    let max_height = props.max_height.unwrap_or(12);

    element! {
        View(
            flex_direction: FlexDirection::Column,
            border_style: Borders::INPUT,
            border_color: Theme::BORDER,
            padding: Spacing::PANEL_PADDING,
            max_height: max_height,
            overflow: Overflow::Hidden,
        ) {
            #(items.iter().enumerate().map(|(i, item)| {
                let is_selected = selected.contains(&i);
                let is_focused = i == focused;
                let pointer = if is_focused { "▸" } else { " " };
                let checkbox = if is_selected { "[x]" } else { "[ ]" };
                let text_color = if is_focused { Theme::TEXT } else { Theme::TEXT_MUTED };
                let pointer_color = if is_focused { Theme::PRIMARY } else { Theme::TEXT_MUTED };
                let checkbox_color = if is_selected { Theme::SUCCESS } else { text_color };
                let name = item.name.clone();

                element! {
                    View(flex_direction: FlexDirection::Row, gap: 1) {
                        Text(content: pointer.to_string(), color: pointer_color)
                        Text(content: checkbox.to_string(), color: checkbox_color)
                        Text(content: name, color: text_color)
                    }
                }
            }))
        }
    }
}

// =============================================================================
// Layout Patterns (Documentation Only)
// =============================================================================
//
// Standard modal body layout:
// ```
// View(
//     width: 100pct,
//     padding: Spacing::MODAL_PADDING,
//     flex_direction: FlexDirection::Column,
//     flex_grow: 1.0,
//     flex_shrink: 1.0,
//     overflow: Overflow::Hidden,
// ) {
//     // modal content here
// }
// ```
//
// For field groups, use gap: Spacing::SM between fields.
