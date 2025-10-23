// Biscuit token generation and verification for delegated authorization

use crate::{Operation, PolicyError, Result};
use aura_journal::{AccountId, DeviceId};
use biscuit_auth::{Biscuit, KeyPair, PublicKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

/// Capability token for delegated operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityToken {
    /// Serialized biscuit token
    pub token: Vec<u8>,
    /// Public key for verification
    pub public_key: Vec<u8>,
    /// Token metadata
    pub metadata: TokenMetadata,
}

/// Token metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenMetadata {
    pub issuer: DeviceId,
    pub account_id: AccountId,
    pub operation: String,
    pub expires_at: u64,
    pub issued_at: u64,
}

/// Biscuit token builder
pub struct BiscuitBuilder {
    /// Root keypair for signing tokens
    root_keypair: Arc<KeyPair>,
    /// Account ID this builder is for
    account_id: AccountId,
}

impl BiscuitBuilder {
    /// Create a new builder with a generated keypair
    pub fn new(account_id: AccountId) -> Self {
        let root_keypair = KeyPair::new();
        BiscuitBuilder {
            root_keypair: Arc::new(root_keypair),
            account_id,
        }
    }
    
    /// Get keypair bytes for serialization
    pub fn keypair_bytes(&self) -> Vec<u8> {
        self.root_keypair.private().to_bytes().to_vec()
    }
    
    /// Get the public key for verification
    pub fn public_key(&self) -> PublicKey {
        self.root_keypair.public()
    }
    
    /// Build a capability token for an operation
    pub fn build_capability(
        &self,
        issuer: DeviceId,
        operation: Operation,
        ttl_seconds: u64,
        effects: &aura_crypto::Effects,
    ) -> Result<CapabilityToken> {
        let now = effects.now().unwrap_or(0);
        let expires_at = now + ttl_seconds;
        
        info!(
            "Building capability token for operation {:?}, expires in {}s",
            operation.operation_type, ttl_seconds
        );
        
        // Build biscuit with facts and checks
        // For MVP, we'll use a simplified approach with basic datalog
        let mut builder = Biscuit::builder();
        
        // Add authority block with operation metadata
        // Note: biscuit-auth 4.1 uses datalog code blocks
        let code = format!(
            r#"
            account("{:?}");
            operation("{:?}");
            risk_tier("{:?}");
            check if time($time), $time < {};
            "#,
            self.account_id,
            operation.operation_type,
            operation.risk_tier,
            expires_at
        );
        
        builder.add_code(code)
            .map_err(|e| PolicyError::BiscuitError(e.to_string()))?;
        
        // Build and sign the token
        let biscuit = builder
            .build(&self.root_keypair)
            .map_err(|e| PolicyError::BiscuitError(e.to_string()))?;
        
        let token = biscuit
            .to_vec()
            .map_err(|e| PolicyError::BiscuitError(e.to_string()))?;
        
        let public_key = self.public_key().to_bytes().to_vec();
        
        let metadata = TokenMetadata {
            issuer,
            account_id: self.account_id,
            operation: format!("{:?}", operation.operation_type),
            expires_at,
            issued_at: now,
        };
        
        debug!("Capability token built successfully");
        
        Ok(CapabilityToken {
            token,
            public_key,
            metadata,
        })
    }
    
    /// Verify a capability token
    pub fn verify_capability(
        token: &CapabilityToken,
        public_key: &PublicKey,
        effects: &aura_crypto::Effects,
    ) -> Result<HashMap<String, String>> {
        debug!("Verifying capability token");
        
        // Parse the biscuit
        let biscuit = Biscuit::from(&token.token, public_key)
            .map_err(|e| PolicyError::BiscuitError(format!("Failed to parse token: {}", e)))?;
        
        // Create authorizer with current time
        let now = effects.now().unwrap_or(0);
        
        let mut authorizer = biscuit.authorizer()
            .map_err(|e| PolicyError::BiscuitError(e.to_string()))?;
        
        // Add time fact for expiration check
        authorizer.add_code(format!("time({});", now))
            .map_err(|e| PolicyError::BiscuitError(e.to_string()))?;
        
        // Add allow policy
        authorizer.add_code("allow if true;")
            .map_err(|e| PolicyError::BiscuitError(e.to_string()))?;
        
        // Authorize
        authorizer.authorize()
            .map_err(|e| PolicyError::BiscuitError(format!("Authorization failed: {}", e)))?;
        
        // Extract facts
        let mut facts = HashMap::new();
        facts.insert("issued_at".to_string(), token.metadata.issued_at.to_string());
        facts.insert("expires_at".to_string(), token.metadata.expires_at.to_string());
        facts.insert("operation".to_string(), token.metadata.operation.clone());
        
        info!("Capability token verified successfully");
        
        Ok(facts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Operation, OperationType, RiskTier};

    #[test]
    fn test_build_and_verify_capability() {
        let account_id = AccountId::new();
        let builder = BiscuitBuilder::new(account_id);
        let issuer = DeviceId::new();
        
        let operation = Operation {
            operation_type: OperationType::FetchObject,
            risk_tier: RiskTier::Low,
            resource: Some("object:abc123".to_string()),
        };
        
        let effects = aura_crypto::Effects::test();
        let token = builder.build_capability(issuer, operation, 3600, &effects).unwrap();
        
        // Verify with correct public key
        let public_key = builder.public_key();
        let facts = BiscuitBuilder::verify_capability(&token, &public_key, &effects).unwrap();
        
        assert!(facts.contains_key("operation"));
        assert_eq!(facts.get("operation").unwrap(), "FetchObject");
    }

    #[test]
    fn test_expired_token_fails() {
        let account_id = AccountId::new();
        let builder = BiscuitBuilder::new(account_id);
        let issuer = DeviceId::new();
        
        let operation = Operation {
            operation_type: OperationType::StoreObject,
            risk_tier: RiskTier::Low,
            resource: None,
        };
        
        // Create token with 0 second TTL (immediately expired)
        let effects = aura_crypto::Effects::test();
        let token = builder.build_capability(issuer, operation, 0, &effects).unwrap();
        
        // Wait a moment to ensure expiration
        std::thread::sleep(std::time::Duration::from_millis(10));
        
        // Verification should fail
        let public_key = builder.public_key();
        let result = BiscuitBuilder::verify_capability(&token, &public_key, &effects);
        
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_public_key_fails() {
        let account_id = AccountId::new();
        let builder = BiscuitBuilder::new(account_id);
        let issuer = DeviceId::new();
        
        let operation = Operation {
            operation_type: OperationType::FetchObject,
            risk_tier: RiskTier::Low,
            resource: None,
        };
        
        let effects = aura_crypto::Effects::test();
        let token = builder.build_capability(issuer, operation, 3600, &effects).unwrap();
        
        // Try to verify with wrong public key
        let wrong_keypair = KeyPair::new();
        let wrong_public_key = wrong_keypair.public();
        
        let result = BiscuitBuilder::verify_capability(&token, &wrong_public_key, &effects);
        assert!(result.is_err());
    }
}

