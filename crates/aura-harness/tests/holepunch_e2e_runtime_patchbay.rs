#![allow(missing_docs)]
#![cfg(target_os = "linux")]
//! End-to-end UDP holepunch validation via Aura network-lab topology + Patchbay.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use aura_harness::network_lab::scenario_builder::ScenarioBuilder;
use aura_harness::network_lab::{
    AuthoritySpec, FirewallPreset, LinkConditionPreset, LinkSpec, NatPreset, RouterSpec,
    TopologySpec,
};
use patchbay::config::{LabConfig, RouterConfig};
use patchbay::{check_caps, Lab, LabOpts};
use tokio::net::UdpSocket;
use tokio::sync::oneshot;

fn holepunch_topology() -> Result<TopologySpec> {
    ScenarioBuilder::new("holepunch-runtime-harness-e2e")
        .add_router(RouterSpec {
            name: "core-dc".to_string(),
            nat: NatPreset::None,
            upstream: None,
            firewall: FirewallPreset::Open,
        })
        .add_router(RouterSpec {
            name: "nat-a".to_string(),
            nat: NatPreset::Home,
            upstream: Some("core-dc".to_string()),
            firewall: FirewallPreset::Home,
        })
        .add_router(RouterSpec {
            name: "nat-b".to_string(),
            nat: NatPreset::Home,
            upstream: Some("core-dc".to_string()),
            firewall: FirewallPreset::Home,
        })
        .add_link(LinkSpec {
            left: "relay-dev".to_string(),
            right: "core-dc".to_string(),
            condition: LinkConditionPreset::Lan,
        })
        .add_link(LinkSpec {
            left: "alice-dev".to_string(),
            right: "nat-a".to_string(),
            condition: LinkConditionPreset::Wifi,
        })
        .add_link(LinkSpec {
            left: "bob-dev".to_string(),
            right: "nat-b".to_string(),
            condition: LinkConditionPreset::Wifi,
        })
        .require_relay_path("relay")
        .with_authorities(vec![
            AuthoritySpec {
                authority_id: "relay".to_string(),
                device_name: "relay-dev".to_string(),
                gateway_router: "core-dc".to_string(),
                bind_address: "10.0.0.10:46000".to_string(),
                relay_capable: true,
                env: BTreeMap::new(),
            },
            AuthoritySpec {
                authority_id: "alice".to_string(),
                device_name: "alice-dev".to_string(),
                gateway_router: "nat-a".to_string(),
                bind_address: "10.0.0.11:46001".to_string(),
                relay_capable: false,
                env: BTreeMap::new(),
            },
            AuthoritySpec {
                authority_id: "bob".to_string(),
                device_name: "bob-dev".to_string(),
                gateway_router: "nat-b".to_string(),
                bind_address: "10.0.0.12:46002".to_string(),
                relay_capable: false,
                env: BTreeMap::new(),
            },
        ])?
        .with_connected_graph()?
        .build()
}

fn to_patchbay_nat(nat: NatPreset) -> patchbay::Nat {
    match nat {
        NatPreset::None => patchbay::Nat::None,
        NatPreset::Home => patchbay::Nat::Home,
        NatPreset::Corporate => patchbay::Nat::Corporate,
        NatPreset::FullCone => patchbay::Nat::FullCone,
        NatPreset::Cgnat => patchbay::Nat::Cgnat,
        NatPreset::CloudNat => patchbay::Nat::CloudNat,
    }
}

fn to_patchbay_impair_token(condition: LinkConditionPreset) -> &'static str {
    match condition {
        LinkConditionPreset::Lan => "lan",
        LinkConditionPreset::Wifi => "wifi",
        LinkConditionPreset::WifiBad => "wifi-bad",
        LinkConditionPreset::Mobile4G => "mobile-4g",
        LinkConditionPreset::Mobile3G => "mobile-3g",
        LinkConditionPreset::Satellite => "satellite",
    }
}

