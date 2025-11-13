//! Utility functions for the simulator

pub mod time {
    //! Time utilities for the simulator

    /// Get current timestamp in milliseconds since UNIX epoch
    /// For simulation, uses fixed timestamp for deterministic behavior
    pub fn current_unix_timestamp_millis() -> u64 {
        1704067200000 // Fixed timestamp (2024-01-01) for simulation
    }
}
