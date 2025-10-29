//! SSB (Social Backup and Broadcasting) Protocol Suite
//!
//! This module contains the implementation of the SSB protocol family,
//! including gossip, publisher, and recognizer components.
//!
//! ## Components
//!
//! - `gossip` - SSB gossip protocol with neighbor management
//! - `publisher` - Envelope publishing with encryption and routing tags
//! - `recognizer` - Envelope recognition with replay detection
//!
//! ## Protocol Overview
//!
//! The SSB protocol provides decentralized message broadcasting through
//! a gossip network. Messages are encrypted and tagged for efficient
//! recognition by intended recipients without revealing sender/receiver
//! relationships to network observers.

pub mod gossip;
pub mod publisher;
pub mod recognizer;

// Re-export SSB protocol components
pub use gossip::*;
pub use publisher::*;
pub use recognizer::*;
