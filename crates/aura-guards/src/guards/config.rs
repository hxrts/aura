//! Guard runtime configuration.

use super::privacy::AdversaryClass;

#[derive(Debug, Clone)]
pub struct GuardRuntimeConfig {
    pub default_observers: Vec<AdversaryClass>,
}

impl Default for GuardRuntimeConfig {
    fn default() -> Self {
        Self {
            default_observers: vec![
                AdversaryClass::External,
                AdversaryClass::Neighbor,
                AdversaryClass::InGroup,
            ],
        }
    }
}
