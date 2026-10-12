use anyhow::{bail, Context, Result};
use cfg_if::cfg_if;

use super::{AuthoritySpec, LinkConditionPreset, LinkSpec, RouterSpec, TopologySpec};

cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "patchbay-backend"))] {
        use std::collections::BTreeMap;

        /// Convert Aura `TopologySpec` into patchbay `LabConfig`.
        pub fn to_patchbay_lab_config(spec: &TopologySpec) -> Result<patchbay::config::LabConfig> {
            #[derive(serde::Serialize)]
            struct RouterConfigDoc {
                name: String,
                region: Option<String>,
                upstream: Option<String>,
                nat: patchbay::Nat,
                ip_support: patchbay::IpSupport,
                nat_v6: patchbay::NatV6Mode,
                ra_enabled: Option<bool>,
                ra_interval_secs: Option<u64>,
                ra_lifetime_secs: Option<u64>,
            }

            #[derive(serde::Serialize)]
            struct LabConfigDoc {
                region: Option<BTreeMap<String, toml::Value>>,
                router: Vec<RouterConfigDoc>,
                device: BTreeMap<String, toml::Value>,
            }

            let routers = spec
                .routers
                .iter()
                .map(|router| {
                    Ok(RouterConfigDoc {
                        name: router.name.clone(),
                        region: None,
                        upstream: router.upstream.clone(),
                        nat: to_patchbay_nat(router.nat)?,
                        ip_support: patchbay::IpSupport::V4Only,
                        nat_v6: patchbay::NatV6Mode::None,
                        ra_enabled: None,
                        ra_interval_secs: None,
                        ra_lifetime_secs: None,
                    })
                })
                .collect::<Result<Vec<_>>>()?;

            let mut device = BTreeMap::new();
            for authority in &spec.authorities {
                let mut table = toml::map::Map::new();
                table.insert(
                    "default_via".to_string(),
                    toml::Value::String(super::DEVICE_UPLINK_IFACE.to_string()),
                );

                let mut iface = toml::map::Map::new();
                iface.insert(
                    "gateway".to_string(),
                    toml::Value::String(authority.gateway_router.clone()),
                );
                // `Lan` is the unimpaired baseline; leaving `impair` out keeps the
                // interface free of qdiscs instead of installing an empty one.
                if let Some(condition) = find_device_link_condition(spec, authority)
                    .filter(|condition| *condition != LinkConditionPreset::Lan)
                {
                    iface.insert("impair".to_string(), to_patchbay_impair_value(condition));
                }
                table.insert(
                    super::DEVICE_UPLINK_IFACE.to_string(),
                    toml::Value::Table(iface),
                );

                device.insert(authority.device_name.clone(), toml::Value::Table(table));
            }

            let encoded = toml::to_string(&LabConfigDoc {
                region: None,
                router: routers,
                device,
            })
            .context("encode patchbay lab config TOML")?;

            toml::from_str(&encoded).context("decode patchbay lab config from TOML")
        }

        /// Convert patchbay `LabConfig` into Aura `TopologySpec`.
        pub fn from_patchbay_lab_config(
            name: &str,
            cfg: &patchbay::config::LabConfig,
        ) -> Result<TopologySpec> {
            let routers = cfg
                .router
                .iter()
                .map(|router| {
                    Ok(RouterSpec {
                        name: router.name.clone(),
                        nat: from_patchbay_nat(router.nat)?,
                        upstream: router.upstream.clone(),
                        firewall: FirewallPreset::Open,
                    })
                })
                .collect::<Result<Vec<_>>>()?;

            let mut authorities = Vec::new();
            let mut links = Vec::new();
            for (device_name, value) in &cfg.device {
                // patchbay carries its own `toml` major version; re-serialize the
                // device table into this crate's `toml::Value` instead of calling
                // the foreign type's accessors.
                let value = toml::Value::try_from(value).with_context(|| {
                    format!("convert patchbay device {device_name} table")
                })?;
                let table = value
                    .as_table()
                    .ok_or_else(|| anyhow::anyhow!("device {device_name} is not a table"))?;
                let iface = table
                    .get("eth0")
                    .and_then(toml::Value::as_table)
                    .ok_or_else(|| anyhow::anyhow!("device {device_name} is missing eth0 table"))?;
                let gateway = iface
                    .get("gateway")
                    .and_then(toml::Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("device {device_name}.eth0 is missing gateway"))?;

                let authority_id = device_name.replace("-dev", "");
                authorities.push(AuthoritySpec {
                    authority_id,
                    device_name: device_name.clone(),
                    gateway_router: gateway.to_string(),
                    bind_address: "0.0.0.0:0".to_string(),
                    relay_capable: true,
                    env: BTreeMap::default(),
                });

                if let Some(impair) = iface.get("impair") {
                    links.push(LinkSpec {
                        left: device_name.clone(),
                        right: gateway.to_string(),
                        condition: from_patchbay_impair_value(impair)?,
                    });
                }
            }

            Ok(TopologySpec {
                name: name.to_string(),
                routers,
                links,
                authorities,
                required_relay_authority: None,
            })
        }

        fn find_device_link_condition(
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

        /// Map an Aura NAT preset onto patchbay's behavior-only NAT tiers.
        ///
        /// Patchbay 0.7 replaced its deployment-flavored presets with three
        /// RFC 4787 behavior tiers. Aura keeps its deployment names because the
        /// holepunch scenarios are written in those terms, so the mapping is
        /// many-to-one:
        ///
        /// | Aura        | patchbay   | RFC 4787 behavior                     |
        /// |-------------|------------|---------------------------------------|
        /// | `None`      | `None`     | public address, no translation        |
        /// | `FullCone`  | `Open`     | EIM + EIF                             |
        /// | `Home`      | `Moderate` | EIM + APDF                            |
        /// | `Cgnat`     | `Moderate` | EIM + APDF (RFC 6888 compliant CGNAT) |
        /// | `Corporate` | `Strict`   | EDM random ports + APDF               |
        /// | `CloudNat`  | `Strict`   | EDM random ports + APDF               |
        ///
        /// | `Restricted`| custom     | EIM + ADF (RFC 3489 restricted cone)  |
        ///
        /// `Cgnat` follows patchbay's own `IspCgnat` router preset rather than
        /// its `Cgnat -> Open` migration alias: compliant carrier NAT filters on
        /// address and port, and the Aura double-NAT scenarios depend on that.
        /// `Restricted` has no patchbay tier, so it is an explicit `NatConfig`;
        /// the builder validates it (EIM is the only mapping ADF allows).
        pub fn to_patchbay_nat(nat: super::NatPreset) -> Result<patchbay::Nat> {
            Ok(match nat {
                super::NatPreset::None => patchbay::Nat::None,
                super::NatPreset::FullCone => patchbay::Nat::Open,
                super::NatPreset::Home | super::NatPreset::Cgnat => patchbay::Nat::Moderate,
                super::NatPreset::Corporate | super::NatPreset::CloudNat => patchbay::Nat::Strict,
                super::NatPreset::Restricted => patchbay::Nat::Custom(
                    patchbay::NatConfig::builder()
                        .mapping(patchbay::NatMapping::EndpointIndependent)
                        .filtering(patchbay::NatFiltering::AddressDependent)
                        .build()
                        .context("build restricted-cone NAT config")?,
                ),
            })
        }

        /// Inverse of [`to_patchbay_nat`] for the representative Aura preset of
        /// each patchbay tier. The mapping is lossy, so `Moderate` reads back as
        /// `Home` and `Strict` as `Corporate`. The one custom configuration
        /// recognized is the restricted-cone shape; others are rejected.
        fn from_patchbay_nat(nat: patchbay::Nat) -> Result<super::NatPreset> {
            match nat {
                patchbay::Nat::None => Ok(super::NatPreset::None),
                patchbay::Nat::Open => Ok(super::NatPreset::FullCone),
                patchbay::Nat::Moderate => Ok(super::NatPreset::Home),
                patchbay::Nat::Strict => Ok(super::NatPreset::Corporate),
                patchbay::Nat::Custom(config)
                    if config.mapping == patchbay::NatMapping::EndpointIndependent
                        && config.filtering == patchbay::NatFiltering::AddressDependent =>
                {
                    Ok(super::NatPreset::Restricted)
                }
                patchbay::Nat::Custom(config) => {
                    bail!("custom patchbay NAT config has no Aura NatPreset: {config:?}")
                }
            }
        }

        /// Impairment to apply for `condition`, or `None` for the unimpaired
        /// `Lan` baseline, which clears any existing qdisc instead.
        pub fn to_patchbay_link_impairment(
            condition: LinkConditionPreset,
        ) -> Option<patchbay::LinkCondition> {
            match condition {
                LinkConditionPreset::Lan => None,
                other => Some(to_patchbay_link_condition(other)),
            }
        }

        pub fn to_patchbay_link_condition(condition: LinkConditionPreset) -> patchbay::LinkCondition {
            match condition {
                LinkConditionPreset::Lan => patchbay::LinkCondition::lan(),
                LinkConditionPreset::Wifi => patchbay::LinkCondition::wifi(),
                LinkConditionPreset::WifiBad => patchbay::LinkCondition::wifi_bad(),
                LinkConditionPreset::Mobile4G => patchbay::LinkCondition::mobile_4g(),
                LinkConditionPreset::Mobile3G => patchbay::LinkCondition::mobile_3g(),
                LinkConditionPreset::Satellite => patchbay::LinkCondition::satellite(),
                // Wifi fades: about 3% loss in bursts, with the queue bounded to
                // 40 ms so congestion also shows up as overflow loss.
                LinkConditionPreset::WifiBursty => patchbay::LinkCondition::wifi()
                    .bursty_loss(3.0)
                    .buffer_ms(40)
                    .label("wifi-bursty"),
                // 4G at a saturated 8 Mbit bottleneck with a 60 ms queue: loss
                // comes from overflow, so no explicit rate beyond the preset.
                LinkConditionPreset::Mobile4GCongested => patchbay::LinkCondition::mobile_4g()
                    .rate_mbit(8)
                    .buffer_ms(60)
                    .label("mobile-4g-congested"),
                LinkConditionPreset::Custom {
                    latency_ms,
                    jitter_ms,
                    rate_kbit,
                    loss_tenths_pct,
                    loss_burst_pkts,
                    buffer_ms,
                } => {
                    let mut condition = patchbay::LinkCondition::new()
                        .latency_ms(latency_ms)
                        .jitter_ms(jitter_ms)
                        .loss_pct(f32::from(loss_tenths_pct) / 10.0)
                        .label("custom");
                    if let Some(rate_kbit) = rate_kbit {
                        condition = condition.rate_kbit(rate_kbit);
                    }
                    if let Some(burst) = loss_burst_pkts {
                        condition = condition.loss_burst_pkts(burst);
                    }
                    if let Some(buffer_ms) = buffer_ms {
                        condition = condition.buffer_ms(buffer_ms);
                    }
                    condition
                }
            }
        }

        /// Serialize `condition` the way patchbay's TOML `impair` field reads
        /// it: a preset name where patchbay has one, otherwise an explicit table
        /// of the effective parameters.
        pub fn to_patchbay_impair_value(condition: LinkConditionPreset) -> toml::Value {
            match to_patchbay_impair_token(condition) {
                Some(token) => toml::Value::String(token.to_string()),
                None => {
                    let effective = to_patchbay_link_condition(condition);
                    let mut table = toml::map::Map::new();
                    let int = |value: u32| toml::Value::Integer(i64::from(value));
                    table.insert("latency_ms".to_string(), int(effective.latency_ms));
                    table.insert("jitter_ms".to_string(), int(effective.jitter_ms));
                    table.insert(
                        "loss_pct".to_string(),
                        toml::Value::Float(f64::from(effective.loss_pct)),
                    );
                    if let Some(rate_kbit) = effective.rate_kbit {
                        table.insert("rate_kbit".to_string(), int(rate_kbit));
                    }
                    if let Some(burst) = effective.loss_burst_pkts {
                        table.insert("loss_burst_pkts".to_string(), int(burst));
                    }
                    if let Some(buffer_ms) = effective.buffer_ms {
                        table.insert("buffer_ms".to_string(), int(buffer_ms));
                    }
                    toml::Value::Table(table)
                }
            }
        }

        pub fn to_patchbay_firewall(preset: super::FirewallPreset) -> patchbay::Firewall {
            match preset {
                super::FirewallPreset::Open => patchbay::Firewall::None,
                super::FirewallPreset::Home => patchbay::Firewall::BlockInbound,
                super::FirewallPreset::Corporate => patchbay::Firewall::Corporate,
                super::FirewallPreset::UdpBlocked => patchbay::Firewall::Custom(
                    patchbay::FirewallConfig::builder()
                        .block_inbound()
                        .outbound_tcp(patchbay::PortPolicy::AllowAll)
                        .outbound_udp(patchbay::PortPolicy::BlockAll)
                        .build(),
                ),
            }
        }

        use super::FirewallPreset;
    }
}

