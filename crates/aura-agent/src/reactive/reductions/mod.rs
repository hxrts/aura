//! Reactive View Reductions
//!
//! Pure functions that transform journal facts into view deltas.
//! Each reduction implements the `ViewReduction<Delta>` trait and provides
//! a monotone, deterministic mapping from facts to UI-consumable updates.
//!
//! ## Module Structure
//!
//! - `chat`: Chat view reduction delegating to `aura-chat`
//! - `guardian`: Guardian network view reduction
//! - `recovery`: Recovery flow view reduction delegating to `aura-recovery`
//! - `invitation`: Invitation view reduction delegating to `aura-invitation`
//! - `home`: Home/social view reduction for social topology

pub mod chat;
pub mod guardian;
pub mod home;
pub mod invitation;
pub mod recovery;

// Re-export all reduction types for convenience
pub use chat::ChatReduction;
pub use guardian::{GuardianDelta, GuardianReduction};
pub use home::{HomeDelta, HomeReduction};
pub use invitation::InvitationReduction;
pub use recovery::RecoveryReduction;
