//! Choreography definition for the Aura consensus protocol.

use aura_macros::choreography;

// Define the consensus choreography protocol
choreography!(include_str!("src/protocol/choreography.choreo"));
