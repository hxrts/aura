//! Phase 2 regression tests.

#![allow(missing_docs)]

use std::path::Path;

use aura_harness::coordinator::HarnessCoordinator;
use aura_harness::routing::AddressResolver;
use aura_harness::tool_api::ToolApi;

#[test]
fn mixed_local_and_ssh_config_starts_with_dry_run_ssh_backend() {
    let config_path = sample_mixed_config_path();
    let mut run_config = match aura_harness::load_and_validate_run_config(config_path) {
        Ok(config) => config,
        Err(error) => panic!("mixed config must validate: {error}"),
    };
    for instance in &mut run_config.instances {
        if matches!(instance.mode, aura_harness::config::InstanceMode::Local)
            && instance.command.is_none()
        {
            instance.command = Some("bash".to_string());
            instance.args = vec!["-lc".to_string(), "cat".to_string()];
        }
    }

    let coordinator = match HarnessCoordinator::from_run_config(&run_config) {
        Ok(coordinator) => coordinator,
        Err(error) => panic!("coordinator init failed: {error}"),
    };
    let mut tool_api = ToolApi::new(coordinator);

    if let Err(error) = tool_api.start_all() {
        panic!("start_all failed: {error}");
    }
    if let Err(error) = tool_api.stop_all() {
        panic!("stop_all failed: {error}");
    }
}

#[test]
fn address_resolution_metadata_includes_tunnel_route_for_ssh_instance() {
    let config_path = sample_mixed_config_path();
    let run_config = match aura_harness::load_and_validate_run_config(config_path) {
        Ok(config) => config,
        Err(error) => panic!("mixed config must validate: {error}"),
    };

    let ssh_instance = match run_config
        .instances
        .iter()
        .find(|instance| matches!(instance.mode, aura_harness::config::InstanceMode::Ssh))
    {
        Some(instance) => instance,
        None => panic!("expected one ssh instance in sample config"),
    };

    let resolved = AddressResolver::resolve(ssh_instance, "127.0.0.1:41001");
    assert_eq!(resolved.route, "ssh_tunnel_rewrite");
    assert_eq!(resolved.resolved_address, "127.0.0.1:54101");
}

fn sample_mixed_config_path() -> &'static Path {
    static PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("configs")
            .join("harness")
            .join("local-plus-ssh.toml")
    })
}
