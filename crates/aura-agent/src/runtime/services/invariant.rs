//! Typed invariant violations for service state validation.

use std::fmt;

/// A state invariant violation detected during debug-mode validation.
#[derive(Debug, Clone)]
pub struct InvariantViolation {
    /// The service or component that owns the state.
    pub component: &'static str,
    /// Description of the violated invariant.
    pub description: String,
}

impl InvariantViolation {
    /// Create a new invariant violation.
    pub fn new(component: &'static str, description: impl Into<String>) -> Self {
        Self {
            component,
            description: description.into(),
        }
    }
}

impl fmt::Display for InvariantViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.component, self.description)
    }
}

impl std::error::Error for InvariantViolation {}

impl From<String> for InvariantViolation {
    fn from(description: String) -> Self {
        Self {
            component: "unknown",
            description,
        }
    }
}

impl From<&'static str> for InvariantViolation {
    fn from(description: &'static str) -> Self {
        Self {
            component: "unknown",
            description: description.to_string(),
        }
    }
}
