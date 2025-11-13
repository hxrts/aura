//! Cache invalidation helpers used by maintenance flows.

use std::collections::HashMap;

use aura_core::{maintenance::CacheInvalidated, tree::Epoch as TreeEpoch};

/// Tracks per-key epoch floors derived from `CacheInvalidated` events.
#[derive(Debug, Default, Clone)]
pub struct CacheEpochFloors {
    floors: HashMap<String, TreeEpoch>,
}

impl CacheEpochFloors {
    /// Create an empty tracker.
    pub fn new() -> Self {
        Self {
            floors: HashMap::new(),
        }
    }

    /// Apply a cache invalidation event.
    pub fn apply(&mut self, event: &CacheInvalidated) {
        for key in &event.keys {
            let floor = self.floors.entry(key.clone()).or_insert(event.epoch_floor);
            if event.epoch_floor > *floor {
                *floor = event.epoch_floor;
            }
        }
    }

    /// Check whether a cache key can be served under the provided identity epoch.
    pub fn is_fresh(&self, key: &str, current_epoch: TreeEpoch) -> bool {
        match self.floors.get(key) {
            Some(floor) => current_epoch >= *floor,
            None => true,
        }
    }

    /// Return the known epoch floor for a key.
    pub fn epoch_floor(&self, key: &str) -> Option<TreeEpoch> {
        self.floors.get(key).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::maintenance::CacheInvalidated;

    #[test]
    fn cache_floor_monotonic() {
        let mut tracker = CacheEpochFloors::new();
        tracker.apply(&CacheInvalidated::new(vec!["foo".into()], 5_u64));
        assert!(!tracker.is_fresh("foo", 4_u64));
        assert!(tracker.is_fresh("foo", 5_u64));

        // higher floor overrides
        tracker.apply(&CacheInvalidated::new(vec!["foo".into()], 7_u64));
        assert!(!tracker.is_fresh("foo", 6_u64));
        assert!(tracker.is_fresh("foo", 7_u64));
    }
}
