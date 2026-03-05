#![allow(missing_docs)]

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

fn main() {
    let manifest_path = Path::new("Cargo.toml");
    let manifest = match fs::read_to_string(manifest_path) {
        Ok(value) => value,
        Err(error) => panic!("failed to read aura-ui Cargo.toml: {error}"),
    };

    let parsed: toml::Value = match toml::from_str(&manifest) {
        Ok(value) => value,
        Err(error) => panic!("failed to parse aura-ui Cargo.toml: {error}"),
    };

    let forbidden = ["web-sys", "js-sys", "wasm-bindgen", "gloo", "gloo-net"];
    let mut found = BTreeSet::new();

    for key in dependency_keys(&parsed) {
        if forbidden.contains(&key.as_str()) {
            found.insert(key);
        }
    }

    assert!(
        found.is_empty(),
        "aura-ui must remain platform-agnostic; move web-only deps to aura-web: {found:?}"
    );

    println!("cargo:rerun-if-changed=Cargo.toml");
}

fn dependency_keys(value: &toml::Value) -> Vec<String> {
    let mut keys = Vec::new();

    if let Some(table) = value.as_table() {
        for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(section_table) = table.get(section).and_then(toml::Value::as_table) {
                keys.extend(section_table.keys().cloned());
            }
        }

        if let Some(target_table) = table.get("target").and_then(toml::Value::as_table) {
            for target in target_table.values() {
                if let Some(target_section) = target.as_table() {
                    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
                        if let Some(section_table) =
                            target_section.get(section).and_then(toml::Value::as_table)
                        {
                            keys.extend(section_table.keys().cloned());
                        }
                    }
                }
            }
        }
    }

    keys
}
