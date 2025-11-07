//! Configuration format support for multiple serialization formats

use crate::AuraError;
use std::path::Path;

/// Supported configuration formats
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigFormat {
    /// TOML format for configuration files
    Toml,
    /// JSON format for configuration files
    Json,
    /// YAML format for configuration files
    Yaml,
    /// RON (Rusty Object Notation) format for configuration files
    Ron,
    /// Environment variables for configuration
    Env,
}

impl ConfigFormat {
    /// Detect format from file extension
    pub fn from_extension(path: &Path) -> Result<Self, AuraError> {
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .ok_or_else(|| AuraError::config_failed("No file extension found"))?;

        match extension.to_lowercase().as_str() {
            "toml" => Ok(ConfigFormat::Toml),
            "json" => Ok(ConfigFormat::Json),
            "yaml" | "yml" => Ok(ConfigFormat::Yaml),
            "ron" => Ok(ConfigFormat::Ron),
            "env" => Ok(ConfigFormat::Env),
            _ => Err(AuraError::config_failed(format!(
                "Unsupported config format: {}",
                extension
            ))),
        }
    }

    /// Get default file extension for this format
    pub fn extension(&self) -> &'static str {
        match self {
            ConfigFormat::Toml => "toml",
            ConfigFormat::Json => "json",
            ConfigFormat::Yaml => "yaml",
            ConfigFormat::Ron => "ron",
            ConfigFormat::Env => "env",
        }
    }

    /// Get MIME type for this format
    pub fn mime_type(&self) -> &'static str {
        match self {
            ConfigFormat::Toml => "application/toml",
            ConfigFormat::Json => "application/json",
            ConfigFormat::Yaml => "application/yaml",
            ConfigFormat::Ron => "application/ron",
            ConfigFormat::Env => "text/plain",
        }
    }
}

/// TOML format support
pub struct TomlFormat;

impl TomlFormat {
    /// Serialize to TOML string
    pub fn serialize<T: serde::Serialize>(value: &T) -> Result<String, AuraError> {
        toml::to_string_pretty(value)
            .map_err(|e| AuraError::config_failed(format!("TOML serialization failed: {}", e)))
    }

    /// Deserialize from TOML string
    pub fn deserialize<T: serde::de::DeserializeOwned>(content: &str) -> Result<T, AuraError> {
        toml::from_str(content)
            .map_err(|e| AuraError::config_failed(format!("TOML deserialization failed: {}", e)))
    }

    /// Load from TOML file
    pub fn load_from_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, AuraError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| AuraError::config_failed(format!("Failed to read file: {}", e)))?;
        Self::deserialize(&content)
    }

    /// Save to TOML file
    pub fn save_to_file<T: serde::Serialize>(value: &T, path: &Path) -> Result<(), AuraError> {
        let content = Self::serialize(value)?;
        std::fs::write(path, content)
            .map_err(|e| AuraError::config_failed(format!("Failed to write file: {}", e)))
    }
}

/// JSON format support
pub struct JsonFormat;

impl JsonFormat {
    /// Serialize to JSON string with pretty printing
    pub fn serialize<T: serde::Serialize>(value: &T) -> Result<String, AuraError> {
        serde_json::to_string_pretty(value)
            .map_err(|e| AuraError::config_failed(format!("JSON serialization failed: {}", e)))
    }

    /// Deserialize from JSON string
    pub fn deserialize<T: serde::de::DeserializeOwned>(content: &str) -> Result<T, AuraError> {
        serde_json::from_str(content)
            .map_err(|e| AuraError::config_failed(format!("JSON deserialization failed: {}", e)))
    }

    /// Load from JSON file
    pub fn load_from_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, AuraError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| AuraError::config_failed(format!("Failed to read file: {}", e)))?;
        Self::deserialize(&content)
    }

    /// Save to JSON file
    pub fn save_to_file<T: serde::Serialize>(value: &T, path: &Path) -> Result<(), AuraError> {
        let content = Self::serialize(value)?;
        std::fs::write(path, content)
            .map_err(|e| AuraError::config_failed(format!("Failed to write file: {}", e)))
    }
}

/// YAML format support
pub struct YamlFormat;

impl YamlFormat {
    /// Serialize to YAML string
    pub fn serialize<T: serde::Serialize>(_value: &T) -> Result<String, AuraError> {
        // TODO: Implement YAML support when serde_yaml is available
        Err(AuraError::config_failed("YAML format not yet implemented"))
    }

    /// Deserialize from YAML string
    pub fn deserialize<T: serde::de::DeserializeOwned>(_content: &str) -> Result<T, AuraError> {
        // TODO: Implement YAML support when serde_yaml is available
        Err(AuraError::config_failed("YAML format not yet implemented"))
    }

    /// Load from YAML file
    pub fn load_from_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, AuraError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| AuraError::config_failed(format!("Failed to read file: {}", e)))?;
        Self::deserialize(&content)
    }

    /// Save to YAML file
    pub fn save_to_file<T: serde::Serialize>(value: &T, path: &Path) -> Result<(), AuraError> {
        let content = Self::serialize(value)?;
        std::fs::write(path, content)
            .map_err(|e| AuraError::config_failed(format!("Failed to write file: {}", e)))
    }
}

