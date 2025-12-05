//! Relay Selection Module
//!
//! Provides relay candidate building based on social topology.
//! This module wraps `SocialTopology::build_relay_candidates` with
//! a more structured API using `RelayContext`.

mod candidates;

pub use candidates::{ReachabilityChecker, RelayCandidateBuilder};
