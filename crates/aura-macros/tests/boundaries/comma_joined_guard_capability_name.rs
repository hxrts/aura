use aura_macros::choreography;

choreography! {
    #[namespace = "example"]
    protocol ExampleProtocol {
        roles: Alice, Bob;

        Alice { guard_capability : "recovery:guardian_setup:accept_invitation,recovery:guardian_setup:verify_invitation" } -> Bob: Message;
    }
}

fn main() {}
