//! Typed Channels for Local Choreographic Runtime Communication
//!
//! This module provides typed channel abstractions for communication within
//! the local session runtime, ensuring type safety for command/event/effect flows.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

// Note: session_types imports removed as they're not needed for channels
use aura_journal::{Event, DeviceId};

/// Trait for messages that can be sent through typed channels
pub trait ChannelMessage: Send + Sync + Clone + std::fmt::Debug + 'static {}

/// Commands that can be sent to the local session runtime
#[derive(Debug, Clone)]
pub enum SessionCommand {
    /// Start a new protocol session
    StartProtocol {
        protocol_type: SessionProtocolType,
        session_id: Uuid,
        config: ProtocolConfig,
    },
    
    /// Send a message to peer devices
    SendToPeers {
        session_id: Uuid,
        recipients: Vec<DeviceId>,
        message: Vec<u8>,
    },
    
    /// Process an inbound event from the journal or transport
    ProcessEvent {
        session_id: Uuid,
        event: Event,
    },
    
    /// Request state transition with witness
    RequestTransition {
        session_id: Uuid,
        target_state: String,
        witness_data: Vec<u8>,
    },
    
    /// Abort a protocol session
    AbortProtocol {
        session_id: Uuid,
        reason: String,
    },
    
    /// Query protocol status
    QueryStatus {
        session_id: Uuid,
        response_channel: ResponseChannel<ProtocolStatus>,
    },
    
    /// Shutdown the runtime
    Shutdown,
}

impl ChannelMessage for SessionCommand {}

/// Events emitted by the local session runtime
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// Protocol session started
    ProtocolStarted {
        session_id: Uuid,
        protocol_type: SessionProtocolType,
        initial_state: String,
    },
    
    /// State transition occurred
    StateTransition {
        session_id: Uuid,
        from_state: String,
        to_state: String,
        timestamp: u64,
    },
    
    /// Message received from peer
    MessageReceived {
        session_id: Uuid,
        from_device: DeviceId,
        message: Vec<u8>,
    },
    
    /// Protocol completed successfully
    ProtocolCompleted {
        session_id: Uuid,
        result: SessionProtocolResult,
    },
    
    /// Protocol failed or was aborted
    ProtocolFailed {
        session_id: Uuid,
        error: String,
        final_state: Option<String>,
    },
    
    /// Runtime error occurred
    RuntimeError {
        session_id: Option<Uuid>,
        error: String,
        recoverable: bool,
    },
}

impl ChannelMessage for SessionEvent {}

/// Effects that should be executed as a result of protocol operations
#[derive(Debug, Clone)]
pub enum SessionEffect {
    /// Write an event to the journal
    WriteEvent {
        event: Event,
        callback: Option<ResponseChannel<Result<(), String>>>,
    },
    
    /// Send a message via transport
    SendMessage {
        recipients: Vec<DeviceId>,
        message: Vec<u8>,
        callback: Option<ResponseChannel<Result<(), String>>>,
    },
    
    /// Store protocol state
    StoreState {
        session_id: Uuid,
        state_data: Vec<u8>,
        callback: Option<ResponseChannel<Result<(), String>>>,
    },
    
    /// Load protocol state
    LoadState {
        session_id: Uuid,
        callback: ResponseChannel<Result<Vec<u8>, String>>,
    },
    
    /// Trigger rehydration from journal
    TriggerRehydration {
        session_id: Uuid,
        from_epoch: u64,
        callback: ResponseChannel<Result<String, String>>, // Returns state name
    },
    
    /// Schedule a delayed action
    ScheduleAction {
        delay_ms: u64,
        action: Box<SessionCommand>,
    },
    
    /// Notify external systems
    NotifyExternal {
        notification_type: String,
        data: Vec<u8>,
    },
}

impl ChannelMessage for SessionEffect {}

/// Protocol types supported by the session runtime
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SessionProtocolType {
    DKD,
    Recovery,
    Resharing,
    Locking,
    Agent,
}

/// Configuration for starting a protocol
#[derive(Debug, Clone)]
pub struct ProtocolConfig {
    /// Protocol-specific parameters
    pub parameters: std::collections::HashMap<String, String>,
    /// Expected participants
    pub participants: Vec<DeviceId>,
    /// Timeout in epochs
    pub timeout_epochs: Option<u64>,
    /// Additional context data
    pub context: Vec<u8>,
}

/// Result of a completed protocol
#[derive(Debug, Clone)]
pub struct SessionProtocolResult {
    /// The type of result produced
    pub result_type: String,
    /// Serialized result data
    pub data: Vec<u8>,
    /// Additional metadata
    pub metadata: std::collections::HashMap<String, String>,
}