/// Serialize topology into TOML for CLI-driven backends (e.g. `patchbay-vm`).
pub fn topology_to_toml(spec: &TopologySpec) -> Result<String> {
    #[derive(serde::Serialize)]
    struct TopologyDoc<'a> {
        name: &'a str,
        routers: &'a [RouterSpec],
        links: &'a [LinkSpec],
        authorities: &'a [AuthoritySpec],
        required_relay_authority: &'a Option<String>,
    }

    toml::to_string_pretty(&TopologyDoc {
        name: &spec.name,
        routers: &spec.routers,
        links: &spec.links,
        authorities: &spec.authorities,
        required_relay_authority: &spec.required_relay_authority,
    })
    .context("failed to encode topology TOML")
}

/// Patchbay's preset name for `condition`, or `None` for the presets that only
/// exist as explicit parameter tables (see `to_patchbay_impair_value`).
pub fn to_patchbay_impair_token(condition: LinkConditionPreset) -> Option<&'static str> {
    match condition {
        LinkConditionPreset::Lan => Some("lan"),
        LinkConditionPreset::Wifi => Some("wifi"),
        LinkConditionPreset::WifiBad => Some("wifi-bad"),
        LinkConditionPreset::Mobile4G => Some("mobile-4g"),
        LinkConditionPreset::Mobile3G => Some("mobile-3g"),
        LinkConditionPreset::Satellite => Some("satellite"),
        LinkConditionPreset::WifiBursty
        | LinkConditionPreset::Mobile4GCongested
        | LinkConditionPreset::Custom { .. } => None,
    }
}

