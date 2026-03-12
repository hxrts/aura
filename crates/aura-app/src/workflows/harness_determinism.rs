use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_lock::RwLock;
use aura_core::{hash::hasher, AuraError};
use cfg_if::cfg_if;

use crate::workflows::time::current_time_ms;
use crate::AppCore;

const HARNESS_MODE_KEY: &str = "AURA_HARNESS_MODE";
const HARNESS_SCENARIO_SEED_KEY: &str = "AURA_HARNESS_SCENARIO_SEED";
const HARNESS_INSTANCE_ID_KEY: &str = "AURA_HARNESS_INSTANCE_ID";
const HARNESS_TIME_BASE_MS: u64 = 1_700_000_000_000;

static HARNESS_SEQUENCE: AtomicU64 = AtomicU64::new(1);
#[allow(dead_code)]
static NON_HARNESS_NONCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
struct HarnessContext {
    scenario_seed: u64,
    instance_id: String,
}

fn parse_seed(raw: &str) -> Option<u64> {
    raw.parse::<u64>().ok()
}

fn next_sequence() -> u64 {
    HARNESS_SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

fn derive_u64(
    seed: u64,
    instance_id: &str,
    scope: &str,
    sequence: u64,
    components: &[&str],
) -> u64 {
    let mut state = hasher();
    state.update(&seed.to_le_bytes());
    state.update(instance_id.as_bytes());
    state.update(scope.as_bytes());
    state.update(&sequence.to_le_bytes());
    for component in components {
        state.update(component.as_bytes());
        state.update(&[0]);
    }
    let digest = state.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    u64::from_le_bytes(bytes)
}

fn harness_context() -> Option<HarnessContext> {
    cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            let window = web_sys::window()?;
            let search = window.location().search().ok()?;
            let query = search.strip_prefix('?').unwrap_or(&search);
            let mut instance_id = None;
            let mut scenario_seed = None;
            for pair in query.split('&') {
                let Some((key, value)) = pair.split_once('=') else {
                    continue;
                };
                if key == "__aura_harness_instance" && !value.is_empty() {
                    instance_id = Some(value.to_string());
                } else if key == "__aura_harness_scenario_seed" {
                    scenario_seed = parse_seed(value);
                }
            }
            Some(HarnessContext {
                scenario_seed: scenario_seed?,
                instance_id: instance_id?,
            })
        } else {
            std::env::var_os(HARNESS_MODE_KEY)?;
            Some(HarnessContext {
                scenario_seed: parse_seed(&std::env::var(HARNESS_SCENARIO_SEED_KEY).ok()?)?,
                instance_id: std::env::var(HARNESS_INSTANCE_ID_KEY).ok()?,
            })
        }
    }
}

pub fn harness_mode_enabled() -> bool {
    harness_context().is_some()
}

pub async fn parity_timestamp_ms(
    app_core: &Arc<RwLock<AppCore>>,
    scope: &str,
    components: &[&str],
) -> Result<u64, AuraError> {
    if let Some(context) = harness_context() {
        let sequence = next_sequence();
        let offset = derive_u64(
            context.scenario_seed,
            &context.instance_id,
            scope,
            sequence,
            components,
        ) % 1_000;
        return Ok(HARNESS_TIME_BASE_MS + sequence.saturating_mul(1_000) + offset);
    }

    current_time_ms(app_core).await
}

#[allow(dead_code)]
pub fn parity_generated_nonce(scope: &str, components: &[&str]) -> u64 {
    if let Some(context) = harness_context() {
        let sequence = next_sequence();
        return derive_u64(
            context.scenario_seed,
            &context.instance_id,
            scope,
            sequence,
            components,
        );
    }

    NON_HARNESS_NONCE.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::derive_u64;

    #[test]
    fn derive_u64_is_stable_for_same_inputs() {
        let first = derive_u64(7, "alice", "message-id", 3, &["chat", "hello"]);
        let second = derive_u64(7, "alice", "message-id", 3, &["chat", "hello"]);
        assert_eq!(first, second);
    }

    #[test]
    fn derive_u64_changes_when_scope_changes() {
        let first = derive_u64(7, "alice", "message-id", 3, &["chat", "hello"]);
        let second = derive_u64(7, "alice", "timestamp", 3, &["chat", "hello"]);
        assert_ne!(first, second);
    }
}
