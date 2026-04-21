//! Threshold Command Handler
//!
//! Effect-based implementation of threshold operations.
//! Returns structured `CliOutput` for testability.

use crate::error::{TerminalError, TerminalResult};
use crate::handlers::config::load_config_utf8;
use crate::handlers::{CliOutput, HandlerContext};
use aura_agent::handlers::{DkdConfig, DkdProtocol};
use aura_core::DeviceId;
use base64::Engine;
use std::path::PathBuf;
use uuid::Uuid;

/// Handle threshold operations through effects
///
/// Returns `CliOutput` instead of printing directly.
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_threshold(
    ctx: &HandlerContext<'_>,
    configs: &str,
    threshold: u32,
    mode: &str,
    message: Option<&str>,
    message_hex: Option<&str>,
    signature: Option<&str>,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();
    let config_paths: Vec<&str> = configs.split(',').collect();

    output.println(format!(
        "Running threshold operation with {} configs (threshold: {}, mode: {})",
        config_paths.len(),
        threshold,
        mode
    ));

    // Validate all config files exist through storage effects
    let mut valid_configs = Vec::with_capacity(config_paths.len());
    for config_path in &config_paths {
        let path = PathBuf::from(config_path);
        let key = format!("device_config:{}", path.display());
        let config_string = load_config_utf8(ctx, &key).await.map_err(|e| {
            output.eprintln(format!("Failed to read config {config_path}: {e}"));
            e
        })?;

        let config = parse_config_data(config_string.as_bytes()).map_err(|e| {
            output.eprintln(format!("Invalid config {config_path}: {e}"));
            e
        })?;

        output.println(format!("Loaded config: {config_path}"));
        valid_configs.push((path, config));
    }

    // Validate threshold parameters
    validate_threshold_params(&valid_configs, threshold, &mut output)?;

    // Execute threshold operation based on mode
    match mode {
        "sign" => execute_threshold_signing(ctx, &valid_configs, threshold, &mut output).await?,
        "verify" => {
            let verify_input = resolve_verify_input(message, message_hex, signature)?;
            execute_threshold_verification(
                ctx,
                &valid_configs,
                threshold,
                &verify_input,
                &mut output,
            )
            .await?;
        }
        "keygen" => execute_threshold_keygen(ctx, &valid_configs, threshold, &mut output).await?,
        "dkd" => execute_dkd_protocol(ctx, &valid_configs, threshold, &mut output).await?,
        _ => {
            output.eprintln(format!("Unknown threshold mode: {mode}"));
            return Err(TerminalError::Input(format!(
                "Unknown threshold mode: {mode}"
            )));
        }
    }

    Ok(output)
}

/// Parse configuration data
fn parse_config_data(data: &[u8]) -> Result<ThresholdConfig, TerminalError> {
    let config_str = String::from_utf8(data.to_vec())
        .map_err(|e| TerminalError::Config(format!("Invalid UTF-8: {e}")))?;

    let config: ThresholdConfig =
        toml::from_str(&config_str).map_err(|e| TerminalError::Config(e.to_string()))?;

    Ok(config)
}

/// Validate threshold parameters using portable workflow
fn validate_threshold_params(
    configs: &[(PathBuf, ThresholdConfig)],
    threshold: u32,
    output: &mut CliOutput,
) -> TerminalResult<()> {
    if configs.is_empty() {
        output.eprintln("No valid configurations provided");
        return Err(TerminalError::Input("No valid configurations".into()));
    }

    let num_devices = configs.len() as u32;

    // Use portable validation from aura-app workflow
    aura_app::ui::workflows::account::validate_threshold_params(threshold, num_devices).map_err(
        |e| {
            output.eprintln(e.to_string());
            TerminalError::Input(e.to_string())
        },
    )?;

    // Verify all configs have compatible threshold settings using portable workflow
    let config_tuples: Vec<(&str, u32)> = configs
        .iter()
        .map(|(path, config)| (path.to_str().unwrap_or("unknown"), config.threshold))
        .collect();

    aura_app::ui::workflows::account::validate_threshold_compatibility(&config_tuples).map_err(
        |e| {
            output.eprintln(e.to_string());
            TerminalError::Config(e.to_string())
        },
    )?;

    output.println("Threshold parameters validated");
    Ok(())
}

