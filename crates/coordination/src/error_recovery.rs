//! Error recovery strategies for Aura protocols
//!
//! Provides pluggable recovery strategies for different types of errors,
//! enabling automatic retry and escalation based on error characteristics.

use crate::tracing::{LogLevel, ProtocolTracer, LogValue};
use aura_journal::{AuraError, AuraErrorKind, DeviceId, NetworkErrorKind, AuthFailureReason};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Context for error recovery attempts
#[derive(Debug, Clone)]
pub struct RecoveryContext {
    /// How many times recovery has been attempted
    pub retry_count: usize,
    /// Maximum retries allowed
    pub max_retries: usize,
    /// Time when error first occurred
    pub first_occurrence: u64,
    /// Time when last recovery was attempted
    pub last_attempt: u64,
    /// Session ID if error occurred in a session
    pub session_id: Option<Uuid>,
    /// Additional context fields
    pub fields: BTreeMap<String, String>,
}

/// Result of a recovery attempt
#[derive(Debug, Clone)]
pub enum RecoveryResult {
    /// Recovery succeeded, operation can continue
    Recovered,
    /// Should retry after the specified delay
    Retry { after_ms: u64 },
    /// Escalate to next recovery strategy or human intervention
    Escalate { to_human: bool },
    /// Abort the operation safely
    Abort { safe: bool },
}

/// Recovery strategy for specific error types
pub trait RecoveryStrategy: Send + Sync {
    /// Check if this strategy can handle the given error
    fn can_recover(&self, error: &AuraError) -> bool;
    
    /// Attempt to recover from the error
    fn attempt_recovery(&self, error: &AuraError, context: &RecoveryContext) -> RecoveryResult;
    
    /// Get the name of this recovery strategy
    fn strategy_name(&self) -> &'static str;
}

/// Manager for error recovery with pluggable strategies
pub struct ErrorRecoveryManager {
    device_id: DeviceId,
    recovery_strategies: Vec<Box<dyn RecoveryStrategy>>,
    tracer: Option<ProtocolTracer>,
    active_recoveries: BTreeMap<String, RecoveryContext>,
}

impl ErrorRecoveryManager {
    /// Create a new error recovery manager
    pub fn new(device_id: DeviceId) -> Self {
        let mut manager = Self {
            device_id,
            recovery_strategies: Vec::new(),
            tracer: None,
            active_recoveries: BTreeMap::new(),
        };
        
        // Add default recovery strategies
        manager.add_strategy(Box::new(NetworkRetryStrategy));
        manager.add_strategy(Box::new(CapabilityRefreshStrategy));
        manager.add_strategy(Box::new(AuthenticationRetryStrategy));
        manager.add_strategy(Box::new(ResourceWaitStrategy));
        manager.add_strategy(Box::new(ChoreographyRetryStrategy));
        
        manager
    }
    
    /// Add a recovery strategy
    pub fn add_strategy(&mut self, strategy: Box<dyn RecoveryStrategy>) {
        self.recovery_strategies.push(strategy);
    }
    
    /// Set the protocol tracer for logging recovery attempts
    pub fn set_tracer(&mut self, tracer: ProtocolTracer) {
        self.tracer = Some(tracer);
    }
    
    /// Attempt to recover from an error
    pub fn attempt_recovery(&mut self, error: &AuraError) -> RecoveryResult {
        let error_key = self.error_key(error);
        
        // Get or create recovery context
        let context = self.active_recoveries.entry(error_key.clone())
            .and_modify(|ctx| {
                ctx.retry_count += 1;
                ctx.last_attempt = error.timestamp;
            })
            .or_insert_with(|| RecoveryContext {
                retry_count: 0,
                max_retries: 3, // Default max retries
                first_occurrence: error.timestamp,
                last_attempt: error.timestamp,
                session_id: error.session_context.as_ref().map(|s| s.session_id),
                fields: BTreeMap::new(),
            })
            .clone();
        
        // Check if we've exceeded max retries
        if context.retry_count >= context.max_retries {
            self.log_recovery_attempt(error, "max_retries_exceeded", &RecoveryResult::Escalate { to_human: true });
            self.active_recoveries.remove(&error_key);
            return RecoveryResult::Escalate { to_human: true };
        }
        
        // Find a strategy that can handle this error
        for strategy in &self.recovery_strategies {
            if strategy.can_recover(error) {
                let result = strategy.attempt_recovery(error, &context);
                self.log_recovery_attempt(error, strategy.strategy_name(), &result);
                
                match &result {
                    RecoveryResult::Recovered | RecoveryResult::Abort { .. } => {
                        self.active_recoveries.remove(&error_key);
                    }
                    RecoveryResult::Escalate { .. } => {
                        // Try next strategy
                        continue;
                    }
                    RecoveryResult::Retry { .. } => {
                        // Keep the recovery context for next attempt
                    }
                }
                
                return result;
            }
        }
        
        // No strategy could handle this error
        self.log_recovery_attempt(error, "no_strategy_found", &RecoveryResult::Escalate { to_human: true });
        self.active_recoveries.remove(&error_key);
        RecoveryResult::Escalate { to_human: true }
    }
    
