//! Tier 2 holepunch patchbay tests.

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

#[tokio::test]
async fn link_conditions_land_on_the_node_that_owns_the_link() -> anyhow::Result<()> {
    let (mut backend, _runtime, _temp, artifact_root) = provision(topology(
        "link-owner",
        NatPreset::Home,
        NatPreset::Home,
        FirewallPreset::Home,
    ))
    .await?;

    // Device link: applies on alice-dev's uplink and affects only alice.
    let device_receipt = backend
        .apply_event(NetworkEvent::SetLinkCondition {
            left_node: "alice-dev".to_string(),
            right_node: "home-a".to_string(),
            condition: LinkConditionPreset::Mobile3G,
        })
        .await?;
    assert_eq!(device_receipt.event_type, "set_link_condition");
    assert_eq!(
        device_receipt.affected_authorities,
        vec!["alice".to_string()]
    );
    assert_eq!(device_receipt.details["target"]["kind"], "device_uplink");

    // Router link: applies on edge-b's downlink and affects everything behind it.
    let router_receipt = backend
        .apply_event(NetworkEvent::SetLinkCondition {
            left_node: "home-a".to_string(),
            right_node: "edge-b".to_string(),
            condition: LinkConditionPreset::Satellite,
        })
        .await?;
    assert_eq!(router_receipt.affected_authorities, vec!["bob".to_string()]);
    assert_eq!(router_receipt.details["target"]["kind"], "router_downlink");
    assert_eq!(
        router_receipt.details["target"]["downstream_router"],
        "edge-b"
    );

    // Lan clears instead of installing an empty qdisc.
    let cleared = backend
        .apply_event(NetworkEvent::SetLinkCondition {
            left_node: "edge-b".to_string(),
            right_node: "home-a".to_string(),
            condition: LinkConditionPreset::Lan,
        })
        .await?;
    assert_eq!(cleared.details["cleared"], true);

    // Non-adjacent endpoints are rejected rather than guessed.
    assert!(backend
        .apply_event(NetworkEvent::SetLinkCondition {
            left_node: "bob-dev".to_string(),
            right_node: "home-a".to_string(),
            condition: LinkConditionPreset::Wifi,
        })
        .await
        .is_err());

    // While the lab is alive the timeline is provisional: patchbay flushes its
    // event log on drop, so only the harness receipts are guaranteed here.
    let bundle = backend.collect_artifacts(&artifact_root).await?;
    let timeline = bundle.timeline.expect("timeline artifact");
    let entries: serde_json::Value = serde_json::from_slice(&std::fs::read(&timeline)?)?;
    let entries = entries.as_array().expect("timeline array");
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry["source"] == "harness")
            .count(),
        3,
        "one harness entry per applied event"
    );
    assert_eq!(
        bundle
            .metadata
            .get("timeline_provisional")
            .map(String::as_str),
        Some("true")
    );
    Ok(())
}

/// Plain test on purpose: `ProvisionedLab` owns its own runtime, so it cannot
/// run inside a Tokio test. This is the lane the harness binary uses.
#[test]
fn finished_lab_timeline_covers_provisioning_through_teardown() -> anyhow::Result<()> {
    use aura_harness::network_lab::{ProvisionedLab, TimelineEntry};

    let temp = tempfile::tempdir()?;
    let artifact_root = std::env::var_os("AURA_HOLEPUNCH_ARTIFACT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| temp.path().to_path_buf())
        .join("finished-lab");
    std::fs::create_dir_all(&artifact_root)?;
    let backend_mode = resolve_backend_mode(NetworkBackendMode::Patchbay).resolved;
    if backend_mode != NetworkBackendMode::Patchbay {
        eprintln!("SKIPPED: native patchbay unavailable (resolved {backend_mode})");
        return Ok(());
    }

    let mut lab = ProvisionedLab::provision(
        backend_mode,
        topology(
            "finished",
            NatPreset::Home,
            NatPreset::Home,
            FirewallPreset::Home,
        ),
        &artifact_root.join("lab"),
    )?;
    lab.apply_event(NetworkEvent::FlushNat {
        router: "edge-b".to_string(),
    })?;
    let finished = lab.finish(&artifact_root.join("out"))?;

    assert_eq!(finished.harness_event_count, 1);
    assert!(
        finished.lab_event_count > 0,
        "flushed lab event log feeds the timeline"
    );
    let timeline = finished.bundle.timeline.expect("final timeline");
    let entries: Vec<TimelineEntry> = serde_json::from_slice(&std::fs::read(&timeline)?)?;
    let kinds: Vec<String> = entries
        .iter()
        .filter_map(|entry| match entry {
            TimelineEntry::Lab { event, .. } => event["kind"].as_str().map(str::to_string),
            TimelineEntry::Harness { .. } => None,
        })
        .collect();
    assert!(
        kinds.iter().any(|kind| kind == "lab_created"),
        "timeline starts at provisioning, got {kinds:?}"
    );
    assert!(
        entries
            .windows(2)
            .all(|pair| pair[0].recorded_at_ms() <= pair[1].recorded_at_ms()),
        "timeline is ordered"
    );
    Ok(())
}

#[tokio::test]
async fn restricted_nat_provisions_and_flushes() -> anyhow::Result<()> {
    // Restricted cone (EIM + ADF) sits between Home and FullCone; patchbay has
    // no tier for it, so it provisions as an explicit NAT config.
    let (mut backend, _runtime, _temp, artifact_root) = provision(topology(
        "restricted",
        NatPreset::Restricted,
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
async fn stress_link_presets_apply_on_device_and_router_links() -> anyhow::Result<()> {
    let (mut backend, _runtime, _temp, _artifact_root) = provision(topology(
        "stress-presets",
        NatPreset::Home,
        NatPreset::Home,
        FirewallPreset::Home,
    ))
    .await?;
    let presets = [
        LinkConditionPreset::WifiBursty,
        LinkConditionPreset::Mobile4GCongested,
        LinkConditionPreset::Custom {
            latency_ms: 80,
            jitter_ms: 20,
            rate_kbit: Some(4_000),
            loss_tenths_pct: 25,
            loss_burst_pkts: Some(3),
            buffer_ms: Some(160),
        },
    ];
    for preset in presets {
        let device = backend
            .apply_event(NetworkEvent::SetLinkCondition {
                left_node: "alice-dev".to_string(),
                right_node: "home-a".to_string(),
                condition: preset,
            })
            .await?;
        assert_eq!(device.affected_authorities, vec!["alice".to_string()]);
        let router = backend
            .apply_event(NetworkEvent::SetLinkCondition {
                left_node: "home-a".to_string(),
                right_node: "edge-b".to_string(),
                condition: preset,
            })
            .await?;
        assert_eq!(router.affected_authorities, vec!["bob".to_string()]);
    }
    Ok(())
}
