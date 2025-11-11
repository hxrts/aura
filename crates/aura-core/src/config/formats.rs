//! Configuration format implementations for different serialization formats

use crate::AuraError;
use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;

/// Trait for configuration formats
pub trait ConfigFormat {
    /// Parse configuration from a string
    fn parse<T>(&self, content: &str) -> Result<T, AuraError>
    where
        T: DeserializeOwned;
        
    /// Serialize configuration to a string
    fn serialize<T>(&self, config: &T) -> Result<String, AuraError>
    where
        T: Serialize;
        
    /// Get file extensions for this format
    fn file_extensions(&self) -> &[&str];
    
    /// Get the format name
    fn name(&self) -> &str;
}

/// TOML configuration format
pub struct TomlFormat;

impl ConfigFormat for TomlFormat {
    fn parse<T>(&self, content: &str) -> Result<T, AuraError>
    where
        T: DeserializeOwned,
    {
        toml::from_str(content)
            .map_err(|e| AuraError::invalid(format!("Invalid TOML: {}", e)))
    }
    
    fn serialize<T>(&self, config: &T) -> Result<String, AuraError>
    where
        T: Serialize,
    {
        toml::to_string_pretty(config)
            .map_err(|e| AuraError::internal(format!("TOML serialization failed: {}", e)))
    }
    
    fn file_extensions(&self) -> &[&str] {
        &["toml"]
    }
    
    fn name(&self) -> &str {
        "TOML"
    }
}

/// JSON configuration format
pub struct JsonFormat;

impl ConfigFormat for JsonFormat {
    fn parse<T>(&self, content: &str) -> Result<T, AuraError>
    where
        T: DeserializeOwned,
    {
        serde_json::from_str(content)
            .map_err(|e| AuraError::invalid(format!("Invalid JSON: {}", e)))
    }
    
    fn serialize<T>(&self, config: &T) -> Result<String, AuraError>
    where
        T: Serialize,
    {
        serde_json::to_string_pretty(config)
            .map_err(|e| AuraError::internal(format!("JSON serialization failed: {}", e)))
    }
    
    fn file_extensions(&self) -> &[&str] {
        &["json"]
    }
    
    fn name(&self) -> &str {
        "JSON"
    }
}

/// YAML configuration format (placeholder implementation)
pub struct YamlFormat;

impl ConfigFormat for YamlFormat {
    fn parse<T>(&self, _content: &str) -> Result<T, AuraError>
    where
        T: DeserializeOwned,
    {
        // YAML support would require serde_yaml dependency
        Err(AuraError::invalid("YAML format not yet implemented"))
    }
    
    fn serialize<T>(&self, _config: &T) -> Result<String, AuraError>
    where
        T: Serialize,
    {
        Err(AuraError::invalid("YAML format not yet implemented"))
    }
    
    fn file_extensions(&self) -> &[&str] {
        &["yaml", "yml"]
    }
    
    fn name(&self) -> &str {
        "YAML"
    }
}

/// Format detector that determines format from file extension or content
pub struct FormatDetector;

impl FormatDetector {
    /// Detect format from file path
    pub fn detect_from_path(path: &Path) -> Result<Box<dyn ConfigFormat>, AuraError> {
        let extension = path.extension()
            .and_then(|ext| ext.to_str())
            .ok_or_else(|| AuraError::invalid(format!(
                "Cannot detect format for file: {}", 
                path.display()
            )))?;
            
        match extension {
            "toml" => Ok(Box::new(TomlFormat)),
            "json" => Ok(Box::new(JsonFormat)),
            "yaml" | "yml" => Ok(Box::new(YamlFormat)),
            _ => Err(AuraError::invalid(format!(
                "Unsupported configuration format: {}", 
                extension
            ))),
        }
    }
    
