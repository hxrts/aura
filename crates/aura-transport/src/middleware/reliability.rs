//! Reliability Middleware

use super::stack::TransportMiddleware;
use super::handler::{TransportHandler, TransportOperation, TransportResult, NetworkAddress};
use aura_protocol::effects::AuraEffects;
use aura_types::{MiddlewareContext, MiddlewareResult};
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone)]
pub struct ReliabilityConfig {
    pub max_retries: u32,
    pub initial_timeout_ms: u64,
    pub max_timeout_ms: u64,
    pub timeout_multiplier: f64,
    pub enable_acknowledgments: bool,
    pub acknowledgment_timeout_ms: u64,
    pub reorder_buffer_size: usize,
}

impl Default for ReliabilityConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_timeout_ms: 1000, // 1 second
            max_timeout_ms: 30000,    // 30 seconds
            timeout_multiplier: 2.0,
            enable_acknowledgments: true,
            acknowledgment_timeout_ms: 5000, // 5 seconds
            reorder_buffer_size: 100,
        }
    }
}

#[derive(Debug, Clone)]
struct PendingMessage {
    message_id: String,
    operation: TransportOperation,
    retry_count: u32,
    next_retry_time: u64,
    timeout_ms: u64,
}

impl PendingMessage {
    fn new(message_id: String, operation: TransportOperation, initial_timeout_ms: u64, current_time: u64) -> Self {
        Self {
            message_id,
            operation,
            retry_count: 0,
            next_retry_time: current_time + initial_timeout_ms,
            timeout_ms: initial_timeout_ms,
        }
    }
    
    fn should_retry(&self, current_time: u64) -> bool {
        current_time >= self.next_retry_time
    }
    
    fn increment_retry(&mut self, current_time: u64, multiplier: f64, max_timeout_ms: u64) {
        self.retry_count += 1;
        self.timeout_ms = ((self.timeout_ms as f64 * multiplier) as u64).min(max_timeout_ms);
        self.next_retry_time = current_time + self.timeout_ms;
    }
}

#[derive(Debug, Clone)]
struct BufferedMessage {
    sequence_number: u64,
    data: Vec<u8>,
    metadata: HashMap<String, String>,
    received_time: u64,
}

pub struct ReliabilityMiddleware {
    config: ReliabilityConfig,
    pending_messages: HashMap<String, PendingMessage>,
    next_message_id: u64,
    sequence_numbers: HashMap<NetworkAddress, u64>,
    reorder_buffers: HashMap<NetworkAddress, VecDeque<BufferedMessage>>,
    expected_sequence: HashMap<NetworkAddress, u64>,
    stats: ReliabilityStats,
}

#[derive(Debug, Default)]
struct ReliabilityStats {
    messages_sent: u64,
    messages_retried: u64,
    messages_failed: u64,
    messages_acknowledged: u64,
    messages_reordered: u64,
    total_retries: u64,
}

impl ReliabilityMiddleware {
    pub fn new() -> Self {
        Self {
            config: ReliabilityConfig::default(),
            pending_messages: HashMap::new(),
            next_message_id: 1,
            sequence_numbers: HashMap::new(),
            reorder_buffers: HashMap::new(),
            expected_sequence: HashMap::new(),
            stats: ReliabilityStats::default(),
        }
    }
    
    pub fn with_config(config: ReliabilityConfig) -> Self {
        Self {
            config,
            pending_messages: HashMap::new(),
            next_message_id: 1,
            sequence_numbers: HashMap::new(),
            reorder_buffers: HashMap::new(),
            expected_sequence: HashMap::new(),
            stats: ReliabilityStats::default(),
        }
    }
    
    fn generate_message_id(&mut self) -> String {
        let id = format!("msg_{}", self.next_message_id);
        self.next_message_id += 1;
        id
    }
    
    fn get_next_sequence_number(&mut self, address: &NetworkAddress) -> u64 {
        let seq = self.sequence_numbers.entry(address.clone()).or_insert(0);
        *seq += 1;
        *seq
    }
    
    fn add_reliability_metadata(&mut self, metadata: &mut HashMap<String, String>, address: &NetworkAddress) {
        let message_id = self.generate_message_id();
        let sequence_number = self.get_next_sequence_number(address);
        
        metadata.insert("message_id".to_string(), message_id);
        metadata.insert("sequence_number".to_string(), sequence_number.to_string());
        metadata.insert("reliable".to_string(), "true".to_string());
        
        if self.config.enable_acknowledgments {
            metadata.insert("requires_ack".to_string(), "true".to_string());
        }
    }
    
