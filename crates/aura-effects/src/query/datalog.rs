//! Datalog Formatting and Parsing
//!
//! Helper functions for converting between typed Datalog structures and
//! string representations used by the Biscuit execution engine.

use aura_core::query::{DatalogRow, DatalogRule, DatalogValue};

/// Format a Datalog rule for Biscuit execution.
///
/// Converts a typed `DatalogRule` to the string format expected by Biscuit:
/// `head(args...) <- body1(args...), body2(args...)`
pub fn format_rule(rule: &DatalogRule) -> String {
    // Build head
    let head_args: Vec<String> = rule.head.args.iter().map(format_value).collect();
    let head = format!("{}({})", rule.head.predicate, head_args.join(", "));

    // Build body
    let body_parts: Vec<String> = rule
        .body
        .iter()
        .map(|fact| {
            let args: Vec<String> = fact.args.iter().map(format_value).collect();
            format!("{}({})", fact.predicate, args.join(", "))
        })
        .collect();

    format!("{} <- {}", head, body_parts.join(", "))
}

/// Format a Datalog value for Biscuit.
///
/// Converts typed values to their string representation:
/// - Variables: `$name`
/// - Strings: `"value"` (with escaped quotes)
/// - Integers: `42`
/// - Booleans: `true` / `false`
/// - Symbols: `symbol_name`
/// - Null: `null`
pub fn format_value(value: &DatalogValue) -> String {
    match value {
        DatalogValue::Variable(name) => format!("${name}"),
        DatalogValue::String(s) => format!("\"{}\"", s.replace('"', "\\\"")),
        DatalogValue::Integer(n) => n.to_string(),
        DatalogValue::Boolean(b) => b.to_string(),
        DatalogValue::Symbol(s) => s.clone(),
        DatalogValue::Null => "null".to_string(),
    }
}

/// Parse a fact string to a DatalogRow.
///
/// Converts Biscuit query results back to typed bindings.
/// Input format: `["predicate(arg1, arg2, ...)"]`
/// Output: DatalogRow with `arg0`, `arg1`, etc. bindings
pub fn parse_fact_to_row(fact_strings: &[String]) -> DatalogRow {
    let mut row = DatalogRow::new();

    for fact_str in fact_strings {
        if let Some(start) = fact_str.find('(') {
            if let Some(end) = fact_str.rfind(')') {
                let args_str = &fact_str[start + 1..end];
                // Split by ", " and create indexed bindings
                for (i, arg) in args_str.split(", ").enumerate() {
                    let value = parse_arg_to_value(arg.trim());
                    row = row.with_binding(format!("arg{i}"), value);
                }
            }
        }
    }

    row
}

/// Parse an argument string to a DatalogValue.
///
/// Infers type from the string representation:
/// - Quoted strings: `"value"` → String
/// - Integers: `42` → Integer
/// - Booleans: `true`/`false` → Boolean
/// - Other: String (fallback)
pub fn parse_arg_to_value(arg: &str) -> DatalogValue {
    // Remove quotes if present
    if arg.starts_with('"') && arg.ends_with('"') && arg.len() >= 2 {
        return DatalogValue::String(arg[1..arg.len() - 1].to_string());
    }

    // Try to parse as integer
    if let Ok(n) = arg.parse::<i64>() {
        return DatalogValue::Integer(n);
    }

    // Try to parse as boolean
    if arg == "true" {
        return DatalogValue::Boolean(true);
    }
    if arg == "false" {
        return DatalogValue::Boolean(false);
    }

    // Default to string
    DatalogValue::String(arg.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_value_string() {
        let value = DatalogValue::String("hello".to_string());
        assert_eq!(format_value(&value), "\"hello\"");
    }

    #[test]
    fn test_format_value_string_with_quotes() {
        let value = DatalogValue::String("say \"hello\"".to_string());
        assert_eq!(format_value(&value), "\"say \\\"hello\\\"\"");
    }

    #[test]
    fn test_format_value_integer() {
        let value = DatalogValue::Integer(42);
        assert_eq!(format_value(&value), "42");
    }

    #[test]
    fn test_format_value_negative_integer() {
        let value = DatalogValue::Integer(-100);
        assert_eq!(format_value(&value), "-100");
    }

    #[test]
    fn test_format_value_variable() {
        let value = DatalogValue::Variable("x".to_string());
        assert_eq!(format_value(&value), "$x");
    }

    #[test]
    fn test_format_value_boolean_true() {
        let value = DatalogValue::Boolean(true);
        assert_eq!(format_value(&value), "true");
    }

    #[test]
    fn test_format_value_boolean_false() {
        let value = DatalogValue::Boolean(false);
        assert_eq!(format_value(&value), "false");
    }

    #[test]
    fn test_format_value_symbol() {
        let value = DatalogValue::Symbol("admin".to_string());
        assert_eq!(format_value(&value), "admin");
    }

    #[test]
    fn test_format_value_null() {
        let value = DatalogValue::Null;
        assert_eq!(format_value(&value), "null");
    }

    #[test]
    fn test_parse_arg_string() {
        let value = parse_arg_to_value("\"hello\"");
        assert!(matches!(value, DatalogValue::String(s) if s == "hello"));
    }

    #[test]
    fn test_parse_arg_integer() {
        let value = parse_arg_to_value("42");
        assert!(matches!(value, DatalogValue::Integer(42)));
    }

    #[test]
    fn test_parse_arg_negative_integer() {
        let value = parse_arg_to_value("-100");
        assert!(matches!(value, DatalogValue::Integer(-100)));
    }

    #[test]
    fn test_parse_arg_boolean_true() {
        let value = parse_arg_to_value("true");
        assert!(matches!(value, DatalogValue::Boolean(true)));
    }

    #[test]
    fn test_parse_arg_boolean_false() {
        let value = parse_arg_to_value("false");
        assert!(matches!(value, DatalogValue::Boolean(false)));
    }

    #[test]
    fn test_parse_arg_unquoted_string() {
        let value = parse_arg_to_value("unquoted");
        assert!(matches!(value, DatalogValue::String(s) if s == "unquoted"));
    }

    #[test]
    fn test_parse_fact_to_row() {
        let facts = vec!["result(\"alice\", 42, true)".to_string()];
        let row = parse_fact_to_row(&facts);

        assert!(matches!(row.get("arg0"), Some(DatalogValue::String(s)) if s == "alice"));
        assert!(matches!(row.get("arg1"), Some(DatalogValue::Integer(42))));
        assert!(matches!(row.get("arg2"), Some(DatalogValue::Boolean(true))));
    }

    #[test]
    fn test_parse_fact_to_row_empty() {
        let facts: Vec<String> = vec![];
        let row = parse_fact_to_row(&facts);
        assert!(row.get("arg0").is_none());
    }
}
