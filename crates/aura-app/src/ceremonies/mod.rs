//! # Type-Safe Ceremony Patterns
//!
//! This module provides type-safe newtypes and builders for ceremony setup operations.
//! These patterns enforce preconditions at compile-time rather than runtime, preventing
//! invalid ceremony configurations from being constructed.
//!
//! ## Design Philosophy
//!
//! Each ceremony type uses the newtype pattern to encapsulate validated state:
//!
//! ```rust,ignore
//! // Instead of runtime validation:
//! fn setup_guardians(contacts: Vec<ContactId>) -> Result<(), Error> {
//!     if contacts.is_empty() {
//!         return Err(Error::NoContacts);
//!     }
//!     // ...
//! }
//!
//! // Use type-safe validation:
//! let candidates = GuardianCandidates::from_contacts(contacts)?;
//! setup_guardians(candidates); // Cannot fail due to empty contacts
//! ```
//!
//! ## Usage
//!
//! UI/CLI code should use these types to validate before opening modals:
//!
//! ```rust,ignore
//! match GuardianCandidates::from_contacts(current_contacts) {
//!     Ok(candidates) => open_guardian_setup_modal(candidates),
//!     Err(e) => show_toast(e.to_string()),
//! }
//! ```

mod channel;
mod enrollment;
mod guardian;
mod invitation;
mod mfa;
mod recovery;
mod threshold;

pub use channel::{ChannelError, ChannelParticipants, MIN_CHANNEL_PARTICIPANTS};
pub use enrollment::{EnrollmentContext, EnrollmentError};
pub use guardian::{GuardianCandidates, GuardianSetupError};
pub use invitation::{ChannelRole, InvitationConfig, InvitationError};
pub use mfa::{MfaDeviceSet, MfaSetupError, MIN_MFA_DEVICES};
pub use recovery::{RecoveryEligible, RecoveryError};
pub use threshold::{ThresholdConfig, ThresholdError};

#[cfg(test)]
mod tests;
