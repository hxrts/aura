//! Re-export of participant identity types.
//!
//! These types live in `crate::types::participants` so they can be shared across
//! modules that should not depend on `crate::threshold` (e.g. low-level crypto).

pub use crate::types::participants::{
    NetworkAddress, NetworkAddressError, ParticipantEndpoint, ParticipantIdentity, SignerIndexError,
    SigningParticipant,
};
