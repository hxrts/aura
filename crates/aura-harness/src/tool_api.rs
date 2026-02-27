use serde::{Deserialize, Serialize};

use crate::config::RunConfig;
use crate::coordinator::HarnessCoordinator;

pub const TOOL_API_VERSION: &str = "0.1.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartupSummary {
    pub tool_api_version: String,
    pub schema_version: u32,
    pub run_name: String,
    pub instance_count: u64,
    pub instances: Vec<StartupInstanceSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartupInstanceSummary {
    pub id: String,
    pub mode: String,
    pub bind_address: String,
    pub data_dir: String,
}

impl StartupSummary {
    pub fn from_run_config(config: &RunConfig) -> Self {
        let instances = config
            .instances
            .iter()
            .map(|instance| StartupInstanceSummary {
                id: instance.id.clone(),
                mode: format!("{:?}", instance.mode).to_lowercase(),
                bind_address: instance.bind_address.clone(),
                data_dir: instance.data_dir.display().to_string(),
            })
            .collect();

        Self {
            tool_api_version: TOOL_API_VERSION.to_string(),
            schema_version: config.schema_version,
            run_name: config.run.name.clone(),
            instance_count: config.instances.len() as u64,
            instances,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ToolRequest {
    Screen {
        instance_id: String,
    },
    SendKeys {
        instance_id: String,
        keys: String,
    },
    WaitFor {
        instance_id: String,
        pattern: String,
        timeout_ms: u64,
    },
    TailLog {
        instance_id: String,
        lines: u64,
    },
    Restart {
        instance_id: String,
    },
    Kill {
        instance_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ToolResponse {
    Ok { payload: serde_json::Value },
    Error { message: String },
}

pub struct ToolApi {
    coordinator: HarnessCoordinator,
}

impl ToolApi {
    pub fn new(coordinator: HarnessCoordinator) -> Self {
        Self { coordinator }
    }

    pub fn start_all(&mut self) -> anyhow::Result<()> {
        self.coordinator.start_all()
    }

    pub fn stop_all(&mut self) -> anyhow::Result<()> {
        self.coordinator.stop_all()
    }

    pub fn handle_request(&mut self, request: ToolRequest) -> ToolResponse {
        let outcome = match request {
            ToolRequest::Screen { instance_id } => self
                .coordinator
                .screen(&instance_id)
                .map(|screen| serde_json::json!({ "screen": screen })),
            ToolRequest::SendKeys { instance_id, keys } => self
                .coordinator
                .send_keys(&instance_id, &keys)
                .map(|_| serde_json::json!({ "status": "sent" })),
            ToolRequest::WaitFor {
                instance_id,
                pattern,
                timeout_ms,
            } => self
                .coordinator
                .wait_for(&instance_id, &pattern, timeout_ms)
                .map(|screen| serde_json::json!({ "matched": true, "screen": screen })),
            ToolRequest::TailLog { instance_id, lines } => self
                .coordinator
                .tail_log(
                    &instance_id,
                    match usize::try_from(lines) {
                        Ok(lines) => lines,
                        Err(_) => {
                            return ToolResponse::Error {
                                message: format!("tail_log lines out of range: {lines}"),
                            };
                        }
                    },
                )
                .map(|lines| serde_json::json!({ "lines": lines })),
            ToolRequest::Restart { instance_id } => self
                .coordinator
                .restart(&instance_id)
                .map(|_| serde_json::json!({ "status": "restarted" })),
            ToolRequest::Kill { instance_id } => self
                .coordinator
                .kill(&instance_id)
                .map(|_| serde_json::json!({ "status": "killed" })),
        };

        match outcome {
            Ok(payload) => ToolResponse::Ok { payload },
            Err(error) => ToolResponse::Error {
                message: error.to_string(),
            },
        }
    }

    pub fn event_snapshot(&self) -> Vec<crate::events::HarnessEvent> {
        self.coordinator.event_snapshot()
    }
}
