//! Storage effect handlers

pub mod memory;
pub mod filesystem;

pub use memory::MemoryStorageHandler;
pub use filesystem::FilesystemStorageHandler;