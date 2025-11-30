//! # Modal Dialog Component
//!
//! Centered overlay dialogs for confirmations, forms, and alerts.
//! Supports customizable buttons and keyboard navigation.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use super::{Component, InputAction, Styles};

/// Button configuration for modals
#[derive(Debug, Clone)]
pub struct ModalButton {
    /// Button label
    pub label: String,
    /// Action to perform when clicked
    pub action: ModalAction,
    /// Whether this is the primary (highlighted) button
    pub primary: bool,
}

impl ModalButton {
    /// Create a new button
    pub fn new(label: impl Into<String>, action: ModalAction) -> Self {
        Self {
            label: label.into(),
            action,
            primary: false,
        }
    }

    /// Create a primary button
    pub fn primary(label: impl Into<String>, action: ModalAction) -> Self {
        Self {
            label: label.into(),
            action,
            primary: true,
        }
    }

    /// Create a cancel button
    pub fn cancel() -> Self {
        Self::new("Cancel", ModalAction::Cancel)
    }

    /// Create a confirm button
    pub fn confirm(label: impl Into<String>) -> Self {
        Self::primary(label, ModalAction::Confirm)
    }

    /// Create an OK button
    pub fn ok() -> Self {
        Self::primary("OK", ModalAction::Confirm)
    }
}

/// Actions that can be performed by modal buttons
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalAction {
    /// Confirm/accept the dialog
    Confirm,
    /// Cancel/dismiss the dialog
    Cancel,
    /// Custom action with identifier
    Custom(String),
}

/// A modal dialog
#[derive(Debug, Clone)]
pub struct Modal {
    /// Dialog title
    pub title: String,
    /// Dialog message/content
    pub message: String,
    /// Available buttons
    pub buttons: Vec<ModalButton>,
    /// Currently selected button index
    selected_button: usize,
    /// Whether the modal is visible
    visible: bool,
    /// Width as percentage of screen (0-100)
    width_percent: u16,
    /// Height as percentage of screen (0-100)
    height_percent: u16,
}

impl Modal {
    /// Create a new modal dialog
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            buttons: vec![ModalButton::ok()],
            selected_button: 0,
            visible: false,
            width_percent: 60,
            height_percent: 40,
        }
    }

    /// Create a confirmation modal with OK/Cancel buttons
    pub fn confirm(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            buttons: vec![ModalButton::cancel(), ModalButton::confirm("Confirm")],
            selected_button: 1, // Default to confirm
            visible: false,
            width_percent: 60,
            height_percent: 40,
        }
    }

    /// Create an alert modal (just OK button)
    pub fn alert(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(title, message)
    }

    /// Set custom buttons
    pub fn with_buttons(mut self, buttons: Vec<ModalButton>) -> Self {
        self.buttons = buttons;
        if self.selected_button >= self.buttons.len() {
            self.selected_button = 0;
        }
        self
    }

    /// Set dialog size
    pub fn with_size(mut self, width_percent: u16, height_percent: u16) -> Self {
        self.width_percent = width_percent.min(100);
        self.height_percent = height_percent.min(100);
        self
    }

    /// Show the modal
    pub fn show(&mut self) {
        self.visible = true;
    }

    /// Hide the modal
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Toggle visibility
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Get the currently selected action
    pub fn selected_action(&self) -> Option<&ModalAction> {
        self.buttons.get(self.selected_button).map(|b| &b.action)
    }

    /// Move selection to next button
    fn select_next(&mut self) {
        if !self.buttons.is_empty() {
            self.selected_button = (self.selected_button + 1) % self.buttons.len();
        }
    }

    /// Move selection to previous button
    fn select_prev(&mut self) {
        if !self.buttons.is_empty() {
            self.selected_button = if self.selected_button == 0 {
                self.buttons.len() - 1
            } else {
                self.selected_button - 1
            };
        }
    }

    /// Calculate centered rect
    fn centered_rect(&self, area: Rect) -> Rect {
        let width = (area.width as u32 * self.width_percent as u32 / 100) as u16;
        let height = (area.height as u32 * self.height_percent as u32 / 100) as u16;

        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;

        Rect::new(x, y, width, height)
    }
}

