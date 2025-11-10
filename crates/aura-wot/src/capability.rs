//! Capability types implementing meet-semilattice laws
//!
//! This module provides the core capability abstractions that follow
//! meet-semilattice laws for monotonic capability restriction.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

/// A capability that can be attenuated via meet operations
///
/// Capabilities represent permissions that can only be restricted (never expanded)
/// through meet operations. This ensures monotonic security properties.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Capability {
    /// Permission to read specific resources
    Read { resource_pattern: String },

    /// Permission to write/modify specific resources
    Write { resource_pattern: String },

    /// Permission to execute operations
    Execute { operation: String },

    /// Permission to delegate capabilities (with restriction)
    Delegate { max_depth: u32 },

    /// Administrative permissions
    Admin { scope: String },

    /// Permission to act as relay with flow budget limits
    Relay {
        /// Maximum bytes per time period
        max_bytes_per_period: u64,
        /// Time period in seconds
        period_seconds: u64,
        /// Maximum concurrent streams
        max_streams: u32,
    },

    /// Wildcard capability (top element ⊤)
    All,

    /// Empty capability (bottom element ⊥)
    None,
}

impl Capability {
    /// Check if this capability implies another capability
    pub fn implies(&self, other: &Capability) -> bool {
        use Capability::*;

        match (self, other) {
            // All implies everything
            (All, _) => true,

            // Nothing is implied by None
            (None, _) => false,
            (_, None) => true,

            // Same capability types
            (
                Read {
                    resource_pattern: a,
                },
                Read {
                    resource_pattern: b,
                },
            ) => pattern_implies(a, b),
            (
                Write {
                    resource_pattern: a,
                },
                Write {
                    resource_pattern: b,
                },
            ) => pattern_implies(a, b),
            (Execute { operation: a }, Execute { operation: b }) => a == "*" || a == b,
            (Delegate { max_depth: a }, Delegate { max_depth: b }) => a >= b,
            (Admin { scope: a }, Admin { scope: b }) => a == "*" || scope_implies(a, b),
            (
                Relay {
                    max_bytes_per_period: bytes_a,
                    period_seconds: period_a,
                    max_streams: streams_a,
                },
                Relay {
                    max_bytes_per_period: bytes_b,
                    period_seconds: period_b,
                    max_streams: streams_b,
                },
            ) => {
                // A relay capability implies another if it has at least the same limits
                bytes_a >= bytes_b && period_a >= period_b && streams_a >= streams_b
            }

            // Write implies Read for same resource
            (
                Write {
                    resource_pattern: a,
                },
                Read {
                    resource_pattern: b,
                },
            ) => pattern_implies(a, b),

            // Admin implies other capabilities in scope
            (Admin { scope }, Read { resource_pattern })
            | (Admin { scope }, Write { resource_pattern }) => {
                scope == "*" || resource_pattern.starts_with(scope)
            }

            // Different capability types don't imply each other
            _ => false,
        }
    }

    /// Compute the meet (intersection) of two capabilities
    ///
    /// The result is the most restrictive capability that both inputs imply.
    /// This operation is commutative, associative, and idempotent.
    pub fn meet(&self, other: &Capability) -> Capability {
        use Capability::*;

        match (self, other) {
            // Meet with All gives the other capability
            (All, other) | (other, All) => other.clone(),

            // Meet with None gives None
            (None, _) | (_, None) => None,

            // Same capability types
            (
                Read {
                    resource_pattern: a,
                },
                Read {
                    resource_pattern: b,
                },
            ) => Read {
                resource_pattern: pattern_intersect(a, b),
            },
            (
                Write {
                    resource_pattern: a,
                },
                Write {
                    resource_pattern: b,
                },
            ) => Write {
                resource_pattern: pattern_intersect(a, b),
            },
            (Execute { operation: a }, Execute { operation: b }) => {
                if a == b {
                    Execute {
                        operation: a.clone(),
                    }
                } else if a == "*" {
                    Execute {
                        operation: b.clone(),
                    }
                } else if b == "*" {
                    Execute {
                        operation: a.clone(),
                    }
                } else {
                    None
                }
            }
            (Delegate { max_depth: a }, Delegate { max_depth: b }) => Delegate {
                max_depth: (*a).min(*b),
            },
            (Admin { scope: a }, Admin { scope: b }) => Admin {
                scope: scope_intersect(a, b),
            },
            (
                Relay {
                    max_bytes_per_period: bytes_a,
                    period_seconds: period_a,
                    max_streams: streams_a,
                },
                Relay {
                    max_bytes_per_period: bytes_b,
                    period_seconds: period_b,
                    max_streams: streams_b,
                },
            ) => Relay {
                // Meet takes the minimum (most restrictive) of each limit
                max_bytes_per_period: (*bytes_a).min(*bytes_b),
                period_seconds: (*period_a).min(*period_b),
                max_streams: (*streams_a).min(*streams_b),
            },

            // Different capability types generally result in None
            // unless there's a specific intersection rule
            _ => None,
        }
    }
}

