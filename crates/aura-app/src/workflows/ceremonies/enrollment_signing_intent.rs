//! App-owned explicit enrollment transcript approval boundary.
//! Runtime admission consumes the original local approval and protected custody.
use std::sync::Arc;

use async_lock::RwLock;
use aura_core::AuraError;
use aura_invitation::enrollment_manifest::EnrollmentTrustManifest;
use aura_invitation::shareable::PublicEnrollmentTransportSigningIntent;
use base64::Engine;

use crate::core::AppCore;

const PREFIX: &str = "aura-enrollment-signing-intent:v2:";
const MAXIMUM_INTENT_BYTES: usize = 131_072;

/// Identification selected by the original device's explicit local user action.
/// Parsing never grants signature permission or supplies trusted active keys.
pub struct UserTransferredEnrollmentSigningIntent {
    manifest: EnrollmentTrustManifest,
    transport: PublicEnrollmentTransportSigningIntent,
    initial_request:
        aura_invitation::enrollment_initial_request::EnrollmentInitialRequestTranscript,
    canonical: Vec<u8>,
}

impl UserTransferredEnrollmentSigningIntent {
    /// Read-only exact reviewed subject, device, policy and domain data.
    pub fn manifest(&self) -> &EnrollmentTrustManifest {
        &self.manifest
    }
    pub fn transport(&self) -> &PublicEnrollmentTransportSigningIntent {
        &self.transport
    }
    pub fn initial_request(
        &self,
    ) -> &aura_invitation::enrollment_initial_request::EnrollmentInitialRequestTranscript {
        &self.initial_request
    }
    pub fn domains(
        &self,
    ) -> [aura_invitation::enrollment_signing_intent::EnrollmentInitiationSigningDomain; 3] {
        use aura_invitation::enrollment_signing_intent::EnrollmentInitiationSigningDomain::*;
        [Manifest, PublicTransport, InitialRequest]
    }
}

/// Local user consent for this exact canonical typed manifest. Neither serde nor
/// a remote transport payload can construct this value. Runtime admission must
/// still verify protected active membership/policy and local original custody.
///
/// ```compile_fail
/// use aura_app::ui::workflows::ceremonies::UserApprovedEnrollmentSigningIntent;
/// let forged = UserApprovedEnrollmentSigningIntent {};
/// ```
///
/// ```compile_fail
/// use aura_app::ui::workflows::ceremonies::UserApprovedEnrollmentSigningIntent;
/// fn duplicate(approval: &UserApprovedEnrollmentSigningIntent) {
///     let _: UserApprovedEnrollmentSigningIntent = Clone::clone(approval);
/// }
/// ```
///
/// ```compile_fail
/// use aura_app::ui::workflows::ceremonies::UserApprovedEnrollmentSigningIntent;
/// fn require_deserialization<T: serde::de::DeserializeOwned>() {}
/// require_deserialization::<UserApprovedEnrollmentSigningIntent>();
/// ```
pub struct UserApprovedEnrollmentSigningIntent {
    manifest: EnrollmentTrustManifest,
    transport: PublicEnrollmentTransportSigningIntent,
    initial_request:
        aura_invitation::enrollment_initial_request::EnrollmentInitialRequestTranscript,
    canonical: Vec<u8>,
    setup: super::UserTransferredEnrollmentSetup,
    runtime_owner: Arc<dyn crate::runtime_bridge::RuntimeBridge>,
}

impl UserApprovedEnrollmentSigningIntent {
    pub fn initial_request(
        &self,
    ) -> &aura_invitation::enrollment_initial_request::EnrollmentInitialRequestTranscript {
        &self.initial_request
    }
    pub fn manifest(&self) -> &EnrollmentTrustManifest {
        &self.manifest
    }
    pub fn canonical_intent(&self) -> &[u8] {
        &self.canonical
    }
    pub fn transport(&self) -> &PublicEnrollmentTransportSigningIntent {
        &self.transport
    }
    pub fn setup(&self) -> &super::UserTransferredEnrollmentSetup {
        &self.setup
    }
    pub fn require_runtime_owner(
        &self,
        runtime: &dyn crate::runtime_bridge::RuntimeBridge,
    ) -> Result<(), AuraError> {
        // Approval retains the original bridge allocation. Trait vtable addresses
        // can differ across codegen units for the same allocation and are not
        // runtime owner identity. A separately allocated bridge remains foreign.
        if !std::ptr::addr_eq(self.runtime_owner.as_ref(), runtime) {
            return Err(AuraError::permission_denied(
                "enrollment signing approval belongs to another original runtime",
            ));
        }
        Ok(())
    }
}

