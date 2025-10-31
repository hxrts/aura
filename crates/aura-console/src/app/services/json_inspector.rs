use serde_json::Value;
/// JSON Tree Inspection Service
///
/// Handles JSON tree traversal, expansion state, search filtering, and rendering logic.
/// Pure business logic separated from UI concerns.
use std::collections::HashMap;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct JsonTreeModel {
    expanded_paths: HashMap<String, bool>,
}

#[allow(dead_code)]
impl JsonTreeModel {
    pub fn new() -> Self {
        Self {
            expanded_paths: HashMap::new(),
        }
    }

    /// Toggle expansion state of a path
    pub fn toggle_expansion(&mut self, path: &str) {
        self.expanded_paths
            .entry(path.to_string())
            .and_modify(|e| *e = !*e)
            .or_insert(true);
    }

    /// Set expansion state for a path
    pub fn set_expanded(&mut self, path: &str, expanded: bool) {
        self.expanded_paths.insert(path.to_string(), expanded);
    }

    /// Get expansion state for a path, defaulting based on level
    pub fn is_expanded(&self, path: &str, default_level: usize) -> bool {
        self.expanded_paths
            .get(path)
            .copied()
            .unwrap_or(default_level < 2)
    }

    /// Get all expanded paths
    pub fn get_expanded_paths(&self) -> HashMap<String, bool> {
        self.expanded_paths.clone()
    }

    /// Set all expanded paths
    pub fn set_expanded_paths(&mut self, paths: HashMap<String, bool>) {
        self.expanded_paths = paths;
    }
}

impl Default for JsonTreeModel {
    fn default() -> Self {
        Self::new()
    }
}

/// JSON value metadata and utilities
#[allow(dead_code)]
pub struct JsonValueInfo {
    pub value_type: &'static str,
    pub display_string: String,
    pub is_container: bool,
}

#[allow(dead_code)]
impl JsonValueInfo {
    /// Get info about a JSON value
    pub fn from_value(value: &Value) -> Self {
        match value {
            Value::String(s) => Self {
                value_type: "string",
                display_string: format!("\"{}\"", s),
                is_container: false,
            },
            Value::Number(n) => Self {
                value_type: "number",
                display_string: n.to_string(),
                is_container: false,
            },
            Value::Bool(b) => Self {
                value_type: "boolean",
                display_string: b.to_string(),
                is_container: false,
            },
            Value::Null => Self {
                value_type: "null",
                display_string: "null".to_string(),
                is_container: false,
            },
            Value::Object(obj) => Self {
                value_type: "object",
                display_string: format!("{{{}}}", obj.len()),
                is_container: true,
            },
            Value::Array(arr) => Self {
                value_type: "array",
                display_string: format!("[{}]", arr.len()),
                is_container: true,
            },
        }
    }
}

/// Search filter for JSON tree
#[allow(dead_code)]
pub struct JsonSearchFilter {
    term: String,
}

#[allow(dead_code)]
impl JsonSearchFilter {
    pub fn new(term: String) -> Self {
        Self { term }
    }

    /// Check if a key/value matches the search term
    pub fn matches(&self, key: &str, value: &Value) -> bool {
        if self.term.is_empty() {
            return true;
        }

        let term_lower = self.term.to_lowercase();

        key.to_lowercase().contains(&term_lower)
            || value_to_string(value).to_lowercase().contains(&term_lower)
    }

    /// Highlight search matches in text (returns text with HTML marks)
    pub fn highlight(&self, text: &str) -> String {
        if self.term.is_empty() {
            return text.to_string();
        }

        text.replace(&self.term, &format!("<mark>{}</mark>", self.term))
    }
}

/// Utilities for JSON value formatting
#[allow(dead_code)]
pub fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => format!("\"{}\"", s),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        _ => "...".to_string(),
    }
}

/// Get the display type for a value
#[allow(dead_code)]
pub fn get_value_type(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "boolean",
        Value::Null => "null",
        Value::Object(_) => "object",
        Value::Array(_) => "array",
    }
}

/// Build the full path for a nested key
#[allow(dead_code)]
pub fn build_path(parent_path: &str, key: &str) -> String {
    if parent_path.is_empty() {
        key.to_string()
    } else {
        format!("{}.{}", parent_path, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_tree_model_toggle() {
        let mut model = JsonTreeModel::new();
        assert!(!model.is_expanded("foo", 5)); // Not expanded by default for deep levels

        model.toggle_expansion("foo");
        assert!(model.is_expanded("foo", 5));

        model.toggle_expansion("foo");
        assert!(!model.is_expanded("foo", 5));
    }

    #[test]
    fn test_json_tree_model_default_level() {
        let model = JsonTreeModel::new();
        assert!(model.is_expanded("foo", 0)); // Default expanded for level 0
        assert!(model.is_expanded("foo", 1)); // Default expanded for level 1
        assert!(!model.is_expanded("foo", 2)); // Default collapsed for level 2+
    }

    #[test]
    fn test_search_filter_empty() {
        let filter = JsonSearchFilter::new(String::new());
        assert!(filter.matches("key", &Value::String("value".into())));
    }

    #[test]
    fn test_search_filter_key_match() {
        let filter = JsonSearchFilter::new("key".to_string());
        assert!(filter.matches("my_key", &Value::String("value".into())));
    }

    #[test]
    fn test_search_filter_value_match() {
        let filter = JsonSearchFilter::new("val".to_string());
        assert!(filter.matches("key", &Value::String("value".into())));
    }

    #[test]
    fn test_search_filter_case_insensitive() {
        let filter = JsonSearchFilter::new("KEY".to_string());
        assert!(filter.matches("my_key", &Value::String("value".into())));
    }

    #[test]
    fn test_json_value_info_string() {
        let info = JsonValueInfo::from_value(&Value::String("test".into()));
        assert_eq!(info.value_type, "string");
        assert!(!info.is_container);
    }

    #[test]
    fn test_json_value_info_object() {
        let obj = serde_json::json!({"a": 1, "b": 2});
        let info = JsonValueInfo::from_value(&obj);
        assert_eq!(info.value_type, "object");
        assert!(info.is_container);
    }

    #[test]
    fn test_build_path() {
        assert_eq!(build_path("", "foo"), "foo");
        assert_eq!(build_path("root", "foo"), "root.foo");
        assert_eq!(build_path("root.parent", "foo"), "root.parent.foo");
    }
}
