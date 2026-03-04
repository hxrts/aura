#![allow(missing_docs)]

use std::collections::BTreeMap;

use aura_harness::network_lab::{
    build_backend, resolve_backend_mode, FirewallPreset, LinkConditionPreset, LinkSpec, NatPreset,
    NetworkBackendMode, NetworkEvent, RouterSpec, TopologySpec,
};

fn topology(name: &str, nat_a: NatPreset, nat_b: NatPreset, fw_b: FirewallPreset) -> TopologySpec {
    TopologySpec {
        name: name.to_string(),
        routers: vec![
            RouterSpec {
                name: "home-a".to_string(),
                nat: nat_a,
                upstream: None,
                firewall: FirewallPreset::Home,
            },
            RouterSpec {
                name: "edge-b".to_string(),
                nat: nat_b,
                upstream: Some("home-a".to_string()),
                firewall: fw_b,
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
                right: "edge-b".to_string(),
                condition: LinkConditionPreset::Wifi,
            },
            LinkSpec {
                left: "relay-dev".to_string(),
                right: "home-a".to_string(),
                condition: LinkConditionPreset::Lan,
            },
        ],
        authorities: vec![
            aura_harness::network_lab::AuthoritySpec {
                authority_id: "relay".to_string(),
                device_name: "relay-dev".to_string(),
                gateway_router: "home-a".to_string(),
                bind_address: "10.0.0.10:44000".to_string(),
                relay_capable: true,
                env: BTreeMap::new(),
            },
            aura_harness::network_lab::AuthoritySpec {
                authority_id: "alice".to_string(),
                device_name: "alice-dev".to_string(),
                gateway_router: "home-a".to_string(),
                bind_address: "10.0.0.11:44001".to_string(),
                relay_capable: false,
                env: BTreeMap::new(),
            },
            aura_harness::network_lab::AuthoritySpec {
                authority_id: "bob".to_string(),
                device_name: "bob-dev".to_string(),
                gateway_router: "edge-b".to_string(),
                bind_address: "10.0.0.12:44002".to_string(),
                relay_capable: false,
                env: BTreeMap::new(),
            },
        ],
        required_relay_authority: Some("relay".to_string()),
    }
}

async fn provision(
    spec: TopologySpec,
) -> anyhow::Result<(
    Box<dyn aura_harness::network_lab::NetworkLabBackend>,
    aura_harness::network_lab::LabRuntime,
    tempfile::TempDir,
    std::path::PathBuf,
)> {
    let temp = tempfile::tempdir()?;
    let artifact_root = std::env::var_os("AURA_HOLEPUNCH_ARTIFACT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| temp.path().to_path_buf());
    std::fs::create_dir_all(&artifact_root)?;
    let backend_mode = resolve_backend_mode(NetworkBackendMode::Patchbay).resolved;
    let mut backend = build_backend(backend_mode, &artifact_root)?;
    let runtime = backend.provision(spec).await?;
    Ok((backend, runtime, temp, artifact_root))
}

#[tokio::test]
async fn home_to_home_succeeds_with_relay_first_policy() -> anyhow::Result<()> {
    let (mut backend, runtime, _temp, artifact_root) = provision(topology(
        "home-home",
        NatPreset::Home,
        NatPreset::Home,
        FirewallPreset::Home,
    ))
    .await?;

    assert_eq!(runtime.authorities.len(), 3);

    let receipt = backend
        .apply_event(NetworkEvent::SetLinkCondition {
            left_node: "alice-dev".to_string(),
            right_node: "home-a".to_string(),
            condition: LinkConditionPreset::Wifi,
        })
        .await?;
    assert_eq!(receipt.event_type, "set_link_condition");

    let bundle = backend.collect_artifacts(&artifact_root).await?;
    assert!(bundle.root.exists());
    Ok(())
}

#[tokio::test]
async fn home_to_corporate_keeps_relay_path() -> anyhow::Result<()> {
    let (mut backend, _runtime, _temp, artifact_root) = provision(topology(
        "home-corporate",
        NatPreset::Home,
        NatPreset::Corporate,
        FirewallPreset::Corporate,
    ))
    .await?;

    let receipt = backend
        .apply_event(NetworkEvent::SetFirewall {
            router: "edge-b".to_string(),
            preset: FirewallPreset::Corporate,
        })
        .await?;
    assert_eq!(receipt.event_type, "set_firewall");

    let bundle = backend.collect_artifacts(&artifact_root).await?;
    assert!(bundle.root.exists());
    Ok(())
}

#[tokio::test]
async fn double_nat_handles_nat_flush_event() -> anyhow::Result<()> {
    let (mut backend, _runtime, _temp, artifact_root) = provision(topology(
        "double-nat",
        NatPreset::Cgnat,
        NatPreset::Home,
        FirewallPreset::Home,
    ))
    .await?;

    let receipt = backend
        .apply_event(NetworkEvent::FlushNat {
            router: "edge-b".to_string(),
        })
        .await?;
    assert_eq!(receipt.event_type, "nat_flush");
    let bundle = backend.collect_artifacts(&artifact_root).await?;
    assert!(bundle.root.exists());
    Ok(())
}

#[tokio::test]
async fn network_handoff_recovers_after_link_down_up() -> anyhow::Result<()> {
    let (mut backend, _runtime, _temp, artifact_root) = provision(topology(
        "handoff",
        NatPreset::Home,
        NatPreset::Cgnat,
        FirewallPreset::Home,
    ))
    .await?;

    let down = backend
        .apply_event(NetworkEvent::LinkDown {
            authority_id: "alice".to_string(),
            iface: "eth0".to_string(),
        })
        .await?;
    assert_eq!(down.event_type, "link_toggle");

    let up = backend
        .apply_event(NetworkEvent::LinkUp {
            authority_id: "alice".to_string(),
            iface: "eth0".to_string(),
        })
        .await?;
    assert_eq!(up.event_type, "link_toggle");
    let bundle = backend.collect_artifacts(&artifact_root).await?;
    assert!(bundle.root.exists());
    Ok(())
}

#[tokio::test]
async fn nat_timeout_scenario_flushes_router_state() -> anyhow::Result<()> {
    let (mut backend, _runtime, _temp, artifact_root) = provision(topology(
        "nat-timeout",
        NatPreset::Home,
        NatPreset::Home,
        FirewallPreset::Home,
    ))
    .await?;

    let receipt = backend
        .apply_event(NetworkEvent::FlushNat {
            router: "home-a".to_string(),
        })
        .await?;
    assert_eq!(receipt.event_type, "nat_flush");
    let bundle = backend.collect_artifacts(&artifact_root).await?;
    assert!(bundle.root.exists());
    Ok(())
}

#[tokio::test]
async fn restrictive_firewall_scenario_still_runs_with_relay_available() -> anyhow::Result<()> {
    let (mut backend, _runtime, _temp, artifact_root) = provision(topology(
        "firewall",
        NatPreset::Home,
        NatPreset::Corporate,
        FirewallPreset::UdpBlocked,
    ))
    .await?;

    let receipt = backend
        .apply_event(NetworkEvent::SetFirewall {
            router: "edge-b".to_string(),
            preset: FirewallPreset::UdpBlocked,
        })
        .await?;
    assert_eq!(receipt.event_type, "set_firewall");
    let bundle = backend.collect_artifacts(&artifact_root).await?;
    assert!(bundle.root.exists());
    Ok(())
}
