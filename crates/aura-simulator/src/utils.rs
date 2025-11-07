//! Utility functions for the simulator

pub mod time {
    //! Time utilities for the simulator

    /// Get current timestamp in milliseconds since UNIX epoch
    pub fn current_unix_timestamp_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}
