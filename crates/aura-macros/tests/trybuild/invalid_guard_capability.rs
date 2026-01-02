use aura_macros::choreography;

choreography! {
    #[namespace = "invalid_guard_capability"]
    choreography InvalidGuardCapability {
        roles: Alice, Bob;

        Alice[guard_capability = 42] -> Bob: Message;
    }
}

fn main() {}
