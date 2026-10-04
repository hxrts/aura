//! Crypto Subsystem
//!
//! Groups cryptographic-related fields from AuraEffectSystem:
//! - `crypto_handler`: Core cryptographic operations (signing, verification, hashing)
//! - `random_rng`: Cryptographically secure RNG for key generation
//! - `secure_storage_handler`: Secure storage for key material (FROST keys, device keys)
//!
//! ## Lock Usage
//!
//! Uses a thread-local RNG in production to avoid contention, and a
//! `parking_lot::Mutex`-backed deterministic RNG in tests/simulation.
//! RNG operations are synchronous and never held across async boundaries.

#![allow(clippy::disallowed_types)]

use aura_effects::{crypto::RealCryptoHandler, secure::ProductionSecureStorageHandler};
use parking_lot::Mutex;
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};
use std::cell::RefCell;
use std::sync::Arc;

thread_local! {
    static THREAD_RNG: RefCell<StdRng> = RefCell::new(StdRng::from_entropy());
}

#[derive(Debug)]
pub(crate) enum CryptoRng {
    Deterministic(Arc<Mutex<StdRng>>),
    ThreadLocal,
}

impl CryptoRng {
    pub(crate) fn deterministic(rng: StdRng) -> Self {
        CryptoRng::Deterministic(Arc::new(Mutex::new(rng)))
    }

    pub(crate) fn thread_local() -> Self {
        CryptoRng::ThreadLocal
    }

    fn fill_bytes(&self, bytes: &mut [u8]) {
        match self {
            CryptoRng::Deterministic(rng) => {
                let mut rng = rng.lock();
                rng.fill_bytes(bytes);
            }
            CryptoRng::ThreadLocal => {
                THREAD_RNG.with(|cell| {
                    let mut rng = cell.borrow_mut();
                    rng.fill_bytes(bytes);
                });
            }
        }
    }

    fn next_u64(&self) -> u64 {
        match self {
            CryptoRng::Deterministic(rng) => {
                let mut rng = rng.lock();
                rng.next_u64()
            }
            CryptoRng::ThreadLocal => THREAD_RNG.with(|cell| {
                let mut rng = cell.borrow_mut();
                rng.next_u64()
            }),
        }
    }
}

impl Clone for CryptoRng {
    fn clone(&self) -> Self {
        match self {
            CryptoRng::Deterministic(rng) => CryptoRng::Deterministic(Arc::clone(rng)),
            CryptoRng::ThreadLocal => CryptoRng::ThreadLocal,
        }
    }
}

/// Crypto subsystem grouping cryptographic operations and key management.
///
/// This subsystem encapsulates:
/// - Cryptographic primitives (signing, verification, key generation)
/// - Secure random number generation
/// - Secure storage for cryptographic key material
pub struct CryptoSubsystem {
    /// Core cryptographic handler for signing, verification, and key operations
    handler: Arc<dyn aura_core::effects::CryptoEffects>,

    /// Cryptographically secure RNG for key generation and nonces.
    ///
    /// Production uses a thread-local RNG. Deterministic modes use a mutex-backed RNG.
    rng: CryptoRng,

    /// Secure storage for key material (FROST keys, device keys)
    ///
    /// Uses platform-specific secure storage (Keychain, TPM, Keystore)
    secure_storage: Arc<ProductionSecureStorageHandler>,
    lifetime_identity: Arc<()>,
    lifetime_handoff_claimed: Arc<std::sync::atomic::AtomicBool>,
    pub(in crate::runtime) allocation_lifetimes:
        Arc<tokio::sync::Mutex<SelectedProfileSecretInventory>>,
}