async fn flush_nat_state_if_present(router: &patchbay::Router, router_name: &str) -> Result<()> {
    match router.flush_nat_state().await {
        Ok(()) => Ok(()),
        Err(error) => {
            // Some backends represent an empty NAT table as a missing state file.
            if error.to_string().contains("No such file or directory") {
                Ok(())
            } else {
                Err(error).with_context(|| format!("flush NAT state for {router_name}"))
            }
        }
    }
}

fn find_link_condition(
    spec: &TopologySpec,
    authority: &AuthoritySpec,
) -> Option<LinkConditionPreset> {
    spec.links.iter().find_map(|link| {
        let forward = link.left == authority.device_name && link.right == authority.gateway_router;
        let reverse = link.right == authority.device_name && link.left == authority.gateway_router;
        if forward || reverse {
            Some(link.condition)
        } else {
            None
        }
    })
}

fn to_patchbay_lab_config(spec: &TopologySpec) -> Result<LabConfig> {
    let routers = spec
        .routers
        .iter()
        .map(|router| RouterConfig {
            name: router.name.clone(),
            region: None,
            upstream: router.upstream.clone(),
            nat: to_patchbay_nat(router.nat),
            ip_support: patchbay::IpSupport::V4Only,
            nat_v6: patchbay::NatV6Mode::None,
        })
        .collect();

    let mut device = std::collections::HashMap::new();
    for authority in &spec.authorities {
        let mut table = toml::map::Map::new();
        table.insert(
            "default_via".to_string(),
            toml::Value::String("eth0".to_string()),
        );

        let mut iface = toml::map::Map::new();
        iface.insert(
            "gateway".to_string(),
            toml::Value::String(authority.gateway_router.clone()),
        );
        if let Some(condition) = find_link_condition(spec, authority) {
            iface.insert(
                "impair".to_string(),
                toml::Value::String(to_patchbay_impair_token(condition).to_string()),
            );
        }
        table.insert("eth0".to_string(), toml::Value::Table(iface));
        device.insert(authority.device_name.clone(), toml::Value::Table(table));
    }

    Ok(LabConfig {
        region: None,
        router: routers,
        device,
    })
}

async fn discover_public_addr(socket: &UdpSocket, reflector: SocketAddr) -> Result<SocketAddr> {
    socket
        .send_to(b"PROBE", reflector)
        .await
        .context("send STUN probe")?;
    let mut buf = [0u8; 256];
    let (n, _) = socket
        .recv_from(&mut buf)
        .await
        .context("recv STUN response")?;
    let response = std::str::from_utf8(&buf[..n]).context("STUN response UTF-8 decode")?;
    let addr = response
        .strip_prefix("OBSERVED ")
        .ok_or_else(|| anyhow!("unexpected STUN response payload: {response:?}"))?;
    addr.parse()
        .with_context(|| format!("parse observed address from {addr:?}"))
}

async fn holepunch_send_recv(socket: &UdpSocket, dst: SocketAddr) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    let mut seq = 0u32;
    let mut buf = [0u8; 512];

    loop {
        let payload = format!("punch-{seq}");
        socket
            .send_to(payload.as_bytes(), dst)
            .await
            .with_context(|| format!("send UDP punch probe to {dst}"))?;
        seq = seq.saturating_add(1);

        match tokio::time::timeout(Duration::from_millis(200), socket.recv_from(&mut buf)).await {
            Ok(Ok((_len, _from))) => {
                for ack_id in 0..3 {
                    let ack = format!("ack-{ack_id}");
                    let _ = socket.send_to(ack.as_bytes(), dst).await;
                }
                return Ok(());
            }
            Ok(Err(error)) => return Err(error).context("recv UDP punch response"),
            Err(_) => {
                if tokio::time::Instant::now() > deadline {
                    bail!("holepunch timed out after 8s sending to {dst}");
                }
            }
        }
    }
}

