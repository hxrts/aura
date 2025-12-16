//! Shared types for scenario handling

/// Scenario information structure
#[derive(Debug)]
pub struct ScenarioInfo {
    pub name: String,
    pub description: String,
    pub devices: u32,
    pub threshold: u32,
}

/// Scenario execution result
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ScenarioResult {
    pub name: String,
    pub success: bool,
    pub duration_ms: u64,
    pub error: Option<String>,
    pub log_path: Option<String>,
}
