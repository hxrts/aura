//! # TUI Screens
//!
//! Screen definitions for different views in Bob's demo.

/// Different screen types in the TUI
#[derive(Debug, Clone, PartialEq)]
pub enum ScreenType {
    /// Welcome/intro screen
    Welcome,
    /// Main demo interface
    Demo,
    /// Guardian interface view
    Guardian,
    /// Technical details view
    Technical,
}

/// Trait for renderable screens
pub trait Screen {
    /// Get the screen type
    fn screen_type(&self) -> ScreenType;

    /// Get the screen title
    fn title(&self) -> &str;
}
