//! Helper abstractions for choreographic protocol implementation
//!
//! This module provides common abstractions to reduce boilerplate in choreographies:
//! - `EventBuilder` - Fluent API for event construction and emission
//! - `EventAwaiter` - Fluent API for waiting on events
//! - `ProtocolContextExt` - Extension trait for common ProtocolContext operations
//! - `SessionLifecycle` - Trait for generic session lifecycle management

use super::{
    EventFilter, EventTypePattern, Instruction, InstructionResult, ProtocolContext, ProtocolError,
    ProtocolErrorType,
};
use aura_journal::{DeviceId, Event, EventAuthorization, EventId, EventType, Session};
use std::collections::BTreeSet;
use uuid::Uuid;

// ========== Event Builder ==========

/// Fluent API for building and emitting events
pub struct EventBuilder<'a> {
    ctx: &'a mut ProtocolContext,
    event_type: Option<EventType>,
    use_threshold_auth: bool,
}

impl<'a> EventBuilder<'a> {
    /// Create a new EventBuilder
    pub fn new(ctx: &'a mut ProtocolContext) -> Self {
        Self {
            ctx,
            event_type: None,
            use_threshold_auth: false,
        }
    }

    /// Set the event type
    pub fn with_type(mut self, event_type: EventType) -> Self {
        self.event_type = Some(event_type);
        self
    }

    /// Use device certificate authorization (default)
    pub fn with_device_auth(mut self) -> Self {
        self.use_threshold_auth = false;
        self
    }

    /// Use threshold signature authorization
    pub fn with_threshold_auth(mut self) -> Self {
        self.use_threshold_auth = true;
        self
    }

    /// Build and sign the event
    pub async fn build_and_sign(self) -> Result<Event, ProtocolError> {
        let event_type = self.event_type.ok_or_else(|| ProtocolError {
            session_id: self.ctx.session_id,
            error_type: ProtocolErrorType::Other,
            message: "Event type must be specified".to_string(),
        })?;

        // Get ledger context
        let ledger_context = self.ctx.fetch_ledger_context().await?;

        // Build base event
        let mut event = Event {
            version: aura_journal::EVENT_VERSION,
            event_id: EventId::new_with_effects(&self.ctx.effects),
            account_id: ledger_context.account_id,
            timestamp: self.ctx.effects.now().map_err(|e| ProtocolError {
                session_id: self.ctx.session_id,
                error_type: ProtocolErrorType::Other,
                message: format!("Failed to get timestamp: {:?}", e),
            })?,
            nonce: ledger_context.nonce,
            parent_hash: ledger_context.parent_hash,
            epoch_at_write: ledger_context.epoch,
            event_type,
            authorization: if self.use_threshold_auth {
                EventAuthorization::ThresholdSignature(aura_journal::ThresholdSig {
                    signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                    signers: vec![],
                    signature_shares: vec![],
                })
            } else {
                EventAuthorization::DeviceCertificate {
                    device_id: DeviceId(self.ctx.device_id),
                    signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                }
            },
        };

        // Sign the event
        let signature = self.ctx.sign_event(&event)?;
        
        // Update authorization with actual signature
        match &mut event.authorization {
            EventAuthorization::DeviceCertificate { signature: sig, .. } => {
                *sig = signature;
            }
            EventAuthorization::ThresholdSignature(threshold_sig) => {
                // For threshold signatures, this would be more complex
                // For now, just update the signature field
                threshold_sig.signature = signature;
            }
            _ => {}
        }

        Ok(event)
    }

    /// Build, sign, and emit the event
    pub async fn build_sign_and_emit(mut self) -> Result<(), ProtocolError> {
        let session_id = self.ctx.session_id;
        
        // Build and sign the event
        let event = {
            let event_type = self.event_type.take().ok_or_else(|| ProtocolError {
                session_id: self.ctx.session_id,
                error_type: ProtocolErrorType::Other,
                message: "Event type must be specified".to_string(),
            })?;

            // Get ledger context
            let ledger_context = self.ctx.fetch_ledger_context().await?;

            // Build base event
            let mut event = Event {
                version: aura_journal::EVENT_VERSION,
                event_id: EventId::new_with_effects(&self.ctx.effects),
                account_id: ledger_context.account_id,
                timestamp: self.ctx.effects.now().map_err(|e| ProtocolError {
                    session_id: self.ctx.session_id,
                    error_type: ProtocolErrorType::Other,
                    message: format!("Failed to get timestamp: {:?}", e),
                })?,
                nonce: ledger_context.nonce,
                parent_hash: ledger_context.parent_hash,
                epoch_at_write: ledger_context.epoch,
                event_type,
                authorization: if self.use_threshold_auth {
                    EventAuthorization::ThresholdSignature(aura_journal::ThresholdSig {
                        signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                        signers: vec![],
                        signature_shares: vec![],
                    })
                } else {
                    EventAuthorization::DeviceCertificate {
                        device_id: DeviceId(self.ctx.device_id),
                        signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                    }
                },
            };

            // Sign the event
            let signature = self.ctx.sign_event(&event)?;
            
            // Update authorization with actual signature
            match &mut event.authorization {
                EventAuthorization::DeviceCertificate { signature: sig, .. } => {
                    *sig = signature;
                }
                EventAuthorization::ThresholdSignature(threshold_sig) => {
                    // For threshold signatures, this would be more complex
                    // For now, just update the signature field
                    threshold_sig.signature = signature;
                }
                _ => {}
            }

            event
        };
        
        // Write the event to the ledger
        match self.ctx.execute(Instruction::WriteToLedger(event)).await? {
            InstructionResult::EventWritten => Ok(()),
            _ => Err(ProtocolError {
                session_id,
                error_type: ProtocolErrorType::InvalidState,
                message: "Failed to write event to ledger".to_string(),
            }),
        }
    }
}

