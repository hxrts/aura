//! Output formatting middleware for CLI operations

use super::{CliMiddleware, CliHandler, CliOperation, CliContext, OutputFormat};
use crate::CliError;
use serde_json::Value;

/// Middleware for output formatting and presentation
pub struct OutputFormattingMiddleware {
    /// Default output format
    default_format: OutputFormat,
    /// Enable color output
    enable_colors: bool,
    /// Pretty print JSON
    pretty_json: bool,
}

impl OutputFormattingMiddleware {
    /// Create new output formatting middleware
    pub fn new() -> Self {
        Self {
            default_format: OutputFormat::Text,
            enable_colors: true,
            pretty_json: true,
        }
    }
    
    /// Set default output format
    pub fn with_default_format(mut self, format: OutputFormat) -> Self {
        self.default_format = format;
        self
    }
    
    /// Enable or disable colors
    pub fn with_colors(mut self, enable: bool) -> Self {
        self.enable_colors = enable;
        self
    }
    
    /// Format the output according to the specified format
    fn format_output(&self, data: &Value, format: &OutputFormat) -> Result<String, CliError> {
        match format {
            OutputFormat::Json => {
                if self.pretty_json {
                    serde_json::to_string_pretty(data)
                } else {
                    serde_json::to_string(data)
                }.map_err(|e| CliError::Serialization(format!("JSON serialization failed: {}", e)))
            }
            OutputFormat::Yaml => {
                serde_yaml::to_string(data)
                    .map_err(|e| CliError::Serialization(format!("YAML serialization failed: {}", e)))
            }
            OutputFormat::Text => {
                Ok(self.format_as_text(data))
            }
            OutputFormat::Table => {
                Ok(self.format_as_table(data))
            }
            OutputFormat::Csv => {
                Ok(self.format_as_csv(data))
            }
        }
    }
    
    /// Format data as human-readable text
    fn format_as_text(&self, data: &Value) -> String {
        match data {
            Value::Object(map) => {
                let mut output = String::new();
                for (key, value) in map {
                    output.push_str(&format!("{}: {}\n", key, self.format_value_text(value)));
                }
                output
            }
            _ => self.format_value_text(data),
        }
    }
    
    /// Format individual value as text
    fn format_value_text(&self, value: &Value) -> String {
        match value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => "null".to_string(),
            Value::Array(arr) => {
                format!("[{}]", arr.iter()
                    .map(|v| self.format_value_text(v))
                    .collect::<Vec<_>>()
                    .join(", "))
            }
            Value::Object(_) => {
                // For nested objects, use JSON representation
                serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
            }
        }
    }
    
    /// Format data as table (simplified)
    fn format_as_table(&self, data: &Value) -> String {
        match data {
            Value::Array(arr) if !arr.is_empty() => {
                if let Some(Value::Object(first)) = arr.first() {
                    let mut output = String::new();
                    
                    // Header
                    let headers: Vec<&String> = first.keys().collect();
                    output.push_str(&headers.join(" | "));
                    output.push('\n');
                    output.push_str(&"-".repeat(headers.len() * 10));
                    output.push('\n');
                    
                    // Rows
                    for item in arr {
                        if let Value::Object(obj) = item {
                            let row: Vec<String> = headers.iter()
                                .map(|h| self.format_value_text(obj.get(*h).unwrap_or(&Value::Null)))
                                .collect();
                            output.push_str(&row.join(" | "));
                            output.push('\n');
                        }
                    }
                    
                    output
                } else {
                    self.format_as_text(data)
                }
            }
            _ => self.format_as_text(data),
        }
    }
    
    /// Format data as CSV (simplified)
    fn format_as_csv(&self, data: &Value) -> String {
        match data {
            Value::Array(arr) if !arr.is_empty() => {
                if let Some(Value::Object(first)) = arr.first() {
                    let mut output = String::new();
                    
                    // Header
                    let headers: Vec<&String> = first.keys().collect();
                    output.push_str(&headers.join(","));
                    output.push('\n');
                    
                    // Rows
                    for item in arr {
                        if let Value::Object(obj) = item {
                            let row: Vec<String> = headers.iter()
                                .map(|h| {
                                    let val = self.format_value_text(obj.get(*h).unwrap_or(&Value::Null));
                                    if val.contains(',') || val.contains('"') {
                                        format!("\"{}\"", val.replace("\"", "\"\""))
                                    } else {
                                        val
                                    }
                                })
                                .collect();
                            output.push_str(&row.join(","));
                            output.push('\n');
                        }
                    }
                    
                    output
                } else {
                    self.format_as_text(data)
                }
            }
            _ => self.format_as_text(data),
        }
    }
}

impl Default for OutputFormattingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl CliMiddleware for OutputFormattingMiddleware {
    fn process(
        &self,
        operation: CliOperation,
        context: &CliContext,
        next: &dyn CliHandler,
    ) -> Result<Value, CliError> {
        let result = next.handle(operation.clone(), context)?;
        
        // For FormatOutput operations, apply formatting
        if let CliOperation::FormatOutput { data, format } = operation {
            let formatted = self.format_output(&data, &format)?;
            Ok(serde_json::json!({
                "formatted": formatted,
                "format": format!("{:?}", format)
            }))
        } else {
            // For other operations, return the result as-is
            Ok(result)
        }
    }
    
    fn name(&self) -> &str {
        "output_formatting"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpCliHandler;
    use serde_json::json;
    
    #[test]
    fn test_json_formatting() {
        let middleware = OutputFormattingMiddleware::new();
        let handler = NoOpCliHandler;
        let context = CliContext::new("test".to_string(), vec![]);
        
        let data = json!({"key": "value", "number": 42});
        let result = middleware.process(
            CliOperation::FormatOutput { 
                data: data.clone(),
                format: OutputFormat::Json 
            },
            &context,
            &handler,
        );
        
        assert!(result.is_ok());
        let formatted = result.unwrap();
        assert!(formatted["formatted"].is_string());
        assert_eq!(formatted["format"], "Json");
    }
    
    #[test]
    fn test_text_formatting() {
        let middleware = OutputFormattingMiddleware::new();
        let handler = NoOpCliHandler;
        let context = CliContext::new("test".to_string(), vec![]);
        
        let data = json!({"name": "test", "value": 123});
        let result = middleware.process(
            CliOperation::FormatOutput { 
                data: data.clone(),
                format: OutputFormat::Text 
            },
            &context,
            &handler,
        );
        
        assert!(result.is_ok());
        let formatted = result.unwrap();
        let text = formatted["formatted"].as_str().unwrap();
        assert!(text.contains("name: test"));
        assert!(text.contains("value: 123"));
    }
}