    /// Clean up completed recovery contexts
    pub fn cleanup_old_recoveries(&mut self, current_time: u64, max_age: u64) {
        self.active_recoveries.retain(|_, context| {
            current_time.saturating_sub(context.first_occurrence) < max_age
        });
    }
    
    /// Get statistics about recovery attempts
    pub fn get_recovery_statistics(&self) -> RecoveryStatistics {
        RecoveryStatistics {
            active_recoveries: self.active_recoveries.len(),
            strategies_count: self.recovery_strategies.len(),
        }
    }
    
    // Private helper methods
    
    fn error_key(&self, error: &AuraError) -> String {
        format!("{}:{}:{}", 
                self.device_id.0,
                error.context.operation,
                format!("{:?}", error.kind))
    }
    
    fn log_recovery_attempt(&self, error: &AuraError, strategy: &str, result: &RecoveryResult) {
        if let Some(tracer) = &self.tracer {
            let fields = crate::btreemap! {
                "strategy" => LogValue::String(strategy.to_string()),
                "result" => LogValue::String(format!("{:?}", result)),
                "error_type" => LogValue::String(format!("{:?}", error.kind)),
            };
            
            tracer.log_sink().log_event(
                LogLevel::Info,
                &tracer.get_or_create_span(
                    error.session_context.as_ref().map(|s| s.session_id), 
                    "error_recovery"
                ),
                format!("Recovery attempt: {} -> {:?}", strategy, result),
                fields,
            );
        }
    }
}

/// Statistics about recovery operations
#[derive(Debug, Clone)]
pub struct RecoveryStatistics {
    /// Number of active recovery contexts
    pub active_recoveries: usize,
    /// Number of registered strategies
    pub strategies_count: usize,
}

// === Recovery Strategy Implementations ===

/// Network retry strategy with exponential backoff
pub struct NetworkRetryStrategy;

impl RecoveryStrategy for NetworkRetryStrategy {
    fn can_recover(&self, error: &AuraError) -> bool {
        matches!(error.kind, AuraErrorKind::Network { .. })
    }
    
    fn attempt_recovery(&self, error: &AuraError, context: &RecoveryContext) -> RecoveryResult {
        if let AuraErrorKind::Network { underlying, .. } = &error.kind {
            match underlying {
                NetworkErrorKind::Timeout | NetworkErrorKind::NetworkUnreachable => {
                    // Exponential backoff: 1s, 2s, 4s, etc.
                    let delay_ms = 1000 * (1 << context.retry_count.min(6));
                    RecoveryResult::Retry { after_ms: delay_ms }
                }
                NetworkErrorKind::ConnectionRefused => {
                    // Longer delay for connection refused
                    RecoveryResult::Retry { after_ms: 5000 }
                }
                NetworkErrorKind::DnsFailure => {
                    RecoveryResult::Escalate { to_human: false }
                }
                NetworkErrorKind::TlsError | NetworkErrorKind::ProtocolMismatch => {
                    RecoveryResult::Escalate { to_human: true }
                }
            }
        } else {
            RecoveryResult::Escalate { to_human: false }
        }
    }
    
    fn strategy_name(&self) -> &'static str {
        "network_retry"
    }
}

/// Capability refresh strategy
pub struct CapabilityRefreshStrategy;

impl RecoveryStrategy for CapabilityRefreshStrategy {
    fn can_recover(&self, error: &AuraError) -> bool {
        matches!(error.kind, AuraErrorKind::Authorization { .. })
    }
    
    fn attempt_recovery(&self, _error: &AuraError, context: &RecoveryContext) -> RecoveryResult {
        if context.retry_count == 0 {
            // First attempt: try to refresh capabilities
            RecoveryResult::Retry { after_ms: 100 }
        } else {
            // Subsequent attempts: escalate
            RecoveryResult::Escalate { to_human: true }
        }
    }
    
    fn strategy_name(&self) -> &'static str {
        "capability_refresh"
    }
}

/// Authentication retry strategy
pub struct AuthenticationRetryStrategy;

impl RecoveryStrategy for AuthenticationRetryStrategy {
    fn can_recover(&self, error: &AuraError) -> bool {
        if let AuraErrorKind::Authentication { reason, .. } = &error.kind {
            matches!(reason, AuthFailureReason::ExpiredCertificate | AuthFailureReason::ReplayAttack)
        } else {
            false
        }
    }
    