    fn is_acknowledgment(&self, metadata: &HashMap<String, String>) -> bool {
        metadata.get("message_type").map(|t| t == "ack").unwrap_or(false)
    }
    
    fn is_reliable_message(&self, metadata: &HashMap<String, String>) -> bool {
        metadata.get("reliable").map(|r| r == "true").unwrap_or(false)
    }
    
    fn handle_acknowledgment(&mut self, metadata: &HashMap<String, String>, effects: &dyn AuraEffects) {
        if let Some(ack_message_id) = metadata.get("ack_for") {
            if self.pending_messages.remove(ack_message_id).is_some() {
                self.stats.messages_acknowledged += 1;
                effects.log_info(
                    &format!("Received acknowledgment for message {}", ack_message_id),
                    &[]
                );
            }
        }
    }
    
    fn send_acknowledgment(&self, original_metadata: &HashMap<String, String>, source: &NetworkAddress, effects: &dyn AuraEffects, next: &mut dyn TransportHandler) -> MiddlewareResult<()> {
        if let Some(message_id) = original_metadata.get("message_id") {
            let mut ack_metadata = HashMap::new();
            ack_metadata.insert("message_type".to_string(), "ack".to_string());
            ack_metadata.insert("ack_for".to_string(), message_id.clone());
            
            let ack_operation = TransportOperation::Send {
                destination: source.clone(),
                data: b"ACK".to_vec(),
                metadata: ack_metadata,
            };
            
            let _ = next.execute(ack_operation, effects);
        }
        Ok(())
    }
    
    fn handle_reordering(&mut self, source: &NetworkAddress, data: Vec<u8>, metadata: HashMap<String, String>, current_time: u64) -> Option<Vec<TransportResult>> {
        if let Some(seq_str) = metadata.get("sequence_number") {
            if let Ok(sequence_number) = seq_str.parse::<u64>() {
                let expected = self.expected_sequence.entry(source.clone()).or_insert(1);
                
                if sequence_number == *expected {
                    // In-order message
                    *expected += 1;
                    let mut results = vec![TransportResult::Received {
                        source: source.clone(),
                        data,
                        metadata,
                    }];
                    
                    // Check if any buffered messages can now be delivered
                    let buffer = self.reorder_buffers.entry(source.clone()).or_insert_with(VecDeque::new);
                    while let Some(buffered) = buffer.iter().position(|msg| msg.sequence_number == *expected) {
                        let buffered_msg = buffer.remove(buffered).unwrap();
                        results.push(TransportResult::Received {
                            source: source.clone(),
                            data: buffered_msg.data,
                            metadata: buffered_msg.metadata,
                        });
                        *expected += 1;
                        self.stats.messages_reordered += 1;
                    }
                    
                    return Some(results);
                } else if sequence_number > *expected {
                    // Out-of-order message, buffer it
                    let buffer = self.reorder_buffers.entry(source.clone()).or_insert_with(VecDeque::new);
                    if buffer.len() < self.config.reorder_buffer_size {
                        buffer.push_back(BufferedMessage {
                            sequence_number,
                            data,
                            metadata,
                            received_time: current_time,
                        });
                        self.stats.messages_reordered += 1;
                    }
                    return Some(vec![]); // Don't deliver yet
                }
                // Duplicate or very old message, ignore
            }
        }
        
        // No sequence number, deliver as-is
        Some(vec![TransportResult::Received {
            source: source.clone(),
            data,
            metadata,
        }])
    }
    
    fn process_retries(&mut self, effects: &dyn AuraEffects, next: &mut dyn TransportHandler) {
        let current_time = effects.current_timestamp();
        let mut to_retry = Vec::new();
        let mut to_remove = Vec::new();
        
        for (message_id, pending) in &mut self.pending_messages {
            if pending.should_retry(current_time) {
                if pending.retry_count < self.config.max_retries {
                    to_retry.push((message_id.clone(), pending.operation.clone()));
                    pending.increment_retry(current_time, self.config.timeout_multiplier, self.config.max_timeout_ms);
                    self.stats.total_retries += 1;
                } else {
                    to_remove.push(message_id.clone());
                    self.stats.messages_failed += 1;
                }
            }
        }
        
        // Remove failed messages
        for message_id in to_remove {
            self.pending_messages.remove(&message_id);
            effects.log_error(
                &format!("Message {} failed after {} retries", message_id, self.config.max_retries),
                &[]
            );
        }
        
        // Retry messages
        for (message_id, operation) in to_retry {
            effects.log_info(
                &format!("Retrying message {}", message_id),
                &[]
            );
            self.stats.messages_retried += 1;
            let _ = next.execute(operation, effects);
        }
    }
}

