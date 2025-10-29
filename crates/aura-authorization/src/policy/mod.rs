//! Authorization policies and evaluation

pub mod authority_graph;
pub mod evaluation;

pub use authority_graph::{AuthorityGraph, AuthorityNode};
pub use evaluation::{evaluate_policy, PolicyContext, PolicyEvaluation};
