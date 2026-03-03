#![allow(missing_docs)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use aura_harness::network_lab::{
    build_backend, resolve_backend_mode, AuthoritySpec, FirewallPreset, LinkConditionPreset,
    LinkSpec, NatPreset, NetworkBackendMode, NetworkEvent, RouterSpec, TopologySpec,
};

fn stress_topology() -> TopologySpec {
    TopologySpec {
        name: "nightly-stress".to_string(),
        routers: vec![
            RouterSpec {
                name: "home-a".to_string(),
                nat: NatPreset::Home,
                upstream: None,
                firewall: FirewallPreset::Home,
            },
            RouterSpec {
                name: "corp-b".to_string(),
                nat: NatPreset::Corporate,
                upstream: Some("home-a".to_string()),
                firewall: FirewallPreset::Corporate,
            },
        ],
        links: vec![
            LinkSpec {
                left: "alice-dev".to_string(),
                right: "home-a".to_string(),
                condition: LinkConditionPreset::Wifi,
            },
            LinkSpec {
                left: "bob-dev".to_string(),
                right: "corp-b".to_string(),
                condition: LinkConditionPreset::Wifi,
            },
            LinkSpec {
                left: "relay-dev".to_string(),
                right: "home-a".to_string(),
                condition: LinkConditionPreset::Lan,
            },
        ],
        authorities: vec![
            AuthoritySpec {
                authority_id: "relay".to_string(),
                device_name: "relay-dev".to_string(),
                gateway_router: "home-a".to_string(),
                bind_address: "10.0.0.10:45000".to_string(),
                relay_capable: true,
                env: BTreeMap::new(),
            },
            AuthoritySpec {
                authority_id: "alice".to_string(),
                device_name: "alice-dev".to_string(),
                gateway_router: "home-a".to_string(),
                bind_address: "10.0.0.11:45001".to_string(),
                relay_capable: false,
                env: BTreeMap::new(),
            },
            AuthoritySpec {
                authority_id: "bob".to_string(),
                device_name: "bob-dev".to_string(),
                gateway_router: "corp-b".to_string(),
                bind_address: "10.0.0.12:45002".to_string(),
                relay_capable: false,
                env: BTreeMap::new(),
            },
        ],
        required_relay_authority: Some("relay".to_string()),
    }
}

fn artifact_root(default_root: &std::path::Path) -> PathBuf {
    std::env::var_os("AURA_HOLEPUNCH_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_root.to_path_buf())
}

#[tokio::test]
async fn stress_handoff_and_nat_flush_cycles() {
    let temp = tempfile::tempdir().unwrap();
    let out_root = artifact_root(temp.path()).join("stress-cycles");
    std::fs::create_dir_all(&out_root).unwrap();

    let mode = resolve_backend_mode(NetworkBackendMode::Patchbay).resolved;
    let mut backend = build_backend(mode, &out_root).unwrap();
    backend.provision(stress_topology()).await.unwrap();

    for _ in 0..25 {
        backend
            .apply_event(NetworkEvent::LinkDown {
                authority_id: "alice".to_string(),
                iface: "eth0".to_string(),
            })
            .await
            .unwrap();
        backend
            .apply_event(NetworkEvent::FlushNat {
                router: "corp-b".to_string(),
            })
            .await
            .unwrap();
        backend
            .apply_event(NetworkEvent::LinkUp {
                authority_id: "alice".to_string(),
                iface: "eth0".to_string(),
            })
            .await
            .unwrap();
    }

    let artifacts = backend.collect_artifacts(&out_root).await.unwrap();
    assert!(artifacts.root.exists());
}

#[tokio::test]
async fn nightly_artifact_capture_and_retention_smoke() {
    let temp = tempfile::tempdir().unwrap();
    let out_root = artifact_root(temp.path()).join("artifact-retention");
    std::fs::create_dir_all(&out_root).unwrap();

    let mode = resolve_backend_mode(NetworkBackendMode::Patchbay).resolved;
    let mut backend = build_backend(mode, &out_root).unwrap();
    backend.provision(stress_topology()).await.unwrap();
    backend
        .apply_event(NetworkEvent::SetFirewall {
            router: "corp-b".to_string(),
            preset: FirewallPreset::UdpBlocked,
        })
        .await
        .unwrap();

    let artifacts = backend.collect_artifacts(&out_root).await.unwrap();
    assert!(artifacts.root.exists());
    assert!(artifacts.timeline.is_some());
    if let Some(timeline) = artifacts.timeline {
        assert!(timeline.exists());
    }
}
