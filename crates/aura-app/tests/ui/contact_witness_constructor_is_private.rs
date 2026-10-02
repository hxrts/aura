use aura_app::views::contacts::ContactAddedWitness;

fn fabricate<T>() -> T {
    panic!("compile-fail fixture")
}

fn main() {
    let _ = ContactAddedWitness::from_fact(&fabricate());
}
