//! Message Framing Handler
//!
//! Stateless message framing and serialization utilities.

use super::{TransportError, TransportResult};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Message framing handler
#[derive(Debug, Clone)]
pub struct FramingHandler {
    max_frame_size: u32,
}

/// Frame header with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameHeader {
    /// Frame type identifier
    pub frame_type: FrameType,
    /// Payload length in bytes
    payload_length: u32,
    /// Frame flags
    pub flags: u8,
    /// Frame sequence number
    pub sequence: u64,
}

impl FrameHeader {
    /// Payload length declared by the wire header.
    pub fn payload_length(&self) -> u32 {
        self.payload_length
    }
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
    /// Frame header containing type, length, and metadata
    pub header: FrameHeader,
    /// Message payload
    pub payload: Vec<u8>,
}

/// Borrowed frame view backed by a reusable receive buffer.
#[derive(Debug)]
pub struct BufferedFrame<'a> {
    /// Frame header containing type, length, and metadata.
    pub header: FrameHeader,
    /// Borrowed payload bytes stored in the reusable receive buffer.
    pub payload: &'a [u8],
}

impl BufferedFrame<'_> {
    /// Convert the buffered view into an owned frame.
    pub fn into_owned(self) -> Frame {
        Frame {
            header: self.header,
            payload: self.payload.to_vec(),
        }
    }
}

/// Reusable inbound frame buffer with a per-connection memory budget.
#[derive(Debug, Clone)]
pub struct FrameReceiveBuffer {
    max_buffered_bytes: usize,
    payload: Vec<u8>,
}

impl FrameReceiveBuffer {
    /// Create a buffer with a fixed maximum inbound byte budget.
    pub fn new(max_buffered_bytes: usize) -> Self {
        Self {
            max_buffered_bytes,
            payload: Vec::new(),
        }
    }

    fn prepare(&mut self, payload_length: usize) -> TransportResult<&mut [u8]> {
        let required = FRAME_HEADER_SIZE
            .checked_add(payload_length)
            .ok_or_else(|| TransportError::Protocol("Frame size overflow".to_string()))?;
        if required > self.max_buffered_bytes {
            return Err(TransportError::Protocol(format!(
                "Inbound frame exceeds buffered byte budget: {} > {}",
                required, self.max_buffered_bytes
            )));
        }

        self.payload.resize(payload_length, 0);
        Ok(self.payload.as_mut_slice())
    }

    fn payload(&self, payload_length: usize) -> &[u8] {
        &self.payload[..payload_length]
    }
}

const FRAME_HEADER_SIZE: usize = 14; // 1 + 4 + 1 + 8 bytes on the wire

impl FramingHandler {
    /// Create new framing handler
    pub fn new(max_frame_size: u32) -> Self {
        Self { max_frame_size }
    }

    /// Create with default configuration (1MB max frame)
    #[allow(clippy::should_implement_trait)] // Method provides default config, not implementing Default trait
    pub fn default() -> Self {
        Self::new(1_048_576)
    }

    fn default_receive_buffer(&self) -> FrameReceiveBuffer {
        FrameReceiveBuffer::new(FRAME_HEADER_SIZE + self.max_frame_size as usize)
    }

    fn parse_header(&self, header_bytes: &[u8; FRAME_HEADER_SIZE]) -> TransportResult<FrameHeader> {
        let frame_type = match header_bytes[0] {
            0 => FrameType::Data,
            1 => FrameType::Control,
            2 => FrameType::Heartbeat,
            3 => FrameType::Error,
            other => {
                return Err(TransportError::Protocol(format!(
                    "Invalid frame type: {other}"
                )))
            }
        };

        let payload_length = u32::from_be_bytes([
            header_bytes[1],
            header_bytes[2],
            header_bytes[3],
            header_bytes[4],
        ]);
        if payload_length > self.max_frame_size {
            return Err(TransportError::Protocol(format!(
                "Payload too large: {} > {}",
                payload_length, self.max_frame_size
            )));
        }

        let flags = header_bytes[5];
        let sequence = u64::from_be_bytes([
            header_bytes[6],
            header_bytes[7],
            header_bytes[8],
            header_bytes[9],
            header_bytes[10],
            header_bytes[11],
            header_bytes[12],
            header_bytes[13],
        ]);

        Ok(FrameHeader {
            frame_type,
            payload_length,
            flags,
            sequence,
        })
    }

