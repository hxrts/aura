//! Allocation-owned secret lifetime channel. Ordinary secure storage never
//! exposes these owners. Effect implementations are a trusted boundary.
use crate::AuraError;
use async_trait::async_trait;
use std::sync::Arc;

/// Maximum plaintext size for one allocation-owned secret.
pub const MAX_SECRET_BYTES: usize = 4096;
/// Maximum immutable birth-scope encoding size.
pub const MAX_SCOPE_BYTES: usize = 1024;
/// Maximum retained first-decision encoding size.
pub const MAX_DECISION_BYTES: usize = 131_072;
/// Maximum original profile allocation inventory size.
pub const MAX_PROFILE_ALLOCATION_COUNT: usize = 4096;

/// Pure routing data. Deserialization grants no lifetime authority.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretAllocationReference {
    /// Provider-issued original allocation identity.
    pub allocation: [u8; 32],
    /// Immutable scope observation, not an authorization.
    pub scope: Vec<u8>,
}

/// Provider-authenticated state read. Domain metadata is not an authorization.
pub enum SecretLifetimeState {
    /// No durable first decision.
    Live,
    /// Required durable positive state.
    Positive {
        /// Original retained first-decision bytes.
        decision: Vec<u8>,
    },
    /// Required durable negative state.
    Negative {
        /// Original retained first-decision bytes.
        decision: Vec<u8>,
    },
    /// Required durable retired state.
    Retired {
        /// Original retained first-decision bytes.
        decision: Vec<u8>,
    },
}

/// Private provider objects implement this infrastructure boundary. A custom
/// implementation controls only its own backend. Real production providers do
/// not expose this object or accept capabilities created for another object.
#[async_trait]
pub trait SecretLifetimeBackend: Send + Sync {
    /// Observe original routing identity without changing custody.
    fn reference(&self) -> &SecretAllocationReference;
    /// Read authenticated original allocation state.
    async fn state(&self) -> Result<SecretLifetimeState, AuraError>;
    /// Read plaintext only while the original allocation is live.
    async fn read_live_secret(&self) -> Result<Vec<u8>, AuraError>;
    /// Require durable positive first-decision acknowledgment.
    async fn decide_positive(&self, decision: &[u8]) -> Result<(), AuraError>;
    /// Require durable negative first-decision acknowledgment.
    async fn decide_negative(&self, decision: &[u8]) -> Result<(), AuraError>;
    /// Require original negative tombstone publication acknowledgment.
    async fn acknowledge_retirement(&self, decision: &[u8]) -> Result<(), AuraError>;
}

/// Opaque actual provider-owner identity. This binds custody, not domain authority.
#[derive(Clone, Debug)]
pub struct SecretLifetimeProviderIdentity(std::sync::Arc<()>);
impl SecretLifetimeProviderIdentity {
    /// Trusted effect implementation seam; new identities cannot match a real
    /// provider's private identity. Custom providers control only their backend.
    pub fn new_trusted_provider_identity() -> Self {
        Self(std::sync::Arc::new(()))
    }
    fn same_owner(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}
/// Owned whole-profile recovery, never an existing-record selector API.
#[async_trait]
pub trait ProfileSecretLifetimeBackend: Send + Sync {
    /// Original concrete physical provider-owner identity, never a routing ID.
    fn provider_identity(&self) -> &SecretLifetimeProviderIdentity;
    /// Acknowledge a fresh bounded original birth before returning custody.
    async fn allocate(
        &self,
        scope: &[u8],
        secret: &[u8],
    ) -> Result<Arc<dyn SecretLifetimeBackend>, AuraError>;
    /// Transfer authenticated whole original inventory custody.
    async fn recover_owned_inventory(
        &self,
    ) -> Result<Vec<Arc<dyn SecretLifetimeBackend>>, AuraError>;
}

/// Existing record references cannot recover a selected-provider lifetime owner.
/// ```compile_fail
/// use aura_core::effects::secret_lifetime::{ProfileSecretLifetimeRecoveryCapability,SecretAllocationReference};
/// async fn select(root:&mut ProfileSecretLifetimeRecoveryCapability,reference:SecretAllocationReference) {
///     let _owner=root.recover_selected_record(reference).await;
/// }
/// ```
/// Move-owned handoff created with a selected-profile writer before it is
/// shared. Runtime assembly retains this separately from ordinary effect APIs.
pub struct ProfileSecretLifetimeRecoveryCapability {
    backend: Box<dyn ProfileSecretLifetimeBackend>,
    inventory_taken: bool,
    handed_out: std::collections::BTreeSet<[u8; 32]>,
}
impl ProfileSecretLifetimeRecoveryCapability {
    /// Check actual selected provider custody without exporting its identity.
    pub fn belongs_to_provider(&self, provider: &SecretLifetimeProviderIdentity) -> bool {
        self.backend.provider_identity().same_owner(provider)
    }

