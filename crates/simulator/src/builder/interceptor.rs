//! Effect interception for Byzantine testing
//!
//! This module provides hooks to intercept and modify effects produced by participants.
//! This enables Byzantine testing by allowing tests to inject faults without modifying
//! the production code.

use crate::{Effect, EffectContext};
use std::sync::Arc;

/// Effect interceptor function
///
/// An interceptor takes an effect and its context, and returns:
/// - `Some(effect)` to forward the effect (possibly modified)
/// - `None` to drop the effect
///
/// Interceptors must be deterministic and not mutate shared state.
pub type InterceptorFn = Arc<dyn Fn(&EffectContext, Effect) -> Option<Effect> + Send + Sync>;

/// Interceptor for outgoing effects (before they leave the participant)
#[derive(Clone)]
pub struct OutgoingInterceptor {
    interceptor: InterceptorFn,
}

impl OutgoingInterceptor {
    /// Create a new outgoing interceptor
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(&EffectContext, Effect) -> Option<Effect> + Send + Sync + 'static,
    {
        OutgoingInterceptor {
            interceptor: Arc::new(f),
        }
    }

    /// Create a pass-through interceptor (no modification)
    pub fn passthrough() -> Self {
        Self::new(|_ctx, effect| Some(effect))
    }

    /// Create a drop-all interceptor (drops everything)
    pub fn drop_all() -> Self {
        Self::new(|_ctx, _effect| None)
    }

    /// Apply the interceptor to an effect
    pub fn apply(&self, ctx: &EffectContext, effect: Effect) -> Option<Effect> {
        (self.interceptor)(ctx, effect)
    }
}

/// Interceptor for incoming effects (before the participant processes them)
#[derive(Clone)]
pub struct IncomingInterceptor {
    interceptor: InterceptorFn,
}

impl IncomingInterceptor {
    /// Create a new incoming interceptor
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(&EffectContext, Effect) -> Option<Effect> + Send + Sync + 'static,
    {
        IncomingInterceptor {
            interceptor: Arc::new(f),
        }
    }

    /// Create a pass-through interceptor (no modification)
    pub fn passthrough() -> Self {
        Self::new(|_ctx, effect| Some(effect))
    }

    /// Create a drop-all interceptor (drops everything)
    pub fn drop_all() -> Self {
        Self::new(|_ctx, _effect| None)
    }

    /// Apply the interceptor to an effect
    pub fn apply(&self, ctx: &EffectContext, effect: Effect) -> Option<Effect> {
        (self.interceptor)(ctx, effect)
    }
}

/// Combined interceptor configuration
///
/// Contains both outgoing and incoming effect interceptors for simulating
/// Byzantine behavior in distributed protocols.
#[derive(Clone)]
pub struct Interceptors {
    /// Interceptor for outgoing effects (sent by this participant)
    pub outgoing: OutgoingInterceptor,
    /// Interceptor for incoming effects (received by this participant)
    pub incoming: IncomingInterceptor,
}

impl Interceptors {
    /// Create honest interceptors (pass-through for everything)
    pub fn honest() -> Self {
        Interceptors {
            outgoing: OutgoingInterceptor::passthrough(),
            incoming: IncomingInterceptor::passthrough(),
        }
    }

    /// Create with custom outgoing interceptor
    pub fn with_outgoing<F>(f: F) -> Self
    where
        F: Fn(&EffectContext, Effect) -> Option<Effect> + Send + Sync + 'static,
    {
        Interceptors {
            outgoing: OutgoingInterceptor::new(f),
            incoming: IncomingInterceptor::passthrough(),
        }
    }

    /// Create with custom incoming interceptor
    pub fn with_incoming<F>(f: F) -> Self
    where
        F: Fn(&EffectContext, Effect) -> Option<Effect> + Send + Sync + 'static,
    {
        Interceptors {
            outgoing: OutgoingInterceptor::passthrough(),
            incoming: IncomingInterceptor::new(f),
        }
    }

    /// Create with custom outgoing and incoming interceptors
    pub fn with_both<FOut, FIn>(f_out: FOut, f_in: FIn) -> Self
    where
        FOut: Fn(&EffectContext, Effect) -> Option<Effect> + Send + Sync + 'static,
        FIn: Fn(&EffectContext, Effect) -> Option<Effect> + Send + Sync + 'static,
    {
        Interceptors {
            outgoing: OutgoingInterceptor::new(f_out),
            incoming: IncomingInterceptor::new(f_in),
        }
    }
}

impl Default for Interceptors {
    fn default() -> Self {
        Self::honest()
    }
}