    fn attempt_recovery(&self, error: &AuraError, _context: &RecoveryContext) -> RecoveryResult {
        if let AuraErrorKind::Authentication { reason, .. } = &error.kind {
            match reason {
                AuthFailureReason::ExpiredCertificate => {
                    // Can try to refresh certificate
                    RecoveryResult::Retry { after_ms: 1000 }
                }
                AuthFailureReason::ReplayAttack => {
                    // Need to generate new nonce
                    RecoveryResult::Retry { after_ms: 100 }
                }
                _ => RecoveryResult::Escalate { to_human: true }
            }
        } else {
            RecoveryResult::Escalate { to_human: false }
        }
    }
    
    fn strategy_name(&self) -> &'static str {
        "authentication_retry"
    }
}

/// Resource wait strategy
pub struct ResourceWaitStrategy;

impl RecoveryStrategy for ResourceWaitStrategy {
    fn can_recover(&self, error: &AuraError) -> bool {
        matches!(error.kind, AuraErrorKind::Resource { .. })
    }
    
    fn attempt_recovery(&self, error: &AuraError, context: &RecoveryContext) -> RecoveryResult {
        if let AuraErrorKind::Resource { resource_type, .. } = &error.kind {
            // Wait longer for each retry
            let delay_ms = match resource_type {
                aura_journal::ResourceType::Memory => 5000, // Wait for GC
                aura_journal::ResourceType::FileDescriptors => 1000,
                aura_journal::ResourceType::StorageSpace => 10000, // Wait for cleanup
                aura_journal::ResourceType::NetworkBandwidth => 2000,
                aura_journal::ResourceType::CpuTime => 1000,
                aura_journal::ResourceType::ConcurrentSessions => 3000,
            };
            
            RecoveryResult::Retry { after_ms: delay_ms * (context.retry_count as u64 + 1) }
        } else {
            RecoveryResult::Escalate { to_human: false }
        }
    }
    
    fn strategy_name(&self) -> &'static str {
        "resource_wait"
    }
}

/// Choreography retry strategy
pub struct ChoreographyRetryStrategy;

impl RecoveryStrategy for ChoreographyRetryStrategy {
    fn can_recover(&self, error: &AuraError) -> bool {
        matches!(error.kind, AuraErrorKind::Choreography { .. })
    }
    
    fn attempt_recovery(&self, _error: &AuraError, context: &RecoveryContext) -> RecoveryResult {
        if context.retry_count < 2 {
            // Choreography can often be retried due to temporary coordination issues
            RecoveryResult::Retry { after_ms: 2000 }
        } else {
            // After 2 retries, likely a protocol issue
            RecoveryResult::Escalate { to_human: false }
        }
    }
    
    fn strategy_name(&self) -> &'static str {
        "choreography_retry"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_journal::{AccountId, ErrorContext, NetworkOperation};
    use aura_crypto::Effects;
    
    #[test]
    fn test_network_retry_strategy() {
        let strategy = NetworkRetryStrategy;
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new_with_effects(&effects);
        
        let error = AuraError {
            kind: AuraErrorKind::Network {
                peer_id: None,
                operation: NetworkOperation::Connect,
                underlying: NetworkErrorKind::Timeout,
            },
            context: ErrorContext {
                operation: "test".to_string(),
                device_id,
                account_id,
                protocol_phase: None,
                participant_id: None,
                session_epoch: None,
                fields: BTreeMap::new(),
            },
            trace_id: Uuid::new_v4(),
            timestamp: 0,
            session_context: None,
        };
        
        let context = RecoveryContext {
            retry_count: 0,
            max_retries: 3,
            first_occurrence: 0,
            last_attempt: 0,
            session_id: None,
            fields: BTreeMap::new(),
        };
        
        assert!(strategy.can_recover(&error));
        
        match strategy.attempt_recovery(&error, &context) {
            RecoveryResult::Retry { after_ms } => {
                assert_eq!(after_ms, 1000); // First retry after 1 second
            }
            other => panic!("Expected Retry, got {:?}", other),
        }
    }
    
    #[test]
    fn test_capability_refresh_strategy() {
        let strategy = CapabilityRefreshStrategy;
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new_with_effects(&effects);
        
        let error = AuraError {
            kind: AuraErrorKind::Authorization {
                required_capability: "storage:read".to_string(),
                granted_capabilities: vec!["storage:write".to_string()],
                operation: "read_file".to_string(),
            },
            context: ErrorContext {
                operation: "test".to_string(),
                device_id,
                account_id,
                protocol_phase: None,
                participant_id: None,
                session_epoch: None,
                fields: BTreeMap::new(),
            },
            trace_id: Uuid::new_v4(),
            timestamp: 0,
            session_context: None,
        };
        
        assert!(strategy.can_recover(&error));
    }
}