impl CryptoSubsystem {
    /// Create a new crypto subsystem with production random source
    #[allow(dead_code)] // Used by deterministic and integration tests until runtime builders own all crypto construction.
    pub fn new(base_path: std::path::PathBuf) -> Self {
        let lifetime_identity = Arc::new(());
        Self {
            lifetime_handoff_claimed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            lifetime_identity: lifetime_identity.clone(),
            handler: Arc::new(RealCryptoHandler::new()),
            rng: CryptoRng::thread_local(),
            allocation_lifetimes: Arc::new(tokio::sync::Mutex::new(
                SelectedProfileSecretInventory::deferred(None, lifetime_identity.clone()),
            )),
            secure_storage: Arc::new(
                ProductionSecureStorageHandler::filesystem_fallback_for_non_production(base_path),
            ),
        }
    }

    /// Create a crypto subsystem with deterministic seed for simulation/tests.
    #[allow(dead_code)] // Used by deterministic and integration tests until runtime builders own all crypto construction.
    pub fn for_simulation_seed(seed: [u8; 32], base_path: std::path::PathBuf) -> Self {
        let lifetime_identity = Arc::new(());
        Self {
            lifetime_handoff_claimed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            lifetime_identity: lifetime_identity.clone(),
            handler: Arc::new(RealCryptoHandler::for_simulation_seed(seed)),
            rng: CryptoRng::deterministic(StdRng::from_seed(seed)),
            allocation_lifetimes: Arc::new(tokio::sync::Mutex::new(
                SelectedProfileSecretInventory::deferred(None, lifetime_identity.clone()),
            )),
            secure_storage: Arc::new(
                ProductionSecureStorageHandler::filesystem_fallback_for_non_production(base_path),
            ),
        }
    }

    #[cfg(unix)]
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "ProfileSecretLifetimeRecoveryCapability",
        family = "runtime_helper"
    )]
    pub(in crate::runtime) fn retain_selected_secret_lifetimes(
        &mut self,
        root: aura_core::effects::secret_lifetime::ProfileSecretLifetimeRecoveryCapability,
    ) -> Result<(), AuraError> {
        if !self
            .secure_storage
            .owns_selected_profile_lifetime_channel(&root)
        {
            return Err(AuraError::Internal {
                message: "selected lifetime root belongs to another physical provider".into(),
                source: Some(Arc::new(SecretLifetimeHandoffError::ForeignProvider)),
            });
        }
        let inventory =
            Arc::get_mut(&mut self.allocation_lifetimes).ok_or_else(|| AuraError::Internal {
                message: "selected secret lifetime handoff must precede runtime sharing".into(),
                source: Some(Arc::new(SecretLifetimeHandoffError::SharedBeforeHandoff)),
            })?;
        if self
            .lifetime_handoff_claimed
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return Err(AuraError::Internal {
                message: "selected secret lifetime handoff already consumed".into(),
                source: Some(Arc::new(SecretLifetimeHandoffError::AlreadyClaimed)),
            });
        }
        inventory.get_mut().root = Some(root);
        Ok(())
    }
    #[cfg(not(unix))]
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "ProfileSecretLifetimeRecoveryCapability",
        family = "runtime_helper"
    )]
    pub(in crate::runtime) fn retain_selected_secret_lifetimes(
        &mut self,
        _root: aura_core::effects::secret_lifetime::ProfileSecretLifetimeRecoveryCapability,
    ) -> Result<(), AuraError> {
        Err(aura_core::effects::secret_lifetime::SecretLifetimeProviderUnavailable::UnsupportedSelectedProvider.into_aura_error())
    }
    /// Create from existing components
    pub fn from_parts(
        handler: impl aura_core::effects::CryptoEffects + 'static,
        rng: CryptoRng,
        secure_storage: Arc<ProductionSecureStorageHandler>,
    ) -> Self {
        let lifetime_identity = Arc::new(());
        Self {
            lifetime_handoff_claimed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            lifetime_identity: lifetime_identity.clone(),
            handler: Arc::new(handler),
            rng,
            secure_storage,
            allocation_lifetimes: Arc::new(tokio::sync::Mutex::new(
                SelectedProfileSecretInventory::deferred(None, lifetime_identity.clone()),
            )),
        }
    }

    pub(in crate::runtime) fn lifetime_owner_identity(&self) -> Arc<()> {
        self.lifetime_identity.clone()
    }
    /// Get reference to the crypto handler
    pub fn handler(&self) -> &Arc<dyn aura_core::effects::CryptoEffects> {
        &self.handler
    }

    /// Get clone of the crypto handler (for effect trait delegation)
    #[allow(dead_code)] // Retained for effect delegation until all callers can borrow the handler directly.
    pub fn handler_clone(&self) -> Arc<dyn aura_core::effects::CryptoEffects> {
        self.handler.clone()
    }

    /// Get shared secure storage handler
    pub fn secure_storage(&self) -> Arc<ProductionSecureStorageHandler> {
        self.secure_storage.clone()
    }

    /// Generate random bytes using the subsystem's RNG
    ///
    /// This is the single point for random byte generation in the crypto subsystem.
    pub fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; len];
        self.rng.fill_bytes(&mut bytes);
        bytes
    }

    /// Generate a random u64
    pub fn random_u64(&self) -> u64 {
        self.rng.next_u64()
    }

    /// Generate a random [u8; 32] array
    pub fn random_32_bytes(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        self.rng.fill_bytes(&mut bytes);
        bytes
    }
}

