pub mod controllable_time;
mod manual_physical_clock;
#[cfg(not(target_arch = "wasm32"))]
mod quiescent_clock;
#[cfg(not(target_arch = "wasm32"))]
mod stall_watchdog;

pub use manual_physical_clock::ManualPhysicalClock;
#[cfg(not(target_arch = "wasm32"))]
pub use quiescent_clock::QuiescentClock;
#[cfg(not(target_arch = "wasm32"))]
pub use stall_watchdog::VirtualTimeStallWatchdog;

pub use controllable_time::{ControllableTimeSource, TimeScenario, TimeScenarioBuilder};
