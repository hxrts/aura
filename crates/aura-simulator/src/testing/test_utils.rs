//! World State Test Factory
//!
//! Consolidates the many duplicated `create_test_world_state()` functions
//! found across the simulator crate into a single, configurable factory.

use crate::world_state::WorldState;

/// Factory for creating test world states with various configurations
pub struct WorldStateFactory {
    seed: u64,
    participants: Vec<(String, String, String)>, // (name, device_id, account_id)
    current_tick: u64,
}

impl Default for WorldStateFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl WorldStateFactory {
    /// Create a new factory with default test configuration
    pub fn new() -> Self {
        Self {
            seed: 42,
            participants: vec![(
                "alice".to_string(),
                "device_alice".to_string(),
                "account_1".to_string(),
            )],
            current_tick: 0,
        }
    }

    /// Set the random seed for the world state
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Add a participant to the world state
    pub fn with_participant(mut self, name: &str, device_id: &str, account_id: &str) -> Self {
        self.participants.push((
            name.to_string(),
            device_id.to_string(),
            account_id.to_string(),
        ));
        self
    }

    /// Add multiple participants with a common pattern
    pub fn with_participants(mut self, count: usize) -> Self {
        self.participants.clear();
        for i in 0..count {
            let name = match i {
                0 => "alice",
                1 => "bob",
                2 => "charlie",
                3 => "dave",
                4 => "eve",
                _ => "participant",
            };
            self.participants.push((
                name.to_string(),
                format!("device_{}", name),
                "account_1".to_string(),
            ));
        }
        self
    }

    /// Set the current tick for the world state
    pub fn with_current_tick(mut self, tick: u64) -> Self {
        self.current_tick = tick;
        self
    }

    /// Build the world state with the configured parameters
    pub fn build(self) -> WorldState {
        let mut world = WorldState::new(self.seed);

        // Add all participants
        for (name, device_id, account_id) in self.participants {
            world.add_participant(name, device_id, account_id).unwrap();
        }

        // Set current tick if specified
        if self.current_tick > 0 {
            world.current_tick = self.current_tick;
        }

        world
    }

    /// Create a minimal single-participant world state (most common pattern)
    pub fn minimal() -> WorldState {
        Self::new().build()
    }

    /// Create a two-participant world state (second most common pattern)
    pub fn two_party() -> WorldState {
        Self::new().with_participants(2).build()
    }

    /// Create a three-participant world state (common for threshold scenarios)
    pub fn three_party() -> WorldState {
        Self::new().with_participants(3).build()
    }
}

/// Quick access functions for the most common patterns
pub fn minimal_world_state() -> WorldState {
    WorldStateFactory::minimal()
}

pub fn two_party_world_state() -> WorldState {
    WorldStateFactory::two_party()
}

pub fn three_party_world_state() -> WorldState {
    WorldStateFactory::three_party()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_world_state_factory_default() {
        let world = WorldStateFactory::new().build();
        assert_eq!(world.participants.len(), 1);
        assert_eq!(world.current_tick, 0);
    }

    #[test]
    fn test_world_state_factory_multiple_participants() {
        let world = WorldStateFactory::new().with_participants(3).build();
        assert_eq!(world.participants.len(), 3);
    }

    #[test]
    fn test_world_state_factory_custom_tick() {
        let world = WorldStateFactory::new().with_current_tick(100).build();
        assert_eq!(world.current_tick, 100);
    }

    #[test]
    fn test_convenience_functions() {
        let minimal = minimal_world_state();
        assert_eq!(minimal.participants.len(), 1);

        let two_party = two_party_world_state();
        assert_eq!(two_party.participants.len(), 2);

        let three_party = three_party_world_state();
        assert_eq!(three_party.participants.len(), 3);
    }
}
