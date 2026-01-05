//! # Form Draft Lifecycle
//!
//! Generic form draft wrapper that tracks dirty state and validation.
//!
//! This module provides infrastructure for modal form lifecycle management:
//! - Dirty tracking (has user modified the form?)
//! - Validation with field-level error messages
//! - Explicit commit vs discard semantics
//!
//! ## Usage
//!
//! ```rust,ignore
//! // Define form data
//! #[derive(Default, Clone)]
//! struct ChannelFormData {
//!     name: String,
//!     topic: String,
//! }
//!
//! impl Validatable for ChannelFormData {
//!     fn validate(&self) -> Vec<ValidationError> {
//!         let mut errors = vec![];
//!         if self.name.trim().is_empty() {
//!             errors.push(ValidationError::new("name", "Name is required"));
//!         }
//!         errors
//!     }
//! }
//!
//! // Use in modal
//! let mut draft = FormDraft::new(ChannelFormData::default());
//! draft.update(|data| data.name = "General".to_string());
//! assert!(draft.is_dirty());
//!
//! match draft.commit() {
//!     Ok(data) => { /* use validated data */ }
//!     Err(errors) => { /* show validation errors */ }
//! }
//! ```

use std::fmt::Debug;

/// Validation error for a specific field
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    /// Field name (for UI highlighting)
    pub field: String,
    /// Human-readable error message
    pub message: String,
}

impl ValidationError {
    /// Create a new validation error
    #[must_use]
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }

    /// Create a validation error for a required field
    #[must_use]
    pub fn required(field: impl Into<String>) -> Self {
        let f = field.into();
        Self::new(f.clone(), format!("{f} is required"))
    }

    /// Create a validation error for a field that's too long
    #[must_use]
    pub fn too_long(field: impl Into<String>, max: usize) -> Self {
        let f = field.into();
        Self::new(f.clone(), format!("{f} must be at most {max} characters"))
    }
}

/// Trait for types that can be validated
pub trait Validatable {
    /// Validate the data and return any errors
    fn validate(&self) -> Vec<ValidationError>;

    /// Check if data is valid
    fn is_valid(&self) -> bool {
        self.validate().is_empty()
    }
}

/// Form lifecycle phase
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormPhase {
    /// Fresh form, never edited
    #[default]
    Fresh,
    /// User is editing (dirty state)
    Editing,
    /// Form is being submitted
    Submitting,
    /// Submission completed successfully
    Committed,
    /// Submission failed (can retry)
    Failed,
}

/// Form draft wrapper that tracks dirty state and validation
#[derive(Clone, Debug)]
pub struct FormDraft<T> {
    /// The form data
    data: T,
    /// Original data (for reset/comparison)
    original: T,
    /// Current phase
    phase: FormPhase,
    /// Cached validation errors
    errors: Vec<ValidationError>,
    /// External error (from API/backend)
    external_error: Option<String>,
}

impl<T: Default + Clone> Default for FormDraft<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: Clone> FormDraft<T> {
    /// Create a new form draft with initial data
    #[must_use]
    pub fn new(data: T) -> Self {
        Self {
            original: data.clone(),
            data,
            phase: FormPhase::Fresh,
            errors: Vec::new(),
            external_error: None,
        }
    }

    /// Get a reference to the current data
    #[must_use]
    pub fn data(&self) -> &T {
        &self.data
    }

    /// Get a mutable reference to the current data (marks as dirty)
    pub fn data_mut(&mut self) -> &mut T {
        if self.phase == FormPhase::Fresh {
            self.phase = FormPhase::Editing;
        }
        self.errors.clear(); // Clear errors on edit
        self.external_error = None;
        &mut self.data
    }

    /// Update the form data via a closure (marks as dirty)
    pub fn update<F>(&mut self, f: F)
    where
        F: FnOnce(&mut T),
    {
        f(self.data_mut());
    }

