//! Test that `build()` is not available when no effects are provided.
//!
//! This should fail to compile because the CustomPresetBuilder requires
//! all five core effects before `build()` becomes available.

use aura_agent::AgentBuilder;

fn main() {
    // Should NOT compile: build() requires all effects to be provided
    let _builder = AgentBuilder::custom().build();
}