impl Clone for CryptoSubsystem {
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.clone(),
            rng: self.rng.clone(),
            secure_storage: self.secure_storage.clone(),
            lifetime_handoff_claimed: self.lifetime_handoff_claimed.clone(),
            lifetime_identity: self.lifetime_identity.clone(),
            allocation_lifetimes: self.allocation_lifetimes.clone(),
        }
    }
}

impl std::fmt::Debug for CryptoSubsystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CryptoSubsystem")
            .field("handler", &"<selected CryptoEffects>")
            .field("rng", &format_args!("{:?}", self.rng))
            .field("secure_storage", &"<Arc<ProductionSecureStorageHandler>>")
            .finish()
    }
}

// Projected PRIVATE L6 owner inventory, not an exported effect implementation.
// Placement: runtime/subsystems/crypto.rs sibling module. Runtime assembly owns
// initialization before exposing CryptoSubsystem. Do not export this type.

pub(in crate::runtime) struct SelectedProfileSecretInventory {
    identity: Arc<()>,
    root: Option<ProfileSecretLifetimeRecoveryCapability>,
    initialized: bool,
    // Opaque provider owners survive cancellation of borrowed transitions.
    owners: Vec<SecretLifetimeOwner>,
}
impl SelectedProfileSecretInventory {
    fn deferred(root: Option<ProfileSecretLifetimeRecoveryCapability>, identity: Arc<()>) -> Self {
        Self {
            identity,
            root,
            owners: Vec::new(),
            initialized: false,
        }
    }
    pub(in crate::runtime) async fn ready(&mut self) -> Result<&mut Self, AuraError> {
        if !self.initialized {
            let root = self.root.as_mut().ok_or_else(|| {
                aura_core::effects::secret_lifetime::SecretLifetimeProviderUnavailable::MissingSelectedCustody.into_aura_error()
            })?;
            let owners = root.recover_owned_inventory().await?;
            for (index, owner) in owners.iter().enumerate() {
                if owners[..index]
                    .iter()
                    .any(|prior| prior.reference() == owner.reference())
                {
                    return Err(AuraError::invalid(
                        "contradictory original lifetime inventory",
                    ));
                }
            }
            self.owners = owners;
            self.initialized = true;
        }
        if self.initialized {
            let delta = self
                .root
                .as_mut()
                .ok_or_else(|| aura_core::effects::secret_lifetime::SecretLifetimeProviderUnavailable::MissingSelectedCustody.into_aura_error())?
                .reconcile_unhanded_births()
                .await?;
            self.owners.extend(delta);
        }
        Ok(self)
    }

