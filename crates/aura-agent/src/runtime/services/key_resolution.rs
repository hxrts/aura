//! Trusted public-key resolution for verifier boundaries.
//!
//! Remote messages may name an authority, device, guardian, release authority,
//! or epoch, but verification must resolve the expected key from trusted local
//! state. This service is the runtime-owned registry for that lookup.

use aura_core::{hash::hash, AuthorityId, DeviceId, Hash32};
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;

pub use aura_core::key_resolution::{
    KeyResolutionError, TrustedKeyDomain, TrustedKeyResolver, TrustedKeyStatus, TrustedPublicKey,
};

#[derive(Debug, Default)]
struct TrustedKeyRegistry {
    authority_threshold: BTreeMap<(AuthorityId, u64), TrustedPublicKey>,
    devices: BTreeMap<DeviceId, TrustedPublicKey>,
    guardians: BTreeMap<AuthorityId, TrustedPublicKey>,
    releases: BTreeMap<AuthorityId, TrustedPublicKey>,
}

/// Runtime-owned trusted key resolver.
#[derive(Debug, Clone, Default)]
pub struct TrustedKeyResolutionService {
    registry: Arc<RwLock<TrustedKeyRegistry>>,
}

impl TrustedKeyResolutionService {
    /// Create an empty trusted key resolver.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an authority threshold key for a specific epoch.
    ///
    /// Older active epochs for the same authority are marked rotated.
    pub fn register_authority_threshold_key(
        &self,
        authority: AuthorityId,
        epoch: u64,
        key: Vec<u8>,
    ) -> Result<(), KeyResolutionError> {
        ensure_key(TrustedKeyDomain::AuthorityThreshold, &key)?;
        let mut registry = self.registry.write();
        for ((stored_authority, stored_epoch), stored_key) in &mut registry.authority_threshold {
            if *stored_authority == authority
                && *stored_epoch < epoch
                && stored_key.status() == &TrustedKeyStatus::Active
            {
                stored_key.set_status(TrustedKeyStatus::Rotated {
                    replaced_by_epoch: Some(epoch),
                });
            }
        }
        registry.authority_threshold.insert(
            (authority, epoch),
            active_key(TrustedKeyDomain::AuthorityThreshold, Some(epoch), key),
        );
        Ok(())
    }

    /// Register or replace an enrolled device key.
    pub fn register_device_key(
        &self,
        device: DeviceId,
        key: Vec<u8>,
    ) -> Result<(), KeyResolutionError> {
        ensure_key(TrustedKeyDomain::Device, &key)?;
        self.registry
            .write()
            .devices
            .insert(device, active_key(TrustedKeyDomain::Device, None, key));
        Ok(())
    }

    /// Register or replace a guardian authority key.
    pub fn register_guardian_key(
        &self,
        guardian: AuthorityId,
        key: Vec<u8>,
    ) -> Result<(), KeyResolutionError> {
        ensure_key(TrustedKeyDomain::Guardian, &key)?;
        self.registry
            .write()
            .guardians
            .insert(guardian, active_key(TrustedKeyDomain::Guardian, None, key));
        Ok(())
    }

    /// Register or replace a release/OTA signing key.
    pub fn register_release_key(
        &self,
        authority: AuthorityId,
        key: Vec<u8>,
    ) -> Result<(), KeyResolutionError> {
        ensure_key(TrustedKeyDomain::Release, &key)?;
        self.registry
            .write()
            .releases
            .insert(authority, active_key(TrustedKeyDomain::Release, None, key));
        Ok(())
    }

    /// Revoke an authority threshold key for one epoch.
    pub fn revoke_authority_threshold_key(
        &self,
        authority: AuthorityId,
        epoch: u64,
        reason: impl Into<String>,
    ) -> Result<(), KeyResolutionError> {
        revoke(
            self.registry
                .write()
                .authority_threshold
                .get_mut(&(authority, epoch)),
            TrustedKeyDomain::AuthorityThreshold,
            reason,
        )
    }

