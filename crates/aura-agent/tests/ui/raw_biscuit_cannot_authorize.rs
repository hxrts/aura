use aura_authorization::{BiscuitAuthorizationBridge, ResourceScope};
use aura_core::types::scope::AuthorizationOp;

fn value<T>() -> T {
    panic!("compile-fail fixture")
}

fn main() {
    let bridge: BiscuitAuthorizationBridge = value();
    let raw_token: biscuit_auth::Biscuit = value();
    let operation: AuthorizationOp = value();
    let resource: ResourceScope = value();

    let _ = bridge.authorize_with_time(&raw_token, operation, &resource, Some(1));
}
