// Core types for storage layer

use serde::{Deserialize, Serialize};

/// Storage class indicating ownership model
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum StoreClass {
    /// Data owned by this device
    Owned,
    /// Data shared from a friend's device
    SharedFromFriend,
}

/// Pin class indicating storage persistence
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PinClass {
    /// Permanently pinned data that should not be garbage collected
    Pin,
    /// Cached data that can be evicted under storage pressure
    Cache,
}