/// Execute threshold signing operation
async fn execute_threshold_signing(
    ctx: &HandlerContext<'_>,
    configs: &[(PathBuf, ThresholdConfig)],
    threshold: u32,
    output: &mut CliOutput,
) -> TerminalResult<()> {
    use aura_core::effects::ThresholdSigningEffects;
    use aura_core::threshold::{ApprovalContext, SignableOperation, SigningContext};
    use aura_core::tree::{TreeOp, TreeOpKind};
    use aura_core::Epoch;

    output.section("Threshold Signing Operation");

    // Get authority from context
    let authority_id = ctx.effect_context().authority_id();

    // Create a test tree operation for signing
    let test_op = TreeOp {
        parent_epoch: Epoch::initial(),
        parent_commitment: [0u8; 32],
        op: TreeOpKind::RotateEpoch { affected: vec![] },
        version: 1,
    };

    let signing_context = SigningContext {
        authority: authority_id,
        operation: SignableOperation::TreeOp(test_op),
        approval_context: ApprovalContext::SelfOperation,
    };

    output.kv("Signing with authority", authority_id.to_string());
    output.kv(
        "Threshold configuration",
        format!("{}/{} required", threshold, configs.len()),
    );

    // Attempt to sign using the threshold signing service
    match ctx.effects().sign(signing_context).await {
        Ok(signature) => {
            output.println("Threshold signing successful!");
            output.kv("Signers", signature.signer_count.to_string());
            output.kv("Epoch", signature.epoch.to_string());
            output.kv("Signature bytes", signature.signature.len().to_string());
        }
        Err(e) => {
            output.println(format!("Threshold signing failed: {e}"));
            output.println(format!(
                "  This may require {threshold} signers to be online"
            ));
        }
    }

    Ok(())
}

