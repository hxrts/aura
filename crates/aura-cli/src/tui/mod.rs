//! # Aura TUI - Terminal User Interface for Demo
//!
//! Professional terminal interface for Bob's recovery demo journey.
//! Built with Ratatui for rich interactive experiences.

pub mod app;
pub mod components;
pub mod demo;
pub mod guardian;
pub mod screens;
pub mod state;

pub use app::{DemoEvent, TuiApp};
pub use demo::DemoInterface;
pub use guardian::GuardianInterface;
pub use state::AppState;
