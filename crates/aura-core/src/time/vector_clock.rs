//! VectorClock implementation with single-device optimization.

use crate::{types::identifiers::DeviceId, AuraError};
use std::collections::BTreeMap;

use super::VectorClock;

impl Default for VectorClock {
    fn default() -> Self {
        Self::new()
    }
}

impl VectorClock {
    pub fn new() -> Self {
        VectorClock::Multiple(BTreeMap::new())
    }

    /// Create a VectorClock for a single device.
    pub fn single(device: DeviceId, counter: u64) -> Self {
        VectorClock::Single { device, counter }
    }

    pub fn insert(&mut self, device: DeviceId, counter: u64) {
        match self {
            VectorClock::Single {
                device: current_device,
                counter: current_counter,
            } => {
                if device == *current_device {
                    *current_counter = counter;
                } else {
                    let mut map = BTreeMap::new();
                    map.insert(*current_device, *current_counter);
                    map.insert(device, counter);
                    *self = VectorClock::Multiple(map);
                }
            }
            VectorClock::Multiple(map) => {
                if map.is_empty() {
                    *self = VectorClock::Single { device, counter };
                } else {
                    match map.get(&device) {
                        Some(&existing) if existing == counter => {}
                        _ => {
                            map.insert(device, counter);
                        }
                    }
                }
            }
        }
    }

    pub fn get(&self, device: &DeviceId) -> Option<&u64> {
        match self {
            VectorClock::Single {
                device: current_device,
                counter,
            } => {
                if device == current_device {
                    Some(counter)
                } else {
                    None
                }
            }
            VectorClock::Multiple(map) => map.get(device),
        }
    }

    /// Increment a device's counter.
    pub fn increment(&mut self, device: DeviceId) -> Result<(), AuraError> {
        match self {
            VectorClock::Single {
                device: current_device,
                counter: current_counter,
            } => {
                if device == *current_device {
                    *current_counter = current_counter.checked_add(1).ok_or_else(|| {
                        AuraError::invalid("VectorClock overflow on single-device increment")
                    })?;
                } else {
                    let old_counter = *current_counter;
                    let mut map = BTreeMap::new();
                    map.insert(*current_device, old_counter);
                    map.insert(device, 1);
                    *self = VectorClock::Multiple(map);
                }
            }
            VectorClock::Multiple(map) => {
                if map.is_empty() {
                    *self = VectorClock::Single { device, counter: 1 };
                } else if let Some(counter) = map.get_mut(&device) {
                    *counter = counter.checked_add(1).ok_or_else(|| {
                        AuraError::invalid("VectorClock overflow on multi-device increment")
                    })?;
                } else {
                    map.insert(device, 1);
                }
            }
        }
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&DeviceId, &u64)> {
        VectorClockIter::new(self)
    }

    pub fn len(&self) -> usize {
        match self {
            VectorClock::Single { .. } => 1,
            VectorClock::Multiple(map) => map.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            VectorClock::Single { .. } => false,
            VectorClock::Multiple(map) => map.is_empty(),
        }
    }
}

impl PartialOrd for VectorClock {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (
                VectorClock::Single {
                    device: d1,
                    counter: c1,
                },
                VectorClock::Single {
                    device: d2,
                    counter: c2,
                },
            ) => {
                if d1 == d2 {
                    c1.partial_cmp(c2)
                } else {
                    None
                }
            }
            _ => {
                let mut self_le_other = true;
                let mut other_le_self = true;

                for (device, self_counter) in self.iter() {
                    if let Some(other_counter) = other.get(device) {
                        if self_counter > other_counter {
                            self_le_other = false;
                            break;
                        }
                    } else if *self_counter > 0 {
                        self_le_other = false;
                        break;
                    }
                }

                for (device, other_counter) in other.iter() {
                    if let Some(self_counter) = self.get(device) {
                        if other_counter > self_counter {
                            other_le_self = false;
                            break;
                        }
                    } else if *other_counter > 0 {
                        other_le_self = false;
                        break;
                    }
                }

                match (self_le_other, other_le_self) {
                    (true, true) => Some(std::cmp::Ordering::Equal),
                    (true, false) => Some(std::cmp::Ordering::Less),
                    (false, true) => Some(std::cmp::Ordering::Greater),
                    (false, false) => None,
                }
            }
        }
    }
}

/// Iterator for VectorClock that handles both representations.
pub enum VectorClockIter<'a> {
    Single {
        device: &'a DeviceId,
        counter: &'a u64,
        yielded: bool,
    },
    Multiple(std::collections::btree_map::Iter<'a, DeviceId, u64>),
}

impl<'a> VectorClockIter<'a> {
    fn new(clock: &'a VectorClock) -> Self {
        match clock {
            VectorClock::Single { device, counter } => VectorClockIter::Single {
                device,
                counter,
                yielded: false,
            },
            VectorClock::Multiple(map) => VectorClockIter::Multiple(map.iter()),
        }
    }
}

impl<'a> Iterator for VectorClockIter<'a> {
    type Item = (&'a DeviceId, &'a u64);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            VectorClockIter::Single {
                device,
                counter,
                yielded,
            } => {
                if *yielded {
                    None
                } else {
                    *yielded = true;
                    Some((device, counter))
                }
            }
            VectorClockIter::Multiple(iter) => iter.next(),
        }
    }
}