/// Execute threshold verification operation
async fn execute_threshold_verification(
    ctx: &HandlerContext<'_>,
    configs: &[(PathBuf, ThresholdConfig)],
    threshold: u32,
    verify_input: &ThresholdVerifyInput,
    output: &mut CliOutput,
) -> TerminalResult<()> {
    use aura_core::effects::{crypto::SigningMode, CryptoExtendedEffects, ThresholdSigningEffects};

    output.section("Threshold Verification Operation");

    // Get authority from context
    let authority_id = ctx.effect_context().authority_id();

    // Get the public key package for this authority
    let Some(public_key_package) = ctx.effects().public_key_package(&authority_id).await else {
        output.println(format!(
            "No public key package found for authority: {authority_id}"
        ));
        output.println("Run key generation first with: aura threshold --mode keygen");
        return Ok(());
    };

    output.kv("Verifying with authority", authority_id.to_string());
    output.kv(
        "Public key package",
        format!("{} bytes", public_key_package.len()),
    );
    output.kv(
        "Threshold configuration",
        format!("{}/{} required", threshold, configs.len()),
    );

    output.kv("Message source", verify_input.message_description.as_str());
    output.kv("Message bytes", verify_input.message.len().to_string());
    output.kv("Signature bytes", verify_input.signature.len().to_string());

    match ctx
        .effects()
        .verify_signature(
            &verify_input.message,
            &verify_input.signature,
            &public_key_package,
            SigningMode::Threshold,
        )
        .await
    {
        Ok(true) => {
            output.println("Signature verification: VALID");
        }
        Ok(false) => {
            output.println("Signature verification: INVALID");
        }
        Err(e) => {
            output.eprintln(format!("Threshold verification failed: {e}"));
            return Err(TerminalError::Operation(format!(
                "Threshold verification failed: {e}"
            )));
        }
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThresholdVerifyInput {
    message: Vec<u8>,
    message_description: String,
    signature: Vec<u8>,
}

fn resolve_verify_input(
    message: Option<&str>,
    message_hex: Option<&str>,
    signature: Option<&str>,
) -> TerminalResult<ThresholdVerifyInput> {
    let signature = signature.ok_or_else(|| {
        TerminalError::Input(
            "threshold verify requires --signature with hex/base64 signature bytes".into(),
        )
    })?;

    let (message, message_description) = match (message, message_hex) {
        (Some(_), Some(_)) => {
            return Err(TerminalError::Input(
                "threshold verify accepts either --message or --message-hex, not both".into(),
            ));
        }
        (Some(message), None) => (
            message.as_bytes().to_vec(),
            "utf-8 command line argument".to_string(),
        ),
        (None, Some(message_hex)) => (
            decode_cli_bytes(message_hex, "message")?,
            "hex/base64 encoded bytes".to_string(),
        ),
        (None, None) => {
            return Err(TerminalError::Input(
                "threshold verify requires --message or --message-hex".into(),
            ));
        }
    };

    Ok(ThresholdVerifyInput {
        message,
        message_description,
        signature: decode_cli_bytes(signature, "signature")?,
    })
}

fn decode_cli_bytes(encoded: &str, field_name: &str) -> TerminalResult<Vec<u8>> {
    if !encoded.is_empty()
        && encoded.len() % 2 == 0
        && encoded.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return hex::decode(encoded).map_err(|err| {
            TerminalError::Input(format!("Invalid {field_name} hex encoding: {err}"))
        });
    }

    for engine in [
        &base64::engine::general_purpose::STANDARD,
        &base64::engine::general_purpose::STANDARD_NO_PAD,
        &base64::engine::general_purpose::URL_SAFE,
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ] {
        if let Ok(bytes) = engine.decode(encoded) {
            return Ok(bytes);
        }
    }

    Err(TerminalError::Input(format!(
        "Invalid {field_name} encoding: expected hex or base64"
    )))
}

/// Execute threshold key generation operation
async fn execute_threshold_keygen(
    ctx: &HandlerContext<'_>,
    configs: &[(PathBuf, ThresholdConfig)],
    threshold: u32,
    output: &mut CliOutput,
) -> TerminalResult<()> {
    use aura_core::effects::ThresholdSigningEffects;

    output.section("Threshold Key Generation");

    // Get authority from context
    let authority_id = ctx.effect_context().authority_id();

    output.kv("Generating keys for authority", authority_id.to_string());
    output.kv(
        "Threshold configuration",
        format!("{}/{} participants", threshold, configs.len()),
    );

    if threshold > 1 {
        output.blank();
        output.println("Multi-device DKG requires network coordination.");
        output.println("For single-device bootstrap, use threshold=1.");
        output.println("For multi-device setup, use `aura init` with participant coordination.");
        return Ok(());
    }

    // Bootstrap 1-of-1 keys for single-device operation
    match ctx.effects().bootstrap_authority(&authority_id).await {
        Ok(public_key_package) => {
            output.blank();
            output.println("Key generation successful!");
            output.kv("Authority", authority_id.to_string());
            output.kv(
                "Public key package",
                format!("{} bytes", public_key_package.len()),
            );
            output.kv("Threshold", "1/1 (single-device)");
            output.blank();
            output.println("Keys stored in secure storage. You can now sign operations.");
        }
        Err(e) => {
            output.println(format!("Key generation failed: {e}"));
            output.println("  This may occur if keys already exist for this authority.");
        }
    }

    Ok(())
}

/// Execute DKD (Distributed Key Derivation) protocol
async fn execute_dkd_protocol(
    ctx: &HandlerContext<'_>,
    configs: &[(PathBuf, ThresholdConfig)],
    threshold: u32,
    output: &mut CliOutput,
) -> TerminalResult<()> {
    output.section("DKD (Distributed Key Derivation) Protocol");

    // Create participant device IDs from configs
    let participants: Vec<DeviceId> = configs
        .iter()
        .map(|(_, config)| {
            // Create deterministic DeviceId from device_id string
            let device_bytes = config.device_id.as_bytes();
            let mut uuid_bytes = [0u8; 16];
            for (i, &byte) in device_bytes.iter().take(16).enumerate() {
                uuid_bytes[i] = byte;
            }
            DeviceId(Uuid::from_bytes(uuid_bytes))
        })
        .collect();

    let participant_list = participants
        .iter()
        .enumerate()
        .map(|(i, id)| format!("{}:{}", i + 1, id))
        .collect::<Vec<_>>()
        .join(", ");
    output.kv("DKD participants", participant_list);

    let total_participants = participants.len() as u16;
    let config = DkdConfig {
        threshold: threshold as u16,
        total_participants,
        app_id: "aura-terminal".to_string(),
        context: format!("threshold-mode:{threshold}"),
        ..Default::default()
    };

    let mut protocol = DkdProtocol::new(config);
    let session_id = protocol
        .initiate_session(ctx.effects(), participants.clone(), None)
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to initiate DKD session: {e}")))?;

    let result = protocol
        .execute_protocol(ctx.effects(), &session_id, participants[0])
        .await
        .map_err(|e| TerminalError::Operation(format!("DKD protocol execution failed: {e}")))?;

    output.blank();
    output.println("DKD protocol completed successfully!");
    output.kv("Session", result.session_id.0.clone());
    output.kv("Participants", result.participant_count.to_string());
    output.kv("Derived key length", result.derived_key.len().to_string());
    output.kv("Epoch", result.epoch.to_string());

    Ok(())
}

/// Handle DKD testing with specific parameters
///
/// Returns `CliOutput` instead of printing directly.
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_dkd_test(
    ctx: &HandlerContext<'_>,
    app_id: &str,
    context: &str,
    threshold: u16,
    total: u16,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    output.println(format!(
        "Starting DKD test: app_id={app_id}, context={context}, threshold={threshold}, total={total}"
    ));

    // Create test participants
    let participants: Vec<DeviceId> = (0..total)
        .map(|i| {
            let mut uuid_bytes = [0u8; 16];
            uuid_bytes[0] = i as u8 + 1;
            DeviceId(Uuid::from_bytes(uuid_bytes))
        })
        .collect();

    // Validate parameters using portable workflow (uses workflow)
    aura_app::ui::workflows::account::validate_threshold_params(threshold as u32, total as u32)
        .map_err(|e| TerminalError::Input(format!("Invalid DKD parameters: {e}")))?;

    let config = DkdConfig {
        threshold,
        total_participants: total,
        app_id: app_id.to_string(),
        context: context.to_string(),
        ..Default::default()
    };

    let mut protocol = DkdProtocol::new(config);
    let session_id = protocol
        .initiate_session(ctx.effects(), participants.clone(), None)
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to initiate DKD session: {e}")))?;

    let result = protocol
        .execute_protocol(ctx.effects(), &session_id, participants[0])
        .await
        .map_err(|e| TerminalError::Operation(format!("DKD protocol execution failed: {e}")))?;

    output.blank();
    output.println("DKD test completed successfully!");
    output.kv("Session", result.session_id.0.clone());
    output.kv("Participants", result.participant_count.to_string());
    output.kv("Key length", result.derived_key.len().to_string());

    Ok(output)
}

/// Threshold configuration structure
#[derive(Debug, serde::Deserialize)]
struct ThresholdConfig {
    device_id: String,
    threshold: u32,
    #[serde(rename = "total_devices")]
    #[allow(dead_code)]
    /* TODO(2026-07): delete if total-device config never becomes part of threshold execution. */
    total_devices: u32,
    #[allow(dead_code)]
    /* TODO(2026-07): delete if threshold execution never consumes structured logging config. */
    logging: Option<LoggingConfig>,
    #[allow(dead_code)]
    /* TODO(2026-07): delete if threshold execution never consumes transport tuning config. */
    network: Option<NetworkConfig>,
}

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)] // Retain the parsed logging shape until threshold execution wires these knobs through.
struct LoggingConfig {
    level: String,
    structured: bool,
}

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)] // Retain the parsed network shape until threshold execution wires these knobs through.
struct NetworkConfig {
    default_port: u16,
    timeout: u64,
    max_retries: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config_data() {
        let config_str = r#"
device_id = "device_1"
threshold = 2
total_devices = 3
"#;
        let config = parse_config_data(config_str.as_bytes()).unwrap();
        assert_eq!(config.device_id, "device_1");
        assert_eq!(config.threshold, 2);
    }

