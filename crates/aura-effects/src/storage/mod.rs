//! Storage effect handlers

// NOTE: Encrypted storage creates circular dependency through aura-store
// It belongs in a higher layer (aura-protocol or application crates)
// pub mod encrypted;
pub mod filesystem;
pub mod memory;

// pub use encrypted::EncryptedStorageHandler;
pub use filesystem::FilesystemStorageHandler;
pub use memory::MemoryStorageHandler;
