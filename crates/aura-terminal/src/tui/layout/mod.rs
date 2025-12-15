//! # TUI Layout System
//!
//! Compile-time enforced micro tiling system for the Aura TUI.
//!
//! ## Overview
//!
//! This module provides a fixed-dimension layout system that guarantees:
//! - All screens have identical dimensions (80×31)
//! - Nav bar and footer have fixed height/width/placement (3 rows each)
//! - Middle content area is exactly 80×25
//! - Modals overlay the middle region exactly
//! - Toasts overlay the footer region exactly
//! - Overflow is detected at compile time (static assertions) and runtime (debug assertions)
//!
//! ## Layout Structure
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────────────────┐
//! │                              NAV BAR (3 rows)                                   │
//! ├────────────────────────────────────────────────────────────────────────────────┤
//! │                                                                                 │
//! │                                                                                 │
//! │                         MIDDLE WINDOW (25 rows)                                 │
//! │                     (screen-specific content here)                              │
//! │                                                                                 │
//! │                                                                                 │
//! ├────────────────────────────────────────────────────────────────────────────────┤
//! │                              FOOTER (3 rows)                                    │
//! └────────────────────────────────────────────────────────────────────────────────┘
//!                              Total: 80 × 31
//! ```
//!
//! ## Usage
//!
//! ### Building Screen Content
//!
//! ```ignore
//! use crate::tui::layout::{MiddleBuilder, dim};
//!
//! fn render_chat_screen() -> MiddleContent {
//!     MiddleBuilder::middle()
//!         .add(render_header(), 2)      // 2 rows for header
//!         .add(render_messages(), 21)   // 21 rows for messages
//!         .add(render_input(), 2)       // 2 rows for input
//!         .build()                      // Total: 25 = MIDDLE_HEIGHT
//! }
//! ```
//!
//! ### Using the Compositor
//!
//! ```ignore
//! use crate::tui::layout::LayoutCompositor;
//!
//! let compositor = LayoutCompositor::new(terminal_width, terminal_height)?;
//!
//! // Get fixed regions for absolute positioning
//! let nav = compositor.nav_rect();
//! let middle = compositor.middle_rect();
//! let footer = compositor.footer_rect();
//! ```
//!
//! ## Modules
//!
//! - [`dimensions`]: Fixed dimension constants with compile-time validation
//! - [`regions`]: Phantom types for type-level region enforcement
//! - [`content`]: Bounded content types that guarantee fit
//! - [`compositor`]: Layout manager for fixed region placement
//! - [`overflow`]: Content builder with overflow prevention

pub mod compositor;
pub mod content;
pub mod dimensions;
pub mod modal_overlay;
pub mod modal_trait;
pub mod overflow;
pub mod regions;
pub mod screen_trait;
pub mod toast_overlay;
pub mod toast_trait;

// Re-export commonly used types
pub use compositor::{LayoutCompositor, LayoutError, Rect};
pub use content::{
    empty_content, footer_content, middle_content, nav_content, BoundedContent, FooterContent,
    MiddleContent, ModalContent, NavContent, ToastContent,
};
pub use dimensions::dim;
pub use modal_overlay::{modal_rect, ConfirmationModal, SimpleModal};
pub use modal_trait::{ModalContext, ModalLayout};
pub use overflow::{
    ContentBuilder, FooterBuilder, MiddleBuilder, ModalBuilder, NavBuilder, ToastBuilder,
};
pub use regions::{region, Footer, LayoutRegion, Middle, Nav, RegionMarker};
pub use screen_trait::{KeyHint, ScreenContext, ScreenLayout};
pub use toast_overlay::{
    toast_rect, ErrorToast, InfoToast, SuccessToast, ToastOverlay, WarningToast,
};
pub use toast_trait::{ToastContext, ToastLayout, ToastLevel};
