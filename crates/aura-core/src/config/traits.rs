//! Core configuration traits for the Aura configuration system

use crate::AuraError;
use std::path::Path;

/// Core trait for Aura configuration types
pub trait AuraConfig: Clone + Default + Send + Sync + 'static {
    /// Error type for configuration operations
    type Error: Into<AuraError> + From<AuraError>;

    /// Get default configuration values
    fn defaults() -> Self {
        Self::default()
    }

    /// Load configuration from a file
    fn load_from_file(path: &Path) -> Result<Self, Self::Error>;

    /// Merge with environment variables
    fn merge_with_env(&mut self) -> Result<(), Self::Error>;

    /// Merge with another configuration
    fn merge_with(&mut self, other: &Self) -> Result<(), Self::Error>;

    /// Validate the configuration
    fn validate(&self) -> Result<(), Self::Error>;

    /// Set a configuration value from a string (for CLI parsing)
    fn set_from_string(&mut self, key: &str, value: &str) -> Result<(), Self::Error>;

    /// Handle positional arguments (for CLI parsing)
    fn handle_positional_arg(&mut self, arg: &str) -> Result<(), Self::Error>;
}

/// Trait for configuration defaults
pub trait ConfigDefaults {
    /// Get default values for this configuration
    fn defaults() -> Self;
}

/// Trait for configuration merging
pub trait ConfigMerge {
    /// Merge this configuration with another
    fn merge_with(&mut self, other: &Self) -> Result<(), AuraError>;
}

/// Trait for configuration validation
pub trait ConfigValidation {
    /// Validate this configuration
    fn validate(&self) -> Result<(), AuraError>;
}

/// Default implementation of AuraConfig for basic types
impl AuraConfig for serde_json::Value {
    type Error = AuraError;

    fn load_from_file(path: &Path) -> Result<Self, Self::Error> {
        use std::fs;

        let content = fs::read_to_string(path)
            .map_err(|e| AuraError::internal(format!("Failed to read config file: {}", e)))?;

        let value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| AuraError::invalid(format!("Invalid JSON: {}", e)))?;

        Ok(value)
    }

    fn merge_with_env(&mut self) -> Result<(), Self::Error> {
        // For JSON values, we can merge environment variables by convention
        // e.g., AURA_KEY_SUBKEY maps to {"key": {"subkey": "value"}}
        for (key, value) in std::env::vars() {
            if let Some(config_key_str) = key.strip_prefix("AURA_") {
                let config_key = config_key_str.to_lowercase().replace('_', ".");
                self.set_nested_value(&config_key, value)?;
            }
        }
        Ok(())
    }

    fn merge_with(&mut self, other: &Self) -> Result<(), Self::Error> {
        merge_json_values(self, other);
        Ok(())
    }

    fn validate(&self) -> Result<(), Self::Error> {
        // Basic validation - just check that it's a valid JSON object
        if !self.is_object() && !self.is_array() {
            return Err(AuraError::invalid(
                "Configuration must be a JSON object or array",
            ));
        }
        Ok(())
    }

    fn set_from_string(&mut self, key: &str, value: &str) -> Result<(), Self::Error> {
        self.set_nested_value(key, value)?;
        Ok(())
    }

    fn handle_positional_arg(&mut self, _arg: &str) -> Result<(), Self::Error> {
        // Default: ignore positional arguments for JSON configs
        Ok(())
    }
}

/// Extension trait for serde_json::Value
pub trait JsonValueExt {
    /// Set a nested value using dot notation (e.g., "a.b.c")
    fn set_nested_value(
        &mut self,
        key: &str,
        value: impl Into<serde_json::Value>,
    ) -> Result<(), AuraError>;
}

impl JsonValueExt for serde_json::Value {
    fn set_nested_value(
        &mut self,
        key: &str,
        value: impl Into<serde_json::Value>,
    ) -> Result<(), AuraError> {
        let parts: Vec<&str> = key.split('.').collect();
        if parts.is_empty() {
            return Err(AuraError::invalid("Empty key"));
        }

        // Ensure we have an object to work with
        if !self.is_object() {
            *self = serde_json::Value::Object(serde_json::Map::new());
        }

        let mut current = self;

        // Navigate to the parent of the final key
        for part in &parts[..parts.len() - 1] {
            current = current
                .as_object_mut()
                .ok_or_else(|| AuraError::invalid("Expected object"))?
                .entry(part.to_string())
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        }

        // Set the final value
        let final_key = parts[parts.len() - 1];
        if let Some(obj) = current.as_object_mut() {
            obj.insert(final_key.to_string(), value.into());
        } else {
            return Err(AuraError::invalid("Cannot set value on non-object"));
        }

        Ok(())
    }
}

