use aura_macros::capability_family;

#[capability_family(namespace = "chat")]
enum ChatCapability {
    Send,
}

fn main() {}
