#[aura_macros::authoritative_source(kind = "runtime")]
async fn runtime_source() {}

struct Demo;

impl Demo {
    #[aura_macros::authoritative_source(kind = "signal")]
    async fn signal_source(&self) {}
}

fn main() {}
