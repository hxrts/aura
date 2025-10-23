// Policy engine for evaluating authorization decisions

use crate::{Constraint, PolicyContext, PolicyDecision, Result, RiskTier};
use tracing::{debug, info};

/// Simple policy engine (MVP - would integrate Cedar in production)
pub struct PolicyEngine {
    /// Require threshold signature for high-risk ops
    require_threshold_for_high_risk: bool,
    /// Require native device for critical ops
    require_native_for_critical: bool,
    /// Minimum guardians for high-risk ops
    min_guardians_high_risk: u32,
}

impl PolicyEngine {
    pub fn new() -> Self {
        PolicyEngine {
            require_threshold_for_high_risk: true,
            require_native_for_critical: true,
            min_guardians_high_risk: 1,
        }
    }
    
    /// Create a permissive engine for testing
    pub fn permissive() -> Self {
        PolicyEngine {
            require_threshold_for_high_risk: false,
            require_native_for_critical: false,
            min_guardians_high_risk: 0,
        }
    }
    
    /// Evaluate a policy decision
    pub fn evaluate(&self, ctx: &PolicyContext, effects: &aura_crypto::Effects) -> Result<PolicyDecision> {
        info!(
            "Evaluating policy for operation {:?} at risk tier {:?}",
            ctx.operation.operation_type, ctx.operation.risk_tier
        );
        
        // Check device posture for critical operations
        if ctx.operation.risk_tier == RiskTier::Critical {
            if self.require_native_for_critical 
                && ctx.device_posture.device_type != crate::types::DeviceType::Native {
                return Ok(PolicyDecision::Deny(
                    "Critical operations require a native device".to_string()
                ));
            }
            
            if ctx.device_posture.is_jailbroken {
                return Ok(PolicyDecision::Deny(
                    "Jailbroken devices cannot perform critical operations".to_string()
                ));
            }
        }
        
        // Build constraints based on risk tier
        let mut constraints = Vec::new();
        
        match ctx.operation.risk_tier {
            RiskTier::Low => {
                // No additional constraints for low-risk ops
            }
            RiskTier::Medium => {
                // May require attestation for browser devices
                if ctx.device_posture.device_type == crate::types::DeviceType::Browser {
                    if let Some(last_attestation) = ctx.device_posture.last_attestation {
                        let now = effects.now().unwrap_or(0);
                        // Attestation must be recent (within 24 hours)
                        if now - last_attestation > 86400 {
                            constraints.push(Constraint::RequiresAttestation);
                        }
                    } else {
                        constraints.push(Constraint::RequiresAttestation);
                    }
                }
            }
            RiskTier::High => {
                // Check guardian count
                if ctx.guardians_count < self.min_guardians_high_risk {
                    return Ok(PolicyDecision::Deny(
                        format!(
                            "High-risk operations require at least {} guardians, found {}",
                            self.min_guardians_high_risk, ctx.guardians_count
                        )
                    ));
                }
                
                // Require threshold signature
                if self.require_threshold_for_high_risk {
                    constraints.push(Constraint::RequiresThresholdSignature);
                }
            }
            RiskTier::Critical => {
                // Critical ops always require threshold signature and cooldown
                constraints.push(Constraint::RequiresThresholdSignature);
                constraints.push(Constraint::RequiresCooldown(48 * 3600)); // 48 hours
                
                // Require guardian approvals
                let required_guardians = ctx.guardians_count.div_ceil(2); // Majority
                if required_guardians > 0 {
                    constraints.push(Constraint::RequiresGuardianApprovals(required_guardians));
                }
            }
        }
        
        if constraints.is_empty() {
            debug!("Policy decision: Allow (no constraints)");
            Ok(PolicyDecision::Allow)
        } else {
            debug!("Policy decision: Allow with {:?} constraints", constraints);
            Ok(PolicyDecision::AllowWithConstraints(constraints))
        }
    }
    
    /// Update policy configuration
    pub fn set_require_threshold_for_high_risk(&mut self, value: bool) {
        self.require_threshold_for_high_risk = value;
    }
    
    pub fn set_min_guardians_high_risk(&mut self, count: u32) {
        self.min_guardians_high_risk = count;
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DevicePosture, DeviceType, Operation};
    use aura_journal::{AccountId, DeviceId};

    fn mock_context(risk_tier: RiskTier, device_type: DeviceType, effects: &aura_crypto::Effects) -> PolicyContext {
        PolicyContext {
            account_id: AccountId::new(),
            requester: DeviceId::new(),
            device_posture: DevicePosture {
                device_id: DeviceId::new(),
                device_type,
                is_hardware_backed: device_type == DeviceType::Native,
                has_secure_boot: true,
                is_jailbroken: false,
                last_attestation: Some(effects.now().unwrap_or(0)),
            },
            operation: Operation {
                operation_type: crate::types::OperationType::StoreObject,
                risk_tier,
                resource: None,
            },
            guardians_count: 2,
            active_devices_count: 3,
            session_epoch: 1,
        }
    }

    #[test]
    fn test_low_risk_allows() {
        let effects = aura_crypto::Effects::test();
        let engine = PolicyEngine::new();
        let ctx = mock_context(RiskTier::Low, DeviceType::Native, &effects);
        
        let decision = engine.evaluate(&ctx, &effects).unwrap();
        assert_eq!(decision, PolicyDecision::Allow);
    }

    #[test]
    fn test_high_risk_requires_threshold() {
        let engine = PolicyEngine::new();
        let effects = aura_crypto::Effects::test();
        let ctx = mock_context(RiskTier::High, DeviceType::Native, &effects);
        
        let decision = engine.evaluate(&ctx, &effects).unwrap();
        match decision {
            PolicyDecision::AllowWithConstraints(constraints) => {
                assert!(constraints.contains(&Constraint::RequiresThresholdSignature));
            }
            _ => panic!("Expected AllowWithConstraints"),
        }
    }

    #[test]
    fn test_critical_denies_browser_device() {
        let mut engine = PolicyEngine::new();
        engine.require_native_for_critical = true;
        let effects = aura_crypto::Effects::test();
        let ctx = mock_context(RiskTier::Critical, DeviceType::Browser, &effects);
        
        let decision = engine.evaluate(&ctx, &effects).unwrap();
        match decision {
            PolicyDecision::Deny(reason) => {
                assert!(reason.contains("native device"));
            }
            _ => panic!("Expected Deny"),
        }
    }

    #[test]
    fn test_jailbroken_device_denied() {
        let engine = PolicyEngine::new();
        let effects = aura_crypto::Effects::test();
        let mut ctx = mock_context(RiskTier::Critical, DeviceType::Native, &effects);
        ctx.device_posture.is_jailbroken = true;
        
        let decision = engine.evaluate(&ctx, &effects).unwrap();
        match decision {
            PolicyDecision::Deny(reason) => {
                assert!(reason.contains("Jailbroken"));
            }
            _ => panic!("Expected Deny"),
        }
    }

    #[test]
    fn test_insufficient_guardians() {
        let engine = PolicyEngine::new();
        let effects = aura_crypto::Effects::test();
        let mut ctx = mock_context(RiskTier::High, DeviceType::Native, &effects);
        ctx.guardians_count = 0;
        
        let decision = engine.evaluate(&ctx, &effects).unwrap();
        match decision {
            PolicyDecision::Deny(reason) => {
                assert!(reason.contains("guardians"));
            }
            _ => panic!("Expected Deny"),
        }
    }
}

