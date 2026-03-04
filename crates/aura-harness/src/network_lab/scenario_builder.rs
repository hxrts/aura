use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::marker::PhantomData;

use anyhow::{bail, Result};

use super::{AuthoritySpec, LinkSpec, RouterSpec, TopologySpec};

pub struct Missing;
pub struct Ready;

/// Stateful topology builder with compile-time validity gates.
#[derive(Debug, Clone)]
pub struct ScenarioBuilder<R, M, G> {
    spec: TopologySpec,
    _relay_gate: PhantomData<R>,
    _mapping_gate: PhantomData<M>,
    _graph_gate: PhantomData<G>,
}

impl ScenarioBuilder<Missing, Missing, Missing> {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            spec: TopologySpec {
                name: name.into(),
                routers: Vec::new(),
                links: Vec::new(),
                authorities: Vec::new(),
                required_relay_authority: None,
            },
            _relay_gate: PhantomData,
            _mapping_gate: PhantomData,
            _graph_gate: PhantomData,
        }
    }
}

impl<R, M, G> ScenarioBuilder<R, M, G> {
    pub fn add_router(mut self, router: RouterSpec) -> Self {
        self.spec.routers.push(router);
        self
    }

    pub fn add_link(mut self, link: LinkSpec) -> Self {
        self.spec.links.push(link);
        self
    }
}

impl<M, G> ScenarioBuilder<Missing, M, G> {
    pub fn require_relay_path(
        mut self,
        authority_id: impl Into<String>,
    ) -> ScenarioBuilder<Ready, M, G> {
        self.spec.required_relay_authority = Some(authority_id.into());
        ScenarioBuilder {
            spec: self.spec,
            _relay_gate: PhantomData,
            _mapping_gate: PhantomData,
            _graph_gate: PhantomData,
        }
    }
}

impl<R, G> ScenarioBuilder<R, Missing, G> {
    pub fn with_authorities(
        mut self,
        authorities: Vec<AuthoritySpec>,
    ) -> Result<ScenarioBuilder<R, Ready, G>> {
        if authorities.is_empty() {
            bail!("authority-device mapping is empty");
        }
        self.spec.authorities = authorities;
        ensure_complete_authority_mapping(&self.spec)?;

        Ok(ScenarioBuilder {
            spec: self.spec,
            _relay_gate: PhantomData,
            _mapping_gate: PhantomData,
            _graph_gate: PhantomData,
        })
    }
}

impl<R, M> ScenarioBuilder<R, M, Missing> {
    pub fn with_connected_graph(self) -> Result<ScenarioBuilder<R, M, Ready>> {
        validate_graph_connectivity(&self.spec)?;
        Ok(ScenarioBuilder {
            spec: self.spec,
            _relay_gate: PhantomData,
            _mapping_gate: PhantomData,
            _graph_gate: PhantomData,
        })
    }
}

impl ScenarioBuilder<Ready, Ready, Ready> {
    pub fn build(self) -> Result<TopologySpec> {
        validate_topology(&self.spec)?;
        Ok(self.spec)
    }
}

/// Runtime validator used by backends before provisioning.
pub fn validate_topology(spec: &TopologySpec) -> Result<()> {
    if spec.name.trim().is_empty() {
        bail!("topology name must be non-empty");
    }

    ensure_complete_authority_mapping(spec)?;
    ensure_relay_requirement(spec)?;
    validate_graph_connectivity(spec)?;

    Ok(())
}

fn ensure_complete_authority_mapping(spec: &TopologySpec) -> Result<()> {
    let mut authorities = BTreeSet::new();
    let mut devices = BTreeSet::new();
    let routers: BTreeSet<_> = spec
        .routers
        .iter()
        .map(|router| router.name.as_str())
        .collect();

    for authority in &spec.authorities {
        if authority.authority_id.trim().is_empty() {
            bail!("authority_id must be non-empty");
        }
        if !authorities.insert(authority.authority_id.as_str()) {
            bail!("duplicate authority mapping: {}", authority.authority_id);
        }
        if !devices.insert(authority.device_name.as_str()) {
            bail!(
                "device {} mapped to multiple authorities",
                authority.device_name
            );
        }
        if !routers.contains(authority.gateway_router.as_str()) {
            bail!(
                "authority {} maps to unknown router {}",
                authority.authority_id,
                authority.gateway_router
            );
        }
    }

    Ok(())
}