// ========== Event Awaiter ==========

/// Fluent API for waiting on events
pub struct EventAwaiter<'a> {
    ctx: &'a mut ProtocolContext,
    session_id: Option<Uuid>,
    event_types: Option<Vec<EventTypePattern>>,
    authors: Option<BTreeSet<DeviceId>>,
}

impl<'a> EventAwaiter<'a> {
    /// Create a new EventAwaiter
    pub fn new(ctx: &'a mut ProtocolContext) -> Self {
        Self {
            ctx,
            session_id: None,
            event_types: None,
            authors: None,
        }
    }

    /// Filter by session ID
    pub fn for_session(mut self, session_id: Uuid) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Filter by event types
    pub fn for_event_types(mut self, types: Vec<EventTypePattern>) -> Self {
        self.event_types = Some(types);
        self
    }

    /// Filter by authors
    pub fn from_authors(mut self, authors: impl IntoIterator<Item = DeviceId>) -> Self {
        self.authors = Some(authors.into_iter().collect());
        self
    }

    /// Wait for a single event
    pub async fn await_single(self, timeout_epochs: u64) -> Result<Event, ProtocolError> {
        let filter = EventFilter {
            session_id: self.session_id,
            event_types: self.event_types,
            authors: self.authors,
            predicate: None,
        };

        match self
            .ctx
            .execute(Instruction::AwaitEvent {
                filter,
                timeout_epochs: Some(timeout_epochs),
            })
            .await?
        {
            InstructionResult::EventReceived(event) => Ok(event),
            _ => Err(ProtocolError {
                session_id: self.ctx.session_id,
                error_type: ProtocolErrorType::Timeout,
                message: "Timeout waiting for event".to_string(),
            }),
        }
    }

    /// Wait for threshold number of events
    pub async fn await_threshold(
        self,
        count: usize,
        timeout_epochs: u64,
    ) -> Result<Vec<Event>, ProtocolError> {
        let filter = EventFilter {
            session_id: self.session_id,
            event_types: self.event_types,
            authors: self.authors,
            predicate: None,
        };

        match self
            .ctx
            .execute(Instruction::AwaitThreshold {
                count,
                filter,
                timeout_epochs: Some(timeout_epochs),
            })
            .await?
        {
            InstructionResult::EventsReceived(events) => Ok(events),
            _ => Err(ProtocolError {
                session_id: self.ctx.session_id,
                error_type: ProtocolErrorType::Timeout,
                message: format!("Timeout waiting for {} events", count),
            }),
        }
    }
}

// ========== Protocol Context Extensions ==========

/// Ledger context containing commonly needed values
pub struct LedgerContext {
    pub account_id: aura_journal::AccountId,
    pub nonce: u64,
    pub parent_hash: Option<[u8; 32]>,
    pub epoch: u64,
}

/// Extension trait for ProtocolContext
pub trait ProtocolContextExt {
    /// Fetch all commonly needed ledger values in one call
    async fn fetch_ledger_context(&mut self) -> Result<LedgerContext, ProtocolError>;
    
    /// Generate a new nonce
    async fn generate_nonce(&mut self) -> Result<u64, ProtocolError>;
}

impl ProtocolContextExt for ProtocolContext {
    async fn fetch_ledger_context(&mut self) -> Result<LedgerContext, ProtocolError> {
        // Get ledger state
        let ledger_state = match self.execute(Instruction::GetLedgerState).await? {
            InstructionResult::LedgerState(state) => state,
            _ => {
                return Err(ProtocolError {
                    session_id: self.session_id,
                    error_type: ProtocolErrorType::InvalidState,
                    message: "Expected ledger state".to_string(),
                })
            }
        };

        // Get current epoch
        let epoch = match self.execute(Instruction::GetCurrentEpoch).await? {
            InstructionResult::CurrentEpoch(e) => e,
            _ => {
                return Err(ProtocolError {
                    session_id: self.session_id,
                    error_type: ProtocolErrorType::InvalidState,
                    message: "Expected current epoch".to_string(),
                })
            }
        };

        Ok(LedgerContext {
            account_id: ledger_state.account_id,
            nonce: ledger_state.next_nonce,
            parent_hash: ledger_state.last_event_hash,
            epoch,
        })
    }

