#![allow(missing_docs)]

use aura_macros::capability_family;

#[capability_family(namespace = "")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenericCapability {
    #[capability("read")]
    Read,
    #[capability("write")]
    Write,
    #[capability("execute")]
    Execute,
    #[capability("delegate")]
    Delegate,
    #[capability("moderator")]
    Moderator,
    #[capability("flow_charge")]
    FlowCharge,
}

pub fn evaluation_candidates_for_generic_policy() -> &'static [GenericCapability] {
    GenericCapability::declared_names()
}
