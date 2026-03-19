//! Wire protocol stability and correctness tests.
//!
//! If any test here fails, peers cannot communicate reliably — envelope
//! formats diverge, ratchet windows reject valid messages, or context
//! isolation is breached.

#[allow(clippy::expect_used, missing_docs)]
#[path = "wire/envelope_roundtrip.rs"]
mod envelope_roundtrip;

#[path = "wire/amp_ratchet.rs"]
mod amp_ratchet;

#[path = "wire/context_isolation.rs"]
mod context_isolation;
