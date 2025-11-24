//! AMP Transport Telemetry and Observability
//!
//! Centralized observability hooks for AMP message processing, flow charges,
//! window validations, and performance metrics. Provides structured logging
//! and metrics collection without scattering tracing calls across transport paths.

use aura_core::identifiers::{ChannelId, ContextId};
use aura_core::{AuraError, Receipt};
use aura_transport::amp::{AmpError, AmpHeader};
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Centralized AMP telemetry collector and structured logging interface
#[derive(Debug, Clone)]
pub struct AmpTelemetry {
    /// Component identifier for telemetry context
    component: &'static str,
    /// Whether to enable detailed debug logging
    debug_enabled: bool,
}

/// AMP operation performance metrics
#[derive(Debug, Clone)]
pub struct AmpMetrics {
    /// Total operation duration
    pub duration: Duration,
    /// Time spent on cryptographic operations (AEAD)
    pub crypto_time: Option<Duration>,
    /// Time spent on guard chain evaluation
    pub guard_time: Option<Duration>,
    /// Time spent on journal operations
    pub journal_time: Option<Duration>,
    /// Number of bytes processed
    pub bytes_processed: usize,
    /// Flow budget charged
    pub flow_charged: u32,
}

/// AMP send operation telemetry data
#[derive(Debug)]
pub struct AmpSendTelemetry {
    pub context: ContextId,
    pub channel: ChannelId,
    pub header: AmpHeader,
    pub payload_size: usize,
    pub encrypted_size: usize,
    pub flow_charge: u32,
    pub receipt: Option<Receipt>,
    pub metrics: AmpMetrics,
}

/// AMP receive operation telemetry data
#[derive(Debug)]
pub struct AmpReceiveTelemetry {
    pub context: ContextId,
    pub header: AmpHeader,
    pub wire_size: usize,
    pub decrypted_size: usize,
    pub window_validation: WindowValidationResult,
    pub metrics: AmpMetrics,
}

/// AMP flow charge telemetry data
#[derive(Debug)]
pub struct AmpFlowTelemetry {
    pub context: ContextId,
    pub peer: aura_core::AuthorityId,
    pub operation: &'static str,
    pub cost: u32,
    pub receipt: Option<Receipt>,
    pub budget_remaining: Option<u32>,
    pub charge_duration: Duration,
}

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
#[derive(Debug, Clone)]
pub enum AmpErrorCategory {
    /// Cryptographic errors (AEAD failures, key derivation)
    Cryptographic,
    /// Protocol errors (window validation, epoch mismatch)
    Protocol,
    /// Authorization errors (guard chain failures)
    Authorization,
    /// Network errors (transport failures)
    Network,
    /// Serialization errors
    Serialization,
    /// Unknown/other errors
    Unknown,
}

impl AmpTelemetry {
    /// Create a new AMP telemetry instance
    pub fn new(component: &'static str) -> Self {
        Self {
            component,
            debug_enabled: tracing::enabled!(tracing::Level::DEBUG),
        }
    }

    /// Log successful AMP send operation with structured telemetry
    pub fn log_send_success(&self, telemetry: AmpSendTelemetry) {
        let AmpSendTelemetry {
            context,
            channel,
            header,
            payload_size,
            encrypted_size,
            flow_charge,
            receipt,
            metrics,
        } = telemetry;

        info!(
            component = self.component,
            operation = "amp_send",
            status = "success",
            context = %context,
            channel = %channel,
            epoch = header.chan_epoch,
            generation = header.ratchet_gen,
            payload_bytes = payload_size,
            encrypted_bytes = encrypted_size,
            flow_charged = flow_charge,
            receipt_nonce = receipt.as_ref().map(|r| r.nonce),
            duration_ms = metrics.duration.as_millis(),
            crypto_ms = metrics.crypto_time.map(|d| d.as_millis()),
            guard_ms = metrics.guard_time.map(|d| d.as_millis()),
            journal_ms = metrics.journal_time.map(|d| d.as_millis()),
            "AMP send completed successfully"
        );

        if self.debug_enabled {
            debug!(
                component = self.component,
                context = %context,
                channel = %channel,
                "AMP send metrics: payload={} -> encrypted={}, flow={}, total_time={}ms",
                payload_size,
                encrypted_size,
                flow_charge,
                metrics.duration.as_millis()
            );
        }
    }