/// A set of capabilities implementing meet-semilattice laws
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet {
    capabilities: BTreeSet<Capability>,
}

impl CapabilitySet {
    /// Create new empty capability set (bottom element ⊥)
    pub fn empty() -> Self {
        Self {
            capabilities: BTreeSet::new(),
        }
    }

    /// Create capability set with all permissions (top element ⊤)
    pub fn all() -> Self {
        let mut capabilities = BTreeSet::new();
        capabilities.insert(Capability::All);
        Self { capabilities }
    }

    /// Create capability set from permission strings
    pub fn from_permissions(permissions: &[&str]) -> Self {
        let mut capabilities = BTreeSet::new();

        for perm in permissions {
            let cap = match *perm {
                "*" => Capability::All,
                perm if perm.starts_with("read:") => Capability::Read {
                    resource_pattern: perm[5..].to_string(),
                },
                perm if perm.starts_with("write:") => Capability::Write {
                    resource_pattern: perm[6..].to_string(),
                },
                perm if perm.starts_with("execute:") => Capability::Execute {
                    operation: perm[8..].to_string(),
                },
                perm if perm.starts_with("admin:") => Capability::Admin {
                    scope: perm[6..].to_string(),
                },
                perm if perm.starts_with("relay:") => {
                    // Parse relay capabilities: "relay:bytes_per_period:period_seconds:max_streams"
                    let parts: Vec<&str> = perm[6..].split(':').collect();
                    if parts.len() == 3 {
                        if let (Ok(bytes), Ok(period), Ok(streams)) = (
                            parts[0].parse::<u64>(),
                            parts[1].parse::<u64>(),
                            parts[2].parse::<u32>(),
                        ) {
                            Capability::Relay {
                                max_bytes_per_period: bytes,
                                period_seconds: period,
                                max_streams: streams,
                            }
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }
                "relay" => Capability::Relay {
                    max_bytes_per_period: 1024 * 1024, // 1MB default
                    period_seconds: 3600,              // 1 hour default
                    max_streams: 10,                   // 10 streams default
                },
                "read" => Capability::Read {
                    resource_pattern: "*".to_string(),
                },
                "write" => Capability::Write {
                    resource_pattern: "*".to_string(),
                },
                // Support tree: and other operation namespaces as Execute capabilities
                perm if perm.contains(':') => Capability::Execute {
                    operation: perm.to_string(),
                },
                _ => continue,
            };
            capabilities.insert(cap);
        }

        Self { capabilities }
    }

    /// Check if this capability set permits a specific operation
    pub fn permits(&self, operation: &str) -> bool {
        for cap in &self.capabilities {
            if capability_permits(cap, operation) {
                return true;
            }
        }
        false
    }

    /// Check if this capability set is a subset of another
    pub fn is_subset_of(&self, other: &CapabilitySet) -> bool {
        self.capabilities.iter().all(|cap| {
            other
                .capabilities
                .iter()
                .any(|other_cap| other_cap.implies(cap))
        })
    }

    /// Compute the meet (intersection) of two capability sets
    ///
    /// This implements the meet operation for the capability semilattice.
    /// The result contains the most restrictive capabilities from both sets.
    pub fn meet(&self, other: &CapabilitySet) -> Self {
        let mut result_caps = BTreeSet::new();

        // For each capability in self, find if there's a compatible one in other
        for cap1 in &self.capabilities {
            for cap2 in &other.capabilities {
                // Check if they can be met (same type or compatible types)
                let meet_cap = cap1.meet(cap2);
                if meet_cap != Capability::None {
                    result_caps.insert(meet_cap);
                }
            }
        }

        // If no meets were possible, return empty set (most restrictive)
        if result_caps.is_empty() {
            return Self::empty();
        }

        Self {
            capabilities: result_caps,
        }
    }

    /// Get the capabilities in this set
    pub fn capabilities(&self) -> impl Iterator<Item = &Capability> {
        self.capabilities.iter()
    }
}

impl fmt::Display for CapabilitySet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.capabilities.is_empty() {
            write!(f, "⊥ (empty)")
        } else if self.capabilities.contains(&Capability::All) {
            write!(f, "⊤ (all)")
        } else {
            let caps: Vec<String> = self
                .capabilities
                .iter()
                .map(|cap| format!("{:?}", cap))
                .collect();
            write!(f, "{{{}}}", caps.join(", "))
        }
    }
}