    // ONLY the held authenticated generation birth owner calls this method.
    // The outer boundary first binds its strongest plan/reservation to effects,
    // validates original immutable allocation and encodes its complete scope.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OwnedSecretBirthCapability",
        family = "runtime_helper"
    )]
    pub(in crate::runtime) async fn fresh_birth(
        &mut self,
        authority: &crate::runtime::effects::OwnedSecretBirthCapability<'_, '_>,
        secret: &[u8],
    ) -> Result<SecretAllocationReference, AuraError> {
        authority.require_runtime_owner(&self.identity)?;
        let owner = self
            .root
            .as_mut()
            .ok_or_else(|| aura_core::effects::secret_lifetime::SecretLifetimeProviderUnavailable::MissingSelectedCustody.into_aura_error())?
            .allocate(&authority.scope_bytes()?, secret)
            .await?;
        let reference = owner.reference().clone();
        if self
            .owners
            .iter()
            .any(|prior| prior.reference().allocation == reference.allocation)
        {
            return Err(AuraError::invalid(
                "new provider birth conflicts with owned inventory",
            ));
        }
        self.owners.push(owner);
        Ok(reference)
    }
    pub(in crate::runtime) fn references(&self) -> Vec<SecretAllocationReference> {
        self.owners
            .iter()
            .map(|owner| owner.reference().clone())
            .collect()
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OwnedSecretPositiveCapability",
        family = "runtime_helper"
    )]
    pub(in crate::runtime) async fn seal_positive(
        &self,
        reference: &SecretAllocationReference,
        authority: &crate::runtime::effects::OwnedSecretPositiveCapability<'_, '_>,
    ) -> Result<(), AuraError> {
        authority.require_runtime_owner(&self.identity)?;
        authority.require_scope(&reference.scope)?;
        self.owned(reference)?
            .decide_positive(authority.decision())
            .await
    }
    // Private lookup does NOT mint a provider capability. The caller already
    // possesses this entire original inventory from the selected-profile root.
    fn owned(
        &self,
        reference: &SecretAllocationReference,
    ) -> Result<&SecretLifetimeOwner, AuraError> {
        self.owners
            .iter()
            .find(|owner| owner.reference() == reference)
            .ok_or_else(|| AuraError::invalid("original owned secret allocation is missing"))
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OwnedSecretNegativeCapability",
        family = "runtime_helper"
    )]
    pub(in crate::runtime) async fn retire_original(
        &self,
        reference: &SecretAllocationReference,
        authority: &crate::runtime::effects::OwnedSecretNegativeCapability<'_, '_>,
    ) -> Result<(), AuraError> {
        authority.require_runtime_owner(&self.identity)?;
        // Outer actual EnrollmentRetirementCapability must remain borrowed,
        // retaining generation+first-decision guards, throughout BOTH awaits.
        authority.require_scope(&reference.scope)?;
        let original = self.owned(reference)?;
        let negative = original.decide_negative(authority.decision()).await?;
        let acknowledged = negative.retire().await?;
        if acknowledged.reference() != reference {
            return Err(AuraError::invalid(
                "provider acknowledged another original allocation",
            ));
        }
        Ok(())
    }
}