    async fn generate_nonce(&mut self) -> Result<u64, ProtocolError> {
        let ledger_state = match self.execute(Instruction::GetLedgerState).await? {
            InstructionResult::LedgerState(state) => state,
            _ => {
                return Err(ProtocolError {
                    session_id: self.session_id,
                    error_type: ProtocolErrorType::InvalidState,
                    message: "Expected ledger state".to_string(),
                })
            }
        };

        Ok(ledger_state.next_nonce)
    }
}

// ========== Session Lifecycle ==========

/// Result of collision check
pub enum CollisionResult {
    /// No collision, proceed normally
    NoCollision,
    /// Collision detected, we won the lottery
    WonLottery,
    /// Collision detected, we lost the lottery
    LostLottery(Session),
}

/// Trait for protocols that follow the standard session lifecycle
#[async_trait::async_trait]
pub trait SessionLifecycle: Sized {
    /// The result type returned by this protocol
    type Result: Clone + Send + Sync;

    /// Get the operation type for this protocol
    fn operation_type(&self) -> aura_journal::OperationType;

    /// Generate context ID for collision detection
    fn generate_context_id(&self) -> Vec<u8>;

    /// Create a new session
    async fn create_session(&mut self) -> Result<Session, ProtocolError>;

    /// Execute the protocol logic
    async fn execute_protocol(&mut self, session: &Session) -> Result<Self::Result, ProtocolError>;

    /// Wait for another session to complete
    async fn wait_for_completion(
        &mut self,
        winning_session: &Session,
    ) -> Result<Self::Result, ProtocolError>;

    /// Handle session completion (optional override)
    async fn complete_session(&mut self, _session: &Session) -> Result<(), ProtocolError> {
        Ok(()) // Default implementation does nothing
    }

    /// Handle session abortion (optional override)
    async fn abort_session(
        &mut self,
        _session: &Session,
        _error: &ProtocolError,
    ) -> Result<(), ProtocolError> {
        Ok(()) // Default implementation does nothing
    }
    
    /// Execute the full protocol lifecycle (default implementation)
    async fn execute(&mut self) -> Result<Self::Result, ProtocolError>
    where
        Self: Sized,
    {
        // Check for session collision
        let _context_id = self.generate_context_id();
        let session = self.create_session().await?;
        
        // Execute protocol with error handling
        match self.execute_protocol(&session).await {
            Ok(result) => {
                self.complete_session(&session).await?;
                Ok(result)
            }
            Err(error) => {
                self.abort_session(&session, &error).await?;
                Err(error)
            }
        }
    }
}

/// Run a protocol that implements SessionLifecycle
pub async fn run_session_protocol<P: SessionLifecycle + Send>(
    protocol: &mut P,
    ctx: &mut ProtocolContext,
) -> Result<P::Result, ProtocolError> {
    // Step 1: Check for session collision
    let context_id = protocol.generate_context_id();
    
    let collision_check = ctx
        .execute(Instruction::CheckSessionCollision {
            operation_type: protocol.operation_type(),
            context_id,
        })
        .await?;

    match collision_check {
        InstructionResult::SessionStatus {
            existing_sessions,
            winner,
        } => {
            // If there are existing sessions and we didn't win the lottery, wait for winner
            if !existing_sessions.is_empty() && winner != Some(DeviceId(ctx.device_id)) {
                // Wait for the winning session to complete
                if let Some(winning_session) = existing_sessions.first() {
                    return protocol.wait_for_completion(winning_session).await;
                }
            }
            // Either no collision or we won, proceed normally
        }
        _ => {
            return Err(ProtocolError {
                session_id: ctx.session_id,
                error_type: ProtocolErrorType::Other,
                message: "Unexpected result from collision check".to_string(),
            })
        }
    }

    // Step 2: Create session
    let session = protocol.create_session().await?;

    // Step 3: Execute protocol with error handling
    match protocol.execute_protocol(&session).await {
        Ok(result) => {
            // Step 4: Mark session as completed
            protocol.complete_session(&session).await?;
            Ok(result)
        }
        Err(error) => {
            // Step 5: Mark session as aborted
            protocol.abort_session(&session, &error).await?;
            Err(error)
        }
    }
}

// ========== Common Helper Functions ==========

// Note: No current_timestamp() function - use ctx.effects.now() for injectable time