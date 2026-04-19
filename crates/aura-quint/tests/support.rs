#![allow(clippy::expect_used, dead_code)]

use std::path::PathBuf;

use aura_quint::BridgeBundleV1;
use serde::de::DeserializeOwned;

pub(crate) fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("bridge")
        .join(name)
}

pub(crate) fn load_fixture<T: DeserializeOwned>(name: &str) -> T {
    let payload = std::fs::read(fixture_path(name)).expect("read fixture");
    serde_json::from_slice(&payload).expect("decode fixture payload")
}

pub(crate) fn load_bundle_fixture(name: &str) -> BridgeBundleV1 {
    load_fixture(name)
}
