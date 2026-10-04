//! Bounded public signature input parsing. Successful parsing proves neither
//! signature validity nor authority, quorum custody or independent pinning.
use super::single_signer::{SigningMode, SingleSignerPublicKeyPackage};
use crate::util::serialization::SerializationError;

/// Concrete malformed public inputs, independently of an effect provider.
#[derive(Debug, thiserror::Error)]
pub enum SignatureInputError {
    /// Public input exceeds its canonical encoding limit.
    #[error("signature input encoding exceeds bounds")]
    Bounds,
    /// A single-signer public package did not decode.
    #[error("single-signer public package encoding is invalid")]
    SinglePackage(#[source] SerializationError),
    /// The public point encoding did not decode.
    #[error("single-signer public key encoding is invalid")]
    SingleKey(#[source] ed25519_dalek::SignatureError),
    /// The native public package encoding did not decode.
    #[error("threshold public package encoding is invalid")]
    ThresholdPackage(#[source] frost_ed25519::Error),
    /// The native threshold signature encoding did not decode.
    #[error("threshold signature encoding is invalid")]
    ThresholdSignature(#[source] frost_ed25519::Error),
    /// Signature or key did not have the native fixed length.
    #[error("signature public input has invalid fixed length")]
    Length(#[source] std::array::TryFromSliceError),
}
/// Parse only public encodings with the same audited native decoders used by
/// the effect implementation. The subsequent effect still verifies the actual
/// signature and may return a required provider failure.
pub fn validate_signature_encoding(
    public_key_package: &[u8],
    signature: &[u8],
    mode: SigningMode,
) -> Result<(), SignatureInputError> {
    if public_key_package.is_empty() || public_key_package.len() > 65_536 || signature.len() > 512 {
        return Err(SignatureInputError::Bounds);
    }
    let signature_bytes: [u8; 64] = signature.try_into().map_err(SignatureInputError::Length)?;
    match mode {
        SigningMode::SingleSigner => {
            let package = SingleSignerPublicKeyPackage::from_bytes(public_key_package)
                .map_err(SignatureInputError::SinglePackage)?;
            let key: [u8; 32] = package
                .verifying_key()
                .try_into()
                .map_err(SignatureInputError::Length)?;
            ed25519_dalek::VerifyingKey::from_bytes(&key)
                .map_err(SignatureInputError::SingleKey)?;
            // Ed25519 Signature::from_bytes is an infallible fixed-width parse;
            // actual signature validity remains the effect provider's verdict.
            let _ = ed25519_dalek::Signature::from_bytes(&signature_bytes);
        }
        SigningMode::Threshold => {
            frost_ed25519::keys::PublicKeyPackage::deserialize(public_key_package)
                .map_err(SignatureInputError::ThresholdPackage)?;
            frost_ed25519::Signature::deserialize(signature_bytes)
                .map_err(SignatureInputError::ThresholdSignature)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Signer;
    use rand::SeedableRng;

    #[test]
    fn single_signer_ingress_parsing_preserves_native_input_failures(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let key = ed25519_dalek::SigningKey::from_bytes(&[0x91; 32]);
        let package =
            SingleSignerPublicKeyPackage::try_new(key.verifying_key().to_bytes().to_vec())
                .ok_or("real single key has unexpected length")?
                .to_bytes()?;
        let signature = key.sign(b"public parsing proves no authority").to_bytes();
        validate_signature_encoding(&package, &signature, SigningMode::SingleSigner)?;
        let bad_package =
            match validate_signature_encoding(&[0xff], &signature, SigningMode::SingleSigner) {
                Err(source) => source,
                Ok(()) => return Err("malformed package must fail before effect invocation".into()),
            };
        assert!(matches!(bad_package, SignatureInputError::SinglePackage(_)));
        assert!(std::error::Error::source(&bad_package).is_some());
        let short = match validate_signature_encoding(
            &package,
            &signature[..63],
            SigningMode::SingleSigner,
        ) {
            Err(source) => source,
            Ok(()) => return Err("short native signature cannot parse".into()),
        };
        assert!(matches!(short, SignatureInputError::Length(_)));
        assert!(std::error::Error::source(&short).is_some());
        let invalid_point = (0..=u8::MAX)
            .map(|byte| [byte; 32])
            .find(|point| ed25519_dalek::VerifyingKey::from_bytes(point).is_err())
            .ok_or("bounded fixture search found no invalid compressed point")?;
        let malformed = SingleSignerPublicKeyPackage::try_new(invalid_point.to_vec())
            .ok_or("malformed point fixture must retain native key length")?
            .to_bytes()?;
        let point =
            match validate_signature_encoding(&malformed, &signature, SigningMode::SingleSigner) {
                Err(source) => source,
                Ok(()) => {
                    return Err("well-encoded package cannot hide a malformed native point".into())
                }
            };
        assert!(matches!(point, SignatureInputError::SingleKey(_)));
        assert!(std::error::Error::source(&point).is_some());
        #[derive(serde::Serialize)]
        struct UntrustedPackage {
            verifying_key: Vec<u8>,
        }
        let bad_key_length = crate::util::serialization::to_vec(&UntrustedPackage {
            verifying_key: vec![0x91; 31],
        })?;
        assert!(matches!(
            validate_signature_encoding(&bad_key_length, &signature, SigningMode::SingleSigner),
            Err(SignatureInputError::Length(_))
        ));
        Ok(())
    }
    #[test]
    fn threshold_ingress_parsing_rejects_malformed_public_package_and_signature(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut rng = rand::rngs::StdRng::from_seed([173; 32]);
        let (_shares, public) = frost_ed25519::keys::generate_with_dealer(
            3,
            2,
            frost_ed25519::keys::IdentifierList::Default,
            &mut rng,
        )?;
        let public = public.serialize()?;
        let signature = [0xff; 64]; // noncanonical scalar encoding
        let package_error =
            match validate_signature_encoding(&[0xff], &signature, SigningMode::Threshold) {
                Err(source) => source,
                Ok(()) => {
                    return Err("native package decode must precede provider verification".into())
                }
            };
        assert!(matches!(
            package_error,
            SignatureInputError::ThresholdPackage(_)
        ));
        assert!(std::error::Error::source(&package_error).is_some());
        let signature_error =
            match validate_signature_encoding(&public, &signature, SigningMode::Threshold) {
                Err(source) => source,
                Ok(()) => {
                    return Err(
                        "malformed signature must fail against an actual dealer public package"
                            .into(),
                    )
                }
            };
        assert!(matches!(
            signature_error,
            SignatureInputError::ThresholdSignature(_)
        ));
        assert!(std::error::Error::source(&signature_error).is_some());
        assert!(matches!(
            validate_signature_encoding(&public, &signature[..63], SigningMode::Threshold),
            Err(SignatureInputError::Length(_))
        ));
        assert!(matches!(
            validate_signature_encoding(&public, &[0; 513], SigningMode::Threshold),
            Err(SignatureInputError::Bounds)
        ));
        Ok(())
    }
}
