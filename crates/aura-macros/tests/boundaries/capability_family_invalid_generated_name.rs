use aura_macros::capability_family;

#[capability_family(namespace = "chat")]
enum ChatCapability {
    #[capability("Message:Send")]
    MessageSend,
}

fn main() {}
