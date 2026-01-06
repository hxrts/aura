//! Choreography annotations for AMP transport messages.
//!
//! Provides MPST-style metadata so guard capabilities/flow costs/journal facts
//! are enforced per message direction, aligning with docs/803_coordination_guide.md.

use aura_macros::choreography;

// Simple two-party choreography for AMP data + receipt exchange.
choreography!(include_str!("src/choreography.choreo"));
