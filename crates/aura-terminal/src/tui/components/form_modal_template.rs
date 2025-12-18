//! # Form Modal Component
//!
//! Modal dialog with multiple form fields.

use iocraft::prelude::*;

use super::modal::ModalContent;
use crate::tui::theme::Theme;

/// A form field definition
#[derive(Clone, Debug, Default)]
pub struct FormField {
    /// Field identifier
    pub id: String,
    /// Display label
    pub label: String,
    /// Current value
    pub value: String,
    /// Placeholder text
    pub placeholder: String,
    /// Whether this field is required
    pub required: bool,
    /// Optional validation error
    pub error: Option<String>,
}

impl FormField {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value: String::new(),
            placeholder: String::new(),
            required: false,
            error: None,
        }
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

/// Props for FormFieldComponent
#[derive(Default, Props)]
pub struct FormFieldProps {
    /// Field label
    pub label: String,
    /// Current value
    pub value: String,
    /// Placeholder text
    pub placeholder: String,
    /// Whether this field is focused
    pub focused: bool,
    /// Whether this field is required
    pub required: bool,
    /// Validation error
    pub error: String,
}

/// A single form field display
#[component]
pub fn FormFieldComponent(props: &FormFieldProps) -> impl Into<AnyElement<'static>> {
    let label = props.label.clone();
    let value = props.value.clone();
    let placeholder = props.placeholder.clone();
    let has_error = !props.error.is_empty();
    let error = props.error.clone();

    let display_text = if value.is_empty() { placeholder } else { value };

    let text_color = if props.value.is_empty() {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
    };

    let border_color = if has_error {
        Theme::ERROR
    } else if props.focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

    let required_marker = if props.required { " *" } else { "" };
    let full_label = format!("{}{}", label, required_marker);

    element! {
        View(flex_direction: FlexDirection::Column, margin_bottom: 1) {
            Text(content: full_label, color: Theme::TEXT_MUTED)
            View(
                border_style: BorderStyle::Round,
                border_color: border_color,
                padding_left: 1,
                padding_right: 1,
            ) {
                Text(content: display_text, color: text_color)
            }
            #(if has_error {
                Some(element! {
                    Text(content: error, color: Theme::ERROR)
                })
            } else {
                None
            })
        }
    }
}

/// Props for FormModal
#[derive(Default, Props)]
pub struct FormModalProps {
    /// Modal title
    pub title: String,
    /// Form fields
    pub fields: Vec<FormField>,
    /// Currently focused field index
    pub focused_field: usize,
    /// Submit button text
    pub submit_text: String,
    /// Cancel button text
    pub cancel_text: String,
    /// Whether the modal is visible
    pub visible: bool,
    /// Whether submit is enabled (all required fields filled)
    pub can_submit: bool,
}

/// A modal with multiple form fields
#[component]
pub fn FormModal(props: &FormModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        }
        .into_any();
    }

    let title = props.title.clone();
    let fields = props.fields.clone();
    let focused_field = props.focused_field;
    let submit_text = if props.submit_text.is_empty() {
        "Submit".to_string()
    } else {
        props.submit_text.clone()
    };
    let cancel_text = if props.cancel_text.is_empty() {
        "Cancel".to_string()
    } else {
        props.cancel_text.clone()
    };
    let can_submit = props.can_submit;

    element! {
        ModalContent(
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Round,
            border_color: Some(Theme::BORDER_FOCUS),
        ) {
            // Title bar
            View(
                width: 100pct,
                padding: 1,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
            ) {
                Text(content: title, weight: Weight::Bold, color: Theme::PRIMARY)
            }
            // Form fields - fills available space
            View(
                width: 100pct,
                padding: 1,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                overflow: Overflow::Hidden,
            ) {
                    #(fields.into_iter().enumerate().map(|(idx, field)| {
                        let is_focused = idx == focused_field;
                        element! {
                            FormFieldComponent(
                                label: field.label,
                                value: field.value,
                                placeholder: field.placeholder,
                                focused: is_focused,
                                required: field.required,
                                error: field.error.unwrap_or_default(),
                            )
                        }
                    }))
            }
            // Buttons and hints
            View(
                width: 100pct,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                padding: 1,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            ) {
                    View(flex_direction: FlexDirection::Row, gap: 1) {
                        Text(content: "Tab", color: Theme::SECONDARY)
                        Text(content: "Next field", color: Theme::TEXT_MUTED)
                    }
                    View(flex_direction: FlexDirection::Row, gap: 2) {
                        View(
                            padding_left: 2,
                            padding_right: 2,
                            border_style: BorderStyle::Round,
                            border_color: Theme::BORDER,
                        ) {
                            Text(content: cancel_text, color: Theme::TEXT)
                        }
                        View(
                            padding_left: 2,
                            padding_right: 2,
                            border_style: BorderStyle::Round,
                            border_color: if can_submit { Theme::PRIMARY } else { Theme::BORDER },
                        ) {
                            Text(
                                content: submit_text,
                                color: if can_submit { Theme::PRIMARY } else { Theme::TEXT_MUTED },
                            )
                        }
                }
            }
        }
    }
    .into_any()
}