    /// Effect-provider implementation entrypoint, not a storage locator factory.
    /// A fabricated provider acts only on that fabricated backend; it cannot
    /// access the private physical backend of the selected production provider.
    pub fn from_trusted_provider(backend: Box<dyn ProfileSecretLifetimeBackend>) -> Self {
        Self {
            backend,
            inventory_taken: false,
            handed_out: std::collections::BTreeSet::new(),
        }
    }
    /// Transfer authenticated whole original inventory custody.
    pub async fn recover_owned_inventory(&mut self) -> Result<Vec<SecretLifetimeOwner>, AuraError> {
        if self.inventory_taken {
            return Err(AuraError::invalid(
                "profile lifetime inventory already transferred",
            ));
        }
        let backends = self.backend.recover_owned_inventory().await?;
        if backends.len() > MAX_PROFILE_ALLOCATION_COUNT {
            return Err(AuraError::invalid("oversized owned secret inventory"));
        }
        self.inventory_taken = true;
        self.handed_out
            .extend(backends.iter().map(|owner| owner.reference().allocation));
        Ok(backends
            .into_iter()
            .map(|backend| SecretLifetimeOwner { backend })
            .collect())
    }
    /// Reconcile births left by a required provider error. This is a delta from
    /// the SAME actor-owned whole-profile channel, not selector recovery or
    /// issuance of another root. Previously transferred owners never reappear.
    pub async fn reconcile_unhanded_births(
        &mut self,
    ) -> Result<Vec<SecretLifetimeOwner>, AuraError> {
        if !self.inventory_taken {
            return Err(AuraError::invalid("original inventory was not transferred"));
        }
        let original = self.backend.recover_owned_inventory().await?;
        if original.len() > MAX_PROFILE_ALLOCATION_COUNT {
            return Err(AuraError::invalid(
                "oversized original birth reconciliation",
            ));
        }
        let mut delta = Vec::new();
        for backend in original {
            if self.handed_out.insert(backend.reference().allocation) {
                delta.push(SecretLifetimeOwner { backend });
            }
        }
        Ok(delta)
    }
    /// Acknowledge a fresh bounded original birth before returning custody.
    pub async fn allocate(
        &mut self,
        scope: &[u8],
        secret: &[u8],
    ) -> Result<SecretLifetimeOwner, AuraError> {
        if scope.is_empty()
            || scope.len() > MAX_SCOPE_BYTES
            || secret.is_empty()
            || secret.len() > MAX_SECRET_BYTES
        {
            return Err(AuraError::invalid("invalid owned secret birth shape"));
        }
        let backend = self.backend.allocate(scope, secret).await?;
        self.handed_out.insert(backend.reference().allocation);
        Ok(SecretLifetimeOwner { backend })
    }
}

/// Opaque actual allocation custody; no constructor, clone, or serde contract.
/// Domain code keeps this private and checks its held domain owner before use.
pub struct SecretLifetimeOwner {
    backend: Arc<dyn SecretLifetimeBackend>,
}
impl SecretLifetimeOwner {
    /// Observe original routing identity without changing custody.
    pub fn reference(&self) -> &SecretAllocationReference {
        self.backend.reference()
    }
    /// Read authenticated original allocation state.
    pub async fn state(&self) -> Result<SecretLifetimeState, AuraError> {
        self.backend.state().await
    }
    /// Read plaintext only while the original allocation is live.
    pub async fn read_live_secret(&self) -> Result<Vec<u8>, AuraError> {
        self.backend.read_live_secret().await
    }
    /// Require durable positive first-decision acknowledgment.
    pub async fn decide_positive(&self, decision: &[u8]) -> Result<(), AuraError> {
        bounded_decision(decision)?;
        self.backend.decide_positive(decision).await
    }
    /// Only required durable negative CAS acknowledgment mints this capability.
    /// A borrowed operation preserves recovery custody across future cancellation.
    pub async fn decide_negative(
        &self,
        decision: &[u8],
    ) -> Result<NegativeSecretDecisionCapability, AuraError> {
        bounded_decision(decision)?;
        self.backend.decide_negative(decision).await?;
        Ok(NegativeSecretDecisionCapability {
            backend: self.backend.clone(),
            decision: decision.to_vec(),
        })
    }
}
fn bounded_decision(decision: &[u8]) -> Result<(), AuraError> {
    if decision.is_empty() || decision.len() > MAX_DECISION_BYTES {
        return Err(AuraError::invalid("invalid secret first-decision shape"));
    }
    Ok(())
}

/// Original backend custody cannot be rebound to a different storage receiver.
/// ```compile_fail
/// use aura_core::effects::secret_lifetime::{NegativeSecretDecisionCapability,SecretAllocationReference};
/// async fn retarget(negative:&NegativeSecretDecisionCapability,reference:SecretAllocationReference) {
///     negative.retire(reference).await;
/// }
/// ```
/// Genuine provider negative-decision custody, never a domain reason claim.
/// ```compile_fail
/// use aura_core::effects::secret_lifetime::NegativeSecretDecisionCapability;
/// fn forge(bytes: &[u8]) -> NegativeSecretDecisionCapability {
///     serde_json::from_slice(bytes).unwrap()
/// }
/// ```
pub struct NegativeSecretDecisionCapability {
    backend: Arc<dyn SecretLifetimeBackend>,
    decision: Vec<u8>,
}
impl NegativeSecretDecisionCapability {
    /// Dispatches through the retained original provider object, not an
    /// arbitrary effect receiver or caller-supplied location.
    pub async fn retire(&self) -> Result<AcknowledgedSecretRetirementCapability, AuraError> {
        self.backend.acknowledge_retirement(&self.decision).await?;
        Ok(AcknowledgedSecretRetirementCapability {
            reference: self.backend.reference().clone(),
        })
    }
}
/// Process-local ACK from the actual retirement operation, not a decoded receipt.
pub struct AcknowledgedSecretRetirementCapability {
    reference: SecretAllocationReference,
}
impl AcknowledgedSecretRetirementCapability {
    /// Observe original routing identity without changing custody.
    pub fn reference(&self) -> &SecretAllocationReference {
        &self.reference
    }
}
#[cfg(test)]
mod lifetime_type_guards {
    use super::*;
    #[test]
    fn provider_lifetime_authority_has_no_clone_or_deserialization() {
        trait AmbiguousIfClone<M> {
            fn absent() {}
        }
        impl<T: ?Sized> AmbiguousIfClone<()> for T {}
        struct Cloned;
        impl<T: Clone> AmbiguousIfClone<Cloned> for T {}
        trait AmbiguousIfDeserialize<M> {
            fn absent() {}
        }
        impl<T: ?Sized> AmbiguousIfDeserialize<()> for T {}
        struct Decoded;
        impl<T: serde::Deserialize<'static>> AmbiguousIfDeserialize<Decoded> for T {}
        let _ = <ProfileSecretLifetimeRecoveryCapability as AmbiguousIfClone<_>>::absent;
        let _ = <SecretLifetimeOwner as AmbiguousIfClone<_>>::absent;
        let _ = <NegativeSecretDecisionCapability as AmbiguousIfClone<_>>::absent;
        let _ = <AcknowledgedSecretRetirementCapability as AmbiguousIfClone<_>>::absent;
        let _ = <ProfileSecretLifetimeRecoveryCapability as AmbiguousIfDeserialize<_>>::absent;
        let _ = <SecretLifetimeOwner as AmbiguousIfDeserialize<_>>::absent;
        let _ = <NegativeSecretDecisionCapability as AmbiguousIfDeserialize<_>>::absent;
        let _ = <AcknowledgedSecretRetirementCapability as AmbiguousIfDeserialize<_>>::absent;
    }
}

/// Structural selected-provider availability, never authority or a retry permit.
#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum SecretLifetimeProviderUnavailable {
    /// The selected backend has no acknowledged lifetime implementation.
    #[error("selected provider has no acknowledged allocation lifetime implementation")]
    UnsupportedSelectedProvider,
    /// Original selected custody was not handed to the actual runtime.
    #[error("runtime has no original selected allocation lifetime custody")]
    MissingSelectedCustody,
    /// Existing permanent records require separately proved migration.
    #[error("selected legacy profile requires original allocation lifetime migration")]
    LegacyMigrationRequired,
}
impl SecretLifetimeProviderUnavailable {
    /// Preserve this concrete availability source without mislabeling it IO.
    pub fn into_aura_error(self) -> AuraError {
        AuraError::Internal {
            message: self.to_string(),
            source: Some(Arc::new(self)),
        }
    }
}
