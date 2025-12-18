//! # ViewState Snapshot Helper
//!
//! Provides synchronous snapshot access to AppCore ViewState for initial rendering.
//! Screens should subscribe directly to AppCore signals for reactive updates.
//!
//! Note: This is a thin wrapper that delegates to IoContext's snapshot methods.
//! The actual snapshot logic remains in IoContext for now to avoid duplication.
