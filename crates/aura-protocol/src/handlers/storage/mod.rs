//! Storage effect handlers

pub mod filesystem;
pub mod memory;

pub use filesystem::FilesystemStorageHandler;
pub use memory::MemoryStorageHandler;
