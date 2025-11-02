//! Protocol adapters for different interfaces

pub mod choreographic;
pub mod protocol;

pub use choreographic::ChoreographicAdapter;
pub use protocol::ProtocolAdapter;
