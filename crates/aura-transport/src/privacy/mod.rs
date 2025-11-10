//! Privacy-Preserving Peer Discovery and Manifest Management
//!
//! This module implements fixes for system incongruency #5:
//! "Device/Protocol manifests leaking metadata"
//!
//! ## Fix Pattern
//!
//! - Split manifests into relationship-scoped views (DKD-derived)
//! - Use capability-blinded manifests: publish only hashes/feature buckets
//! - Reveal details lazily within RID/GID contexts
//! - Add padding to keep size uniform
//!
//! ## Architecture
//!
//! ```
//! Raw DeviceMetadata -> RelationshipScopedView -> BlindedManifest
//!                            ^                         ^
//!                    (DKD-derived context)     (public broadcast)
//! ```

pub mod blinded_manifest;
pub mod manifest_manager;
pub mod relationship_scope;

pub use blinded_manifest::*;
pub use manifest_manager::*;
pub use relationship_scope::*;
