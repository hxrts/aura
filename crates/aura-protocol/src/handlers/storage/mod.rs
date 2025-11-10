//! Storage effect handlers

#[cfg(feature = "aura-store")]
pub mod encrypted;
pub mod filesystem;
pub mod memory;

#[cfg(feature = "aura-store")]
pub use encrypted::EncryptedStorageHandler;
pub use filesystem::FilesystemStorageHandler;
pub use memory::MemoryStorageHandler;
