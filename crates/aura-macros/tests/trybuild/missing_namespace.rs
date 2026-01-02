use aura_macros::choreography;

choreography! {
    choreography MissingNamespace {
        roles: Alice, Bob;

        Alice -> Bob: Message;
    }
}

fn main() {}