/// Status of a protocol session
#[derive(Debug, Clone)]
pub struct ProtocolStatus {
    /// Current state name
    pub current_state: String,
    /// Whether the protocol can be safely terminated
    pub can_terminate: bool,
    /// Whether the protocol is in a final state
    pub is_final: bool,
    /// Protocol progress indicator (0.0 to 1.0)
    pub progress: f64,
    /// Last activity timestamp
    pub last_activity: u64,
    /// Current participants
    pub participants: Vec<DeviceId>,
}

/// A typed channel for sending messages of a specific type
pub struct TypedSender<T: ChannelMessage> {
    inner: Arc<Mutex<VecDeque<T>>>,
}

impl<T: ChannelMessage> TypedSender<T> {
    /// Send a message through the channel
    pub fn send(&self, message: T) -> Result<(), ChannelError> {
        let mut queue = self.inner.lock().map_err(|_| ChannelError::Poisoned)?;
        queue.push_back(message);
        Ok(())
    }
    
    /// Try to send a message without blocking
    pub fn try_send(&self, message: T) -> Result<(), ChannelError> {
        // For this simple implementation, try_send is the same as send
        self.send(message)
    }
}

impl<T: ChannelMessage> Clone for TypedSender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// A typed channel for receiving messages of a specific type
pub struct TypedReceiver<T: ChannelMessage> {
    inner: Arc<Mutex<VecDeque<T>>>,
}

impl<T: ChannelMessage> TypedReceiver<T> {
    /// Receive the next message from the channel
    pub fn recv(&self) -> Result<T, ChannelError> {
        let mut queue = self.inner.lock().map_err(|_| ChannelError::Poisoned)?;
        queue.pop_front().ok_or(ChannelError::Empty)
    }
    
    /// Try to receive a message without blocking
    pub fn try_recv(&self) -> Result<T, ChannelError> {
        // For this simple implementation, try_recv is the same as recv
        self.recv()
    }
    
    /// Check if there are messages available
    pub fn is_empty(&self) -> bool {
        if let Ok(queue) = self.inner.lock() {
            queue.is_empty()
        } else {
            true
        }
    }
    
    /// Get the number of messages currently in the channel
    pub fn len(&self) -> usize {
        if let Ok(queue) = self.inner.lock() {
            queue.len()
        } else {
            0
        }
    }
}

/// Create a typed channel pair for communication
pub fn typed_channel<T: ChannelMessage>() -> (TypedSender<T>, TypedReceiver<T>) {
    let queue = Arc::new(Mutex::new(VecDeque::new()));
    
    let sender = TypedSender {
        inner: Arc::clone(&queue),
    };
    
    let receiver = TypedReceiver {
        inner: queue,
    };
    
    (sender, receiver)
}

/// A one-shot response channel for request-response patterns
#[derive(Debug)]
pub struct ResponseChannel<T> {
    inner: Arc<Mutex<Option<T>>>,
}

impl<T> ResponseChannel<T> {
    /// Create a new response channel
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }
    
    /// Send a response through the channel
    pub fn send(self, response: T) -> Result<(), T> {
        match self.inner.lock() {
            Ok(mut slot) => {
                if slot.is_some() {
                    return Err(response); // Already responded
                }
                *slot = Some(response);
                Ok(())
            }
            Err(_) => Err(response), // Lock poisoned
        }
    }
    
    /// Try to receive a response from the channel
    pub fn try_recv(&self) -> Result<T, ChannelError> {
        let mut slot = self.inner.lock().map_err(|_| ChannelError::Poisoned)?;
        slot.take().ok_or(ChannelError::Empty)
    }
}

impl<T> Clone for ResponseChannel<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> Default for ResponseChannel<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur during channel operations
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("Channel is empty")]
    Empty,
    
    #[error("Channel is closed")]
    Closed,
    
    #[error("Channel lock is poisoned")]
    Poisoned,
    
    #[error("Invalid message type")]
    InvalidType,
    
    #[error("Channel capacity exceeded")]
    Full,
}

/// A registry for managing multiple typed channels
pub struct ChannelRegistry {
    /// Command channels for different protocol types
    command_channels: std::collections::HashMap<SessionProtocolType, TypedSender<SessionCommand>>,
    
    /// Event channels for different subscribers
    event_channels: Vec<TypedSender<SessionEvent>>,
    
    /// Effect channels for different executors
    effect_channels: Vec<TypedSender<SessionEffect>>,
}

impl ChannelRegistry {
    /// Create a new channel registry
    pub fn new() -> Self {
        Self {
            command_channels: std::collections::HashMap::new(),
            event_channels: Vec::new(),
            effect_channels: Vec::new(),
        }
    }
    
