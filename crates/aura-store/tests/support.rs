#![allow(missing_docs)]

use aura_core::time::PhysicalTime;

pub fn test_time(ts_ms: u64) -> PhysicalTime {
    PhysicalTime {
        ts_ms,
        uncertainty: None,
    }
}
