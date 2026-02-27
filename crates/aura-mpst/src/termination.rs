//! Termination-weight helpers for choreography runtime budgeting.

use std::collections::BTreeMap;
use telltale_types::LocalTypeR;

/// Snapshot of in-flight per-edge message buffers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionBufferSnapshot {
    /// Buffered message counts keyed by `(from_role, to_role)`.
    pub directed_buffers: BTreeMap<(String, String), u64>,
}

impl SessionBufferSnapshot {
    /// Create an empty buffer snapshot.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one directed edge buffer size.
    #[must_use]
    pub fn with_buffer(
        mut self,
        from_role: impl Into<String>,
        to_role: impl Into<String>,
        buffered_messages: u64,
    ) -> Self {
        self.directed_buffers
            .insert((from_role.into(), to_role.into()), buffered_messages);
        self
    }
}

/// Compute local-type depth used in weighted termination budgets.
#[must_use]
pub fn compute_depth(local_type: &LocalTypeR) -> u64 {
    local_type.depth() as u64
}

/// Compute total in-flight buffer contribution for one session snapshot.
#[must_use]
pub fn compute_buffer_weight(session: &SessionBufferSnapshot) -> u64 {
    session.directed_buffers.values().copied().sum()
}

/// Compute the weighted measure: `W = 2 * sum(depths) + sum(buffer_sizes)`.
#[must_use]
pub fn compute_weighted_measure(
    local_types: &[LocalTypeR],
    session: &SessionBufferSnapshot,
) -> u64 {
    let depth_sum: u64 = local_types.iter().map(compute_depth).sum();
    depth_sum
        .saturating_mul(2)
        .saturating_add(compute_buffer_weight(session))
}

#[cfg(test)]
mod tests {
    use super::*;
    use telltale_types::Label;

    fn simple_send_recv() -> LocalTypeR {
        LocalTypeR::send(
            "B",
            Label::new("m1"),
            LocalTypeR::recv("B", Label::new("ack"), LocalTypeR::End),
        )
    }

    #[test]
    fn depth_uses_telltale_local_type_depth() {
        let local = simple_send_recv();
        assert_eq!(local.depth(), 2);
        assert_eq!(compute_depth(&local), 2);
    }

    #[test]
    fn buffer_weight_sums_all_edges() {
        let session = SessionBufferSnapshot::new()
            .with_buffer("A", "B", 3)
            .with_buffer("B", "A", 4);
        assert_eq!(compute_buffer_weight(&session), 7);
    }

    #[test]
    fn weighted_measure_matches_formula() {
        let local_a = simple_send_recv(); // depth = 2
        let local_b = LocalTypeR::recv("A", Label::new("m2"), LocalTypeR::End); // depth = 1
        let session = SessionBufferSnapshot::new()
            .with_buffer("A", "B", 2)
            .with_buffer("B", "A", 1);

        // W = 2 * (2 + 1) + (2 + 1) = 9
        assert_eq!(compute_weighted_measure(&[local_a, local_b], &session), 9);
    }
}
