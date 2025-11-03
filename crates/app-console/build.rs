//! Build script for the Aura development console.
//!
//! This script ensures that the build system reruns when CSS files change,
//! allowing for proper stylesheet recompilation during development.

fn main() {
    // Tell cargo to rerun if CSS files change
    println!("cargo:rerun-if-changed=styles/");
}
