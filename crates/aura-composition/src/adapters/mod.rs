//! Individual handler adapters for the composition system
//!
//! This module provides adapter structs that wrap individual effect handlers
//! from the effects crate and expose the RegistrableHandler trait for use in
//! the effect registry.

mod console;
mod crypto;
mod logging;
mod random;
mod storage;
mod time;
mod trace;
mod transport;
mod utils;

pub use console::ConsoleHandlerAdapter;
pub use crypto::CryptoHandlerAdapter;
pub use logging::LoggingSystemHandlerAdapter;
pub use random::RandomHandlerAdapter;
pub use storage::StorageHandlerAdapter;
pub use time::TimeHandlerAdapter;
pub use trace::TraceHandlerAdapter;
pub use transport::TransportHandlerAdapter;

use aura_core::effects::registry as effect_registry;
use aura_core::EffectType;

/// Collect operations for an effect type, optionally including extended operations.
pub(crate) fn collect_ops(effect_type: EffectType, include_extended: bool) -> Vec<String> {
    let mut ops: Vec<String> = effect_registry::core_operations_for(effect_type)
        .iter()
        .map(|op| (*op).to_string())
        .collect();
    if include_extended {
        ops.extend(
            effect_registry::extended_operations_for(effect_type)
                .iter()
                .map(|op| (*op).to_string()),
        );
    }
    if ops.is_empty() {
        return effect_registry::operations_for(effect_type)
            .iter()
            .map(|op| (*op).to_string())
            .collect();
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::registry::operations_for;
    use aura_effects::{
        console::RealConsoleHandler, crypto::RealCryptoHandler, random::RealRandomHandler,
        storage::FilesystemStorageHandler, system::logging::LoggingSystemHandler,
        time::PhysicalTimeHandler, trace::TraceHandler,
    };
    use cfg_if::cfg_if;

    #[cfg(not(target_arch = "wasm32"))]
    use aura_effects::TcpTransportHandler as RealTransportHandler;

    fn assert_supported_operations(effect_type: EffectType, actual: Vec<String>) {
        assert_eq!(
            actual,
            operations_for(effect_type)
                .iter()
                .map(|op| (*op).to_string())
                .collect::<Vec<_>>()
        );
    }

    /// Every adapter reports exactly the operations declared in the
    /// aura-core effect registry for its effect type.
    #[test]
    fn test_supported_operations_match_registry_map() {
        use crate::registry::RegistrableHandler;

        assert_supported_operations(
            EffectType::Console,
            ConsoleHandlerAdapter::new(RealConsoleHandler::new())
                .supported_operations(EffectType::Console),
        );
        assert_supported_operations(
            EffectType::Random,
            RandomHandlerAdapter::new(RealRandomHandler::new())
                .supported_operations(EffectType::Random),
        );
        assert_supported_operations(
            EffectType::Crypto,
            CryptoHandlerAdapter::new(RealCryptoHandler::new())
                .supported_operations(EffectType::Crypto),
        );
        assert_supported_operations(
            EffectType::Storage,
            StorageHandlerAdapter::new(FilesystemStorageHandler::with_default_path())
                .supported_operations(EffectType::Storage),
        );
        assert_supported_operations(
            EffectType::Time,
            TimeHandlerAdapter::new(PhysicalTimeHandler::new())
                .supported_operations(EffectType::Time),
        );

        cfg_if! {
            if #[cfg(not(target_arch = "wasm32"))] {
                assert_supported_operations(
                    EffectType::Network,
                    TransportHandlerAdapter::new(RealTransportHandler::default())
                        .supported_operations(EffectType::Network),
                );
            }
        }

        assert_supported_operations(
            EffectType::System,
            LoggingSystemHandlerAdapter::new(LoggingSystemHandler::default())
                .supported_operations(EffectType::System),
        );
        assert_supported_operations(
            EffectType::Trace,
            TraceHandlerAdapter::new(TraceHandler::new()).supported_operations(EffectType::Trace),
        );
    }
}