    /// Register a command channel for a protocol type
    pub fn register_command_channel(&mut self, protocol_type: SessionProtocolType, sender: TypedSender<SessionCommand>) {
        self.command_channels.insert(protocol_type, sender);
    }
    
    /// Get a command channel for a protocol type
    pub fn get_command_channel(&self, protocol_type: &SessionProtocolType) -> Option<&TypedSender<SessionCommand>> {
        self.command_channels.get(protocol_type)
    }
    
    /// Register an event channel
    pub fn register_event_channel(&mut self, sender: TypedSender<SessionEvent>) {
        self.event_channels.push(sender);
    }
    
    /// Broadcast an event to all registered event channels
    pub fn broadcast_event(&self, event: SessionEvent) {
        for channel in &self.event_channels {
            let _ = channel.send(event.clone()); // Ignore send errors
        }
    }
    
    /// Register an effect channel
    pub fn register_effect_channel(&mut self, sender: TypedSender<SessionEffect>) {
        self.effect_channels.push(sender);
    }
    
    /// Send an effect to the first available effect channel
    pub fn send_effect(&self, effect: SessionEffect) -> Result<(), ChannelError> {
        if let Some(channel) = self.effect_channels.first() {
            channel.send(effect)
        } else {
            Err(ChannelError::Closed)
        }
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Utility for creating protocol-specific typed channels
pub struct ProtocolChannels {
    /// Commands sent to this protocol
    pub command_rx: TypedReceiver<SessionCommand>,
    
    /// Events emitted by this protocol
    pub event_tx: TypedSender<SessionEvent>,
    
    /// Effects requested by this protocol
    pub effect_tx: TypedSender<SessionEffect>,
}

impl ProtocolChannels {
    /// Create a new set of protocol channels
    pub fn new() -> (Self, TypedSender<SessionCommand>, TypedReceiver<SessionEvent>, TypedReceiver<SessionEffect>) {
        let (command_tx, command_rx) = typed_channel();
        let (event_tx, event_rx) = typed_channel();
        let (effect_tx, effect_rx) = typed_channel();
        
        let protocol_channels = Self {
            command_rx,
            event_tx,
            effect_tx,
        };
        
        (protocol_channels, command_tx, event_rx, effect_rx)
    }
}

impl Default for ProtocolChannels {
    fn default() -> Self {
        let (protocol_channels, _, _, _) = Self::new();
        protocol_channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_typed_channel_communication() {
        let (sender, receiver) = typed_channel::<SessionCommand>();
        
        let command = SessionCommand::Shutdown;
        sender.send(command.clone()).unwrap();
        
        let received = receiver.recv().unwrap();
        match received {
            SessionCommand::Shutdown => (),
            _ => panic!("Wrong command received"),
        }
    }
    
    #[test]
    fn test_response_channel() {
        let response_channel = ResponseChannel::new();
        let response_channel_clone = response_channel.clone();
        
        // Initially empty
        assert!(response_channel.try_recv().is_err());
        
        // Send response
        response_channel_clone.send("test response".to_string()).unwrap();
        
        // Receive response
        let received = response_channel.try_recv().unwrap();
        assert_eq!(received, "test response");
        
        // Channel is now empty again
        assert!(response_channel.try_recv().is_err());
    }
    
    #[test]
    fn test_channel_registry() {
        let mut registry = ChannelRegistry::new();
        let (sender, _receiver) = typed_channel();
        
        registry.register_command_channel(SessionProtocolType::DKD, sender);
        
        assert!(registry.get_command_channel(&SessionProtocolType::DKD).is_some());
        assert!(registry.get_command_channel(&SessionProtocolType::Recovery).is_none());
    }
    
    #[test]
    fn test_protocol_channels() {
        let (protocol_channels, command_tx, event_rx, _effect_rx) = ProtocolChannels::new();
        
        // Send command
        let command = SessionCommand::Shutdown;
        command_tx.send(command.clone()).unwrap();
        
        // Receive command
        let received_command = protocol_channels.command_rx.recv().unwrap();
        match received_command {
            SessionCommand::Shutdown => (),
            _ => panic!("Wrong command received"),
        }
        
        // Send event
        let event = SessionEvent::RuntimeError {
            session_id: None,
            error: "test error".to_string(),
            recoverable: true,
        };
        protocol_channels.event_tx.send(event.clone()).unwrap();
        
        // Receive event
        let received_event = event_rx.recv().unwrap();
        match received_event {
            SessionEvent::RuntimeError { error, .. } => {
                assert_eq!(error, "test error");
            }
            _ => panic!("Wrong event received"),
        }
    }
}