/// State helper for form modal
#[derive(Clone, Debug, Default)]
pub struct FormModalState {
    /// Form fields
    pub fields: Vec<FormField>,
    /// Currently focused field index
    pub focused: usize,
    /// Whether the modal is visible
    pub visible: bool,
}

impl FormModalState {
    pub fn new(fields: Vec<FormField>) -> Self {
        Self {
            fields,
            focused: 0,
            visible: false,
        }
    }

    /// Show the modal
    pub fn show(&mut self) {
        self.visible = true;
        self.focused = 0;
    }

    /// Hide the modal
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Focus next field
    pub fn focus_next(&mut self) {
        if !self.fields.is_empty() {
            self.focused = (self.focused + 1) % self.fields.len();
        }
    }

    /// Focus previous field
    pub fn focus_prev(&mut self) {
        if !self.fields.is_empty() {
            self.focused = if self.focused == 0 {
                self.fields.len() - 1
            } else {
                self.focused - 1
            };
        }
    }

    /// Get current field
    pub fn current_field(&self) -> Option<&FormField> {
        self.fields.get(self.focused)
    }

    /// Get current field mutably
    pub fn current_field_mut(&mut self) -> Option<&mut FormField> {
        self.fields.get_mut(self.focused)
    }

    /// Set value for current field
    pub fn set_current_value(&mut self, value: impl Into<String>) {
        if let Some(field) = self.current_field_mut() {
            field.value = value.into();
            field.error = None; // Clear error on input
        }
    }

    /// Append char to current field
    pub fn push_char(&mut self, c: char) {
        if let Some(field) = self.current_field_mut() {
            field.value.push(c);
            field.error = None;
        }
    }

    /// Backspace on current field
    pub fn backspace(&mut self) {
        if let Some(field) = self.current_field_mut() {
            field.value.pop();
        }
    }

    /// Check if all required fields are filled
    pub fn can_submit(&self) -> bool {
        self.fields
            .iter()
            .filter(|f| f.required)
            .all(|f| !f.value.is_empty())
    }

    /// Get all field values as a map
    pub fn values(&self) -> std::collections::HashMap<String, String> {
        self.fields
            .iter()
            .map(|f| (f.id.clone(), f.value.clone()))
            .collect()
    }

    /// Get value for a specific field
    pub fn get_value(&self, id: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.value.as_str())
    }

    /// Set error on a field
    pub fn set_error(&mut self, id: &str, error: impl Into<String>) {
        if let Some(field) = self.fields.iter_mut().find(|f| f.id == id) {
            field.error = Some(error.into());
        }
    }

    /// Clear all errors
    pub fn clear_errors(&mut self) {
        for field in &mut self.fields {
            field.error = None;
        }
    }

    /// Reset form to initial state
    pub fn reset(&mut self) {
        for field in &mut self.fields {
            field.value.clear();
            field.error = None;
        }
        self.focused = 0;
    }
}
