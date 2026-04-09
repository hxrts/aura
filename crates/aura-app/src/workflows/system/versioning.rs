//! Portable OTA version parsing and upgrade-kind normalization.

use aura_core::AuraError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeKindValue {
    Soft,
    Hard,
}

impl UpgradeKindValue {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Hard => "hard",
        }
    }
}

impl std::fmt::Display for UpgradeKindValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

pub fn parse_upgrade_kind(input: &str) -> Result<UpgradeKindValue, AuraError> {
    match input.to_lowercase().as_str() {
        "soft" => Ok(UpgradeKindValue::Soft),
        "hard" => Ok(UpgradeKindValue::Hard),
        _ => Err(AuraError::invalid(format!(
            "Invalid upgrade kind: '{input}'. Use 'soft' or 'hard'"
        ))),
    }
}

pub fn parse_semantic_version(input: &str) -> Result<(u16, u16, u16), AuraError> {
    let parts: Vec<&str> = input.split('.').collect();
    if parts.len() != 3 {
        return Err(AuraError::invalid(
            "Invalid semantic version format. Expected: major.minor.patch",
        ));
    }

    let major = parts[0].parse().map_err(|error| {
        AuraError::invalid(format!("Invalid major version '{}': {}", parts[0], error))
    })?;
    let minor = parts[1].parse().map_err(|error| {
        AuraError::invalid(format!("Invalid minor version '{}': {}", parts[1], error))
    })?;
    let patch = parts[2].parse().map_err(|error| {
        AuraError::invalid(format!("Invalid patch version '{}': {}", parts[2], error))
    })?;

    Ok((major, minor, patch))
}

pub fn validate_version_string(input: &str) -> Result<&str, AuraError> {
    parse_semantic_version(input)?;
    Ok(input)
}
