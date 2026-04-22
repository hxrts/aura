use aura_core::secrets::SecretBytes;

fn main() {
    let secret = SecretBytes::import(vec![1, 2, 3]);
    let _ = secret.bytes;
}