    /// Revoke an enrolled device key.
    pub fn revoke_device_key(
        &self,
        device: DeviceId,
        reason: impl Into<String>,
    ) -> Result<(), KeyResolutionError> {
        revoke(
            self.registry.write().devices.get_mut(&device),
            TrustedKeyDomain::Device,
            reason,
        )
    }

    /// Revoke a guardian key.
    pub fn revoke_guardian_key(
        &self,
        guardian: AuthorityId,
        reason: impl Into<String>,
    ) -> Result<(), KeyResolutionError> {
        revoke(
            self.registry.write().guardians.get_mut(&guardian),
            TrustedKeyDomain::Guardian,
            reason,
        )
    }

    /// Revoke a release/OTA signing key.
    pub fn revoke_release_key(
        &self,
        authority: AuthorityId,
        reason: impl Into<String>,
    ) -> Result<(), KeyResolutionError> {
        revoke(
            self.registry.write().releases.get_mut(&authority),
            TrustedKeyDomain::Release,
            reason,
        )
    }

    /// Resolve an active authority threshold key for the requested epoch.
    pub fn resolve_authority_threshold_key(
        &self,
        authority: AuthorityId,
        epoch: u64,
    ) -> Result<TrustedPublicKey, KeyResolutionError> {
        let registry = self.registry.read();
        resolve(
            registry.authority_threshold.get(&(authority, epoch)),
            TrustedKeyDomain::AuthorityThreshold,
        )
    }

    /// Resolve an active enrolled device key.
    pub fn resolve_device_key(
        &self,
        device: DeviceId,
    ) -> Result<TrustedPublicKey, KeyResolutionError> {
        let registry = self.registry.read();
        resolve(registry.devices.get(&device), TrustedKeyDomain::Device)
    }

    /// Resolve an active guardian key.
    pub fn resolve_guardian_key(
        &self,
        guardian: AuthorityId,
    ) -> Result<TrustedPublicKey, KeyResolutionError> {
        let registry = self.registry.read();
        resolve(
            registry.guardians.get(&guardian),
            TrustedKeyDomain::Guardian,
        )
    }

    /// Resolve an active release/OTA signing key.
    pub fn resolve_release_key(
        &self,
        authority: AuthorityId,
    ) -> Result<TrustedPublicKey, KeyResolutionError> {
        let registry = self.registry.read();
        resolve(registry.releases.get(&authority), TrustedKeyDomain::Release)
    }
}

impl TrustedKeyResolver for TrustedKeyResolutionService {
    fn resolve_authority_threshold_key(
        &self,
        authority: AuthorityId,
        epoch: u64,
    ) -> Result<TrustedPublicKey, KeyResolutionError> {
        TrustedKeyResolutionService::resolve_authority_threshold_key(self, authority, epoch)
    }

    fn resolve_device_key(&self, device: DeviceId) -> Result<TrustedPublicKey, KeyResolutionError> {
        TrustedKeyResolutionService::resolve_device_key(self, device)
    }

    fn resolve_guardian_key(
        &self,
        guardian: AuthorityId,
    ) -> Result<TrustedPublicKey, KeyResolutionError> {
        TrustedKeyResolutionService::resolve_guardian_key(self, guardian)
    }

    fn resolve_release_key(
        &self,
        authority: AuthorityId,
    ) -> Result<TrustedPublicKey, KeyResolutionError> {
        TrustedKeyResolutionService::resolve_release_key(self, authority)
    }
}

fn ensure_key(domain: TrustedKeyDomain, key: &[u8]) -> Result<(), KeyResolutionError> {
    if key.is_empty() {
        return Err(KeyResolutionError::EmptyKey { domain });
    }
    Ok(())
}

fn resolve(
    key: Option<&TrustedPublicKey>,
    domain: TrustedKeyDomain,
) -> Result<TrustedPublicKey, KeyResolutionError> {
    let key = key.ok_or(KeyResolutionError::Unknown { domain })?;
    if key.status() != &TrustedKeyStatus::Active {
        return Err(KeyResolutionError::Inactive {
            domain,
            status: key.status().clone(),
        });
    }
    Ok(key.clone())
}

