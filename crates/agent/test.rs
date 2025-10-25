use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Test {
    field: String,
}

fn main() {
    println!("Test");
}