/// RON (Rusty Object Notation) format support
pub struct RonFormat;

impl RonFormat {
    /// Serialize to RON string with pretty printing
    pub fn serialize<T: serde::Serialize>(_value: &T) -> Result<String, AuraError> {
        // TODO: Implement RON support when ron crate is available
        Err(AuraError::config_failed("RON format not yet implemented"))
    }

    /// Deserialize from RON string
    pub fn deserialize<T: serde::de::DeserializeOwned>(_content: &str) -> Result<T, AuraError> {
        // TODO: Implement RON support when ron crate is available
        Err(AuraError::config_failed("RON format not yet implemented"))
    }

    /// Load from RON file
    pub fn load_from_file<T: serde::de::DeserializeOwned>(_path: &Path) -> Result<T, AuraError> {
        // TODO: Implement RON support when ron crate is available
        Err(AuraError::config_failed("RON format not yet implemented"))
    }

    /// Save to RON file
    pub fn save_to_file<T: serde::Serialize>(_value: &T, _path: &Path) -> Result<(), AuraError> {
        // TODO: Implement RON support when ron crate is available
        Err(AuraError::config_failed("RON format not yet implemented"))
    }
}

/// Universal format handler that dispatches to specific format implementations
pub struct UniversalFormat;

impl UniversalFormat {
    /// Load configuration from file, detecting format from extension
    pub fn load_from_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, AuraError> {
        let format = ConfigFormat::from_extension(path)?;

        match format {
            ConfigFormat::Toml => TomlFormat::load_from_file(path),
            ConfigFormat::Json => JsonFormat::load_from_file(path),
            ConfigFormat::Yaml => YamlFormat::load_from_file(path),
            ConfigFormat::Ron => RonFormat::load_from_file(path),
            ConfigFormat::Env => Err(AuraError::config_failed(
                "Environment format requires specific loader",
            )),
        }
    }

    /// Save configuration to file in specified format
    pub fn save_to_file<T: serde::Serialize>(
        value: &T,
        path: &Path,
        format: ConfigFormat,
    ) -> Result<(), AuraError> {
        match format {
            ConfigFormat::Toml => TomlFormat::save_to_file(value, path),
            ConfigFormat::Json => JsonFormat::save_to_file(value, path),
            ConfigFormat::Yaml => YamlFormat::save_to_file(value, path),
            ConfigFormat::Ron => RonFormat::save_to_file(value, path),
            ConfigFormat::Env => Err(AuraError::config_failed(
                "Environment format saving not supported",
            )),
        }
    }

    /// Convert between formats
    pub fn convert<T: serde::Serialize + serde::de::DeserializeOwned>(
        value: &T,
        _from_format: ConfigFormat,
        to_format: ConfigFormat,
    ) -> Result<String, AuraError> {
        // Serialize to intermediate representation then to target format
        match to_format {
            ConfigFormat::Toml => TomlFormat::serialize(value),
            ConfigFormat::Json => JsonFormat::serialize(value),
            ConfigFormat::Yaml => YamlFormat::serialize(value),
            ConfigFormat::Ron => RonFormat::serialize(value),
            ConfigFormat::Env => Err(AuraError::config_failed(
                "Environment format conversion not supported",
            )),
        }
    }
}

/// Environment variable format handler
pub struct EnvFormat;

impl EnvFormat {
    /// Load configuration from environment variables with prefix
    pub fn load_with_prefix<T: serde::de::DeserializeOwned>(_prefix: &str) -> Result<T, AuraError> {
        // This would typically use envy or similar crate for env var deserialization
        // For now, return an error indicating this needs implementation
        Err(AuraError::config_failed(
            "Environment variable loading not yet implemented",
        ))
    }

    /// Merge environment variables into existing configuration
    pub fn merge_env_vars<T: serde::Serialize + serde::de::DeserializeOwned>(
        _config: &mut T,
        _prefix: &str,
    ) -> Result<(), AuraError> {
        // This would merge environment variables into the existing config
        // Implementation depends on the specific serialization strategy
        Err(AuraError::config_failed(
            "Environment variable merging not yet implemented",
        ))
    }
}

/// Trait for types that support multiple configuration formats
pub trait MultiFormat: serde::Serialize + serde::de::DeserializeOwned {
    /// Load from any supported format
    fn load_any_format(path: &Path) -> Result<Self, AuraError>
    where
        Self: Sized,
    {
        UniversalFormat::load_from_file(path)
    }

    /// Save in specified format
    fn save_format(&self, path: &Path, format: ConfigFormat) -> Result<(), AuraError> {
        UniversalFormat::save_to_file(self, path, format)
    }

    /// Convert to different format string
    fn to_format_string(&self, format: ConfigFormat) -> Result<String, AuraError> {
        match format {
            ConfigFormat::Toml => TomlFormat::serialize(self),
            ConfigFormat::Json => JsonFormat::serialize(self),
            ConfigFormat::Yaml => YamlFormat::serialize(self),
            ConfigFormat::Ron => RonFormat::serialize(self),
            ConfigFormat::Env => Err(AuraError::config_failed(
                "Environment format serialization not supported",
            )),
        }
    }
}