impl Default for ReliabilityMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportMiddleware for ReliabilityMiddleware {
    fn process(
        &mut self,
        operation: TransportOperation,
        _context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn TransportHandler,
    ) -> MiddlewareResult<TransportResult> {
        let current_time = effects.current_timestamp();
        
        // Process pending retries
        self.process_retries(effects, next);
        
        match operation {
            TransportOperation::Send { destination, data, mut metadata } => {
                // Add reliability metadata
                self.add_reliability_metadata(&mut metadata, &destination);
                let message_id = metadata.get("message_id").unwrap().clone();
                
                // Create pending message for retry tracking
                let pending = PendingMessage::new(
                    message_id.clone(),
                    TransportOperation::Send {
                        destination: destination.clone(),
                        data: data.clone(),
                        metadata: metadata.clone(),
                    },
                    self.config.initial_timeout_ms,
                    current_time,
                );
                
                self.pending_messages.insert(message_id, pending);
                self.stats.messages_sent += 1;
                
                // Send the message
                next.execute(TransportOperation::Send {
                    destination,
                    data,
                    metadata,
                }, effects)
            }
            
            TransportOperation::Receive { source, timeout_ms } => {
                let result = next.execute(TransportOperation::Receive { source, timeout_ms }, effects)?;
                
                if let TransportResult::Received { source, data, metadata } = result {
                    // Handle acknowledgments
                    if self.is_acknowledgment(&metadata) {
                        self.handle_acknowledgment(&metadata, effects);
                        // Don't deliver ACK messages to upper layers
                        return Ok(TransportResult::Received {
                            source,
                            data: Vec::new(),
                            metadata: HashMap::new(),
                        });
                    }
                    
                    // Send acknowledgment if required
                    if self.config.enable_acknowledgments && 
                       metadata.get("requires_ack").map(|r| r == "true").unwrap_or(false) {
                        let _ = self.send_acknowledgment(&metadata, &source, effects, next);
                    }
                    
                    // Handle message reordering
                    if self.is_reliable_message(&metadata) {
                        if let Some(results) = self.handle_reordering(&source, data, metadata, current_time) {
                            // Return the first result, queue others for later delivery
                            if let Some(first_result) = results.into_iter().next() {
                                return Ok(first_result);
                            }
                        }
                        
                        // Message was buffered, return empty result
                        return Ok(TransportResult::Received {
                            source,
                            data: Vec::new(),
                            metadata: HashMap::new(),
                        });
                    }
                    
                    // Non-reliable message, deliver as-is
                    Ok(TransportResult::Received { source, data, metadata })
                } else {
                    Ok(result)
                }
            }
            
            _ => next.execute(operation, effects),
        }
    }
    
    fn middleware_name(&self) -> &'static str {
        "ReliabilityMiddleware"
    }
    
    fn middleware_info(&self) -> HashMap<String, String> {
        let mut info = HashMap::new();
        info.insert("max_retries".to_string(), self.config.max_retries.to_string());
        info.insert("initial_timeout_ms".to_string(), self.config.initial_timeout_ms.to_string());
        info.insert("enable_acknowledgments".to_string(), self.config.enable_acknowledgments.to_string());
        info.insert("pending_messages".to_string(), self.pending_messages.len().to_string());
        info.insert("messages_sent".to_string(), self.stats.messages_sent.to_string());
        info.insert("messages_retried".to_string(), self.stats.messages_retried.to_string());
        info.insert("messages_failed".to_string(), self.stats.messages_failed.to_string());
        info.insert("messages_acknowledged".to_string(), self.stats.messages_acknowledged.to_string());
        info.insert("messages_reordered".to_string(), self.stats.messages_reordered.to_string());
        info.insert("total_retries".to_string(), self.stats.total_retries.to_string());
        info
    }
}