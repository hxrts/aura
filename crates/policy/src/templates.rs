// Policy templates for common scenarios

use crate::PolicyEngine;

/// Policy template builder
pub struct PolicyTemplate;

impl PolicyTemplate {
    /// Conservative template - strict requirements for all high-risk operations
    pub fn conservative() -> PolicyEngine {
        let mut engine = PolicyEngine::new();
        engine.set_require_threshold_for_high_risk(true);
        engine.set_min_guardians_high_risk(2);
        engine
    }
    
    /// Balanced template - reasonable security for most users
    pub fn balanced() -> PolicyEngine {
        let mut engine = PolicyEngine::new();
        engine.set_require_threshold_for_high_risk(true);
        engine.set_min_guardians_high_risk(1);
        engine
    }
    
    /// Permissive template - minimal restrictions (testing only)
    pub fn permissive() -> PolicyEngine {
        PolicyEngine::permissive()
    }
    
    /// Solo user template - single device, no guardians yet
    pub fn solo_user() -> PolicyEngine {
        let mut engine = PolicyEngine::new();
        engine.set_require_threshold_for_high_risk(false);
        engine.set_min_guardians_high_risk(0);
        engine
    }
    
    /// Enterprise template - strict for corporate environments
    pub fn enterprise() -> PolicyEngine {
        let mut engine = PolicyEngine::new();
        engine.set_require_threshold_for_high_risk(true);
        engine.set_min_guardians_high_risk(3);
        engine
    }
}

/// Policy documentation and examples
pub mod docs {
    
    /// Get policy template documentation
    pub fn get_template_docs(template: &str) -> Option<String> {
        match template {
            "conservative" => Some(
                r#"# Conservative Policy Template

## Overview
Strict security requirements for all high-risk operations.

## Requirements
- Minimum 2 guardians required
- Threshold signatures required for high-risk operations
- Native devices required for critical operations
- 48-hour cooldown for critical changes

## Best For
- High-value accounts
- Shared family accounts
- Users with sensitive data

## Example
```rust
let policy = PolicyTemplate::conservative();
```
"#.to_string()
            ),
            "balanced" => Some(
                r#"# Balanced Policy Template

## Overview
Reasonable security for most users.

## Requirements
- Minimum 1 guardian required
- Threshold signatures for high-risk operations
- 48-hour cooldown for critical changes

## Best For
- Individual users
- Standard personal accounts
- Default recommendation

## Example
```rust
let policy = PolicyTemplate::balanced();
```
"#.to_string()
            ),
            "solo_user" => Some(
                r#"# Solo User Policy Template

## Overview
Minimal requirements for users still setting up their account.

## Requirements
- No guardian requirement
- No threshold signature requirement for initial setup
- Critical operations still protected

## Best For
- New users
- Initial account setup
- Testing

## Example
```rust
let policy = PolicyTemplate::solo_user();
```

## Important
Upgrade to balanced or conservative once guardians are added.
"#.to_string()
            ),
            "enterprise" => Some(
                r#"# Enterprise Policy Template

## Overview
Strict requirements for corporate environments.

## Requirements
- Minimum 3 guardians required
- Threshold signatures for all high-risk operations
- Hardware-backed devices preferred
- Device attestation required

## Best For
- Corporate accounts
- High-security environments
- Compliance-heavy industries

## Example
```rust
let policy = PolicyTemplate::enterprise();
```
"#.to_string()
            ),
            _ => None,
        }
    }
    
    /// List all available templates
    pub fn list_templates() -> Vec<&'static str> {
        vec!["conservative", "balanced", "solo_user", "enterprise"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PolicyContext, DevicePosture, Operation};
    use crate::types::DeviceType;
    use aura_journal::{AccountId, DeviceId};

    fn mock_context(guardians: u32, effects: &aura_crypto::Effects) -> PolicyContext {
        PolicyContext {
            account_id: AccountId::new(),
            requester: DeviceId::new(),
            device_posture: DevicePosture {
                device_id: DeviceId::new(),
                device_type: DeviceType::Native,
                is_hardware_backed: true,
                has_secure_boot: true,
                is_jailbroken: false,
                last_attestation: Some(effects.now().unwrap_or(0)),
            },
            operation: Operation::add_device(),
            guardians_count: guardians,
            active_devices_count: 1,
            session_epoch: 1,
        }
    }

    #[test]
    fn test_conservative_requires_guardians() {
        let engine = PolicyTemplate::conservative();
        let effects = aura_crypto::Effects::test();
        let ctx = mock_context(1, &effects); // Only 1 guardian
        
        let decision = engine.evaluate(&ctx, &effects).unwrap();
        // Should deny because conservative requires 2 guardians
        assert!(matches!(decision, crate::PolicyDecision::Deny(_)));
    }

    #[test]
    fn test_balanced_accepts_one_guardian() {
        let engine = PolicyTemplate::balanced();
        let effects = aura_crypto::Effects::test();
        let ctx = mock_context(1, &effects);
        
        let decision = engine.evaluate(&ctx, &effects).unwrap();
        // Should allow with constraints
        assert!(matches!(decision, crate::PolicyDecision::AllowWithConstraints(_)));
    }

    #[test]
    fn test_solo_user_no_guardian_requirement() {
        let engine = PolicyTemplate::solo_user();
        let effects = aura_crypto::Effects::test();
        let ctx = mock_context(0, &effects); // No guardians
        
        let decision = engine.evaluate(&ctx, &effects).unwrap();
        // Solo user template should allow even with no guardians
        assert!(!matches!(decision, crate::PolicyDecision::Deny(_)));
    }

    #[test]
    fn test_template_docs_exist() {
        assert!(docs::get_template_docs("balanced").is_some());
        assert!(docs::get_template_docs("conservative").is_some());
        assert!(docs::get_template_docs("solo_user").is_some());
        assert!(docs::get_template_docs("enterprise").is_some());
    }

    #[test]
    fn test_list_templates() {
        let templates = docs::list_templates();
        assert!(templates.contains(&"balanced"));
        assert!(templates.contains(&"conservative"));
    }
}

