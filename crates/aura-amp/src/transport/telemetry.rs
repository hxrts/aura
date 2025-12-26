//! AMP Transport Telemetry and Observability
//!
//! Lightweight observability for AMP message processing. Timing is handled
//! by tracing spans at the call site - this module focuses on structured
//! logging of AMP-specific data (headers, window validation, flow charges).

use aura_core::identifiers::{ChannelId, ContextId};
use aura_core::{AuraError, Receipt};
use aura_transport::amp::{AmpError, AmpHeader};
use tracing::{error, info, warn};

/// AMP window validation results for observability
#[derive(Debug, Clone)]
pub struct WindowValidationResult {
    pub epoch_valid: bool,
    pub generation_valid: bool,
    pub window_bounds: (u64, u64),
    pub actual_generation: u64,
    pub rejection_reason: Option<String>,
}

/// AMP error categorization for metrics
#[derive(Debug, Clone, Copy)]
pub enum AmpErrorCategory {
    Cryptographic,
    Protocol,
    Authorization,
    Network,
    Serialization,
    Unknown,
}

/// Centralized AMP telemetry - uses tracing for all logging.
/// Timing is captured via tracing spans at call sites, not here.
pub struct AmpTelemetry;

impl AmpTelemetry {
    /// Log successful AMP send
    pub fn log_send_success(
        &self,
        context: ContextId,
        channel: ChannelId,
        header: &AmpHeader,
        payload_size: usize,
        encrypted_size: usize,
        flow_charge: u32,
        receipt: Option<&Receipt>,
    ) {
        info!(
            operation = "amp_send",
            status = "success",
            context = %context,
            channel = %channel,
            epoch = header.chan_epoch,
            generation = header.ratchet_gen,
            payload_bytes = payload_size,
            encrypted_bytes = encrypted_size,
            flow_charged = flow_charge,
            receipt_nonce = receipt.map(|r| r.nonce),
            "AMP send completed"
        );
    }

    /// Log failed AMP send
    pub fn log_send_failure(&self, context: ContextId, channel: ChannelId, error: &AuraError) {
        let category = categorize_error(error);
        error!(
            operation = "amp_send",
            status = "failure",
            context = %context,
            channel = %channel,
            error_category = ?category,
            error_message = %error,
            "AMP send failed"
        );
    }

    /// Log successful AMP receive
    pub fn log_receive_success(
        &self,
        context: ContextId,
        header: &AmpHeader,
        wire_size: usize,
        decrypted_size: usize,
        window_validation: &WindowValidationResult,
    ) {
        info!(
            operation = "amp_recv",
            status = "success",
            context = %context,
            channel = %header.channel,
            epoch = header.chan_epoch,
            generation = header.ratchet_gen,
            wire_bytes = wire_size,
            decrypted_bytes = decrypted_size,
            window_min = window_validation.window_bounds.0,
            window_max = window_validation.window_bounds.1,
            "AMP receive completed"
        );
    }

    /// Log failed AMP receive
    pub fn log_receive_failure(
        &self,
        context: ContextId,
        header: Option<&AmpHeader>,
        window_validation: Option<&WindowValidationResult>,
        error: &AuraError,
    ) {
        let category = categorize_error(error);

        match (header, window_validation) {
            (Some(h), Some(v)) => {
                error!(
                    operation = "amp_recv",
                    status = "failure",
                    context = %context,
                    channel = %h.channel,
                    epoch = h.chan_epoch,
                    generation = h.ratchet_gen,
                    epoch_valid = v.epoch_valid,
                    generation_valid = v.generation_valid,
                    window_min = v.window_bounds.0,
                    window_max = v.window_bounds.1,
                    rejection_reason = v.rejection_reason.as_deref().unwrap_or("none"),
                    error_category = ?category,
                    error_message = %error,
                    "AMP receive failed"
                );
            }
            (Some(h), None) => {
                error!(
                    operation = "amp_recv",
                    status = "failure",
                    context = %context,
                    channel = %h.channel,
                    epoch = h.chan_epoch,
                    generation = h.ratchet_gen,
                    error_category = ?category,
                    error_message = %error,
                    "AMP receive failed"
                );
            }
            (None, Some(v)) => {
                error!(
                    operation = "amp_recv",
                    status = "failure",
                    context = %context,
                    epoch_valid = v.epoch_valid,
                    generation_valid = v.generation_valid,
                    rejection_reason = v.rejection_reason.as_deref().unwrap_or("none"),
                    error_category = ?category,
                    error_message = %error,
                    "AMP receive failed"
                );
            }
            (None, None) => {
                error!(
                    operation = "amp_recv",
                    status = "failure",
                    context = %context,
                    error_category = ?category,
                    error_message = %error,
                    "AMP receive failed"
                );
            }
        }
    }

