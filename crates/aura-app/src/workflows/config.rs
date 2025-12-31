//! Device Configuration Workflow - Portable Defaults
//!
//! This module contains portable device configuration defaults and
//! template generation that can be used by all frontends.

// ============================================================================
// Constants
// ============================================================================

/// Base port for device network connections.
///
/// Devices are assigned ports starting from this base:
/// - Device 1: 58835
/// - Device 2: 58836
/// - Device N: 58835 + N - 1
pub const DEFAULT_BASE_PORT: u16 = 58835;

/// Default network timeout in seconds.
pub const DEFAULT_NETWORK_TIMEOUT_SECS: u32 = 30;

/// Default maximum retry attempts for network operations.
pub const DEFAULT_MAX_RETRIES: u32 = 3;

/// Default logging level.
pub const DEFAULT_LOG_LEVEL: &str = "info";

// ============================================================================
// Configuration Functions
// ============================================================================

/// Calculate the default port for a device.
///
/// Port assignment: `BASE_PORT + device_num - 1`
/// - Device 1: 58835
/// - Device 2: 58836
/// - etc.
///
/// # Arguments
/// * `device_num` - 1-indexed device number
///
/// # Returns
/// Assigned port number for the device.
#[must_use]
pub fn default_port(device_num: u32) -> u16 {
    let offset = device_num.saturating_sub(1);
    DEFAULT_BASE_PORT.saturating_add(offset as u16)
}

/// Device configuration defaults (portable across frontends).
#[derive(Debug, Clone)]
pub struct DeviceConfigDefaults {
    /// Device number (1-indexed)
    pub device_num: u32,
    /// Threshold for the device group
    pub threshold: u32,
    /// Total devices in the group
    pub total_devices: u32,
    /// Network port
    pub port: u16,
    /// Network timeout in seconds
    pub timeout_secs: u32,
    /// Maximum retry attempts
    pub max_retries: u32,
    /// Logging level
    pub log_level: String,
}

impl DeviceConfigDefaults {
    /// Create configuration defaults for a device.
    #[must_use]
    pub fn new(device_num: u32, threshold: u32, total_devices: u32) -> Self {
        Self {
            device_num,
            threshold,
            total_devices,
            port: default_port(device_num),
            timeout_secs: DEFAULT_NETWORK_TIMEOUT_SECS,
            max_retries: DEFAULT_MAX_RETRIES,
            log_level: DEFAULT_LOG_LEVEL.to_string(),
        }
    }

    /// Create with custom port.
    #[must_use]
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Create with custom timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout_secs: u32) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    /// Create with custom max retries.
    #[must_use]
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Create with custom log level.
    #[must_use]
    pub fn with_log_level(mut self, level: impl Into<String>) -> Self {
        self.log_level = level.into();
        self
    }
}

/// Generate device configuration content as TOML-formatted string.
///
/// This is the portable template generator that frontends can use.
#[must_use]
pub fn generate_device_config(defaults: &DeviceConfigDefaults) -> String {
    format!(
        r#"# Device {} configuration
device_id = "device_{}"
threshold = {}
total_devices = {}

[logging]
level = "{}"
structured = false

[network]
default_port = {}
timeout = {}
max_retries = {}
"#,
        defaults.device_num,
        defaults.device_num,
        defaults.threshold,
        defaults.total_devices,
        defaults.log_level,
        defaults.port,
        defaults.timeout_secs,
        defaults.max_retries,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_port_device_1() {
        assert_eq!(default_port(1), 58835);
    }

    #[test]
    fn test_default_port_device_2() {
        assert_eq!(default_port(2), 58836);
    }

    #[test]
    fn test_default_port_device_10() {
        assert_eq!(default_port(10), 58844);
    }

    #[test]
    fn test_default_port_zero_device() {
        // Device 0 should still work (saturating_sub prevents underflow)
        assert_eq!(default_port(0), 58835);
    }

    #[test]
    fn test_device_config_defaults() {
        let defaults = DeviceConfigDefaults::new(1, 2, 3);
        assert_eq!(defaults.device_num, 1);
        assert_eq!(defaults.threshold, 2);
        assert_eq!(defaults.total_devices, 3);
        assert_eq!(defaults.port, 58835);
        assert_eq!(defaults.timeout_secs, 30);
        assert_eq!(defaults.max_retries, 3);
        assert_eq!(defaults.log_level, "info");
    }

    #[test]
    fn test_device_config_defaults_with_builders() {
        let defaults = DeviceConfigDefaults::new(2, 2, 5)
            .with_port(9000)
            .with_timeout(60)
            .with_max_retries(5)
            .with_log_level("debug");

        assert_eq!(defaults.port, 9000);
        assert_eq!(defaults.timeout_secs, 60);
        assert_eq!(defaults.max_retries, 5);
        assert_eq!(defaults.log_level, "debug");
    }

    #[test]
    fn test_generate_device_config() {
        let defaults = DeviceConfigDefaults::new(1, 2, 3);
        let config = generate_device_config(&defaults);

        assert!(config.contains("device_id = \"device_1\""));
        assert!(config.contains("threshold = 2"));
        assert!(config.contains("total_devices = 3"));
        assert!(config.contains("default_port = 58835"));
        assert!(config.contains("timeout = 30"));
        assert!(config.contains("max_retries = 3"));
        assert!(config.contains("level = \"info\""));
    }

    #[test]
    fn test_generate_device_config_device_2() {
        let defaults = DeviceConfigDefaults::new(2, 2, 3);
        let config = generate_device_config(&defaults);

        assert!(config.contains("device_id = \"device_2\""));
        assert!(config.contains("default_port = 58836"));
    }

    #[test]
    fn test_constants() {
        assert_eq!(DEFAULT_BASE_PORT, 58835);
        assert_eq!(DEFAULT_NETWORK_TIMEOUT_SECS, 30);
        assert_eq!(DEFAULT_MAX_RETRIES, 3);
        assert_eq!(DEFAULT_LOG_LEVEL, "info");
    }
}
