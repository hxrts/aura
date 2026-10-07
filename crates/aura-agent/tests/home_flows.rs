//! Multi-runtime home flows on the shared virtual-time fixture.
//!
//! `scripts/check/accept-chain-stack.sh` runs this binary under a reduced
//! thread stack to bound the home-invitation accept chain.
#![cfg(not(target_arch = "wasm32"))]
#![allow(missing_docs)]

mod support;

mod home_invitation_readiness_hook;
mod home_moderation_workflows;
mod home_two_member_moderation;
