#[derive(Clone)]
struct DemoCommand;

#[aura_macros::actor_owned(
    owner = "demo-actor",
    domain = "demo-domain",
    gate = "demo-ingress",
    command = DemoCommand,
    capacity = 64,
    category = "actor_owned"
)]
struct DemoState;

fn main() {}