use aura_core::effects::secret_lifetime::{
    ProfileSecretLifetimeRecoveryCapability, SecretAllocationReference, SecretLifetimeOwner,
};
use aura_core::AuraError;
impl SelectedProfileSecretInventory {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OwnedSecretReadCapability",
        family = "runtime_helper"
    )]
    pub(in crate::runtime) async fn read_original(
        &self,
        reference: &SecretAllocationReference,
        authority: &crate::runtime::effects::OwnedSecretReadCapability,
    ) -> Result<Vec<u8>, AuraError> {
        authority.require_runtime_owner(&self.identity)?;
        authority.require_scope(&reference.scope)?;
        self.owned(reference)?.read_live_secret().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto_subsystem_creation() {
        let temp_dir = std::env::temp_dir().join("crypto_subsystem_test");
        let subsystem = CryptoSubsystem::new(temp_dir);
        assert!(subsystem.random_bytes(32).len() == 32);
    }

    #[test]
    fn test_seeded_crypto_subsystem() {
        let temp_dir = std::env::temp_dir().join("crypto_subsystem_seeded_test");
        let seed = [42u8; 32];
        let subsystem1 = CryptoSubsystem::for_simulation_seed(seed, temp_dir.clone());
        let subsystem2 = CryptoSubsystem::for_simulation_seed(seed, temp_dir);

        // Seeded subsystems should produce same random values
        let bytes1 = subsystem1.random_bytes(16);
        let bytes2 = subsystem2.random_bytes(16);
        assert_eq!(bytes1, bytes2);
    }
    #[test]
    fn cloned_subsystems_continue_one_original_deterministic_stream() {
        let directory = tempfile::tempdir().unwrap();
        let seed = [73; 32];
        let original = CryptoSubsystem::for_simulation_seed(seed, directory.path().to_path_buf());
        let mut reference = StdRng::from_seed(seed);
        let mut expected = [0; 17];
        reference.fill_bytes(&mut expected);
        assert_eq!(original.random_bytes(17), expected);
        let clone = original.clone();
        assert!(matches!((&original.rng, &clone.rng),
            (CryptoRng::Deterministic(first), CryptoRng::Deterministic(second))
                if Arc::ptr_eq(first, second)));
        assert_eq!(clone.random_u64(), reference.next_u64());
        let mut expected = [0; 32];
        reference.fill_bytes(&mut expected);
        assert_eq!(original.random_32_bytes(), expected);
        let continuation = clone.clone();
        drop(clone);
        drop(original);
        assert_eq!(continuation.random_u64(), reference.next_u64());
        reference.fill_bytes(&mut expected);
        assert_eq!(continuation.random_32_bytes(), expected);
    }

    #[test]
    fn independently_seeded_subsystems_remain_reproducible_without_shared_custody() {
        let directory = tempfile::tempdir().unwrap();
        let seed = [74; 32];
        let first = CryptoSubsystem::for_simulation_seed(seed, directory.path().join("first"));
        let second = CryptoSubsystem::for_simulation_seed(seed, directory.path().join("second"));
        assert!(matches!((&first.rng, &second.rng),
            (CryptoRng::Deterministic(first), CryptoRng::Deterministic(second))
                if !Arc::ptr_eq(first, second)));
        assert_eq!(first.random_u64(), second.random_u64());
        assert_eq!(first.random_bytes(29), second.random_bytes(29));
        assert_eq!(first.random_32_bytes(), second.random_32_bytes());
    }
}

#[cfg(unix)]
#[derive(Debug, thiserror::Error)]
enum SecretLifetimeHandoffError {
    #[error("original selected secret lifetime handoff already consumed")]
    AlreadyClaimed,
    #[error("selected lifetime registry shared before original handoff")]
    SharedBeforeHandoff,
    #[error("selected lifetime root belongs to another physical provider")]
    ForeignProvider,
}

