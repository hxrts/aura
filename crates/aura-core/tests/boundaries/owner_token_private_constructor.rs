use aura_core::ownership::OwnerToken;

fn main() {
    let _token = OwnerToken {
        token_id: "token-1",
        scope: "session",
    };
}
