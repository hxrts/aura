//! Analysis client handler

use crate::error::{WasmError, WasmResult};
use crate::websocket::{ClientHandler, WasmClientEnvelope};
use crate::{console_error, console_log};

/// Handler for trace analysis engine communication
pub struct AnalysisHandler {
    // State specific to analysis client
    pub connected: bool,
    pub analysis_sessions: Vec<String>,
}

impl AnalysisHandler {
    pub fn new() -> Self {
        Self {
            connected: false,
            analysis_sessions: Vec::new(),
        }
    }
}

impl Default for AnalysisHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientHandler for AnalysisHandler {
    fn handle_message(&mut self, data: &str) -> WasmResult<()> {
        console_log!("[Analysis] Received message: {}", data);

        let envelope = WasmClientEnvelope::from_json(data)?;

        match envelope.message_type.as_str() {
            "analysis_result" => {
                // Handle analysis results
                console_log!("[Analysis] Processing analysis result");
                Ok(())
            }
            "causality_graph" => {
                // Handle causality graph updates
                console_log!("[Analysis] Processing causality graph");
                Ok(())
            }
            "property_violation" => {
                // Handle property violations
                console_log!("[Analysis] Processing property violation");
                Ok(())
            }
            "query_response" => {
                // Handle query responses
                console_log!("[Analysis] Processing query response");
                Ok(())
            }
            "error" => {
                console_error!("[Analysis] Analysis error: {:?}", envelope.payload);
                Err(WasmError::Protocol(
                    "Analysis engine reported error".to_string(),
                ))
            }
            _ => {
                console_log!("[Analysis] Unknown message type: {}", envelope.message_type);
                Ok(())
            }
        }
    }

    fn handle_connected(&mut self) -> WasmResult<()> {
        console_log!("[Analysis] Connected to analysis engine");
        self.connected = true;
        Ok(())
    }

    fn handle_disconnected(&mut self, code: u16, reason: &str) -> WasmResult<()> {
        console_log!("[Analysis] Disconnected: {} - {}", code, reason);
        self.connected = false;
        Ok(())
    }

    fn handle_error(&mut self, error: &str) -> WasmResult<()> {
        console_error!("[Analysis] WebSocket error: {}", error);
        self.connected = false;
        Ok(())
    }
}
