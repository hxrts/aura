//! Regression tests for choreography protocol compatibility fixtures.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const BASELINE_PATH: &str = "../fixtures/protocol_compat/compatible_baseline.tell";
const CURRENT_PATH: &str = "../fixtures/protocol_compat/compatible_current.tell";
const BREAKING_BASELINE_PATH: &str = "../fixtures/protocol_compat/breaking_baseline.tell";
const BREAKING_CURRENT_PATH: &str = "../fixtures/protocol_compat/breaking_current.tell";
const INVITATION_EXCHANGE_PATH: &str =
    "../../aura-invitation/src/protocol.invitation_exchange.tell";

const BASELINE: &str = include_str!("../fixtures/protocol_compat/compatible_baseline.tell");
const CURRENT: &str = include_str!("../fixtures/protocol_compat/compatible_current.tell");
const BREAKING_BASELINE: &str = include_str!("../fixtures/protocol_compat/breaking_baseline.tell");
const BREAKING_CURRENT: &str = include_str!("../fixtures/protocol_compat/breaking_current.tell");
const INVITATION_EXCHANGE: &str =
    include_str!("../../aura-invitation/src/protocol.invitation_exchange.tell");

#[test]
fn compatible_protocol_fixture_pair_remains_async_subtype_compatible() {
    if let Err(error) = aura_testkit::check_async_subtype_for_shared_roles(BASELINE, CURRENT) {
        panic!(
            "expected `{CURRENT_PATH}` to remain async-subtype compatible with `{BASELINE_PATH}`: {error}"
        );
    }
}

#[test]
fn breaking_protocol_fixture_pair_is_not_async_subtype_compatible() {
    if aura_testkit::check_async_subtype_for_shared_roles(BREAKING_BASELINE, BREAKING_CURRENT)
        .is_ok()
    {
        panic!(
            "expected `{BREAKING_CURRENT_PATH}` to fail async-subtype compatibility with `{BREAKING_BASELINE_PATH}`"
        );
    }
}

#[test]
fn protocol_compat_fixtures_remain_theorem_pack_free() {
    for (path, source) in [
        (BASELINE_PATH, BASELINE),
        (CURRENT_PATH, CURRENT),
        (BREAKING_BASELINE_PATH, BREAKING_BASELINE),
        (BREAKING_CURRENT_PATH, BREAKING_CURRENT),
    ] {
        assert!(
            !source.contains("proof_bundle"),
            "protocol-compat fixture must stay theorem-pack-free: {path}"
        );
        assert!(
            !source.contains("requires Aura"),
            "protocol-compat fixture must not require Aura theorem packs: {path}"
        );
    }
}

#[test]
fn protocol_compat_plain_invitation_exchange_remains_theorem_pack_free() {
    assert!(
        !INVITATION_EXCHANGE.contains("proof_bundle"),
        "plain invitation exchange must stay theorem-pack-free: {INVITATION_EXCHANGE_PATH}"
    );
    assert!(
        !INVITATION_EXCHANGE.contains("requires Aura"),
        "plain invitation exchange must not require Aura theorem packs: {INVITATION_EXCHANGE_PATH}"
    );
}

#[test]
#[ignore = "invoked by scripts/check/protocol-compat.sh with explicit file paths"]
fn protocol_compat_pair_from_env() {
    let baseline_path = env::var("AURA_PROTOCOL_COMPAT_BASELINE")
        .unwrap_or_else(|_| panic!("missing AURA_PROTOCOL_COMPAT_BASELINE"));
    let current_path = env::var("AURA_PROTOCOL_COMPAT_CURRENT")
        .unwrap_or_else(|_| panic!("missing AURA_PROTOCOL_COMPAT_CURRENT"));
    let baseline = fs::read_to_string(resolve_protocol_compat_path(&baseline_path))
        .unwrap_or_else(|error| panic!("failed to read {baseline_path}: {error}"));
    let current = fs::read_to_string(resolve_protocol_compat_path(&current_path))
        .unwrap_or_else(|error| panic!("failed to read {current_path}: {error}"));

    if let Err(error) = aura_testkit::check_async_subtype_for_shared_roles(&baseline, &current) {
        panic!(
            "expected `{current_path}` to remain async-subtype compatible with `{baseline_path}`: {error}"
        );
    }
}

fn resolve_protocol_compat_path(raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() || path.exists() {
        return path.to_path_buf();
    }

    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}
