#[aura_macros::strong_reference(domain = "channel")]
struct ChannelBinding;

#[aura_macros::strong_reference(domain = "home_scope")]
enum HomeScope {
    Current,
}

fn main() {}
