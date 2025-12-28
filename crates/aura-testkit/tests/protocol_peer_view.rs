#![allow(missing_docs)]
use aura_core::effects::RandomExtendedEffects;
use aura_core::{Bottom, JoinSemilattice};
use aura_protocol::state::peer_view::PeerView;
use aura_testkit::stateful_effects::random::MockRandomHandler;

/// Create a deterministic random handler for tests
fn test_random_handler() -> MockRandomHandler {
    MockRandomHandler::new_with_seed(42)
}

#[tokio::test]
async fn test_add_peer() {
    let mut view = PeerView::new();
    let random = test_random_handler();
    let peer = random.random_uuid().await;

    view.add_peer(peer);
    assert!(view.contains(&peer));
    assert_eq!(view.len(), 1);
}

#[tokio::test]
async fn test_add_peer_idempotent() {
    let mut view = PeerView::new();
    let random = test_random_handler();
    let peer = random.random_uuid().await;

    view.add_peer(peer);
    view.add_peer(peer); // Add twice
    assert_eq!(view.len(), 1); // Still only one
}

#[tokio::test]
async fn test_join_associative() {
    let random = test_random_handler();
    let peer_a = random.random_uuid().await;
    let peer_b = random.random_uuid().await;
    let peer_c = random.random_uuid().await;

    let view_a = PeerView::from_peers(vec![peer_a]);
    let view_b = PeerView::from_peers(vec![peer_b]);
    let view_c = PeerView::from_peers(vec![peer_c]);

    let left = view_a.join(&view_b).join(&view_c);
    let right = view_a.join(&view_b.join(&view_c));

    assert_eq!(left, right);
}

#[tokio::test]
async fn test_join_commutative() {
    let random = test_random_handler();
    let peer_a = random.random_uuid().await;
    let peer_b = random.random_uuid().await;

    let view_a = PeerView::from_peers(vec![peer_a]);
    let view_b = PeerView::from_peers(vec![peer_b]);

    assert_eq!(view_a.join(&view_b), view_b.join(&view_a));
}

#[tokio::test]
async fn test_join_idempotent() {
    let random = test_random_handler();
    let peer = random.random_uuid().await;
    let view = PeerView::from_peers(vec![peer]);

    assert_eq!(view.join(&view), view);
}

#[test]
fn test_bottom() {
    let view = PeerView::bottom();
    assert!(view.is_empty());
    assert_eq!(view.len(), 0);
}

#[tokio::test]
async fn test_join_with_bottom() {
    let random = test_random_handler();
    let peer = random.random_uuid().await;
    let view = PeerView::from_peers(vec![peer]);
    let bottom = PeerView::bottom();

    // Join with bottom should be identity
    assert_eq!(view.join(&bottom), view);
    assert_eq!(bottom.join(&view), view);
}