// Helper functions

fn pattern_implies(pattern_a: &str, pattern_b: &str) -> bool {
    pattern_a == "*" || pattern_a == pattern_b || pattern_b.starts_with(pattern_a)
}

fn pattern_intersect(pattern_a: &str, pattern_b: &str) -> String {
    if pattern_a == "*" {
        pattern_b.to_string()
    } else if pattern_b == "*" || pattern_a == pattern_b {
        pattern_a.to_string()
    } else {
        // Find common prefix for intersection
        let common_len = pattern_a
            .chars()
            .zip(pattern_b.chars())
            .take_while(|(a, b)| a == b)
            .count();

        if common_len > 0 {
            pattern_a.chars().take(common_len).collect()
        } else {
            "∅".to_string() // Empty intersection
        }
    }
}

fn scope_implies(scope_a: &str, scope_b: &str) -> bool {
    scope_a == "*" || scope_b.starts_with(scope_a)
}

fn scope_intersect(scope_a: &str, scope_b: &str) -> String {
    if scope_a == "*" {
        scope_b.to_string()
    } else if scope_b == "*" || scope_a == scope_b {
        scope_a.to_string()
    } else {
        // Find most specific common scope
        let common_parts: Vec<&str> = scope_a
            .split('/')
            .zip(scope_b.split('/'))
            .take_while(|(a, b)| a == b)
            .map(|(a, _)| a)
            .collect();

        if common_parts.is_empty() {
            "∅".to_string()
        } else {
            common_parts.join("/")
        }
    }
}