/// Byzantine behavior presets for common attack patterns
pub mod byzantine {
    use super::*;
    use crate::Operation;

    /// Drop all messages for a specific protocol operation
    pub fn drop_operation(target_op: Operation) -> OutgoingInterceptor {
        OutgoingInterceptor::new(move |ctx, effect| {
            if ctx.matches(target_op) {
                None // Drop
            } else {
                Some(effect)
            }
        })
    }

    /// Corrupt message payloads for a specific operation
    pub fn corrupt_operation(target_op: Operation) -> OutgoingInterceptor {
        OutgoingInterceptor::new(move |ctx, effect| {
            if ctx.matches(target_op) {
                match effect {
                    Effect::Send(mut envelope) => {
                        // Corrupt payload by flipping bits
                        for byte in &mut envelope.payload {
                            *byte = !*byte;
                        }
                        Some(Effect::Send(envelope))
                    }
                    other => Some(other),
                }
            } else {
                Some(effect)
            }
        })
    }

    /// Delay messages by replicating them multiple times
    pub fn duplicate_messages() -> OutgoingInterceptor {
        OutgoingInterceptor::new(|_ctx, effect| {
            // Note: The simulation would need to handle this by processing the effect twice
            // For now, just pass through
            Some(effect)
        })
    }

    /// Silent participant (drops all outgoing messages)
    pub fn silent() -> OutgoingInterceptor {
        OutgoingInterceptor::drop_all()
    }

    /// Crash after N ticks
    pub fn crash_after_ticks(n: u64) -> OutgoingInterceptor {
        OutgoingInterceptor::new(move |ctx, effect| {
            if ctx.tick > n {
                None // Drop everything after crash
            } else {
                Some(effect)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use crate::{DeliverySemantics, Envelope, Operation, ParticipantId};
    #[allow(unused_imports)]
    use uuid::Uuid;

    #[test]
    fn test_passthrough_interceptor() {
        let interceptor = OutgoingInterceptor::passthrough();

        let ctx = EffectContext {
            tick: 0,
            sender: ParticipantId::from_name("alice"),
            recipients: vec![],
            operation: None,
        };

        let effect = Effect::Log {
            participant: ParticipantId::from_name("alice"),
            level: crate::LogLevel::Info,
            message: "test".to_string(),
        };

        let result = interceptor.apply(&ctx, effect);
        assert!(result.is_some());
    }

    #[test]
    fn test_drop_all_interceptor() {
        let interceptor = OutgoingInterceptor::drop_all();

        let ctx = EffectContext {
            tick: 0,
            sender: ParticipantId::from_name("alice"),
            recipients: vec![],
            operation: None,
        };

        let effect = Effect::Log {
            participant: ParticipantId::from_name("alice"),
            level: crate::LogLevel::Info,
            message: "test".to_string(),
        };

        let result = interceptor.apply(&ctx, effect);
        assert!(result.is_none());
    }

    #[test]
    fn test_byzantine_drop_operation() {
        let interceptor = byzantine::drop_operation(Operation::DkdCommitment);

        let ctx_commit = EffectContext {
            tick: 0,
            sender: ParticipantId::from_name("alice"),
            recipients: vec![],
            operation: Some(Operation::DkdCommitment),
        };

        let ctx_other = EffectContext {
            tick: 0,
            sender: ParticipantId::from_name("alice"),
            recipients: vec![],
            operation: Some(Operation::DkdReveal),
        };

        let effect = Effect::Log {
            participant: ParticipantId::from_name("alice"),
            level: crate::LogLevel::Info,
            message: "test".to_string(),
        };

        // Should drop DkdCommitment operation
        assert!(interceptor.apply(&ctx_commit, effect.clone()).is_none());

        // Should pass through other operations
        assert!(interceptor.apply(&ctx_other, effect).is_some());
    }

    #[test]
    fn test_byzantine_crash_after_ticks() {
        let interceptor = byzantine::crash_after_ticks(5);

        let effect = Effect::Log {
            participant: ParticipantId::from_name("alice"),
            level: crate::LogLevel::Info,
            message: "test".to_string(),
        };

        // Before crash
        let ctx_before = EffectContext {
            tick: 3,
            sender: ParticipantId::from_name("alice"),
            recipients: vec![],
            operation: None,
        };
        assert!(interceptor.apply(&ctx_before, effect.clone()).is_some());

        // After crash
        let ctx_after = EffectContext {
            tick: 10,
            sender: ParticipantId::from_name("alice"),
            recipients: vec![],
            operation: None,
        };
        assert!(interceptor.apply(&ctx_after, effect).is_none());
    }
}
