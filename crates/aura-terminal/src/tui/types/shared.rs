/// A keyboard shortcut hint.
#[derive(Clone, Debug)]
pub struct KeyHint {
    pub key: String,
    pub description: String,
}

impl KeyHint {
    pub fn new(key: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            description: description.into(),
        }
    }
}

/// Truncate an ID string to the specified length.
pub fn short_id(id: &str, len: usize) -> String {
    let trimmed = id.trim();
    if trimmed.len() <= len {
        trimmed.to_string()
    } else {
        trimmed.chars().take(len).collect()
    }
}
