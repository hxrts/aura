fn main() {
    // Tell cargo to rerun if CSS files change
    println!("cargo:rerun-if-changed=styles/");
}