    /// Log failed AMP send operation with error categorization
    pub fn log_send_failure(
        &self,
        context: ContextId,
        channel: ChannelId,
        error: &AuraError,
        metrics: Option<AmpMetrics>,
    ) {
        let category = Self::categorize_error(error);
        let duration_ms = metrics.as_ref().map(|m| m.duration.as_millis());

        error!(
            component = self.component,
            operation = "amp_send",
            status = "failure",
            context = %context,
            channel = %channel,
            error_category = ?category,
            error_message = %error,
            duration_ms = duration_ms,
            "AMP send failed"
        );
    }

    /// Log successful AMP receive operation with structured telemetry
    pub fn log_receive_success(&self, telemetry: AmpReceiveTelemetry) {
        let AmpReceiveTelemetry {
            context,
            header,
            wire_size,
            decrypted_size,
            window_validation,
            metrics,
        } = telemetry;

        info!(
            component = self.component,
            operation = "amp_recv",
            status = "success",
            context = %context,
            channel = %header.channel,
            epoch = header.chan_epoch,
            generation = header.ratchet_gen,
            wire_bytes = wire_size,
            decrypted_bytes = decrypted_size,
            window_valid = window_validation.generation_valid,
            window_bounds_min = window_validation.window_bounds.0,
            window_bounds_max = window_validation.window_bounds.1,
            duration_ms = metrics.duration.as_millis(),
            crypto_ms = metrics.crypto_time.map(|d| d.as_millis()),
            "AMP receive completed successfully"
        );
    }

    /// Log failed AMP receive operation with detailed window validation info
    pub fn log_receive_failure(
        &self,
        context: ContextId,
        header: Option<AmpHeader>,
        window_validation: Option<WindowValidationResult>,
        error: &AuraError,
        metrics: Option<AmpMetrics>,
    ) {
        let category = Self::categorize_error(error);
        let duration_ms = metrics.as_ref().map(|m| m.duration.as_millis());

        // Build structured fields for conditional logging
        if let Some(h) = header {
            if let Some(validation) = window_validation {
                error!(
                    component = self.component,
                    operation = "amp_recv",
                    status = "failure",
                    context = %context,
                    channel = %h.channel,
                    epoch = h.chan_epoch,
                    generation = h.ratchet_gen,
                    epoch_valid = validation.epoch_valid,
                    generation_valid = validation.generation_valid,
                    window_bounds_min = validation.window_bounds.0,
                    window_bounds_max = validation.window_bounds.1,
                    actual_generation = validation.actual_generation,
                    rejection_reason = validation.rejection_reason.as_deref().unwrap_or("none"),
                    error_category = ?category,
                    error_message = %error,
                    duration_ms = duration_ms,
                    "AMP receive failed"
                );
            } else {
                error!(
                    component = self.component,
                    operation = "amp_recv",
                    status = "failure",
                    context = %context,
                    channel = %h.channel,
                    epoch = h.chan_epoch,
                    generation = h.ratchet_gen,
                    error_category = ?category,
                    error_message = %error,
                    duration_ms = duration_ms,
                    "AMP receive failed"
                );
            }
        } else if let Some(validation) = window_validation {
            error!(
                component = self.component,
                operation = "amp_recv",
                status = "failure",
                context = %context,
                epoch_valid = validation.epoch_valid,
                generation_valid = validation.generation_valid,
                window_bounds_min = validation.window_bounds.0,
                window_bounds_max = validation.window_bounds.1,
                actual_generation = validation.actual_generation,
                rejection_reason = validation.rejection_reason.as_deref().unwrap_or("none"),
                error_category = ?category,
                error_message = %error,
                duration_ms = duration_ms,
                "AMP receive failed"
            );
        } else {
            error!(
                component = self.component,
                operation = "amp_recv",
                status = "failure",
                context = %context,
                error_category = ?category,
                error_message = %error,
                duration_ms = duration_ms,
                "AMP receive failed"
            );
        }
    }