#[cfg(all(test, unix))]
#[test]
fn selected_secret_handoff_rejects_replacement_and_preexisting_shared_registry() {
    fn selected() -> (
        ProductionSecureStorageHandler,
        aura_core::effects::secret_lifetime::ProfileSecretLifetimeRecoveryCapability,
        Arc<aura_effects::profile_storage::OwnedProfileLease>,
    ) {
        let directory = tempfile::tempdir()
            .expect("isolated actual selected profile")
            .keep();
        let owner = Arc::new(
            aura_effects::profile_storage::FilesystemProfileStorageHandler::new(directory)
                .acquire_owned_native()
                .expect("actual exclusive physical profile"),
        );
        let (ordinary, root) =
            ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(owner.clone())
                .expect("actual selected descriptor provider")
                .into_selected_profile_lifetime_channel()
                .expect("original physical handoff");
        (ordinary, root, owner)
    }
    let (ordinary, root, owner) = selected();
    assert!(
        ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(owner)
            .expect("same retained physical lease")
            .into_selected_profile_lifetime_channel()
            .is_err(),
        "same original physical provider cannot issue a second genuine root for replacement"
    );
    let mut original = CryptoSubsystem::from_parts(
        RealCryptoHandler::new(),
        CryptoRng::thread_local(),
        Arc::new(ordinary),
    );
    let (_, foreign_first_root, _) = selected();
    let first_attachment = original
        .retain_selected_secret_lifetimes(foreign_first_root)
        .expect_err(
            "first attachment must match actual ordinary provider, not only registry identity",
        );
    assert!(matches!(
        std::error::Error::source(&first_attachment)
            .and_then(|source| source.downcast_ref::<SecretLifetimeHandoffError>()),
        Some(SecretLifetimeHandoffError::ForeignProvider)
    ));
    original
        .retain_selected_secret_lifetimes(root)
        .expect("single original handoff before sharing");
    let (_, other_root, _) = selected();
    let replacement = original
        .retain_selected_secret_lifetimes(other_root)
        .expect_err("another actual provider cannot replace original registry custody");
    assert!(matches!(
        std::error::Error::source(&replacement)
            .and_then(|source| source.downcast_ref::<SecretLifetimeHandoffError>()),
        Some(SecretLifetimeHandoffError::ForeignProvider)
    ));
    let (ordinary, root, _) = selected();
    let mut unclaimed = CryptoSubsystem::from_parts(
        RealCryptoHandler::new(),
        CryptoRng::thread_local(),
        Arc::new(ordinary),
    );
    let retained_clone = unclaimed.clone();
    let failure = unclaimed
        .retain_selected_secret_lifetimes(root)
        .expect_err("original handoff cannot replace a registry already shared with a sibling");
    assert!(matches!(
        std::error::Error::source(&failure)
            .and_then(|source| source.downcast_ref::<SecretLifetimeHandoffError>()),
        Some(SecretLifetimeHandoffError::SharedBeforeHandoff)
    ));
    assert!(Arc::ptr_eq(
        &unclaimed.allocation_lifetimes,
        &retained_clone.allocation_lifetimes
    ));
}

#[cfg(all(test, unix))]
#[tokio::test]
async fn selected_native_provider_without_lifetime_support_retains_structural_unavailability() {
    let directory = tempfile::tempdir()
        .expect("actual selected native profile")
        .keep();
    let owner = Arc::new(
        aura_effects::profile_storage::FilesystemProfileStorageHandler::new(directory.clone())
            .acquire_owned_native()
            .expect("actual original physical lease"),
    );
    let selected = ProductionSecureStorageHandler::for_production(directory.clone())
        .retain_profile_owner(owner.clone())
        .expect("actual native selected namespace owner");
    assert!(
        matches!(&selected, ProductionSecureStorageHandler::ProfileOwned(owned) if !owned.uses_filesystem_fallback()),
        "production provider cannot silently substitute the descriptor backend"
    );
    let unsupported = match selected.into_selected_profile_lifetime_channel() {
        Err(error) => error,
        Ok(_) => panic!("native backend has no lifetime implementation in this tranche"),
    };
    assert!(matches!(std::error::Error::source(&unsupported)
        .and_then(|source| source.downcast_ref::<aura_core::effects::secret_lifetime::SecretLifetimeProviderUnavailable>()),
        Some(aura_core::effects::secret_lifetime::SecretLifetimeProviderUnavailable::UnsupportedSelectedProvider)));
    let selected = ProductionSecureStorageHandler::for_production(directory)
        .retain_profile_owner(owner)
        .expect("ordinary storage reconstructed from same retained original lease");
    let runtime = CryptoSubsystem::from_parts(
        RealCryptoHandler::new(),
        CryptoRng::thread_local(),
        Arc::new(selected),
    );
    let mut custody = runtime.allocation_lifetimes.lock().await;
    let failure = match custody.ready().await {
        Err(error) => error,
        Ok(_) => panic!("ordinary selected storage cannot fabricate lifetime custody"),
    };
    assert!(matches!(std::error::Error::source(&failure)
        .and_then(|source| source.downcast_ref::<aura_core::effects::secret_lifetime::SecretLifetimeProviderUnavailable>()),
        Some(aura_core::effects::secret_lifetime::SecretLifetimeProviderUnavailable::MissingSelectedCustody)));
}
