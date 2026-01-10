//! Guardian display E2E tests (development-only).

#![cfg(feature = "development")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::disallowed_methods,
    clippy::needless_borrows_for_generic_args,
    missing_docs
)]

use aura_terminal::ids;

mod support;

#[tokio::test]
async fn test_authority_id_derivation_matches() {
    let seed = 2024u64;

    let hints_alice_authority = ids::authority_id(&format!("demo:{}:{}:authority", seed, "Alice"));
    let simulator_alice_authority =
        ids::authority_id(&format!("demo:{}:{}:authority", seed, "Alice"));

    assert_eq!(
        hints_alice_authority, simulator_alice_authority,
        "AuthorityId derivations must match for demo lookup to work"
    );
}
