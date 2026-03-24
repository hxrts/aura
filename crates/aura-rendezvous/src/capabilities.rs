#![allow(missing_docs)]

use aura_macros::capability_family;

#[capability_family(namespace = "rendezvous")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RendezvousCapability {
    #[capability("publish")]
    Publish,
    #[capability("connect")]
    Connect,
    #[capability("relay")]
    Relay,
}

#[capability_family(namespace = "relay")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelayCapability {
    #[capability("forward")]
    Forward,
}

pub fn evaluation_candidates_for_rendezvous_guard() -> &'static [RendezvousCapability] {
    RendezvousCapability::declared_names()
}

pub fn evaluation_candidates_for_relay_guard() -> &'static [RelayCapability] {
    RelayCapability::declared_names()
}
