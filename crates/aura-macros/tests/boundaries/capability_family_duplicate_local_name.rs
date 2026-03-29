use aura_macros::capability_family;

#[capability_family(namespace = "chat")]
enum ChatCapability {
    #[capability("message:send")]
    SendMessage,
    #[capability("message:send")]
    SendMessageAgain,
}

fn main() {}