    /// Log AMP window rejection with detailed validation info
    pub fn log_window_rejection(
        &self,
        context: ContextId,
        header: &AmpHeader,
        validation: &WindowValidationResult,
        amp_error: &AmpError,
    ) {
        warn!(
            component = self.component,
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
            actual_generation = validation.actual_generation,
            rejection_reason = validation.rejection_reason.as_deref().unwrap_or("unknown"),
            amp_error = %amp_error,
            "AMP message rejected due to window validation failure"
        );
    }

    /// Log flow budget charge operation
    pub fn log_flow_charge(&self, telemetry: AmpFlowTelemetry) {
        let AmpFlowTelemetry {
            context,
            peer,
            operation,
            cost,
            receipt,
            budget_remaining,
            charge_duration,
        } = telemetry;

        info!(
            component = self.component,
            operation = "flow_charge",
            flow_operation = operation,
            context = %context,
            peer = %peer,
            cost = cost,
            receipt_nonce = receipt.as_ref().map(|r| r.nonce),
            budget_remaining = budget_remaining,
            charge_duration_us = charge_duration.as_micros(),
            "Flow budget charged for AMP operation"
        );
    }

    /// Log flow budget charge failure
    pub fn log_flow_charge_failure(
        &self,
        context: ContextId,
        peer: aura_core::AuthorityId,
        operation: &'static str,
        cost: u32,
        error: &AuraError,
    ) {
        warn!(
            component = self.component,
            operation = "flow_charge",
            status = "failure",
            flow_operation = operation,
            context = %context,
            peer = %peer,
            cost = cost,
            error_message = %error,
            "Flow budget charge failed for AMP operation"
        );
    }

    /// Log AMP protocol statistics and health metrics
    pub fn log_protocol_stats(
        &self,
        context: ContextId,
        channel: ChannelId,
        stats: AmpProtocolStats,
    ) {
        info!(
            component = self.component,
            operation = "protocol_stats",
            context = %context,
            channel = %channel,
            messages_sent = stats.messages_sent,
            messages_received = stats.messages_received,
            bytes_sent = stats.bytes_sent,
            bytes_received = stats.bytes_received,
            flow_total_charged = stats.total_flow_charged,
            window_rejections = stats.window_rejections,
            crypto_failures = stats.crypto_failures,
            avg_send_latency_ms = stats.avg_send_latency.as_millis(),
            avg_recv_latency_ms = stats.avg_recv_latency.as_millis(),
            "AMP protocol statistics"
        );
    }

    /// Categorize AuraError for metrics and observability
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
}

/// AMP protocol health and performance statistics
#[derive(Debug, Clone, Default)]
pub struct AmpProtocolStats {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub total_flow_charged: u64,
    pub window_rejections: u64,
    pub crypto_failures: u64,
    pub avg_send_latency: Duration,
    pub avg_recv_latency: Duration,
}

/// Helper to create window validation result from AMP validation
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

/// Global AMP telemetry instance for the transport layer
pub static AMP_TELEMETRY: AmpTelemetry = AmpTelemetry {
    component: "amp_transport",
    debug_enabled: false, // Will be dynamically checked
};

/// Convenience macro for timing AMP operations with automatic telemetry
#[macro_export]
macro_rules! time_amp_operation {
    ($operation:expr, $block:block) => {{
        let start = aura_effects::time::RealTimeHandler::default()
            .now_instant()
            .await;
        let result = $block;
        let duration = start.elapsed();
        tracing::debug!(
            operation = $operation,
            duration_ms = duration.as_millis(),
            "AMP operation timing"
        );
        (result, duration)
    }};
}
