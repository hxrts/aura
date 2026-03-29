use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Canonical validated authorization capability name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CapabilityName(String);

impl CapabilityName {
    /// Parse and validate a capability name at an explicit string boundary.
    pub fn parse(value: &str) -> Result<Self, CapabilityNameError> {
        if value.is_empty() {
            return Err(CapabilityNameError::Empty);
        }
        if !is_valid_capability_name_literal(value) {
            return Err(CapabilityNameError::Invalid {
                value: value.to_string(),
            });
        }
        Ok(Self(value.to_string()))
    }

    /// Borrow the canonical string form.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for CapabilityName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl TryFrom<&str> for CapabilityName {
    type Error = CapabilityNameError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl TryFrom<String> for CapabilityName {
    type Error = CapabilityNameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl fmt::Display for CapabilityName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for CapabilityName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CapabilityName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(D::Error::custom)
    }
}

/// Capability-name parsing error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CapabilityNameError {
    #[error("capability cannot be empty")]
    Empty,
    #[error("invalid capability token: {value}")]
    Invalid { value: String },
}

/// Compile-time-friendly capability-name validator used by `capability_name!`.
#[doc(hidden)]
pub const fn is_valid_capability_name_literal(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return false;
    }

    let mut idx = 0;
    while idx < bytes.len() {
        let byte = bytes[idx];
        let allowed = byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || byte == b'_'
            || byte == b'-'
            || byte == b':';
        if !allowed {
            return false;
        }
        idx += 1;
    }

    true
}

/// Construct a validated [`CapabilityName`] from a string literal.
///
/// This is the only sanctioned first-party literal declaration path. Product
/// code must not gain a public unchecked or static constructor on
/// [`CapabilityName`].
#[macro_export]
macro_rules! capability_name {
    ($value:literal) => {{
        const _: () = {
            if !$crate::capability_name::is_valid_capability_name_literal($value) {
                panic!("invalid capability name literal");
            }
        };

        match $crate::CapabilityName::parse($value) {
            Ok(name) => name,
            Err(_) => unreachable!("capability_name! validated the literal"),
        }
    }};
}

#[cfg(test)]
mod tests {
    use super::CapabilityName;
    use std::error::Error;

    #[test]
    fn parses_valid_names() -> Result<(), Box<dyn Error>> {
        let parsed = CapabilityName::parse("invitation:guardian:accept")?;
        assert_eq!(parsed.as_str(), "invitation:guardian:accept");
        Ok(())
    }

    #[test]
    fn rejects_empty_names() {
        assert!(CapabilityName::parse("").is_err());
    }

    #[test]
    fn rejects_uppercase_names() {
        assert!(CapabilityName::parse("Invitation:Send").is_err());
    }

    #[test]
    fn rejects_whitespace() {
        assert!(CapabilityName::parse("invitation:send now").is_err());
    }

    #[test]
    fn serde_round_trip_preserves_value() -> Result<(), Box<dyn Error>> {
        let name = CapabilityName::parse("chat:message:send")?;
        let encoded = serde_json::to_string(&name)?;
        assert_eq!(encoded, "\"chat:message:send\"");

        let decoded: CapabilityName = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, name);
        Ok(())
    }

    #[test]
    fn deserialization_rejects_invalid_names() {
        match serde_json::from_str::<CapabilityName>("\"Chat:Send\"") {
            Ok(name) => panic!("invalid name should fail, got {}", name.as_str()),
            Err(error) => assert!(error.to_string().contains("invalid capability token")),
        }
    }

    #[test]
    fn macro_constructs_valid_name() {
        let name = crate::capability_name!("auth:request");
        assert_eq!(name.as_str(), "auth:request");
    }
}