    #[test]
    fn test_validate_threshold_params_empty() {
        let configs: Vec<(PathBuf, ThresholdConfig)> = vec![];
        let mut output = CliOutput::new();
        let result = validate_threshold_params(&configs, 2, &mut output);
        assert!(result.is_err());
        assert!(output.stderr_lines().iter().any(|l| l.contains("No valid")));
    }

    #[test]
    fn test_validate_threshold_params_threshold_too_high() {
        let configs = vec![(
            PathBuf::from("test.toml"),
            ThresholdConfig {
                device_id: "d1".into(),
                threshold: 2,
                total_devices: 3,
                logging: None,
                network: None,
            },
        )];
        let mut output = CliOutput::new();
        let result = validate_threshold_params(&configs, 5, &mut output);
        assert!(result.is_err());
        assert!(output.stderr_lines().iter().any(|l| l.contains("exceed")));
    }

    #[test]
    fn test_resolve_verify_input_requires_signature() {
        let result = resolve_verify_input(Some("hello"), None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_verify_input_decodes_hex_message_and_signature() {
        let input =
            resolve_verify_input(None, Some("68656c6c6f"), Some("7369676e6174757265")).unwrap();
        assert_eq!(input.message, b"hello");
        assert_eq!(input.signature, b"signature");
        assert_eq!(input.message_description, "hex/base64 encoded bytes");
    }

    #[test]
    fn test_decode_cli_bytes_accepts_base64() {
        let decoded = decode_cli_bytes("aGVsbG8=", "message").unwrap();
        assert_eq!(decoded, b"hello");
    }
}
