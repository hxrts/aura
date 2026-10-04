//! Device-owned setup-code integrity. Physical-device trust requires a separate
//! explicit user transfer or independently authenticated binding in the app.

use aura_core::crypto::single_signer::SigningMode;
use aura_core::effects::CryptoExtendedEffects;
use aura_core::hash::hash;
use aura_core::threshold::{SigningContext, ThresholdSignature};
use aura_core::{AuthorityId, DeviceId};
use aura_signature::{threshold_signing_context_transcript_bytes, SecurityTranscript};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};

/// Public statement exported by the actual provisional device runtime.
/// Untrusted key material: decoded identity and public package require possession
/// verification and explicit user transfer before enrollment may trust this device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceEnrollmentSetupStatement {
    pub version: u16,
    pub authority: AuthorityId,
    pub device: DeviceId,
    pub nonce: [u8; 32],
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
    pub signing_epoch: u64,
    pub signing_mode: SigningMode,
    pub threshold: u16,
    pub participants: u16,
    /// Untrusted key material until possession verification and explicit user transfer.
    pub public_key_package: Vec<u8>,
}

/// Untrusted invitation wire binding. Runtime authorization requires matching
/// this binding to an exact locally retained, signed setup request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceEnrollmentSetupBinding {
    pub nonce: [u8; 32],
    pub digest: [u8; 32],
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_effects::crypto::RealCryptoHandler;

    #[derive(Debug, thiserror::Error)]
    #[error("actual injected setup verification provider outage")]
    struct InjectedSetupProviderFailure;

    #[tokio::test]
    async fn malformed_peer_encoding_is_distinct_from_required_provider_failure(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (crypto, request) = signed_request().await;
        let source: std::sync::Arc<dyn std::error::Error + Send + Sync> =
            std::sync::Arc::new(InjectedSetupProviderFailure);
        let fixture=aura_testkit::stateful_effects::verification_failure_fixture::VerificationFailureFixture::new(crypto,source.clone());
        let mut malformed = request.clone();
        malformed.statement.public_key_package = vec![0xff];
        malformed.proof.public_key_package = vec![0xff];
        let rejection = match malformed.verify_possession(&fixture, 100).await {
            Err(source) => source,
            Ok(_) => {
                return Err(
                    "peer package parsing must reject before the failing provider is invoked"
                        .into(),
                )
            }
        };
        assert!(matches!(rejection, EnrollmentSetupError::InputEncoding(_)));
        let mut short = request.clone();
        short.proof.signature.truncate(63);
        assert!(matches!(
            short.verify_possession(&fixture, 100).await,
            Err(EnrollmentSetupError::InputEncoding(_))
        ));
        let failure = match request.verify_possession(&fixture, 100).await {
            Err(source) => source,
            Ok(_) => {
                return Err(
                    "a genuine signed request must reach the required failing provider".into(),
                )
            }
        };
        let EnrollmentSetupError::Crypto(aura_core::AuraError::Crypto {
            source: Some(retained),
            ..
        }) = failure
        else {
            return Err("required provider source was reclassified or erased".into());
        };
        assert!(retained
            .downcast_ref::<InjectedSetupProviderFailure>()
            .is_some());
        assert!(std::sync::Arc::ptr_eq(&retained, &source));
        Ok(())
    }

    async fn signed_request() -> (RealCryptoHandler, DeviceEnrollmentSetupRequest) {
        let crypto = RealCryptoHandler::for_simulation_seed([93; 32]);
        let keys = crypto.generate_signing_keys(1, 1).await.unwrap();
        let statement = DeviceEnrollmentSetupStatement {
            version: DeviceEnrollmentSetupRequest::VERSION,
            authority: AuthorityId::new_from_entropy([31; 32]),
            device: DeviceId::new_from_entropy([32; 32]),
            nonce: [33; 32],
            issued_at_ms: 100,
            expires_at_ms: 200,
            signing_epoch: 7,
            signing_mode: SigningMode::SingleSigner,
            threshold: 1,
            participants: 1,
            public_key_package: keys.public_key_package.clone(),
        };
        let context = DeviceEnrollmentSetupRequest::signing_context(&statement).unwrap();
        let bytes =
            threshold_signing_context_transcript_bytes(&context, statement.signing_epoch).unwrap();
        let signature = crypto
            .sign_with_key(&bytes, &keys.key_packages[0], keys.mode)
            .await
            .unwrap();
        let proof =
            ThresholdSignature::single_signer(signature, keys.public_key_package.clone(), 7);
        (crypto, DeviceEnrollmentSetupRequest { statement, proof })
    }

    #[tokio::test]
    async fn setup_code_round_trip_preserves_possession_and_validity() {
        let (crypto, request) = signed_request().await;
        let restored = DeviceEnrollmentSetupRequest::decode(&request.encode().unwrap()).unwrap();
        assert_eq!(restored, request);
        let verified = restored.verify_possession(&crypto, 100).await.unwrap();
        assert_eq!(verified.statement(), &request.statement);
        assert_eq!(
            verified.digest(),
            hash(&request.statement.transcript_bytes().unwrap())
        );
        for now in [99, 200, 201] {
            assert!(matches!(
                request.clone().verify_possession(&crypto, now).await,
                Err(EnrollmentSetupError::OutsideValidity)
            ));
        }
    }

    #[tokio::test]
    async fn setup_possession_rejects_substituted_statement_dimensions() {
        let (crypto, request) = signed_request().await;
        let mut altered = vec![request.clone(); 8];
        altered[0].statement.authority = AuthorityId::new_from_entropy([34; 32]);
        altered[1].statement.device = DeviceId::new_from_entropy([35; 32]);
        altered[2].statement.nonce[0] ^= 1;
        altered[3].statement.issued_at_ms += 1;
        altered[4].statement.expires_at_ms += 1;
        altered[5].statement.signing_epoch += 1;
        altered[5].proof.epoch += 1;
        altered[6].proof.signature[0] ^= 1;
        let second = crypto.generate_signing_keys(1, 1).await.unwrap();
        altered[7].statement.public_key_package = second.public_key_package.clone();
        altered[7].proof.public_key_package = second.public_key_package.clone();
        for (index, tampered) in altered.into_iter().enumerate() {
            assert!(
                tampered.verify_possession(&crypto, 150).await.is_err(),
                "accepted tampering case {index}"
            );
        }
    }

    #[tokio::test]
    async fn setup_rejects_proof_metadata_and_policy_downgrades() {
        let (crypto, request) = signed_request().await;
        let mut altered = vec![request.clone(); 7];
        altered[0].proof.epoch += 1;
        altered[1].proof.public_key_package[0] ^= 1;
        altered[2].proof.signer_count = 0;
        altered[3].proof.signers = vec![0];
        altered[4].statement.threshold = 0;
        altered[5].statement.signing_mode = SigningMode::Threshold;
        altered[6].statement.participants = 2;
        for (index, tampered) in altered.into_iter().enumerate() {
            assert!(
                tampered.verify_possession(&crypto, 150).await.is_err(),
                "accepted policy case {index}"
            );
        }
    }

    #[test]
    fn issuance_preserves_concrete_cause_and_limits_retries_to_tree_reads() {
        use EnrollmentIssuanceStage as Stage;
        let stages = [
            Stage::TreeRead,
            Stage::Rotation,
            Stage::PendingPackageRead,
            Stage::PendingConfigRead,
            Stage::PrestateEncoding,
            Stage::OperationEncoding,
            Stage::PrestateValidation,
            Stage::Supersession,
            Stage::CeremonyRegistration,
            Stage::SetupVerifierRetention,
            Stage::InvitationService,
            Stage::BaselineExport,
            Stage::BaselineEncoding,
            Stage::InvitationCreation,
            Stage::InvitationExport,
        ];
        for stage in stages {
            let error = EnrollmentIssuanceError::at(
                stage,
                aura_core::AuraError::network("transient network failure"),
            );
            assert_eq!(error.is_retryable(), stage == Stage::TreeRead);
        }
        let permanent = EnrollmentIssuanceError::at(
            Stage::TreeRead,
            aura_core::AuraError::serialization("corrupt tree"),
        );
        assert!(!permanent.is_retryable());
        let failure = EnrollmentIssuanceError::at(
            Stage::PendingConfigRead,
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "original storage cause",
            ),
        );
        let stage_source = std::error::Error::source(&failure).expect("typed stage source");
        let original = stage_source
            .source()
            .expect("original cause")
            .downcast_ref::<std::io::Error>()
            .expect("concrete storage cause");
        assert_eq!(original.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(!failure.is_retryable());
    }

    #[test]
    fn setup_decoder_bounds_input_before_parsing() {
        let oversized = format!(
            "{}{}",
            DeviceEnrollmentSetupRequest::PREFIX,
            "A".repeat(DeviceEnrollmentSetupRequest::MAX_BYTES * 2)
        );
        assert!(matches!(
            DeviceEnrollmentSetupRequest::decode(&oversized),
            Err(EnrollmentSetupError::SizeLimit)
        ));
        assert!(matches!(
            DeviceEnrollmentSetupRequest::decode("aura-setup:!"),
            Err(EnrollmentSetupError::InvalidFormat)
        ));
    }
}