/// Transfer parsing is deliberately separate from explicit approval. The code
/// contains no private share, nonce, peer clock or renewable duration.
pub fn select_user_transferred_enrollment_signing_intent(
    code: String,
) -> Result<UserTransferredEnrollmentSigningIntent, AuraError> {
    let encoded = code
        .strip_prefix(PREFIX)
        .ok_or_else(|| AuraError::invalid("unsupported enrollment signing intent"))?;
    if encoded.len() > aura_core::envelope::max_base64_encoded_len(MAXIMUM_INTENT_BYTES) {
        return Err(AuraError::invalid(
            "enrollment signing intent exceeds bounds",
        ));
    }
    let canonical = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|source| AuraError::Serialization {
            message: "decode user-selected enrollment signing intent".into(),
            source: Some(Arc::new(source)),
        })?;
    if canonical.len() > MAXIMUM_INTENT_BYTES {
        return Err(AuraError::invalid(
            "enrollment signing intent exceeds decoded bounds",
        ));
    }
    let intent: aura_invitation::enrollment_signing_intent::EnrollmentSigningIntent =
        aura_core::util::serialization::from_slice(&canonical).map_err(|source| {
            AuraError::Serialization {
                message: "decode typed enrollment signing intent".into(),
                source: Some(Arc::new(source)),
            }
        })?;
    intent.validate()?;
    let manifest = intent.manifest().clone();
    let transport = intent.transport().clone();
    let initial_request = intent.initial_request().clone();
    let reencoded = aura_core::util::serialization::to_vec(&intent).map_err(|source| {
        AuraError::Serialization {
            message: "verify canonical enrollment signing intent".into(),
            source: Some(Arc::new(source)),
        }
    })?;
    if canonical != reencoded || manifest.version != 3 || manifest.final_inventory.is_empty() {
        return Err(AuraError::invalid(
            "noncanonical or historical enrollment signing intent",
        ));
    }
    manifest.validate_shape().map_err(|source| {
        AuraError::crypto_with_source(
            "validate user-selected enrollment signing intent",
            Arc::new(source),
        )
    })?;
    aura_invitation::shareable::require_transport_manifest(&transport, &manifest).map_err(
        |source| {
            AuraError::crypto_with_source(
                "bind exact public transport intent to selected manifest",
                Arc::new(source),
            )
        },
    )?;
    Ok(UserTransferredEnrollmentSigningIntent {
        manifest,
        transport,
        initial_request,
        canonical,
    })
}

/// Called by the local approve action after presenting the exact manifest.
/// The runtime consumes this sealed user approval; receiving or displaying an
/// intent never invokes this function automatically. This boundary must use the
/// app semantic owner declaration and actual terminal publication before wiring.
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "UserApprovedEnrollmentSigningIntent",
    family = "authorizer"
)]
pub async fn approve_user_selected_enrollment_signing_intent(
    app_core: &Arc<RwLock<AppCore>>,
    selected: UserTransferredEnrollmentSigningIntent,
    setup: super::UserTransferredEnrollmentSetup,
) -> Result<UserApprovedEnrollmentSigningIntent, AuraError> {
    let runtime_owner = crate::workflows::runtime::require_runtime(app_core).await?;
    selected
        .manifest
        .validate_setup_validity(setup.statement())
        .map_err(|source| {
            AuraError::crypto_with_source(
                "bind selected setup validity to approval",
                Arc::new(source),
            )
        })?;
    if selected.manifest.invitee_authority != setup.statement().authority
        || selected.manifest.invitee_device != setup.statement().device
        || selected.manifest.setup.nonce != setup.statement().nonce
        || selected.manifest.setup.digest != setup.digest()
    {
        return Err(AuraError::invalid(
            "user-selected setup differs from approved manifest intent",
        ));
    }
    Ok(UserApprovedEnrollmentSigningIntent {
        manifest: selected.manifest,
        transport: selected.transport,
        initial_request: selected.initial_request,
        canonical: selected.canonical,
        setup,
        runtime_owner,
    })
}
