//! AMP ratchet protocol contracts — window acceptance, epoch validation,
//! and cross-epoch replay prevention.

use aura_core::types::identifiers::{ChannelId, ContextId};
use aura_transport::amp::{derive_for_recv, derive_for_send, AmpHeader, AmpRatchetState};

/// Messages within 2× the skip window are accepted even if out-of-order —
/// the dual-window design allows for reordering during transit.
#[test]
fn dual_window_out_of_order_accepts_within_span() {
    let ctx = ContextId::new_from_entropy([1u8; 32]);
    let channel = ChannelId::from_bytes([2u8; 32]);
    let state = AmpRatchetState {
        chan_epoch: 0,
        last_checkpoint_gen: 0,
        skip_window: 4,
        pending_epoch: None,
    };

    let send0 = derive_for_send(ctx, channel, &state, 0).unwrap();
    assert_eq!(send0.header.ratchet_gen, 0);

    let header = AmpHeader {
        context: ctx,
        channel,
        chan_epoch: 0,
        ratchet_gen: 6, // within 2 * window (8)
    };

    assert!(derive_for_recv(&state, header).is_ok());
}
