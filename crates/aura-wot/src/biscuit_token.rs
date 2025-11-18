use aura_core::{AccountId, DeviceId};
use biscuit_auth::{macros::*, Biscuit, KeyPair, PublicKey};
use serde::{Deserialize, Serialize};

pub struct AccountAuthority {
    account_id: AccountId,
    root_keypair: KeyPair,
}

impl AccountAuthority {
    pub fn new(account_id: AccountId) -> Self {
        Self {
            account_id,
            root_keypair: KeyPair::new(),
        }
    }

    pub fn create_device_token(&self, device_id: DeviceId) -> Result<Biscuit, BiscuitError> {
        let account = self.account_id.to_string();
        let device = device_id.to_string();

        let token = biscuit!(
            r#"
            account({account});
            device({device});
            role("owner");
            capability("read");
            capability("write");
            capability("execute");
            capability("delegate");
            capability("admin");
        "#
        )
        .build(&self.root_keypair)?;

        Ok(token)
    }

    pub fn root_public_key(&self) -> PublicKey {
        self.root_keypair.public()
    }

    pub fn root_keypair(&self) -> &KeyPair {
        &self.root_keypair
    }

    /// Get the account ID associated with this authority
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Create a new AccountAuthority from existing keypair (for loading from storage)
    pub fn from_keypair(account_id: AccountId, keypair: KeyPair) -> Self {
        Self {
            account_id,
            root_keypair: keypair,
        }
    }

    /// Export the keypair for secure storage
    pub fn export_keypair(&self) -> &KeyPair {
        &self.root_keypair
    }

    /// Check if this authority can verify tokens for the given account
    pub fn can_verify_for_account(&self, account_id: &AccountId) -> bool {
        &self.account_id == account_id
    }
}

pub struct BiscuitTokenManager {
    device_id: DeviceId,
    current_token: Biscuit,
}

impl BiscuitTokenManager {
    pub fn new(device_id: DeviceId, initial_token: Biscuit) -> Self {
        Self {
            device_id,
            current_token: initial_token,
        }
    }

    pub fn attenuate_read(&self, resource_prefix: &str) -> Result<Biscuit, BiscuitError> {
        let prefix = resource_prefix.to_string();
        let attenuated = self.current_token.append(block!(
            r#"
            check if operation("read");
            check if resource($res), $res.starts_with({prefix});
        "#
        ))?;
        Ok(attenuated)
    }

    pub fn attenuate_write(&self, resource_prefix: &str) -> Result<Biscuit, BiscuitError> {
        let prefix = resource_prefix.to_string();
        let attenuated = self.current_token.append(block!(
            r#"
            check if operation("write");
            check if resource($res), $res.starts_with({prefix});
        "#
        ))?;
        Ok(attenuated)
    }

    pub fn current_token(&self) -> &Biscuit {
        &self.current_token
    }

    /// Get the device ID associated with this token manager
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Update the current token (for token refresh scenarios)
    pub fn update_token(&mut self, new_token: Biscuit) {
        self.current_token = new_token;
    }

    pub fn serialize_token(&self) -> Result<Vec<u8>, BiscuitError> {
        self.current_token
            .to_vec()
            .map_err(BiscuitError::BiscuitLib)
    }

    pub fn deserialize_token(bytes: &[u8], root_key: &PublicKey) -> Result<Biscuit, BiscuitError> {
        Biscuit::from(bytes, *root_key).map_err(BiscuitError::BiscuitLib)
    }
}

/// Serializable wrapper for Biscuit tokens
#[derive(Debug, Clone)]
pub struct SerializableBiscuit {
    inner: Biscuit,
    root_key: PublicKey,
}

impl SerializableBiscuit {
    pub fn new(biscuit: Biscuit, root_key: PublicKey) -> Self {
        Self {
            inner: biscuit,
            root_key,
        }
    }

    pub fn biscuit(&self) -> &Biscuit {
        &self.inner
    }

    pub fn into_biscuit(self) -> Biscuit {
        self.inner
    }
}

impl Serialize for SerializableBiscuit {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::Error;

        // Serialize public key + token together
        let token_bytes = self
            .inner
            .to_vec()
            .map_err(|e| S::Error::custom(e.to_string()))?;

        // Format: [32 bytes public key][N bytes token]
        let mut all_bytes = Vec::with_capacity(32 + token_bytes.len());
        all_bytes.extend_from_slice(&self.root_key.to_bytes());
        all_bytes.extend_from_slice(&token_bytes);

        serializer.serialize_bytes(&all_bytes)
    }
}

impl<'de> Deserialize<'de> for SerializableBiscuit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        // Serialized format: [32 bytes public key][N bytes token]
        // This ensures the public key is always available for verification
        let all_bytes: Vec<u8> = serde::Deserialize::deserialize(deserializer)?;

        if all_bytes.len() < 32 {
            return Err(D::Error::custom(
                "SerializableBiscuit requires at least 32 bytes for public key",
            ));
        }

        // Extract public key from first 32 bytes
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&all_bytes[..32]);
        let root_key = PublicKey::from_bytes(&key_bytes)
            .map_err(|e| D::Error::custom(format!("Invalid public key: {}", e)))?;

        // Extract token bytes from remaining data
        let token_bytes = &all_bytes[32..];
        let biscuit =
            Biscuit::from(token_bytes, root_key).map_err(|e| D::Error::custom(e.to_string()))?;

        Ok(SerializableBiscuit::new(biscuit, root_key))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BiscuitError {
    #[error("Biscuit library error: {0}")]
    BiscuitLib(#[from] biscuit_auth::error::Token),

    #[error("Authorization failed: {0}")]
    AuthorizationFailed(String),
}