pub fn from_patchbay_impair_token(token: &str) -> Result<LinkConditionPreset> {
    match token {
        "lan" => Ok(LinkConditionPreset::Lan),
        "wifi" => Ok(LinkConditionPreset::Wifi),
        "wifi-bad" => Ok(LinkConditionPreset::WifiBad),
        "mobile-4g" | "mobile" => Ok(LinkConditionPreset::Mobile4G),
        "mobile-3g" => Ok(LinkConditionPreset::Mobile3G),
        "satellite" => Ok(LinkConditionPreset::Satellite),
        other => bail!("unsupported patchbay impair token: {other}"),
    }
}

/// Read a patchbay `impair` value back: preset names map to the named presets,
/// parameter tables map to `Custom`. The named stress presets therefore read
/// back as `Custom` with their effective parameters, which is the same qdisc.
pub fn from_patchbay_impair_value(value: &toml::Value) -> Result<LinkConditionPreset> {
    match value {
        toml::Value::String(token) => from_patchbay_impair_token(token),
        toml::Value::Table(table) => {
            let int = |key: &str| -> Result<Option<u32>> {
                match table.get(key) {
                    None => Ok(None),
                    Some(toml::Value::Integer(value)) => Ok(Some(u32::try_from(*value)?)),
                    Some(other) => bail!("impair.{key} must be an integer, got {other}"),
                }
            };
            let loss_pct = match table.get("loss_pct") {
                None => 0.0,
                Some(toml::Value::Float(value)) => *value,
                Some(toml::Value::Integer(value)) => *value as f64,
                Some(other) => bail!("impair.loss_pct must be a number, got {other}"),
            };
            Ok(LinkConditionPreset::Custom {
                latency_ms: int("latency_ms")?.unwrap_or(0),
                jitter_ms: int("jitter_ms")?.unwrap_or(0),
                rate_kbit: int("rate_kbit")?,
                loss_tenths_pct: u16::try_from((loss_pct * 10.0).round() as i64)
                    .context("impair.loss_pct out of range")?,
                loss_burst_pkts: int("loss_burst_pkts")?,
                buffer_ms: int("buffer_ms")?,
            })
        }
        other => bail!("unsupported patchbay impair value: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network_lab::{FirewallPreset, NatPreset};

    fn sample_topology() -> TopologySpec {
        TopologySpec {
            name: "sample".to_string(),
            routers: vec![RouterSpec {
                name: "home-a".to_string(),
                nat: NatPreset::Home,
                upstream: None,
                firewall: FirewallPreset::Home,
            }],
            links: vec![LinkSpec {
                left: "alice-dev".to_string(),
                right: "home-a".to_string(),
                condition: LinkConditionPreset::Wifi,
            }],
            authorities: vec![AuthoritySpec {
                authority_id: "alice".to_string(),
                device_name: "alice-dev".to_string(),
                gateway_router: "home-a".to_string(),
                bind_address: "10.0.0.10:44001".to_string(),
                relay_capable: true,
                env: std::collections::BTreeMap::default(),
            }],
            required_relay_authority: Some("alice".to_string()),
        }
    }

    #[test]
    fn topology_toml_roundtrip_contains_required_fields() {
        let toml = topology_to_toml(&sample_topology()).unwrap();
        assert!(toml.contains("required_relay_authority"));
        assert!(toml.contains("alice-dev"));
    }
}

#[cfg(all(test, target_os = "linux", feature = "patchbay-backend"))]
mod impair_value_tests {
    use super::*;

    #[test]
    fn named_presets_round_trip_as_patchbay_tokens() {
        for preset in [
            LinkConditionPreset::Lan,
            LinkConditionPreset::Wifi,
            LinkConditionPreset::WifiBad,
            LinkConditionPreset::Mobile4G,
            LinkConditionPreset::Mobile3G,
            LinkConditionPreset::Satellite,
        ] {
            let value = to_patchbay_impair_value(preset);
            assert!(matches!(value, toml::Value::String(_)), "{preset:?}");
            assert_eq!(from_patchbay_impair_value(&value).unwrap(), preset);
        }
    }

    #[test]
    fn stress_presets_serialize_their_effective_parameters() {
        let bursty = to_patchbay_impair_value(LinkConditionPreset::WifiBursty);
        let table = bursty.as_table().expect("table");
        assert_eq!(table["loss_pct"].as_float(), Some(3.0));
        assert!(
            table.contains_key("loss_burst_pkts"),
            "bursty loss selects a burst length"
        );
        assert_eq!(table["buffer_ms"].as_integer(), Some(40));
        assert!(matches!(
            from_patchbay_impair_value(&bursty).unwrap(),
            LinkConditionPreset::Custom {
                loss_tenths_pct: 30,
                loss_burst_pkts: Some(_),
                buffer_ms: Some(40),
                ..
            }
        ));

        let congested = to_patchbay_impair_value(LinkConditionPreset::Mobile4GCongested);
        let table = congested.as_table().expect("table");
        assert_eq!(table["rate_kbit"].as_integer(), Some(8_000));
        assert_eq!(table["buffer_ms"].as_integer(), Some(60));
    }

    #[test]
    fn custom_round_trips_exactly() {
        let custom = LinkConditionPreset::Custom {
            latency_ms: 120,
            jitter_ms: 15,
            rate_kbit: Some(2_500),
            loss_tenths_pct: 7,
            loss_burst_pkts: Some(4),
            buffer_ms: Some(240),
        };
        let value = to_patchbay_impair_value(custom);
        assert_eq!(from_patchbay_impair_value(&value).unwrap(), custom);
    }

    #[test]
    fn restricted_nat_is_endpoint_independent_with_address_dependent_filtering() {
        let nat = to_patchbay_nat(crate::network_lab::NatPreset::Restricted).unwrap();
        let patchbay::Nat::Custom(config) = nat else {
            panic!("restricted is a custom config, got {nat:?}");
        };
        assert_eq!(config.mapping, patchbay::NatMapping::EndpointIndependent);
        assert_eq!(config.filtering, patchbay::NatFiltering::AddressDependent);
        assert_eq!(
            from_patchbay_nat(nat).unwrap(),
            crate::network_lab::NatPreset::Restricted
        );
    }
}
