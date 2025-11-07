//! Console effect handlers

pub mod silent;
pub mod stdout;
pub mod structured;

pub use silent::SilentConsoleHandler;
pub use stdout::StdoutConsoleHandler;
pub use structured::StructuredConsoleHandler;
