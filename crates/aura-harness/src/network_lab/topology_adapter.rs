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
                .map(|router| RouterConfigDoc {
                    name: router.name.clone(),
                    region: None,
                    upstream: router.upstream.clone(),
                    nat: to_patchbay_nat(router.nat),
                    ip_support: patchbay::IpSupport::V4Only,
                    nat_v6: patchbay::NatV6Mode::None,
                    ra_enabled: None,
                    ra_interval_secs: None,
                    ra_lifetime_secs: None,
                })
                .collect();

            let mut device = BTreeMap::new();
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
                if let Some(condition) = find_device_link_condition(spec, authority) {
                    iface.insert(
                        "impair".to_string(),
                        toml::Value::String(to_patchbay_impair_token(condition).to_string()),
                    );
                }
                table.insert("eth0".to_string(), toml::Value::Table(iface));

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

                if let Some(impair) = iface.get("impair").and_then(toml::Value::as_str) {
                    links.push(LinkSpec {
                        left: device_name.clone(),
                        right: gateway.to_string(),
                        condition: from_patchbay_impair_token(impair)?,
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
        /// `Cgnat` follows patchbay's own `IspCgnat` router preset rather than
        /// its `Cgnat -> Open` migration alias: compliant carrier NAT filters on
        /// address and port, and the Aura double-NAT scenarios depend on that.
        pub fn to_patchbay_nat(nat: super::NatPreset) -> patchbay::Nat {
            match nat {
                super::NatPreset::None => patchbay::Nat::None,
                super::NatPreset::FullCone => patchbay::Nat::Open,
                super::NatPreset::Home | super::NatPreset::Cgnat => patchbay::Nat::Moderate,
                super::NatPreset::Corporate | super::NatPreset::CloudNat => patchbay::Nat::Strict,
            }
        }

        /// Inverse of [`to_patchbay_nat`] for the representative Aura preset of
        /// each patchbay tier. The mapping is lossy, so `Moderate` reads back as
        /// `Home` and `Strict` as `Corporate`. Custom configurations have no
        /// Aura preset and are rejected.
        fn from_patchbay_nat(nat: patchbay::Nat) -> Result<super::NatPreset> {
            match nat {
                patchbay::Nat::None => Ok(super::NatPreset::None),
                patchbay::Nat::Open => Ok(super::NatPreset::FullCone),
                patchbay::Nat::Moderate => Ok(super::NatPreset::Home),
                patchbay::Nat::Strict => Ok(super::NatPreset::Corporate),
                patchbay::Nat::Custom(config) => {
                    bail!("custom patchbay NAT config has no Aura NatPreset: {config:?}")
                }
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

pub fn to_patchbay_impair_token(condition: LinkConditionPreset) -> &'static str {
    match condition {
        LinkConditionPreset::Lan => "lan",
        LinkConditionPreset::Wifi => "wifi",
        LinkConditionPreset::WifiBad => "wifi-bad",
        LinkConditionPreset::Mobile4G => "mobile-4g",
        LinkConditionPreset::Mobile3G => "mobile-3g",
        LinkConditionPreset::Satellite => "satellite",
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
