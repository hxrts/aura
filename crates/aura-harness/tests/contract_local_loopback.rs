#![allow(missing_docs)]

use std::path::{Path, PathBuf};

use aura_harness::{build_startup_summary, load_and_validate_run_config};

#[test]
fn local_loopback_sample_config_is_valid() {
    let path = sample_run_config_path();
    let config = match load_and_validate_run_config(path) {
        Ok(config) => config,
        Err(error) => panic!("sample config must validate: {error}"),
    };

    assert_eq!(config.run.name, "local-loopback-smoke");
    assert_eq!(config.instances.len(), 2);
}

#[test]
fn startup_summary_contains_all_instances() {
    let path = sample_run_config_path();
    let config = match load_and_validate_run_config(path) {
        Ok(config) => config,
        Err(error) => panic!("sample config must validate: {error}"),
    };
    let summary = build_startup_summary(&config);

    assert_eq!(summary.instance_count, 2);
    assert!(summary
        .instances
        .iter()
        .any(|instance| instance.id == "alice"));
    assert!(summary
        .instances
        .iter()
        .any(|instance| instance.id == "bob"));
}

fn sample_run_config_path() -> &'static Path {
    static PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("configs")
            .join("harness")
            .join("local-loopback.toml")
    })
}