    /// Detect format from content (simple heuristics)
    pub fn detect_from_content(content: &str) -> Result<Box<dyn ConfigFormat>, AuraError> {
        let trimmed = content.trim();
        
        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            // Likely JSON
            Ok(Box::new(JsonFormat))
        } else if trimmed.starts_with('[') || trimmed.contains(" = ") {
            // Likely TOML (arrays or key = value pairs)
            Ok(Box::new(TomlFormat))
        } else if trimmed.contains(':') && (trimmed.contains("---") || trimmed.starts_with(' ')) {
            // Likely YAML (key: value with indentation or document separator)
            Ok(Box::new(YamlFormat))
        } else {
            // Default to JSON for simple values
            Ok(Box::new(JsonFormat))
        }
    }
    
    /// Get all supported formats
    pub fn supported_formats() -> Vec<Box<dyn ConfigFormat>> {
        vec![
            Box::new(TomlFormat),
            Box::new(JsonFormat),
            Box::new(YamlFormat),
        ]
    }
    
    /// Check if a file extension is supported
    pub fn is_supported_extension(extension: &str) -> bool {
        Self::supported_formats()
            .iter()
            .any(|format| format.file_extensions().contains(&extension))
    }
}

/// Universal configuration parser that can handle multiple formats
pub struct UniversalParser;

impl UniversalParser {
    /// Parse configuration from file, auto-detecting format
    pub fn parse_file<T>(path: &Path) -> Result<T, AuraError>
    where
        T: DeserializeOwned,
    {
        let content = std::fs::read_to_string(path)
            .map_err(|e| AuraError::internal(format!(
                "Failed to read config file {}: {}", 
                path.display(), e
            )))?;
            
        let format = FormatDetector::detect_from_path(path)?;
        format.parse(&content)
    }
    
    /// Parse configuration from string, detecting format from content
    pub fn parse_string<T>(content: &str) -> Result<T, AuraError>
    where
        T: DeserializeOwned,
    {
        let format = FormatDetector::detect_from_content(content)?;
        format.parse(content)
    }
    
    /// Serialize configuration to string using specified format
    pub fn serialize_with_format<T>(config: &T, format_name: &str) -> Result<String, AuraError>
    where
        T: Serialize,
    {
        let format: Box<dyn ConfigFormat> = match format_name.to_lowercase().as_str() {
            "toml" => Box::new(TomlFormat),
            "json" => Box::new(JsonFormat),
            "yaml" | "yml" => Box::new(YamlFormat),
            _ => return Err(AuraError::invalid(format!(
                "Unknown format: {}", format_name
            ))),
        };
        
        format.serialize(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct TestConfig {
        name: String,
        value: i32,
        enabled: bool,
    }
    
    impl Default for TestConfig {
        fn default() -> Self {
            Self {
                name: "test".to_string(),
                value: 42,
                enabled: true,
            }
        }
    }
    
    #[test]
    fn test_toml_format() {
        let format = TomlFormat;
        let config = TestConfig::default();
        
        let serialized = format.serialize(&config).unwrap();
        let parsed: TestConfig = format.parse(&serialized).unwrap();
        
        assert_eq!(config, parsed);
    }
    
    #[test]
    fn test_json_format() {
        let format = JsonFormat;
        let config = TestConfig::default();
        
        let serialized = format.serialize(&config).unwrap();
        let parsed: TestConfig = format.parse(&serialized).unwrap();
        
        assert_eq!(config, parsed);
    }
    
    #[test]
    fn test_format_detection() {
        // Test JSON detection
        let json_content = r#"{"name": "test", "value": 42}"#;
        let format = FormatDetector::detect_from_content(json_content).unwrap();
        assert_eq!(format.name(), "JSON");
        
        // Test TOML detection
        let toml_content = r#"name = "test"\nvalue = 42"#;
        let format = FormatDetector::detect_from_content(toml_content).unwrap();
        assert_eq!(format.name(), "TOML");
    }
    
    #[test]
    fn test_universal_parser() {
        let json_content = r#"{"name": "test", "value": 42, "enabled": true}"#;
        let config: TestConfig = UniversalParser::parse_string(json_content).unwrap();
        
        assert_eq!(config, TestConfig::default());
    }
}