    /// Check if the form has been modified
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        !matches!(self.phase, FormPhase::Fresh)
    }

    /// Get the current phase
    #[must_use]
    pub fn phase(&self) -> FormPhase {
        self.phase
    }

    /// Check if currently submitting
    #[must_use]
    pub fn is_submitting(&self) -> bool {
        self.phase == FormPhase::Submitting
    }

    /// Reset to original data
    pub fn reset(&mut self) {
        self.data = self.original.clone();
        self.phase = FormPhase::Fresh;
        self.errors.clear();
        self.external_error = None;
    }

    /// Reset to new initial data
    pub fn reset_to(&mut self, data: T) {
        self.original = data.clone();
        self.data = data;
        self.phase = FormPhase::Fresh;
        self.errors.clear();
        self.external_error = None;
    }

    /// Get validation errors (if any)
    #[must_use]
    pub fn errors(&self) -> &[ValidationError] {
        &self.errors
    }

    /// Check if there are validation errors
    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty() || self.external_error.is_some()
    }

    /// Get error for a specific field
    #[must_use]
    pub fn field_error(&self, field: &str) -> Option<&str> {
        self.errors
            .iter()
            .find(|e| e.field == field)
            .map(|e| e.message.as_str())
    }

    /// Get external error message
    #[must_use]
    pub fn external_error(&self) -> Option<&str> {
        self.external_error.as_deref()
    }

    /// Set an external error (from API/backend)
    pub fn set_external_error(&mut self, error: String) {
        self.external_error = Some(error);
        self.phase = FormPhase::Failed;
    }

    /// Clear external error
    pub fn clear_external_error(&mut self) {
        self.external_error = None;
        if self.phase == FormPhase::Failed {
            self.phase = FormPhase::Editing;
        }
    }

    /// Mark as submitting
    pub fn begin_submit(&mut self) {
        self.phase = FormPhase::Submitting;
    }

    /// Mark as committed (successfully submitted)
    pub fn mark_committed(&mut self) {
        self.phase = FormPhase::Committed;
    }

    /// Mark as failed
    pub fn mark_failed(&mut self, error: Option<String>) {
        self.phase = FormPhase::Failed;
        if let Some(e) = error {
            self.external_error = Some(e);
        }
    }
}

impl<T: Clone + Validatable> FormDraft<T> {
    /// Validate the form and cache errors
    pub fn validate(&mut self) -> bool {
        self.errors = self.data.validate();
        self.errors.is_empty()
    }

    /// Attempt to commit the form
    ///
    /// Returns the data if valid, or errors if not.
    pub fn commit(mut self) -> Result<T, Vec<ValidationError>> {
        self.errors = self.data.validate();
        if self.errors.is_empty() {
            Ok(self.data)
        } else {
            Err(self.errors)
        }
    }

    /// Check if form can be submitted (valid and not already submitting)
    #[must_use]
    pub fn can_submit(&mut self) -> bool {
        if self.is_submitting() {
            return false;
        }
        self.validate()
    }
}

/// Convenience trait for form data types
pub trait FormData: Clone + Default + Validatable {}

// Auto-implement for any type that satisfies the bounds
impl<T: Clone + Default + Validatable> FormData for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default, Debug)]
    struct TestFormData {
        name: String,
        email: String,
    }

    impl Validatable for TestFormData {
        fn validate(&self) -> Vec<ValidationError> {
            let mut errors = vec![];
            if self.name.trim().is_empty() {
                errors.push(ValidationError::required("name"));
            }
            if self.email.trim().is_empty() {
                errors.push(ValidationError::required("email"));
            } else if !self.email.contains('@') {
                errors.push(ValidationError::new("email", "Invalid email format"));
            }
            errors
        }
    }

    #[test]
    fn test_fresh_form_not_dirty() {
        let draft: FormDraft<TestFormData> = FormDraft::default();
        assert!(!draft.is_dirty());
        assert_eq!(draft.phase(), FormPhase::Fresh);
    }

    #[test]
    fn test_editing_marks_dirty() {
        let mut draft: FormDraft<TestFormData> = FormDraft::default();
        draft.update(|data| data.name = "Alice".to_string());
        assert!(draft.is_dirty());
        assert_eq!(draft.phase(), FormPhase::Editing);
    }

    #[test]
    fn test_reset_clears_dirty() {
        let mut draft: FormDraft<TestFormData> = FormDraft::default();
        draft.update(|data| data.name = "Alice".to_string());
        draft.reset();
        assert!(!draft.is_dirty());
        assert!(draft.data().name.is_empty());
    }

    #[test]
    fn test_validation_errors() {
        let mut draft: FormDraft<TestFormData> = FormDraft::default();
        assert!(!draft.validate());
        assert_eq!(draft.errors().len(), 2);
        assert_eq!(draft.field_error("name"), Some("name is required"));
    }

    #[test]
    fn test_commit_invalid() {
        let draft: FormDraft<TestFormData> = FormDraft::default();
        let result = draft.commit();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().len(), 2);
    }

    #[test]
    fn test_commit_valid() {
        let mut data = TestFormData::default();
        data.name = "Alice".to_string();
        data.email = "alice@example.com".to_string();
        let draft = FormDraft::new(data);
        let result = draft.commit();
        assert!(result.is_ok());
        let committed = result.unwrap();
        assert_eq!(committed.name, "Alice");
    }

    #[test]
    fn test_external_error() {
        let mut draft: FormDraft<TestFormData> = FormDraft::default();
        draft.set_external_error("Server error".to_string());
        assert!(draft.has_errors());
        assert_eq!(draft.external_error(), Some("Server error"));
        assert_eq!(draft.phase(), FormPhase::Failed);
    }
}