fn capability_permits(capability: &Capability, operation: &str) -> bool {
    use Capability::*;

    match capability {
        All => true,
        None => false,
        Read { resource_pattern } => {
            if operation == "read" {
                true // Simple "read" permission check
            } else {
                operation.starts_with("read:")
                    && (resource_pattern == "*" || operation[5..].starts_with(resource_pattern))
            }
        }
        Write { resource_pattern } => {
            if operation == "write" || operation == "read" {
                true // Simple permission check
            } else {
                (operation.starts_with("write:") || operation.starts_with("read:"))
                    && (resource_pattern == "*"
                        || operation
                            .split(':')
                            .nth(1)
                            .is_some_and(|res| res.starts_with(resource_pattern)))
            }
        }
        Execute { operation: op } => {
            // Support both "execute:op" and direct "namespace:operation" formats
            if operation.starts_with("execute:") {
                op == "*" || operation == format!("execute:{}", op).as_str()
            } else {
                // Direct operation name match (e.g., "tree:add_leaf")
                op == "*" || op == operation
            }
        }
        Delegate { .. } => operation.starts_with("delegate:"),
        Admin { scope } => scope == "*" || operation.contains(&format!(":{}", scope)),
        Relay {
            max_bytes_per_period,
            period_seconds,
            max_streams,
        } => {
            operation.starts_with("relay:") && {
                // Parse operation parameters if provided: "relay:bytes_needed:streams_needed"
                if let Some(params) = operation.strip_prefix("relay:") {
                    let parts: Vec<&str> = params.split(':').collect();
                    if parts.len() >= 1 {
                        // Check byte limit
                        if let Ok(bytes_needed) = parts[0].parse::<u64>() {
                            if bytes_needed > *max_bytes_per_period {
                                return false;
                            }
                        }
                        // Check stream limit if provided
                        if parts.len() >= 2 {
                            if let Ok(streams_needed) = parts[1].parse::<u32>() {
                                if streams_needed > *max_streams {
                                    return false;
                                }
                            }
                        }
                    }
                }
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_meet_laws() {
        let read_all = Capability::Read {
            resource_pattern: "*".to_string(),
        };
        let read_docs = Capability::Read {
            resource_pattern: "docs/".to_string(),
        };
        let write_all = Capability::Write {
            resource_pattern: "*".to_string(),
        };

        // Commutativity: a ⊓ b = b ⊓ a
        assert_eq!(read_all.meet(&read_docs), read_docs.meet(&read_all));

        // Idempotency: a ⊓ a = a
        assert_eq!(read_all.meet(&read_all), read_all);

        // Associativity: (a ⊓ b) ⊓ c = a ⊓ (b ⊓ c)
        let left = read_all.meet(&read_docs).meet(&write_all);
        let right = read_all.meet(&read_docs.meet(&write_all));
        assert_eq!(left, right);
    }

    #[test]
    fn test_capability_set_meet_laws() {
        let set1 = CapabilitySet::from_permissions(&["read", "write"]);
        let set2 = CapabilitySet::from_permissions(&["read"]);
        let set3 = CapabilitySet::from_permissions(&["execute:test"]);

        // Commutativity
        assert_eq!(set1.meet(&set2), set2.meet(&set1));

        // Idempotency
        assert_eq!(set1.meet(&set1), set1);

        // Associativity
        assert_eq!(set1.meet(&set2).meet(&set3), set1.meet(&set2.meet(&set3)));

        // Monotonicity - meet result is subset of both inputs
        let meet_result = set1.meet(&set2);
        assert!(meet_result.is_subset_of(&set1));
        assert!(meet_result.is_subset_of(&set2));
    }

    #[test]
    fn test_relay_capability() {
        let relay_cap = Capability::Relay {
            max_bytes_per_period: 1024 * 1024, // 1MB
            period_seconds: 3600,              // 1 hour
            max_streams: 5,
        };

        // Basic relay operation should be permitted
        assert!(capability_permits(&relay_cap, "relay:"));
        assert!(capability_permits(&relay_cap, "relay:500000")); // 500KB < 1MB
        assert!(capability_permits(&relay_cap, "relay:500000:3")); // 3 streams < 5

        // Exceeding limits should not be permitted
        assert!(!capability_permits(&relay_cap, "relay:2000000")); // 2MB > 1MB
        assert!(!capability_permits(&relay_cap, "relay:500000:10")); // 10 streams > 5

        // Non-relay operations should not be permitted
        assert!(!capability_permits(&relay_cap, "read:"));
        assert!(!capability_permits(&relay_cap, "write:"));
    }

    #[test]
    fn test_relay_capability_meet() {
        let relay_a = Capability::Relay {
            max_bytes_per_period: 1024 * 1024, // 1MB
            period_seconds: 3600,              // 1 hour
            max_streams: 10,
        };

        let relay_b = Capability::Relay {
            max_bytes_per_period: 512 * 1024, // 512KB
            period_seconds: 1800,             // 30 minutes
            max_streams: 5,
        };

        let meet_result = relay_a.meet(&relay_b);

        if let Capability::Relay {
            max_bytes_per_period,
            period_seconds,
            max_streams,
        } = meet_result
        {
            assert_eq!(max_bytes_per_period, 512 * 1024); // min of 1MB and 512KB
            assert_eq!(period_seconds, 1800); // min of 3600 and 1800
            assert_eq!(max_streams, 5); // min of 10 and 5
        } else {
            panic!("Expected relay capability from meet");
        }
    }

    #[test]
    fn test_relay_capability_from_permissions() {
        let cap_set = CapabilitySet::from_permissions(&["relay:1048576:3600:5"]);
        assert!(cap_set.permits("relay:500000:3"));
        assert!(!cap_set.permits("relay:2000000:3"));

        let default_relay_set = CapabilitySet::from_permissions(&["relay"]);
        assert!(default_relay_set.permits("relay:1000000:5"));
    }
}
