async fn low_level_raw_bytes_are_not_a_transcript<E>(crypto: &E)
where
    E: aura_core::effects::CryptoEffects + Send + Sync,
{
    let raw_bytes: &[u8] = b"raw protocol bytes";
    let private_key: &[u8] = &[0; 32];

    let _ = aura_signature::sign_ed25519_transcript(crypto, raw_bytes, private_key).await;
}

fn main() {}
