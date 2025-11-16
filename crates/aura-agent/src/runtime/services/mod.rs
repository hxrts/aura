//! State management services for the stateless effect system
//!
//! These services provide isolated state management with consistent
//! lock ordering and atomic operations.

pub mod budget_manager;
pub mod context_manager;
pub mod receipt_manager;

#[cfg(any(test, feature = "testing"))]
pub mod sync_context_manager;

pub use budget_manager::{BudgetKey, FlowBudgetManager};
pub use context_manager::ContextManager;
pub use receipt_manager::{ReceiptChain, ReceiptManager};

#[cfg(any(test, feature = "testing"))]
pub use sync_context_manager::SyncContextManager;