/// Merge two JSON values recursively
fn merge_json_values(target: &mut serde_json::Value, source: &serde_json::Value) {
    match (target.as_object_mut(), source.as_object()) {
        (Some(target_obj), Some(source_obj)) => {
            for (key, source_value) in source_obj {
                match target_obj.get_mut(key) {
                    Some(target_value) => {
                        merge_json_values(target_value, source_value);
                    }
                    None => {
                        target_obj.insert(key.clone(), source_value.clone());
                    }
                }
            }
        }
        _ => {
            // For non-objects, source overwrites target
            *target = source.clone();
        }
    }
}

/// Example configuration implementation using derive macro pattern
/// (This would normally be provided by a proc macro)
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ExampleConfig {
    /// Server hostname or IP address
    pub host: String,
    /// Server port number
    pub port: u16,
    /// Enable debug logging
    pub debug: bool,
    /// Connection timeout in seconds
    pub timeout_seconds: u64,
}

impl Default for ExampleConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 8080,
            debug: false,
            timeout_seconds: 30,
        }
    }
}

impl AuraConfig for ExampleConfig {
    type Error = AuraError;

    fn load_from_file(path: &Path) -> Result<Self, Self::Error> {
        use std::fs;

        let content = fs::read_to_string(path)
            .map_err(|e| AuraError::internal(format!("Failed to read config file: {}", e)))?;

        match path.extension().and_then(|ext| ext.to_str()) {
            Some("toml") => toml::from_str(&content)
                .map_err(|e| AuraError::invalid(format!("Invalid TOML: {}", e))),
            Some("json") => serde_json::from_str(&content)
                .map_err(|e| AuraError::invalid(format!("Invalid JSON: {}", e))),
            _ => Err(AuraError::invalid("Unsupported file format")),
        }
    }

    fn merge_with_env(&mut self) -> Result<(), Self::Error> {
        if let Ok(host) = std::env::var("AURA_HOST") {
            self.host = host;
        }
        if let Ok(port_str) = std::env::var("AURA_PORT") {
            self.port = port_str
                .parse()
                .map_err(|_| AuraError::invalid("Invalid port number in AURA_PORT"))?;
        }
        if let Ok(debug_str) = std::env::var("AURA_DEBUG") {
            self.debug = debug_str
                .parse()
                .map_err(|_| AuraError::invalid("Invalid boolean in AURA_DEBUG"))?;
        }
        if let Ok(timeout_str) = std::env::var("AURA_TIMEOUT_SECONDS") {
            self.timeout_seconds = timeout_str
                .parse()
                .map_err(|_| AuraError::invalid("Invalid timeout in AURA_TIMEOUT_SECONDS"))?;
        }
        Ok(())
    }

    fn merge_with(&mut self, other: &Self) -> Result<(), Self::Error> {
        // Merge non-default values from other config
        if other.host != Self::default().host {
            self.host = other.host.clone();
        }
        if other.port != Self::default().port {
            self.port = other.port;
        }
        if other.debug != Self::default().debug {
            self.debug = other.debug;
        }
        if other.timeout_seconds != Self::default().timeout_seconds {
            self.timeout_seconds = other.timeout_seconds;
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), Self::Error> {
        if self.port == 0 {
            return Err(AuraError::invalid("Port cannot be 0"));
        }
        if self.timeout_seconds == 0 {
            return Err(AuraError::invalid("Timeout cannot be 0"));
        }
        if self.host.is_empty() {
            return Err(AuraError::invalid("Host cannot be empty"));
        }
        Ok(())
    }

    fn set_from_string(&mut self, key: &str, value: &str) -> Result<(), Self::Error> {
        match key {
            "host" => self.host = value.to_string(),
            "port" => {
                self.port = value
                    .parse()
                    .map_err(|_| AuraError::invalid("Invalid port number"))?;
            }
            "debug" => {
                self.debug = value
                    .parse()
                    .map_err(|_| AuraError::invalid("Invalid boolean for debug"))?;
            }
            "timeout-seconds" | "timeout_seconds" => {
                self.timeout_seconds = value
                    .parse()
                    .map_err(|_| AuraError::invalid("Invalid timeout value"))?;
            }
            _ => {
                return Err(AuraError::invalid(format!(
                    "Unknown configuration key: {}",
                    key
                )))
            }
        }
        Ok(())
    }

    fn handle_positional_arg(&mut self, arg: &str) -> Result<(), Self::Error> {
        // For example config, treat positional arg as host
        self.host = arg.to_string();
        Ok(())
    }
}
