//! Provider-owned retirement of immutable secure records.
//!
//! Ordinary secure-storage capabilities cannot delete protected records. A
//! selected profile provider may instead transfer its whole original retirement
//! inventory as opaque owners. Each owner decides expiry using its retained
//! physical-time provider and exact protected record, never a caller timestamp
//! or a storage location supplied at retirement.

use super::secret_lifetime::SecretLifetimeProviderIdentity;
use crate::AuraError;
use async_trait::async_trait;
use std::collections::BTreeSet;
use std::sync::Arc;

/// Upper bound on one selected profile's protected retirement inventory.
pub const MAX_PROTECTED_RETIREMENT_RECORD_COUNT: usize = 4096;

/// Pure inventory identity. It cannot select or retire an existing record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtectedRetirementReference {
    /// Original provider-issued inventory identity.
    pub inventory: [u8; 32],
}

/// The selected provider retains exact record bytes and the original expiry.
/// Implementations must compare and remove under their namespace mutation
/// owner, and acknowledge a durable result before returning success.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ProtectedRetirementBackend: Send + Sync {
    /// Exact original profile provider that owns this retirement state.
    fn provider_identity(&self) -> &SecretLifetimeProviderIdentity;

    /// Provider-issued identity for diagnostics and inventory matching only.
    fn reference(&self) -> &ProtectedRetirementReference;

    /// Retire only after this backend's original selected physical provider
    /// observes the sealed expiry. A rollback, unavailable clock, wrong exact
    /// bytes, or incomplete cleanup is an error, never successful retirement.
    async fn retire_after_selected_expiry(&self) -> Result<(), AuraError>;
}

/// Private effect-provider object for an owned whole-profile inventory.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ProfileProtectedRetirementBackend: Send + Sync {
    /// Same physical profile identity as its selected secure-storage writer.
    fn provider_identity(&self) -> &SecretLifetimeProviderIdentity;

    /// Recover every original owner without accepting a caller-selected key.
    /// Interrupted child cleanup must reappear with its original identity and
    /// exact protected bytes; a missing child alone never implies completion.
    async fn recover_owned_inventory(
        &self,
    ) -> Result<Vec<Arc<dyn ProtectedRetirementBackend>>, AuraError>;
}

/// Move-owned whole-profile recovery. Public routing references cannot select
/// an owner, and no generic Delete capability can create this value.
///
/// ```compile_fail
/// use aura_core::effects::protected_retirement::{ProtectedRetirementRecoveryCapability, ProtectedRetirementReference};
/// async fn select(root: &mut ProtectedRetirementRecoveryCapability, reference: ProtectedRetirementReference) {
///     let _owner = root.recover_selected_record(reference).await;
/// }
/// ```
pub struct ProtectedRetirementRecoveryCapability {
    backend: Box<dyn ProfileProtectedRetirementBackend>,
    inventory_taken: bool,
    handed_out: BTreeSet<[u8; 32]>,
}

impl ProtectedRetirementRecoveryCapability {
    /// Effect-provider entrypoint. A fabricated backend controls only its own
    /// records and cannot match a real selected profile's private identity.
    pub fn from_trusted_provider(backend: Box<dyn ProfileProtectedRetirementBackend>) -> Self {
        Self {
            backend,
            inventory_taken: false,
            handed_out: BTreeSet::new(),
        }
    }

    /// Require the actual selected secure-storage provider before handoff.
    pub fn belongs_to_provider(&self, selected: &SecretLifetimeProviderIdentity) -> bool {
        self.backend.provider_identity().same_owner(selected)
    }

    /// Transfer the whole original inventory exactly once.
    pub async fn recover_owned_inventory(
        &mut self,
    ) -> Result<Vec<ProtectedRetirementOwner>, AuraError> {
        if self.inventory_taken {
            return Err(AuraError::invalid(
                "protected retirement inventory already transferred",
            ));
        }
        let owners = self.backend.recover_owned_inventory().await?;
        if owners.len() > MAX_PROTECTED_RETIREMENT_RECORD_COUNT {
            return Err(AuraError::invalid(
                "protected retirement inventory exceeded",
            ));
        }
        if owners.iter().any(|owner| {
            !owner
                .provider_identity()
                .same_owner(self.backend.provider_identity())
        }) {
            return Err(AuraError::permission_denied(
                "protected retirement owner belongs to another profile",
            ));
        }
        let identities: BTreeSet<_> = owners
            .iter()
            .map(|owner| owner.reference().inventory)
            .collect();
        if identities.len() != owners.len() {
            return Err(AuraError::invalid(
                "protected retirement inventory contains duplicate owners",
            ));
        }
        self.inventory_taken = true;
        self.handed_out = identities;
        Ok(owners
            .into_iter()
            .map(|backend| ProtectedRetirementOwner { backend })
            .collect())
    }