impl SecurityTranscript for DeviceEnrollmentSetupStatement {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.device-enrollment-setup";

    fn transcript_payload(&self) -> Self::Payload {
        self.clone()
    }
}

/// Serializable code; neither decoding nor its embedded key establishes trust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceEnrollmentSetupRequest {
    pub statement: DeviceEnrollmentSetupStatement,
    pub proof: ThresholdSignature,
}

#[derive(Debug, thiserror::Error)]
pub enum EnrollmentSetupError {
    #[error("invalid enrollment setup code format")]
    InvalidFormat,
    #[error("enrollment setup exceeds its size limit")]
    SizeLimit,
    #[error("unsupported enrollment setup version {0}")]
    UnsupportedVersion(u16),
    #[error("invalid enrollment setup validity interval")]
    InvalidValidity,
    #[error("enrollment setup is expired or not yet valid")]
    OutsideValidity,
    #[error("invalid enrollment setup signing policy")]
    InvalidSigningPolicy,
    #[error("enrollment setup proof does not match its signing statement")]
    ProofBinding,
    #[error("enrollment setup possession signature is invalid")]
    InvalidSignature,
    #[error("enrollment setup public signature inputs are malformed")]
    InputEncoding(#[source] aura_core::crypto::signature_input::SignatureInputError),
    #[error("enrollment setup codec failed")]
    Codec(#[from] serde_json::Error),
    #[error("enrollment setup transcript failed")]
    Transcript(#[from] aura_signature::AuthenticationError),
    #[error("enrollment setup cryptographic verification failed")]
    Crypto(#[from] aura_core::AuraError),
}

/// Stable stage of an enrollment issuance failure. No stage after key rotation
/// is automatically retryable: replay-safe issuance is a separate contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrollmentIssuanceStage {
    SetupVerifierRetention,
    TreeRead,
    Rotation,
    PendingPackageRead,
    PendingConfigRead,
    PrestateEncoding,
    OperationEncoding,
    PrestateValidation,
    Supersession,
    CeremonyRegistration,
    InvitationService,
    BaselineExport,
    BaselineEncoding,
    InvitationCreation,
    InvitationExport,
}

/// Enrollment issuance retains structural policy failures and original sources.
#[derive(Debug, thiserror::Error)]
pub enum EnrollmentIssuanceError {
    #[error("enrollment issuance is unavailable")]
    Unavailable,
    #[error("enrollment setup is outside its validity interval")]
    OutsideValidity,
    #[error("enrollment setup identifies the current account or device")]
    CurrentIdentity,
    #[error("enrollment device is already enrolled")]
    AlreadyEnrolled,
    #[error("invalid enrollment key generation or threshold policy")]
    InvalidPolicy,
    #[error("missing enrollment key package for device {0}")]
    MissingPackage(aura_core::DeviceId),
    #[error("empty pending enrollment public package")]
    EmptyPendingPackage,
    #[error("empty pending enrollment threshold configuration")]
    EmptyPendingConfig,
    #[error("enrollment issuance time read failed: {0}")]
    Time(#[from] aura_core::effects::time::TimeError),
    #[error("enrollment issuance {stage:?} failed: {source}")]
    Failure {
        stage: EnrollmentIssuanceStage,
        #[source]
        source: aura_core::AuraError,
    },
}

impl EnrollmentIssuanceError {
    /// Preserve an arbitrary lower-layer source without depending on its crate.
    pub fn at<E>(stage: EnrollmentIssuanceStage, source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Failure {
            stage,
            source: aura_core::AuraError::Internal {
                message: format!("Enrollment issuance {stage:?}"),
                source: Some(std::sync::Arc::new(source)),
            },
        }
    }

    /// Only a pre-mutation tree read may be retried using its typed core cause.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Failure {
                stage: EnrollmentIssuanceStage::TreeRead,
                source,
            } => std::error::Error::source(source)
                .and_then(|cause| cause.downcast_ref::<aura_core::AuraError>())
                .is_some_and(aura_core::AuraError::is_retryable),
            _ => false,
        }
    }
}

/// Verification failures preserved across the runtime inversion boundary.
#[derive(Debug, thiserror::Error)]
pub enum EnrollmentSetupVerificationError {
    #[error("device enrollment setup verification is unavailable")]
    Unavailable,
    #[error("enrollment setup physical time read failed: {0}")]
    Time(#[from] aura_core::effects::time::TimeError),
    #[error("enrollment setup verification failed: {0}")]
    Setup(#[from] EnrollmentSetupError),
    #[error("enrollment setup workflow boundary failed: {0}")]
    Boundary(#[source] aura_core::AuraError),
}

/// Export failures shared across the runtime inversion boundary. Sources stay
/// typed so callers can distinguish readiness, admission, time and persistence.
#[derive(Debug, thiserror::Error)]
pub enum EnrollmentSetupExportError {
    #[error("device enrollment setup export is unavailable")]
    Unavailable,
    #[error("enrollment setup export workflow boundary failed: {0}")]
    Boundary(#[source] aura_core::AuraError),
    #[error("no signing context for enrollment setup authority {0}")]
    MissingSigningContext(AuthorityId),
    #[error("this device cannot sign an enrollment setup request")]
    NotParticipant,
    #[error("enrollment setup expiry exceeds the physical time domain")]
    TimeOverflow,
    #[error("enrollment setup time read failed: {0}")]
    Time(#[from] aura_core::effects::time::TimeError),
    #[error("enrollment setup effect failed: {0}")]
    Effect(#[from] aura_core::AuraError),
    #[error("enrollment setup request failed: {0}")]
    Setup(#[from] EnrollmentSetupError),
    #[error("enrollment setup request persistence failed: {0}")]
    Storage(#[source] aura_core::AuraError),
    #[error("enrollment setup signing capability denied: {0}")]
    Admission(#[from] aura_core::effects::AdmissionError),
}

/// Sealed possession evidence. Deliberately not serializable and not an authority
/// or device-binding capability. The app must record explicit user transfer.
///
/// A decoded request cannot be promoted by constructing this evidence:
/// ```compile_fail
/// use aura_invitation::enrollment_setup::{DeviceEnrollmentSetupRequest, VerifiedEnrollmentSetupPossession};
/// fn forge(request: DeviceEnrollmentSetupRequest) -> VerifiedEnrollmentSetupPossession {
///     VerifiedEnrollmentSetupPossession { request, digest: [0; 32] }
/// }
/// ```
/// Persisted data cannot deserialize directly into verified evidence:
/// ```compile_fail
/// use aura_invitation::enrollment_setup::VerifiedEnrollmentSetupPossession;
/// fn restore(bytes: &[u8]) -> VerifiedEnrollmentSetupPossession {
///     serde_json::from_slice(bytes).unwrap()
/// }
/// ```
#[derive(Debug, Clone)]
pub struct VerifiedEnrollmentSetupPossession {
    request: DeviceEnrollmentSetupRequest,
    digest: [u8; 32],
}

impl VerifiedEnrollmentSetupPossession {
    /// Export the same public proof envelope. A recipient must independently
    /// verify/pin this transfer; serialized bytes cannot mint possession trust.
    pub fn transfer_code(&self) -> Result<String, EnrollmentSetupError> {
        self.request.encode()
    }
    pub fn statement(&self) -> &DeviceEnrollmentSetupStatement {
        &self.request.statement
    }

    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

impl DeviceEnrollmentSetupRequest {
    pub const VERSION: u16 = 1;
    pub const PREFIX: &'static str = "aura-setup:";
    pub const MAX_BYTES: usize = 131_072;
    pub const MAX_PUBLIC_KEY_PACKAGE_BYTES: usize = 65_536;
    pub const MAX_SIGNATURE_BYTES: usize = 512;
    pub const MAX_VALIDITY_MS: u64 = 24 * 60 * 60 * 1000;

    pub fn signing_context(
        statement: &DeviceEnrollmentSetupStatement,
    ) -> Result<SigningContext, EnrollmentSetupError> {
        Ok(SigningContext::message(
            statement.authority,
            DeviceEnrollmentSetupStatement::DOMAIN_SEPARATOR.to_owned(),
            statement.transcript_bytes()?,
        ))
    }

    fn validate_shape(&self) -> Result<(), EnrollmentSetupError> {
        let statement = &self.statement;
        if statement.version != Self::VERSION {
            return Err(EnrollmentSetupError::UnsupportedVersion(statement.version));
        }
        let lifetime = statement.expires_at_ms.checked_sub(statement.issued_at_ms);
        if !matches!(lifetime, Some(1..=Self::MAX_VALIDITY_MS)) {
            return Err(EnrollmentSetupError::InvalidValidity);
        }
        if statement.public_key_package.is_empty()
            || statement.public_key_package.len() > Self::MAX_PUBLIC_KEY_PACKAGE_BYTES
            || self.proof.signature.is_empty()
            || self.proof.signature.len() > Self::MAX_SIGNATURE_BYTES
        {
            return Err(EnrollmentSetupError::SizeLimit);
        }
        if statement.threshold == 0
            || statement.threshold > statement.participants
            || (statement.signing_mode == SigningMode::SingleSigner
                && (statement.threshold != 1 || statement.participants != 1))
            || (statement.signing_mode == SigningMode::Threshold && statement.threshold < 2)
        {
            return Err(EnrollmentSetupError::InvalidSigningPolicy);
        }
        if self.proof.epoch != statement.signing_epoch
            || self.proof.public_key_package != statement.public_key_package
            || self.proof.signer_count < statement.threshold
            || usize::from(self.proof.signer_count) != self.proof.signers.len()
            || self.proof.signer_count > statement.participants
            || self
                .proof
                .signers
                .iter()
                .any(|&index| index == 0 || index > statement.participants)
            || self.proof.signers.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(EnrollmentSetupError::ProofBinding);
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<String, EnrollmentSetupError> {
        self.validate_shape()?;
        let bytes = serde_json::to_vec(self)?;
        if bytes.len() > Self::MAX_BYTES {
            return Err(EnrollmentSetupError::SizeLimit);
        }
        Ok(format!("{}{}", Self::PREFIX, URL_SAFE_NO_PAD.encode(bytes)))
    }

    pub fn decode(code: &str) -> Result<Self, EnrollmentSetupError> {
        let encoded = code
            .strip_prefix(Self::PREFIX)
            .ok_or(EnrollmentSetupError::InvalidFormat)?;
        if encoded.len() > aura_core::envelope::max_base64_encoded_len(Self::MAX_BYTES) {
            return Err(EnrollmentSetupError::SizeLimit);
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| EnrollmentSetupError::InvalidFormat)?;
        if bytes.len() > Self::MAX_BYTES {
            return Err(EnrollmentSetupError::SizeLimit);
        }
        let request: Self = serde_json::from_slice(&bytes)?;
        request.validate_shape()?;
        Ok(request)
    }

    /// Checks possession under the embedded key, with no physical trust claim.
    pub async fn verify_possession<E: CryptoExtendedEffects + ?Sized>(
        self,
        crypto: &E,
        now_ms: u64,
    ) -> Result<VerifiedEnrollmentSetupPossession, EnrollmentSetupError> {
        self.validate_shape()?;
        if now_ms < self.statement.issued_at_ms || now_ms >= self.statement.expires_at_ms {
            return Err(EnrollmentSetupError::OutsideValidity);
        }
        aura_core::crypto::signature_input::validate_signature_encoding(
            &self.statement.public_key_package,
            &self.proof.signature,
            self.statement.signing_mode,
        )
        .map_err(EnrollmentSetupError::InputEncoding)?;
        let context = Self::signing_context(&self.statement)?;
        let bytes =
            threshold_signing_context_transcript_bytes(&context, self.statement.signing_epoch)?;
        if !crypto
            .verify_signature(
                &bytes,
                &self.proof.signature,
                &self.statement.public_key_package,
                self.statement.signing_mode,
            )
            .await?
        {
            return Err(EnrollmentSetupError::InvalidSignature);
        }
        let digest = hash(&self.statement.transcript_bytes()?);
        Ok(VerifiedEnrollmentSetupPossession {
            request: self,
            digest,
        })
    }
}