impl Component for Modal {
    fn handle_key(&mut self, key: KeyEvent) -> Option<InputAction> {
        if !self.visible {
            return None;
        }

        match key.code {
            KeyCode::Enter => {
                let action = self.selected_action().cloned();
                self.hide();
                match action {
                    Some(ModalAction::Confirm) => Some(InputAction::Submit("confirm".to_string())),
                    Some(ModalAction::Cancel) => Some(InputAction::ExitToNormal),
                    Some(ModalAction::Custom(id)) => Some(InputAction::Submit(id)),
                    None => Some(InputAction::ExitToNormal),
                }
            }
            KeyCode::Esc => {
                self.hide();
                Some(InputAction::ExitToNormal)
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Down => {
                self.select_next();
                Some(InputAction::None)
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Up => {
                self.select_prev();
                Some(InputAction::None)
            }
            _ => Some(InputAction::None),
        }
    }

    fn render(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        if !self.visible {
            return;
        }

        let modal_area = self.centered_rect(area);

        // Clear background
        f.render_widget(Clear, modal_area);

        // Create layout: title, content, buttons
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Min(1),    // Content
                Constraint::Length(3), // Buttons
            ])
            .split(modal_area);

        // Render modal block
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(styles.border_focused())
            .title(self.title.as_str());

        f.render_widget(block, modal_area);

        // Render message
        let message = Paragraph::new(self.message.as_str())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        f.render_widget(message, chunks[0]);

        // Render buttons
        let button_text: Vec<Span> = self
            .buttons
            .iter()
            .enumerate()
            .flat_map(|(i, button)| {
                let is_selected = i == self.selected_button;
                let style = if is_selected {
                    if button.primary {
                        styles.text_highlight().add_modifier(Modifier::REVERSED)
                    } else {
                        styles.text().add_modifier(Modifier::REVERSED)
                    }
                } else if button.primary {
                    styles.text_highlight()
                } else {
                    styles.text()
                };

                vec![
                    Span::styled(format!(" {} ", button.label), style),
                    Span::raw("  "),
                ]
            })
            .collect();

        let buttons = Paragraph::new(Line::from(button_text)).alignment(Alignment::Center);

        f.render_widget(buttons, chunks[1]);
    }

    fn is_focused(&self) -> bool {
        self.visible
    }

    fn set_focused(&mut self, focused: bool) {
        if focused {
            self.show();
        }
    }

    fn min_size(&self) -> (u16, u16) {
        (40, 10)
    }

    fn is_visible(&self) -> bool {
        self.visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modal_creation() {
        let modal = Modal::new("Title", "Message");
        assert_eq!(modal.title, "Title");
        assert_eq!(modal.message, "Message");
        assert_eq!(modal.buttons.len(), 1);
        assert!(!modal.visible);
    }

    #[test]
    fn test_modal_confirm() {
        let modal = Modal::confirm("Confirm?", "Are you sure?");
        assert_eq!(modal.buttons.len(), 2);
        assert_eq!(modal.buttons[0].action, ModalAction::Cancel);
        assert_eq!(modal.buttons[1].action, ModalAction::Confirm);
    }

    #[test]
    fn test_modal_visibility() {
        let mut modal = Modal::new("Test", "Test");
        assert!(!modal.visible);

        modal.show();
        assert!(modal.visible);

        modal.hide();
        assert!(!modal.visible);

        modal.toggle();
        assert!(modal.visible);
    }

    #[test]
    fn test_modal_button_navigation() {
        let mut modal = Modal::confirm("Test", "Test");
        assert_eq!(modal.selected_button, 1); // Default to confirm

        modal.select_prev();
        assert_eq!(modal.selected_button, 0);

        modal.select_prev();
        assert_eq!(modal.selected_button, 1); // Wrap around

        modal.select_next();
        assert_eq!(modal.selected_button, 0); // Wrap around
    }

    #[test]
    fn test_modal_selected_action() {
        let modal = Modal::confirm("Test", "Test");
        // Default selection is 1 (Confirm)
        assert_eq!(modal.selected_action(), Some(&ModalAction::Confirm));
    }

    #[test]
    fn test_modal_custom_buttons() {
        let modal = Modal::new("Custom", "Test").with_buttons(vec![
            ModalButton::new("Yes", ModalAction::Custom("yes".to_string())),
            ModalButton::new("No", ModalAction::Custom("no".to_string())),
            ModalButton::new("Maybe", ModalAction::Custom("maybe".to_string())),
        ]);

        assert_eq!(modal.buttons.len(), 3);
    }
}
