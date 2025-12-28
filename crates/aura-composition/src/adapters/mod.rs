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
        TcpTransportHandler as RealTransportHandler,
    };

    #[test]
    fn test_supported_operations_match_registry_map() {
        use crate::registry::RegistrableHandler;

        let console_ops = ConsoleHandlerAdapter::new(RealConsoleHandler::new())
            .supported_operations(EffectType::Console);
        assert_eq!(
            console_ops,
            operations_for(EffectType::Console)
                .iter()
                .map(|op| (*op).to_string())
                .collect::<Vec<_>>()
        );

        let random_ops = RandomHandlerAdapter::new(RealRandomHandler::new())
            .supported_operations(EffectType::Random);
        assert_eq!(
            random_ops,
            operations_for(EffectType::Random)
                .iter()
                .map(|op| (*op).to_string())
                .collect::<Vec<_>>()
        );

        let crypto_ops = CryptoHandlerAdapter::new(RealCryptoHandler::new())
            .supported_operations(EffectType::Crypto);
        assert_eq!(
            crypto_ops,
            operations_for(EffectType::Crypto)
                .iter()
                .map(|op| (*op).to_string())
                .collect::<Vec<_>>()
        );

        let storage_ops = StorageHandlerAdapter::new(FilesystemStorageHandler::with_default_path())
            .supported_operations(EffectType::Storage);
        assert_eq!(
            storage_ops,
            operations_for(EffectType::Storage)
                .iter()
                .map(|op| (*op).to_string())
                .collect::<Vec<_>>()
        );

        let time_ops = TimeHandlerAdapter::new(PhysicalTimeHandler::new())
            .supported_operations(EffectType::Time);
        assert_eq!(
            time_ops,
            operations_for(EffectType::Time)
                .iter()
                .map(|op| (*op).to_string())
                .collect::<Vec<_>>()
        );

        let network_ops = TransportHandlerAdapter::new(RealTransportHandler::default())
            .supported_operations(EffectType::Network);
        assert_eq!(
            network_ops,
            operations_for(EffectType::Network)
                .iter()
                .map(|op| (*op).to_string())
                .collect::<Vec<_>>()
        );

        let system_ops = LoggingSystemHandlerAdapter::new(LoggingSystemHandler::default())
            .supported_operations(EffectType::System);
        assert_eq!(
            system_ops,
            operations_for(EffectType::System)
                .iter()
                .map(|op| (*op).to_string())
                .collect::<Vec<_>>()
        );

        let trace_ops =
            TraceHandlerAdapter::new(TraceHandler::new()).supported_operations(EffectType::Trace);
        assert_eq!(
            trace_ops,
            operations_for(EffectType::Trace)
                .iter()
                .map(|op| (*op).to_string())
                .collect::<Vec<_>>()
        );
    }
}
