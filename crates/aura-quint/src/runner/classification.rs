use crate::PropertySpec;

/// Capability-related property metadata used by Aura integration helpers.
#[derive(Debug, Clone)]
pub(crate) struct CapabilityProperty {
    pub(crate) name: String,
    pub(crate) property_type: CapabilityPropertyType,
}

/// Privacy-related property metadata used by Aura integration helpers.
#[derive(Debug, Clone)]
pub(crate) struct PrivacyProperty {
    pub(crate) name: String,
    pub(crate) property_type: PrivacyPropertyType,
}

/// Types of capability properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CapabilityPropertyType {
    Authorization,
    Budget,
    Integrity,
    NonInterference,
    Monotonicity,
    TemporalConsistency,
    ContextIsolation,
    AuthorizationSoundness,
    General,
}

/// Types of privacy properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PrivacyPropertyType {
    LeakageBounds,
    Unlinkability,
    ContextIsolation,
    ObserverSimulation,
    General,
}

pub(crate) fn extract_capability_properties(spec: &PropertySpec) -> Vec<CapabilityProperty> {
    spec.properties
        .iter()
        .filter(|property_name| is_capability_property(property_name))
        .map(|property_name| CapabilityProperty {
            name: property_name.clone(),
            property_type: determine_capability_property_type(property_name),
        })
        .collect()
}

pub(crate) fn extract_privacy_properties(spec: &PropertySpec) -> Vec<PrivacyProperty> {
    spec.properties
        .iter()
        .filter(|property_name| is_privacy_property(property_name))
        .map(|property_name| PrivacyProperty {
            name: property_name.clone(),
            property_type: determine_privacy_property_type(property_name),
        })
        .collect()
}

pub(crate) fn is_capability_property(property_name: &str) -> bool {
    let property_name = property_name.to_lowercase();
    [
        "cap",
        "capability",
        "permission",
        "auth",
        "grant",
        "restrict",
        "soundness",
        "monotonic",
        "interference",
    ]
    .iter()
    .any(|keyword| property_name.contains(keyword))
}

pub(crate) fn is_privacy_property(property_name: &str) -> bool {
    let property_name = property_name.to_lowercase();
    [
        "privacy",
        "leakage",
        "unlinkable",
        "anonymous",
        "isolated",
        "context",
        "observer",
        "bound",
    ]
    .iter()
    .any(|keyword| property_name.contains(keyword))
}

pub(crate) fn determine_capability_property_type(property_name: &str) -> CapabilityPropertyType {
    let name_lower = property_name.to_lowercase();

    if name_lower.contains("monotonic") {
        CapabilityPropertyType::Monotonicity
    } else if name_lower.contains("interference") {
        CapabilityPropertyType::NonInterference
    } else if name_lower.contains("temporal") {
        CapabilityPropertyType::TemporalConsistency
    } else if name_lower.contains("isolation") {
        CapabilityPropertyType::ContextIsolation
    } else if name_lower.contains("soundness") {
        CapabilityPropertyType::AuthorizationSoundness
    } else if name_lower.contains("grant")
        || name_lower.contains("permit")
        || name_lower.contains("restrict")
        || name_lower.contains("guard")
        || name_lower.contains("capguard")
        || name_lower.contains("authorization")
    {
        CapabilityPropertyType::Authorization
    } else if name_lower.contains("budget")
        || name_lower.contains("charge")
        || name_lower.contains("spent")
        || name_lower.contains("limit")
        || name_lower.contains("flowguard")
        || name_lower.contains("receipt")
        || name_lower.contains("epoch")
    {
        CapabilityPropertyType::Budget
    } else if name_lower.contains("integrity")
        || name_lower.contains("attenuat")
        || name_lower.contains("forgea")
        || name_lower.contains("signature")
        || name_lower.contains("biscuit")
        || name_lower.contains("chain")
    {
        CapabilityPropertyType::Integrity
    } else {
        CapabilityPropertyType::General
    }
}

pub(crate) fn determine_privacy_property_type(property_name: &str) -> PrivacyPropertyType {
    let name_lower = property_name.to_lowercase();

    if name_lower.contains("leakage") {
        PrivacyPropertyType::LeakageBounds
    } else if name_lower.contains("unlinkable") {
        PrivacyPropertyType::Unlinkability
    } else if name_lower.contains("isolation") {
        PrivacyPropertyType::ContextIsolation
    } else if name_lower.contains("observer") {
        PrivacyPropertyType::ObserverSimulation
    } else {
        PrivacyPropertyType::General
    }
}