    /// Serialize frame to bytes
    pub fn serialize_frame(&self, frame: &Frame) -> TransportResult<Vec<u8>> {
        let payload_length = u32::try_from(frame.payload.len()).map_err(|_| {
            TransportError::Protocol(format!(
                "Frame too large: {} > {}",
                frame.payload.len(),
                self.max_frame_size
            ))
        })?;
        if (frame.payload.len() as u32) > self.max_frame_size {
            return Err(TransportError::Protocol(format!(
                "Frame too large: {} > {}",
                frame.payload.len(),
                self.max_frame_size
            )));
        }
        if frame.header.payload_length as usize != frame.payload.len() {
            return Err(TransportError::Protocol(
                "Frame header payload length does not match payload".to_string(),
            ));
        }

        let mut buffer = Vec::with_capacity(FRAME_HEADER_SIZE + frame.payload.len());

        // Serialize header
        buffer.push(frame.header.frame_type as u8);
        buffer.extend_from_slice(&payload_length.to_be_bytes());
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
                "Insufficient data for frame header".to_string(),
            ));
        }
        let header_bytes: [u8; FRAME_HEADER_SIZE] = data[..FRAME_HEADER_SIZE]
            .try_into()
            .map_err(|_| TransportError::Protocol("Invalid frame header".to_string()))?;
        let header = self.parse_header(&header_bytes)?;
        let payload_length = header.payload_length;

        // Validate payload length
        let expected_total_size = FRAME_HEADER_SIZE + payload_length as usize;
        if data.len() != expected_total_size {
            return Err(TransportError::Protocol(
                "Frame payload length is not canonical".to_string(),
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
        let mut receive_buffer = self.default_receive_buffer();
        self.receive_frame_buffered(reader, &mut receive_buffer)
            .await
            .map(BufferedFrame::into_owned)
    }

    /// Receive a framed message into a reusable bounded inbound buffer.
    pub async fn receive_frame_buffered<'a, R>(
        &self,
        reader: &mut R,
        receive_buffer: &'a mut FrameReceiveBuffer,
    ) -> TransportResult<BufferedFrame<'a>>
    where
        R: AsyncRead + Unpin,
    {
        // Read header first
        let mut header_bytes = [0u8; FRAME_HEADER_SIZE];
        reader
            .read_exact(&mut header_bytes)
            .await
            .map_err(TransportError::Io)?;

        let header = self.parse_header(&header_bytes)?;
        let payload_length_usize = header.payload_length as usize;
        // Read payload
        let payload_bytes = receive_buffer.prepare(payload_length_usize)?;
        reader
            .read_exact(payload_bytes)
            .await
            .map_err(TransportError::Io)?;

        Ok(BufferedFrame {
            header,
            payload: receive_buffer.payload(payload_length_usize),
        })
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
    pub fn serialize_json<T: Serialize>(
        &self,
        message: &T,
        sequence: u64,
    ) -> TransportResult<Frame> {
        let payload = serde_json::to_vec(message)?;
        Ok(self.create_data_frame(payload, sequence))
    }

    /// Deserialize JSON frame to message
    pub fn deserialize_json<T: for<'de> Deserialize<'de>>(
        &self,
        frame: &Frame,
    ) -> TransportResult<T> {
        if frame.header.frame_type != FrameType::Data {
            return Err(TransportError::Protocol(
                "Expected data frame for JSON deserialization".to_string(),
            ));
        }

        let message = serde_json::from_slice(&frame.payload)?;
        Ok(message)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn receive_frame_buffered_reuses_bounded_buffer_across_frames() {
        let handler = FramingHandler::new(128);
        let first = handler
            .serialize_frame(&handler.create_data_frame(vec![1; 32], 1))
            .unwrap();
        let second = handler
            .serialize_frame(&handler.create_data_frame(vec![2; 48], 2))
            .unwrap();
        let data = [first, second].concat();
        let mut stream = BufReader::new(data.as_slice());
        let mut buffer = FrameReceiveBuffer::new(FRAME_HEADER_SIZE + 64);

        {
            let first_frame = handler
                .receive_frame_buffered(&mut stream, &mut buffer)
                .await
                .expect("first frame within buffered budget");
            assert_eq!(first_frame.header.sequence, 1);
            assert_eq!(first_frame.payload.len(), 32);
        }

        let second_frame = handler
            .receive_frame_buffered(&mut stream, &mut buffer)
            .await
            .expect("second frame within buffered budget");
        assert_eq!(second_frame.header.sequence, 2);
        assert_eq!(second_frame.payload.len(), 48);
    }

    #[tokio::test]
    async fn receive_frame_buffered_rejects_frames_above_buffer_budget() {
        let handler = FramingHandler::new(128);
        let frame = handler
            .serialize_frame(&handler.create_data_frame(vec![9; 48], 7))
            .unwrap();
        let mut stream = BufReader::new(frame.as_slice());
        let mut buffer = FrameReceiveBuffer::new(FRAME_HEADER_SIZE + 32);

        let error = handler
            .receive_frame_buffered(&mut stream, &mut buffer)
            .await
            .expect_err("payload above buffered budget must fail");

        assert!(matches!(error, TransportError::Protocol(_)));
    }

    #[test]
    fn helper_constructors_set_canonical_payload_lengths() {
        let handler = FramingHandler::new(128);
        let data = handler.create_data_frame(vec![1, 2, 3], 1);
        let control = handler.create_control_frame(vec![4, 5], 2);
        let heartbeat = handler.create_heartbeat_frame(3);

        assert_eq!(data.header.payload_length(), 3);
        assert_eq!(control.header.payload_length(), 2);
        assert_eq!(heartbeat.header.payload_length(), 0);
    }

    #[test]
    fn serialize_frame_rejects_shorter_advertised_payload_length() {
        let handler = FramingHandler::new(128);
        let frame = Frame {
            header: FrameHeader {
                frame_type: FrameType::Data,
                payload_length: 2,
                flags: 0,
                sequence: 9,
            },
            payload: vec![1, 2, 3],
        };

        let error = handler
            .serialize_frame(&frame)
            .expect_err("shorter advertised payload length must fail");
        assert!(matches!(error, TransportError::Protocol(_)));
    }

    #[test]
    fn serialize_frame_rejects_longer_advertised_payload_length() {
        let handler = FramingHandler::new(128);
        let frame = Frame {
            header: FrameHeader {
                frame_type: FrameType::Data,
                payload_length: 4,
                flags: 0,
                sequence: 10,
            },
            payload: vec![1, 2, 3],
        };

        let error = handler
            .serialize_frame(&frame)
            .expect_err("longer advertised payload length must fail");
        assert!(matches!(error, TransportError::Protocol(_)));
    }

    #[test]
    fn deserialize_frame_rejects_trailing_byte_smuggling() {
        let handler = FramingHandler::new(128);
        let mut encoded = handler
            .serialize_frame(&handler.create_data_frame(vec![1, 2, 3], 11))
            .unwrap();
        encoded.extend_from_slice(&[99, 100]);

        let error = handler
            .deserialize_frame(&encoded)
            .expect_err("trailing bytes must fail canonical decode");
        assert!(matches!(error, TransportError::Protocol(_)));
    }

    #[test]
    fn deserialize_frame_rejects_concatenated_frames_as_noncanonical_single_buffer() {
        let handler = FramingHandler::new(128);
        let first = handler
            .serialize_frame(&handler.create_data_frame(vec![1, 2, 3], 12))
            .unwrap();
        let second = handler
            .serialize_frame(&handler.create_data_frame(vec![4, 5, 6], 13))
            .unwrap();
        let concatenated = [first, second].concat();

        let error = handler
            .deserialize_frame(&concatenated)
            .expect_err("concatenated frames must fail single-buffer decode");
        assert!(matches!(error, TransportError::Protocol(_)));
    }
}
