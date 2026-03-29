use aura_macros::capability_family;

#[capability_family(namespace = "chat:message")]
enum ChatCapability {
    #[capability("send")]
    Send,
}

fn main() {}