async fn run_holepunch_round(lab: &Lab, reflector: SocketAddr, stagger: Duration) -> Result<()> {
    let alice = lab
        .device_by_name("alice-dev")
        .ok_or_else(|| anyhow!("alice-dev not found in lab"))?;
    let bob = lab
        .device_by_name("bob-dev")
        .ok_or_else(|| anyhow!("bob-dev not found in lab"))?;

    let (alice_public_tx, alice_public_rx) = oneshot::channel::<SocketAddr>();
    let (bob_public_tx, bob_public_rx) = oneshot::channel::<SocketAddr>();

    let alice_task = alice.spawn({
        async move |_| {
            let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
                .await
                .context("alice bind UDP socket")?;
            let public_addr = discover_public_addr(&socket, reflector).await?;
            alice_public_tx
                .send(public_addr)
                .map_err(|_| anyhow!("alice failed to publish observed address"))?;

            let peer_addr = bob_public_rx
                .await
                .map_err(|_| anyhow!("alice failed to receive bob observed address"))?;
            holepunch_send_recv(&socket, peer_addr).await
        }
    })?;

    let bob_task = bob.spawn(async move |_| {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
            .await
            .context("bob bind UDP socket")?;
        let public_addr = discover_public_addr(&socket, reflector).await?;
        bob_public_tx
            .send(public_addr)
            .map_err(|_| anyhow!("bob failed to publish observed address"))?;

        let peer_addr = alice_public_rx
            .await
            .map_err(|_| anyhow!("bob failed to receive alice observed address"))?;
        tokio::time::sleep(stagger).await;
        holepunch_send_recv(&socket, peer_addr).await
    })?;

    let alice_result = alice_task
        .await
        .map_err(|error| anyhow!("alice holepunch task join failure: {error}"))?;
    let bob_result = bob_task
        .await
        .map_err(|error| anyhow!("bob holepunch task join failure: {error}"))?;

    alice_result?;
    bob_result?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_harness_patchbay_holepunch_works_e2e() -> Result<()> {
    check_caps().context("patchbay capability check failed")?;

    let topology = holepunch_topology()?;
    let lab_config = to_patchbay_lab_config(&topology)?;
    let out = tempfile::tempdir().context("create holepunch artifact tempdir")?;

    let lab = Lab::from_config_with_opts(
        lab_config,
        LabOpts::default()
            .outdir(out.path())
            .label("aura-harness-holepunch-e2e"),
    )
    .await
    .context("provision patchbay lab for holepunch e2e")?;

    let relay = lab
        .device_by_name("relay-dev")
        .ok_or_else(|| anyhow!("relay-dev not found in lab"))?;
    let (reflector_tx, reflector_rx) = oneshot::channel::<SocketAddr>();

    let relay_task = relay.spawn(async move |ctx| {
        let relay_ip = ctx
            .ip()
            .ok_or_else(|| anyhow!("relay context missing IPv4 address"))?;
        let reflector = SocketAddr::from((relay_ip, 46_000));
        ctx.spawn_reflector(reflector)
            .context("spawn relay reflector")?;
        reflector_tx
            .send(reflector)
            .map_err(|_| anyhow!("failed to publish reflector address"))?;
        Ok::<(), anyhow::Error>(())
    })?;

    relay_task
        .await
        .map_err(|error| anyhow!("relay setup task join failure: {error}"))??;

    let reflector = tokio::time::timeout(Duration::from_secs(5), reflector_rx)
        .await
        .context("timed out waiting for reflector address")?
        .map_err(|_| anyhow!("reflector setup channel closed"))?;

    // Round 1: baseline simultaneous open through Home NAT on both sides.
    run_holepunch_round(&lab, reflector, Duration::ZERO).await?;

    // Flush both NAT tables to force full re-establishment.
    let nat_a = lab
        .router_by_name("nat-a")
        .ok_or_else(|| anyhow!("nat-a router missing"))?;
    flush_nat_state_if_present(&nat_a, "nat-a").await?;

    let nat_b = lab
        .router_by_name("nat-b")
        .ok_or_else(|| anyhow!("nat-b router missing"))?;
    flush_nat_state_if_present(&nat_b, "nat-b").await?;

    // Round 2: realistic staggered start after NAT state reset.
    run_holepunch_round(&lab, reflector, Duration::from_millis(200)).await?;

    Ok(())
}