    /// Reconcile only newly sealed owners from the same physical profile.
    /// Previously transferred owners cannot be recreated by a selector.
    pub async fn reconcile_unhanded_births(
        &mut self,
    ) -> Result<Vec<ProtectedRetirementOwner>, AuraError> {
        if !self.inventory_taken {
            return Err(AuraError::invalid(
                "protected retirement inventory was not transferred",
            ));
        }
        let owners = self.backend.recover_owned_inventory().await?;
        if owners.len() > MAX_PROTECTED_RETIREMENT_RECORD_COUNT {
            return Err(AuraError::invalid(
                "protected retirement inventory exceeded",
            ));
        }
        if owners.iter().any(|owner| {
            !owner
                .provider_identity()
                .same_owner(self.backend.provider_identity())
        }) {
            return Err(AuraError::permission_denied(
                "protected retirement owner belongs to another profile",
            ));
        }
        let seen: BTreeSet<_> = owners
            .iter()
            .map(|owner| owner.reference().inventory)
            .collect();
        if seen.len() != owners.len() {
            return Err(AuraError::invalid(
                "protected retirement inventory contains duplicate owners",
            ));
        }
        let mut delta = Vec::new();
        for backend in owners {
            let identity = backend.reference().inventory;
            if self.handed_out.insert(identity) {
                delta.push(ProtectedRetirementOwner { backend });
            }
        }
        Ok(delta)
    }
}

/// An original provider-issued owner; no public constructor or raw delete.
///
/// ```compile_fail
/// use aura_core::effects::protected_retirement::{ProtectedRetirementOwner, ProtectedRetirementReference};
/// fn forge(reference: ProtectedRetirementReference) -> ProtectedRetirementOwner {
///     ProtectedRetirementOwner { reference }
/// }
/// ```
/// ```compile_fail
/// use aura_core::effects::{protected_retirement::ProtectedRetirementOwner, SecureStorageLocation};
/// async fn caller_selected_expiry(owner: &ProtectedRetirementOwner, location: SecureStorageLocation, now_ms: u64) {
///     owner.retire_after_selected_expiry(&location, now_ms).await.unwrap();
/// }
/// ```
/// ```compile_fail
/// use aura_core::effects::protected_retirement::ProtectedRetirementOwner;
/// async fn unverified_terminal(owner: &ProtectedRetirementOwner, observed_success: bool) {
///     owner.retire_after_terminal(observed_success).await.unwrap();
/// }
/// ```
pub struct ProtectedRetirementOwner {
    backend: Arc<dyn ProtectedRetirementBackend>,
}

impl ProtectedRetirementOwner {
    /// Observe the original opaque inventory identity.
    pub fn reference(&self) -> &ProtectedRetirementReference {
        self.backend.reference()
    }

    /// Ask the original provider to retire only after its selected expiry.
    pub async fn retire_after_selected_expiry(&self) -> Result<(), AuraError> {
        self.backend.retire_after_selected_expiry().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestOwner {
        reference: ProtectedRetirementReference,
        identity: SecretLifetimeProviderIdentity,
    }

    #[async_trait]
    impl ProtectedRetirementBackend for TestOwner {
        fn provider_identity(&self) -> &SecretLifetimeProviderIdentity {
            &self.identity
        }

        fn reference(&self) -> &ProtectedRetirementReference {
            &self.reference
        }

        async fn retire_after_selected_expiry(&self) -> Result<(), AuraError> {
            Ok(())
        }
    }

    struct TestProfile {
        identity: SecretLifetimeProviderIdentity,
        owner_identity: SecretLifetimeProviderIdentity,
    }

    #[async_trait]
    impl ProfileProtectedRetirementBackend for TestProfile {
        fn provider_identity(&self) -> &SecretLifetimeProviderIdentity {
            &self.identity
        }

        async fn recover_owned_inventory(
            &self,
        ) -> Result<Vec<Arc<dyn ProtectedRetirementBackend>>, AuraError> {
            Ok(vec![Arc::new(TestOwner {
                reference: ProtectedRetirementReference { inventory: [7; 32] },
                identity: self.owner_identity.clone(),
            })])
        }
    }

    #[tokio::test]
    async fn whole_profile_transfer_refuses_foreign_owner_and_second_recovery() {
        let selected = SecretLifetimeProviderIdentity::new_trusted_provider_identity();
        let foreign = SecretLifetimeProviderIdentity::new_trusted_provider_identity();
        let mut root =
            ProtectedRetirementRecoveryCapability::from_trusted_provider(Box::new(TestProfile {
                identity: selected.clone(),
                owner_identity: selected.clone(),
            }));
        assert!(root.belongs_to_provider(&selected));
        assert!(!root.belongs_to_provider(&foreign));
        let owners = root.recover_owned_inventory().await.unwrap();
        assert_eq!(owners.len(), 1);
        assert_eq!(owners[0].reference().inventory, [7; 32]);
        assert!(root.recover_owned_inventory().await.is_err());
        assert!(root.reconcile_unhanded_births().await.unwrap().is_empty());

        let mut foreign_inventory =
            ProtectedRetirementRecoveryCapability::from_trusted_provider(Box::new(TestProfile {
                identity: selected,
                owner_identity: foreign,
            }));
        assert!(foreign_inventory.recover_owned_inventory().await.is_err());
    }
}
