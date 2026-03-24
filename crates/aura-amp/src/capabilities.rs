#![allow(missing_docs)]

use aura_macros::capability_family;

#[capability_family(namespace = "amp")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmpCapability {
    #[capability("send")]
    Send,
    #[capability("receive")]
    Receive,
}

pub fn evaluation_candidates_for_amp_protocol() -> &'static [AmpCapability] {
    AmpCapability::declared_names()
}
