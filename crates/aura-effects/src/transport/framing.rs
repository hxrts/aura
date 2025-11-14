//! Message Framing Handler
//!
//! Stateless message framing and serialization utilities.
//! NO choreography - single-party effect handler only.
//! Target: <200 lines, use serde ecosystem.

use super::{TransportError, TransportResult};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Message framing handler
#[derive(Debug, Clone)]
pub struct FramingHandler {
    max_frame_size: usize,
}

/// Frame header with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameHeader {
    /// Frame type identifier
    pub frame_type: FrameType,
    /// Payload length in bytes
    pub payload_length: u32,
    /// Frame flags
    pub flags: u8,
    /// Frame sequence number
    pub sequence: u64,
}

/// Frame type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameType {
    /// Data frame
    Data = 0,
    /// Control frame
    Control = 1,
    /// Heartbeat frame
    Heartbeat = 2,
    /// Error frame
    Error = 3,
}

/// Complete frame with header and payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub header: FrameHeader,
    pub payload: Vec<u8>,
}

const FRAME_HEADER_SIZE: usize = 17; // 1 + 4 + 1 + 8 + 3 padding

impl FramingHandler {
    /// Create new framing handler
    pub fn new(max_frame_size: usize) -> Self {
        Self { max_frame_size }
    }

    /// Create with default configuration (1MB max frame)
    pub fn default() -> Self {
        Self::new(1024 * 1024)
    }

    /// Serialize frame to bytes
    pub fn serialize_frame(&self, frame: &Frame) -> TransportResult<Vec<u8>> {
        if frame.payload.len() > self.max_frame_size {
            return Err(TransportError::Protocol(format!(
                "Frame too large: {} > {}",
                frame.payload.len(),
                self.max_frame_size
            )));
        }

        let mut buffer = Vec::with_capacity(FRAME_HEADER_SIZE + frame.payload.len());
        
        // Serialize header
        buffer.push(frame.header.frame_type as u8);
        buffer.extend_from_slice(&frame.header.payload_length.to_be_bytes());
        buffer.push(frame.header.flags);
        buffer.extend_from_slice(&frame.header.sequence.to_be_bytes());
        
        // Add payload
        buffer.extend_from_slice(&frame.payload);
        
        Ok(buffer)
    }

    /// Deserialize frame from bytes
    pub fn deserialize_frame(&self, data: &[u8]) -> TransportResult<Frame> {
        if data.len() < FRAME_HEADER_SIZE {
            return Err(TransportError::Protocol(
                "Insufficient data for frame header".to_string()
            ));
        }

        let mut cursor = 0;
        
        // Parse header
        let frame_type = match data[cursor] {
            0 => FrameType::Data,
            1 => FrameType::Control,
            2 => FrameType::Heartbeat,
            3 => FrameType::Error,
            other => return Err(TransportError::Protocol(
                format!("Invalid frame type: {}", other)
            )),
        };
        cursor += 1;
        
        let payload_length = u32::from_be_bytes([
            data[cursor], data[cursor + 1], data[cursor + 2], data[cursor + 3]
        ]);
        cursor += 4;
        
        let flags = data[cursor];
        cursor += 1;
        
        let sequence = u64::from_be_bytes([
            data[cursor], data[cursor + 1], data[cursor + 2], data[cursor + 3],
            data[cursor + 4], data[cursor + 5], data[cursor + 6], data[cursor + 7],
        ]);
        cursor += 8;
        
        let header = FrameHeader {
            frame_type,
            payload_length,
            flags,
            sequence,
        };
        
        // Validate payload length
        if payload_length as usize > self.max_frame_size {
            return Err(TransportError::Protocol(format!(
                "Payload too large: {} > {}",
                payload_length, self.max_frame_size
            )));
        }
        
        let expected_total_size = FRAME_HEADER_SIZE + payload_length as usize;
        if data.len() < expected_total_size {
            return Err(TransportError::Protocol(
                "Insufficient data for frame payload".to_string()
            ));
        }
        
        // Extract payload
        let payload = data[FRAME_HEADER_SIZE..expected_total_size].to_vec();
        
        Ok(Frame { header, payload })
    }

    /// Send framed message async
    pub async fn send_frame<W>(&self, writer: &mut W, frame: &Frame) -> TransportResult<()>
    where
        W: AsyncWrite + Unpin,
    {
        let data = self.serialize_frame(frame)?;
        writer.write_all(&data).await.map_err(TransportError::Io)?;
        writer.flush().await.map_err(TransportError::Io)?;
        Ok(())
    }

    /// Receive framed message async
    pub async fn receive_frame<R>(&self, reader: &mut R) -> TransportResult<Frame>
    where
        R: AsyncRead + Unpin,
    {
        // Read header first
        let mut header_bytes = [0u8; FRAME_HEADER_SIZE];
        reader.read_exact(&mut header_bytes).await.map_err(TransportError::Io)?;
        
        // Parse payload length from header
        let payload_length = u32::from_be_bytes([
            header_bytes[1], header_bytes[2], header_bytes[3], header_bytes[4]
        ]) as usize;
        
        if payload_length > self.max_frame_size {
            return Err(TransportError::Protocol(format!(
                "Payload too large: {} > {}",
                payload_length, self.max_frame_size
            )));
        }
        
        // Read payload
        let mut payload_bytes = vec![0u8; payload_length];
        reader.read_exact(&mut payload_bytes).await.map_err(TransportError::Io)?;
        
        // Combine and deserialize
        let mut frame_data = Vec::with_capacity(FRAME_HEADER_SIZE + payload_length);
        frame_data.extend_from_slice(&header_bytes);
        frame_data.extend_from_slice(&payload_bytes);
        
        self.deserialize_frame(&frame_data)
    }

    /// Create data frame
    pub fn create_data_frame(&self, payload: Vec<u8>, sequence: u64) -> Frame {
        Frame {
            header: FrameHeader {
                frame_type: FrameType::Data,
                payload_length: payload.len() as u32,
                flags: 0,
                sequence,
            },
            payload,
        }
    }

    /// Create control frame
    pub fn create_control_frame(&self, payload: Vec<u8>, sequence: u64) -> Frame {
        Frame {
            header: FrameHeader {
                frame_type: FrameType::Control,
                payload_length: payload.len() as u32,
                flags: 0,
                sequence,
            },
            payload,
        }
    }

    /// Create heartbeat frame
    pub fn create_heartbeat_frame(&self, sequence: u64) -> Frame {
        Frame {
            header: FrameHeader {
                frame_type: FrameType::Heartbeat,
                payload_length: 0,
                flags: 0,
                sequence,
            },
            payload: Vec::new(),
        }
    }
}

/// JSON message serialization helpers
impl FramingHandler {
    /// Serialize message as JSON frame
    pub fn serialize_json<T: Serialize>(&self, message: &T, sequence: u64) -> TransportResult<Frame> {
        let payload = serde_json::to_vec(message)?;
        Ok(self.create_data_frame(payload, sequence))
    }

    /// Deserialize JSON frame to message
    pub fn deserialize_json<T: for<'de> Deserialize<'de>>(&self, frame: &Frame) -> TransportResult<T> {
        if frame.header.frame_type != FrameType::Data {
            return Err(TransportError::Protocol(
                "Expected data frame for JSON deserialization".to_string()
            ));
        }
        
        let message = serde_json::from_slice(&frame.payload)?;
        Ok(message)
    }
}
