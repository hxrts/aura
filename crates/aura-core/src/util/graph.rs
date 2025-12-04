//! Graph Utilities
//!
//! Traits for graph-based algorithms like topological sorting.
//!
//! **Layer 1**: Interface definitions only. Implementations live in higher layers.

use std::hash::Hash;

/// A node in a directed acyclic graph (DAG) that can be topologically sorted.
///
/// This trait provides the interface for topological sorting algorithms.
/// Implementations define what constitutes a node's identity and its dependencies.
///
/// # Design
///
/// Different crates implement this trait with domain-specific semantics:
///
/// - **Scheduler (aura-agent)**: Views with explicit declared dependencies
/// - **Journal (aura-journal)**: Tree operations with parent references
///
/// Each crate provides its own topological sort function with appropriate
/// semantics (cycle detection, tie-breaking, etc.).
///
/// # Example
///
/// ```ignore
/// use aura_core::util::DagNode;
///
/// struct Task {
///     id: String,
///     depends_on: Vec<String>,
/// }
///
/// impl DagNode for Task {
///     type Id = String;
///
///     fn dag_id(&self) -> Self::Id {
///         self.id.clone()
///     }
///
///     fn dag_dependencies(&self) -> Vec<Self::Id> {
///         self.depends_on.clone()
///     }
/// }
/// ```
pub trait DagNode {
    /// The type used to uniquely identify nodes.
    ///
    /// Must be `Eq + Hash` for efficient lookup during sorting.
    /// Must be `Clone` since IDs are stored in collections.
    type Id: Eq + Hash + Clone;

    /// Returns the unique identifier for this node.
    ///
    /// Two nodes with the same ID are considered identical for sorting purposes.
    fn dag_id(&self) -> Self::Id;

    /// Returns the IDs of nodes this node depends on.
    ///
    /// These nodes must be processed before this node in topological order.
    /// Returns an empty vector if this node has no dependencies.
    ///
    /// # Semantics
    ///
    /// - Dependencies on IDs not present in the graph may be ignored or treated
    ///   as errors depending on the sorting algorithm.
    /// - Circular dependencies create cycles which may panic or return errors.
    fn dag_dependencies(&self) -> Vec<Self::Id>;
}

/// Error returned when a topological sort encounters a cycle.
///
/// Contains the IDs of nodes involved in the cycle for debugging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleError<Id> {
    /// IDs of nodes that are part of the cycle
    pub cycle_members: Vec<Id>,
}

impl<Id: std::fmt::Debug> std::fmt::Display for CycleError<Id> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Dependency cycle detected among nodes: {:?}",
            self.cycle_members
        )
    }
}

impl<Id: std::fmt::Debug> std::error::Error for CycleError<Id> {}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestNode {
        id: String,
        deps: Vec<String>,
    }

    impl DagNode for TestNode {
        type Id = String;

        fn dag_id(&self) -> Self::Id {
            self.id.clone()
        }

        fn dag_dependencies(&self) -> Vec<Self::Id> {
            self.deps.clone()
        }
    }

    #[test]
    fn test_dag_node_impl() {
        let node = TestNode {
            id: "a".to_string(),
            deps: vec!["b".to_string(), "c".to_string()],
        };

        assert_eq!(node.dag_id(), "a");
        assert_eq!(node.dag_dependencies(), vec!["b", "c"]);
    }

    #[test]
    fn test_cycle_error_display() {
        let err: CycleError<String> = CycleError {
            cycle_members: vec!["a".to_string(), "b".to_string()],
        };
        assert!(err.to_string().contains("cycle"));
        assert!(err.to_string().contains("a"));
    }
}
