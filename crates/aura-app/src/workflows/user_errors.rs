//! User-facing wording for workflow and runtime errors.
//!
//! Frontends show these short sentences in toasts and keep the raw error as
//! copyable details; internal error chains never reach the toast text.

/// Markers that identify internal error chains rather than user-facing text.
const INTERNAL_ERROR_MARKERS: &[&str] = &[
    "internal error",
    "agent configuration error",
    "json parsing",
    "detail=",
    "_id=",
    "amp operation failed",
    "operation failed:",
    "invalid:",
];

/// A user-facing rendering of a raw error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserFacingError {
    /// The raw text is already user-facing; show it as is.
    Unchanged,
    /// A complete sentence to show instead of the raw text.
    Sentence(String),
    /// Only the leading human label survives (or none, if `None`); the
    /// frontend should point at the copyable details.
    SeeDetails(Option<String>),
}

/// Classify a raw error string for display.
#[must_use]
pub fn classify(raw: &str) -> UserFacingError {
    if let Some(outcome) = crate::workflows::invitation::contact_acceptance_outcome_message(raw) {
        return UserFacingError::Sentence(outcome);
    }
    let lowered = raw.to_ascii_lowercase();
    if lowered.contains("invalid invite code") || lowered.contains("invalid invitation code") {
        return UserFacingError::Sentence("That invitation code isn't valid".to_string());
    }
    // Home budget limits (docs/115, `BudgetError`) are user-facing outcomes,
    // not internal failures, even when wrapped in an operation error chain.
    if lowered.contains("home at neighborhood capacity") {
        return UserFacingError::Sentence(format!(
            "This home is already in the maximum of {} neighborhoods",
            crate::workflows::budget::MAX_NEIGHBORHOODS
        ));
    }
    if lowered.contains("only members can be designated as moderators") {
        return UserFacingError::Sentence(
            "Only home members can be moderators; admit this participant first with /admit <name>"
                .to_string(),
        );
    }
    if lowered.contains("home at member capacity") {
        return UserFacingError::Sentence(format!(
            "This home already has the maximum of {} members",
            crate::workflows::budget::MAX_MEMBERS
        ));
    }
    if lowered.contains("amp_send_message") || lowered.contains("send_message failed") {
        return UserFacingError::Sentence("Couldn't send the message - retry".to_string());
    }
    if !INTERNAL_ERROR_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        return UserFacingError::Unchanged;
    }
    // Keep the leading human label ("Failed to import invitation") and drop
    // the internal chain behind it.
    let head = raw.split(": ").next().unwrap_or(raw).trim();
    let head_is_internal = INTERNAL_ERROR_MARKERS
        .iter()
        .any(|marker| head.to_ascii_lowercase().contains(marker))
        || head.chars().next().is_some_and(|c| !c.is_ascii_uppercase());
    if head_is_internal || head.len() == raw.trim().len() {
        UserFacingError::SeeDetails(None)
    } else {
        UserFacingError::SeeDetails(Some(head.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_budget_limits_get_a_sentence() {
        assert_eq!(
            classify("Operation failed: budget exceeded: Home at neighborhood capacity (4/4)"),
            UserFacingError::Sentence(
                "This home is already in the maximum of 4 neighborhoods".to_string()
            )
        );
        assert_eq!(
            classify("Internal error: Home at member capacity (8/8)"),
            UserFacingError::Sentence("This home already has the maximum of 8 members".to_string())
        );
        assert_eq!(
            classify("Operation failed: Invalid: Only members can be designated as moderators"),
            UserFacingError::Sentence(
                "Only home members can be moderators; admit this participant first with /admit <name>".to_string()
            )
        );
    }

    #[test]
    fn invalid_codes_get_a_sentence() {
        assert_eq!(
            classify("Internal error: import invitation: Invalid invite code: JSON parsing failed"),
            UserFacingError::Sentence("That invitation code isn't valid".to_string())
        );
    }

    #[test]
    fn internal_chains_keep_only_their_human_label() {
        assert_eq!(
            classify("Failed to import invitation: Internal error: storage"),
            UserFacingError::SeeDetails(Some("Failed to import invitation".to_string()))
        );
        assert_eq!(
            classify("internal error: boom"),
            UserFacingError::SeeDetails(None)
        );
    }

    #[test]
    fn user_facing_text_is_unchanged() {
        assert_eq!(classify("Select a home first"), UserFacingError::Unchanged);
    }
}
