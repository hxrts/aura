//! Page-owned browser harness globals.
//!
//! Ownership boundary:
//! - `driver_contract.rs` owns driver queue/callback globals shared with the
//!   Playwright driver.
//! - this module owns page-local observation, publication, generation, and
//!   harness API globals that stay inside the browser shell.

pub(crate) const HARNESS_API_KEY: &str = "__AURA_HARNESS__";
pub(crate) const HARNESS_OBSERVE_KEY: &str = "__AURA_HARNESS_OBSERVE__";
pub(crate) const HARNESS_CLIPBOARD_KEY: &str = "__AURA_HARNESS_CLIPBOARD__";

pub(crate) const UI_STATE_CACHE_KEY: &str = "__AURA_UI_STATE_CACHE__";
pub(crate) const UI_STATE_JSON_KEY: &str = "__AURA_UI_STATE_JSON__";
pub(crate) const UI_STATE_OBSERVE_KEY: &str = "__AURA_UI_STATE__";

pub(crate) const UI_PUBLICATION_STATE_KEY: &str = "__AURA_UI_PUBLICATION_STATE__";
pub(crate) const RENDER_HEARTBEAT_PUBLICATION_STATE_KEY: &str =
    "__AURA_RENDER_HEARTBEAT_PUBLICATION_STATE__";
pub(crate) const SEMANTIC_SUBMIT_PUBLICATION_STATE_KEY: &str =
    "__AURA_SEMANTIC_SUBMIT_PUBLICATION_STATE__";
pub(crate) const SEMANTIC_DEBUG_KEY: &str = "__AURA_SEMANTIC_DEBUG__";

pub(crate) const UI_ACTIVE_GENERATION_KEY: &str = "__AURA_UI_ACTIVE_GENERATION__";
pub(crate) const UI_READY_GENERATION_KEY: &str = "__AURA_UI_READY_GENERATION__";
pub(crate) const UI_GENERATION_PHASE_KEY: &str = "__AURA_UI_GENERATION_PHASE__";

pub(crate) const DRIVER_PUSH_UI_STATE_KEY: &str = "__AURA_DRIVER_PUSH_UI_STATE";
pub(crate) const DRIVER_PUSH_RENDER_HEARTBEAT_KEY: &str =
    "__AURA_DRIVER_PUSH_RENDER_HEARTBEAT";
pub(crate) const RENDER_HEARTBEAT_KEY: &str = "__AURA_RENDER_HEARTBEAT__";
pub(crate) const RENDER_HEARTBEAT_JSON_KEY: &str = "__AURA_RENDER_HEARTBEAT_JSON__";
pub(crate) const RENDER_HEARTBEAT_STATE_KEY: &str = "__AURA_RENDER_HEARTBEAT_STATE__";
