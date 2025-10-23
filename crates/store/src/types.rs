// Core types for storage layer

use serde::{Deserialize, Serialize};

/// Storage class
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum StoreClass {
    Owned,
    SharedFromFriend,
}

/// Pin class
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PinClass {
    Pin,
    Cache,
}

