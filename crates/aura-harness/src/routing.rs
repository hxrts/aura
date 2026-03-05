//! Network address resolution and SSH tunnel mapping.
//!
//! Resolves advertised peer addresses to reachable endpoints, handling SSH tunnels
//! and network topology differences between local and remote instances.

use serde::{Deserialize, Serialize};

use crate::config::InstanceConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TunnelMapping {
    pub local_host: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedDialPath {
    pub instance_id: String,
    pub advertised_address: String,
    pub resolved_address: String,
    pub route: String,
}

pub struct AddressResolver;

impl AddressResolver {
    pub fn tunnel_mappings(instance: &InstanceConfig) -> Vec<TunnelMapping> {
        let Some(tunnel) = &instance.tunnel else {
            return Vec::new();
        };

        if tunnel.kind != "ssh" {
            return Vec::new();
        }

        tunnel
            .local_forward
            .iter()
            .filter_map(|mapping| parse_local_forward(mapping))
            .collect()
    }

    pub fn resolve(instance: &InstanceConfig, advertised_address: &str) -> ResolvedDialPath {
        let mappings = Self::tunnel_mappings(instance);

        for mapping in &mappings {
            let remote = format!("{}:{}", mapping.remote_host, mapping.remote_port);
            if remote == advertised_address {
                let resolved = format!("{}:{}", mapping.local_host, mapping.local_port);
                return ResolvedDialPath {
                    instance_id: instance.id.clone(),
                    advertised_address: advertised_address.to_string(),
                    resolved_address: resolved,
                    route: "ssh_tunnel_rewrite".to_string(),
                };
            }
        }

        ResolvedDialPath {
            instance_id: instance.id.clone(),
            advertised_address: advertised_address.to_string(),
            resolved_address: advertised_address.to_string(),
            route: "direct".to_string(),
        }
    }
}

fn parse_local_forward(mapping: &str) -> Option<TunnelMapping> {
    let mut parts = mapping.split(':');
    let local_port = parts.next()?.parse::<u16>().ok()?;
    let remote_host = parts.next()?.to_string();
    let remote_port = parts.next()?.parse::<u16>().ok()?;

    if parts.next().is_some() {
        return None;
    }

    Some(TunnelMapping {
        local_host: "127.0.0.1".to_string(),
        local_port,
        remote_host,
        remote_port,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::{InstanceMode, TunnelConfig};

    #[test]
    fn address_resolver_rewrites_mapped_tunnel_address() {
        let instance = InstanceConfig {
            id: "bob".to_string(),
            mode: InstanceMode::Ssh,
            data_dir: PathBuf::from("/tmp/bob"),
            device_id: None,
            bind_address: "0.0.0.0:41001".to_string(),
            demo_mode: false,
            command: None,
            args: vec![],
            env: vec![],
            log_path: None,
            ssh_host: Some("devbox-b".to_string()),
            ssh_user: Some("dev".to_string()),
            ssh_port: Some(22),
            ssh_strict_host_key_checking: true,
            ssh_known_hosts_file: None,
            ssh_fingerprint: Some("SHA256:test".to_string()),
            ssh_require_fingerprint: false,
            ssh_dry_run: true,
            remote_workdir: Some(PathBuf::from("/home/dev/aura")),
            lan_discovery: None,
            tunnel: Some(TunnelConfig {
                kind: "ssh".to_string(),
                local_forward: vec!["54101:127.0.0.1:41001".to_string()],
            }),
        };

        let resolved = AddressResolver::resolve(&instance, "127.0.0.1:41001");
        assert_eq!(resolved.resolved_address, "127.0.0.1:54101");
        assert_eq!(resolved.route, "ssh_tunnel_rewrite");
    }
}
