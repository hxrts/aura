//! # TUI Theming and Styles
//!
//! Centralized color palette and styling for consistent TUI appearance.
//! Supports future dark/light theme switching.

use ratatui::style::{Color, Modifier, Style};

/// Color palette for the Aura TUI
#[derive(Debug, Clone, Copy)]
pub struct ColorPalette {
    /// Primary brand color (used for highlights, active elements)
    pub primary: Color,
    /// Secondary color (used for accents)
    pub secondary: Color,
    /// Success/positive state
    pub success: Color,
    /// Warning state
    pub warning: Color,
    /// Error/danger state
    pub error: Color,
    /// Informational text
    pub info: Color,
    /// Main background color
    pub background: Color,
    /// Secondary/elevated background
    pub surface: Color,
    /// Primary text color
    pub text_primary: Color,
    /// Secondary/muted text
    pub text_secondary: Color,
    /// Border color
    pub border: Color,
    /// Border color when focused
    pub border_focused: Color,
}

impl Default for ColorPalette {
    fn default() -> Self {
        Self::dark()
    }
}

impl ColorPalette {
    /// Dark theme palette (default)
    pub const fn dark() -> Self {
        Self {
            primary: Color::Blue,
            secondary: Color::Cyan,
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            info: Color::LightBlue,
            background: Color::Reset,
            surface: Color::DarkGray,
            text_primary: Color::White,
            text_secondary: Color::Gray,
            border: Color::DarkGray,
            border_focused: Color::Blue,
        }
    }

    /// Light theme palette
    pub const fn light() -> Self {
        Self {
            primary: Color::Blue,
            secondary: Color::Cyan,
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            info: Color::LightBlue,
            background: Color::White,
            surface: Color::LightYellow,
            text_primary: Color::Black,
            text_secondary: Color::DarkGray,
            border: Color::Gray,
            border_focused: Color::Blue,
        }
    }
}

/// Reusable style definitions
#[derive(Debug, Clone)]
pub struct Styles {
    /// Color palette
    pub palette: ColorPalette,
}

impl Default for Styles {
    fn default() -> Self {
        Self::new(ColorPalette::default())
    }
}

impl Styles {
    /// Create styles with the given palette
    pub const fn new(palette: ColorPalette) -> Self {
        Self { palette }
    }

    /// Style for normal text
    pub fn text(&self) -> Style {
        Style::default().fg(self.palette.text_primary)
    }

    /// Style for muted/secondary text
    pub fn text_muted(&self) -> Style {
        Style::default().fg(self.palette.text_secondary)
    }

    /// Style for highlighted/selected text
    pub fn text_highlight(&self) -> Style {
        Style::default()
            .fg(self.palette.primary)
            .add_modifier(Modifier::BOLD)
    }

    /// Style for success messages
    pub fn text_success(&self) -> Style {
        Style::default().fg(self.palette.success)
    }

    /// Style for warning messages
    pub fn text_warning(&self) -> Style {
        Style::default().fg(self.palette.warning)
    }

    /// Style for error messages
    pub fn text_error(&self) -> Style {
        Style::default().fg(self.palette.error)
    }

    /// Style for info messages
    pub fn text_info(&self) -> Style {
        Style::default().fg(self.palette.info)
    }

    /// Style for normal borders
    pub fn border(&self) -> Style {
        Style::default().fg(self.palette.border)
    }

    /// Style for focused borders
    pub fn border_focused(&self) -> Style {
        Style::default().fg(self.palette.border_focused)
    }

    /// Style for status bar background
    pub fn status_bar(&self) -> Style {
        Style::default()
            .fg(self.palette.text_primary)
            .bg(self.palette.surface)
    }

    /// Style for mode indicator (NORMAL, EDIT, COMMAND)
    pub fn mode_indicator(&self, mode: &str) -> Style {
        let color = match mode {
            "NORMAL" => self.palette.primary,
            "EDIT" => self.palette.success,
            "COMMAND" => self.palette.warning,
            _ => self.palette.info,
        };
        Style::default()
            .fg(Color::Black)
            .bg(color)
            .add_modifier(Modifier::BOLD)
    }

    /// Style for progress gauge
    pub fn gauge(&self) -> Style {
        Style::default().fg(self.palette.primary)
    }

    /// Style for progress gauge when complete
    pub fn gauge_complete(&self) -> Style {
        Style::default().fg(self.palette.success)
    }

    /// Style for guardian status (approved)
    pub fn guardian_approved(&self) -> Style {
        Style::default().fg(self.palette.success)
    }

    /// Style for guardian status (pending)
    pub fn guardian_pending(&self) -> Style {
        Style::default().fg(self.palette.warning)
    }

    /// Style for guardian status (offline)
    pub fn guardian_offline(&self) -> Style {
        Style::default().fg(self.palette.error)
    }

    /// Style for toast notifications based on level
    pub fn toast(&self, level: ToastLevel) -> Style {
        let color = match level {
            ToastLevel::Info => self.palette.info,
            ToastLevel::Success => self.palette.success,
            ToastLevel::Warning => self.palette.warning,
            ToastLevel::Error => self.palette.error,
        };
        Style::default().fg(color)
    }
}

/// Toast notification severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    /// Informational message
    Info,
    /// Success message
    Success,
    /// Warning message
    Warning,
    /// Error message
    Error,
}

/// Global styles instance
static DEFAULT_STYLES: std::sync::OnceLock<Styles> = std::sync::OnceLock::new();

/// Get the default styles instance
pub fn styles() -> &'static Styles {
    DEFAULT_STYLES.get_or_init(Styles::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dark_palette() {
        let palette = ColorPalette::dark();
        assert_eq!(palette.primary, Color::Blue);
        assert_eq!(palette.success, Color::Green);
    }

    #[test]
    fn test_light_palette() {
        let palette = ColorPalette::light();
        assert_eq!(palette.background, Color::White);
        assert_eq!(palette.text_primary, Color::Black);
    }

    #[test]
    fn test_styles() {
        let styles = Styles::default();
        let text = styles.text();
        assert_eq!(text.fg, Some(Color::White));
    }

    #[test]
    fn test_mode_indicator() {
        let styles = Styles::default();
        let normal = styles.mode_indicator("NORMAL");
        assert_eq!(normal.bg, Some(Color::Blue));
    }

    #[test]
    fn test_toast_styles() {
        let styles = Styles::default();
        let error_style = styles.toast(ToastLevel::Error);
        assert_eq!(error_style.fg, Some(Color::Red));
    }
}