fn revoke(
    key: Option<&mut TrustedPublicKey>,
    domain: TrustedKeyDomain,
    reason: impl Into<String>,
) -> Result<(), KeyResolutionError> {
    let key = key.ok_or(KeyResolutionError::Unknown { domain })?;
    key.set_status(TrustedKeyStatus::Revoked {
        reason: reason.into(),
    });
    Ok(())
}

fn active_key(domain: TrustedKeyDomain, epoch: Option<u64>, key: Vec<u8>) -> TrustedPublicKey {
    let key_hash = Hash32::new(hash(&key));
    TrustedPublicKey::active(domain, epoch, key, key_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn device(seed: u8) -> DeviceId {
        DeviceId::new_from_entropy([seed; 32])
    }

    #[test]
    fn resolves_registered_key_domains() {
        let resolver = TrustedKeyResolutionService::new();
        let authority_id = authority(1);
        let guardian_id = authority(2);
        let release_id = authority(3);
        let device_id = device(4);

        resolver
            .register_authority_threshold_key(authority_id, 7, vec![7; 32])
            .unwrap();
        resolver
            .register_guardian_key(guardian_id, vec![8; 32])
            .unwrap();
        resolver
            .register_release_key(release_id, vec![9; 32])
            .unwrap();
        resolver
            .register_device_key(device_id, vec![10; 32])
            .unwrap();

        assert_eq!(
            resolver
                .resolve_authority_threshold_key(authority_id, 7)
                .unwrap()
                .bytes(),
            &[7; 32]
        );
        assert_eq!(
            resolver.resolve_guardian_key(guardian_id).unwrap().bytes(),
            &[8; 32]
        );
        assert_eq!(
            resolver.resolve_release_key(release_id).unwrap().bytes(),
            &[9; 32]
        );
        assert_eq!(
            resolver.resolve_device_key(device_id).unwrap().bytes(),
            &[10; 32]
        );
    }

    #[test]
    fn authority_epoch_rotation_rejects_stale_epoch() {
        let resolver = TrustedKeyResolutionService::new();
        let authority = authority(11);

        resolver
            .register_authority_threshold_key(authority, 1, vec![1; 32])
            .unwrap();
        resolver
            .register_authority_threshold_key(authority, 2, vec![2; 32])
            .unwrap();

        assert!(matches!(
            resolver.resolve_authority_threshold_key(authority, 1),
            Err(KeyResolutionError::Inactive {
                status: TrustedKeyStatus::Rotated {
                    replaced_by_epoch: Some(2)
                },
                ..
            })
        ));
        assert_eq!(
            resolver
                .resolve_authority_threshold_key(authority, 2)
                .unwrap()
                .bytes(),
            &[2; 32]
        );
    }

    #[test]
    fn revoked_keys_fail_closed() {
        let resolver = TrustedKeyResolutionService::new();
        let device = device(21);

        resolver.register_device_key(device, vec![3; 32]).unwrap();
        resolver.revoke_device_key(device, "compromised").unwrap();

        assert!(matches!(
            resolver.resolve_device_key(device),
            Err(KeyResolutionError::Inactive {
                status: TrustedKeyStatus::Revoked { .. },
                ..
            })
        ));
    }

    #[test]
    fn unknown_and_empty_keys_fail_closed() {
        let resolver = TrustedKeyResolutionService::new();

        assert_eq!(
            resolver.register_release_key(authority(31), Vec::new()),
            Err(KeyResolutionError::EmptyKey {
                domain: TrustedKeyDomain::Release
            })
        );
        assert_eq!(
            resolver.resolve_guardian_key(authority(32)),
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::Guardian
            })
        );
    }

    #[test]
    fn unknown_devices_and_wrong_release_authorities_fail_closed() {
        let resolver = TrustedKeyResolutionService::new();
        let release_authority = authority(41);
        let wrong_release_authority = authority(42);

        resolver
            .register_release_key(release_authority, vec![4; 32])
            .unwrap();

        assert_eq!(
            resolver.resolve_device_key(device(43)),
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::Device
            })
        );
        assert_eq!(
            resolver.resolve_release_key(wrong_release_authority),
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::Release
            })
        );
    }
}