fn ensure_relay_requirement(spec: &TopologySpec) -> Result<()> {
    let Some(required) = &spec.required_relay_authority else {
        bail!("scenario is missing required relay path authority");
    };

    let Some(authority) = spec
        .authorities
        .iter()
        .find(|authority| authority.authority_id == *required)
    else {
        bail!("required relay authority {required} is not mapped");
    };

    if !authority.relay_capable {
        bail!("required relay authority {required} is not marked relay_capable");
    }

    Ok(())
}

fn validate_graph_connectivity(spec: &TopologySpec) -> Result<()> {
    if spec.routers.is_empty() {
        bail!("topology graph requires at least one router");
    }

    let router_names: Vec<_> = spec
        .routers
        .iter()
        .map(|router| router.name.clone())
        .collect();
    let router_set: BTreeSet<_> = router_names.iter().map(String::as_str).collect();

    for router in &spec.routers {
        if let Some(upstream) = &router.upstream {
            if !router_set.contains(upstream.as_str()) {
                bail!(
                    "router {} references unknown upstream {}",
                    router.name,
                    upstream
                );
            }
        }
    }

    let mut adjacency: BTreeMap<&str, BTreeSet<&str>> = router_set
        .iter()
        .map(|router| (*router, BTreeSet::new()))
        .collect();

    for router in &spec.routers {
        if let Some(upstream) = &router.upstream {
            let Some(router_neighbors) = adjacency.get_mut(router.name.as_str()) else {
                bail!(
                    "internal topology adjacency missing router {}",
                    router.name.as_str()
                );
            };
            router_neighbors.insert(upstream.as_str());

            let Some(upstream_neighbors) = adjacency.get_mut(upstream.as_str()) else {
                bail!(
                    "internal topology adjacency missing upstream {}",
                    upstream.as_str()
                );
            };
            upstream_neighbors.insert(router.name.as_str());
        }
    }

    if let Some(first) = spec.routers.first() {
        let mut queue = VecDeque::new();
        let mut visited = BTreeSet::new();
        queue.push_back(first.name.as_str());

        while let Some(router) = queue.pop_front() {
            if !visited.insert(router) {
                continue;
            }
            if let Some(neighbors) = adjacency.get(router) {
                for neighbor in neighbors {
                    queue.push_back(neighbor);
                }
            }
        }

        if visited.len() != spec.routers.len() {
            bail!(
                "topology graph is disconnected: visited {} of {} routers",
                visited.len(),
                spec.routers.len()
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network_lab::{
        AuthoritySpec, FirewallPreset, LinkConditionPreset, NatPreset, RouterSpec,
    };

    fn router(name: &str, upstream: Option<&str>) -> RouterSpec {
        RouterSpec {
            name: name.to_string(),
            nat: NatPreset::Home,
            upstream: upstream.map(ToString::to_string),
            firewall: FirewallPreset::Home,
        }
    }

    fn authority(id: &str, device: &str, gateway: &str, relay: bool) -> AuthoritySpec {
        AuthoritySpec {
            authority_id: id.to_string(),
            device_name: device.to_string(),
            gateway_router: gateway.to_string(),
            bind_address: "10.0.0.1:44001".to_string(),
            relay_capable: relay,
            env: std::collections::BTreeMap::default(),
        }
    }

    #[test]
    fn builder_enforces_all_validity_gates() {
        let built = ScenarioBuilder::new("home-home")
            .add_router(router("home-a", None))
            .add_router(router("home-b", Some("home-a")))
            .add_link(LinkSpec {
                left: "alice-dev".to_string(),
                right: "home-a".to_string(),
                condition: LinkConditionPreset::Wifi,
            })
            .require_relay_path("relay")
            .with_authorities(vec![
                authority("relay", "relay-dev", "home-a", true),
                authority("alice", "alice-dev", "home-a", false),
            ])
            .unwrap()
            .with_connected_graph()
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(built.required_relay_authority.as_deref(), Some("relay"));
        assert_eq!(built.authorities.len(), 2);
    }

    #[test]
    fn runtime_validator_rejects_missing_relay_requirement() {
        let topology = TopologySpec {
            name: "invalid".to_string(),
            routers: vec![router("home-a", None)],
            links: Vec::new(),
            authorities: vec![authority("alice", "alice-dev", "home-a", true)],
            required_relay_authority: None,
        };

        let error = validate_topology(&topology).unwrap_err();
        assert!(error.to_string().contains("required relay path"));
    }
}