    /// Log AMP window rejection
    pub fn log_window_rejection(
        &self,
        context: ContextId,
        header: &AmpHeader,
        validation: &WindowValidationResult,
        amp_error: &AmpError,
    ) {
        warn!(
            operation = "window_validation",
            status = "rejected",
            context = %context,
            channel = %header.channel,
            epoch = header.chan_epoch,
            generation = header.ratchet_gen,
            epoch_valid = validation.epoch_valid,
            generation_valid = validation.generation_valid,
            window_min = validation.window_bounds.0,
            window_max = validation.window_bounds.1,
            rejection_reason = validation.rejection_reason.as_deref().unwrap_or("unknown"),
            amp_error = %amp_error,
            "AMP window validation failed"
        );
    }

    /// Log flow budget charge
    pub fn log_flow_charge(
        &self,
        context: ContextId,
        peer: aura_core::AuthorityId,
        operation: &'static str,
        cost: u32,
        receipt: Option<&Receipt>,
    ) {
        info!(
            operation = "flow_charge",
            flow_operation = operation,
            context = %context,
            peer = %peer,
            cost = cost,
            receipt_nonce = receipt.map(|r| r.nonce),
            "Flow budget charged"
        );
    }
}

/// Categorize error for observability
fn categorize_error(error: &AuraError) -> AmpErrorCategory {
    let error_str = error.to_string().to_lowercase();

    if error_str.contains("aead")
        || error_str.contains("crypto")
        || error_str.contains("seal")
        || error_str.contains("decrypt")
    {
        AmpErrorCategory::Cryptographic
    } else if error_str.contains("window")
        || error_str.contains("epoch")
        || error_str.contains("generation")
        || error_str.contains("amp")
    {
        AmpErrorCategory::Protocol
    } else if error_str.contains("authorization")
        || error_str.contains("permission")
        || error_str.contains("guard")
    {
        AmpErrorCategory::Authorization
    } else if error_str.contains("network")
        || error_str.contains("transport")
        || error_str.contains("broadcast")
    {
        AmpErrorCategory::Network
    } else if error_str.contains("serialization")
        || error_str.contains("json")
        || error_str.contains("deserialize")
    {
        AmpErrorCategory::Serialization
    } else {
        AmpErrorCategory::Unknown
    }
}

/// Helper to create window validation result
pub fn create_window_validation_result(
    epoch_valid: bool,
    generation_valid: bool,
    window_bounds: (u64, u64),
    actual_generation: u64,
    amp_error: Option<&AmpError>,
) -> WindowValidationResult {
    let rejection_reason = if !epoch_valid || !generation_valid {
        Some(match amp_error {
            Some(AmpError::EpochMismatch { .. }) => "epoch_mismatch".to_string(),
            Some(AmpError::GenerationOutOfWindow { .. }) => "generation_out_of_window".to_string(),
            _ => "validation_failed".to_string(),
        })
    } else {
        None
    };

    WindowValidationResult {
        epoch_valid,
        generation_valid,
        window_bounds,
        actual_generation,
        rejection_reason,
    }
}

/// Global AMP telemetry instance
pub static AMP_TELEMETRY: AmpTelemetry = AmpTelemetry;
