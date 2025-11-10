//! Macros for standardized ID type definition and implementation
//!
//! This module provides macros to reduce boilerplate when defining new ID types
//! with consistent patterns for new(), from(), Display, and serde support.

/// Define a UUID-based ID type with standard implementations
///
/// Generates a newtype wrapper around `Uuid` with:
/// - `#[derive(Debug, Clone, Copy, ...)]` attributes
/// - `new()` method using `Uuid::new_v4()`
/// - `from_uuid()` constructor
/// - `uuid()` accessor
/// - `Display` implementation with optional prefix
/// - `From<Uuid>` and `From<Self> -> Uuid` conversions
/// - `Default` implementation
///
/// # Examples
///
/// ```ignore
/// define_uuid_id!(SessionId, "session");
/// // Generates: SessionId with Display format "session-{uuid}"
///
/// define_uuid_id!(DeviceId);
/// // Generates: DeviceId with Display format "{uuid}" (no prefix)
/// ```
#[macro_export]
macro_rules! define_uuid_id {
    ($name:ident, $display_prefix:expr) => {
        #[doc = concat!("Identifier for ", stringify!($name))]
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            serde::Serialize,
            serde::Deserialize,
        )]
        pub struct $name(pub uuid::Uuid);

        impl $name {
            #[doc = concat!("Create a new random ", stringify!($name))]
            #[allow(clippy::disallowed_methods)]
            pub fn new() -> Self {
                Self(uuid::Uuid::new_v4())
            }

            #[doc = concat!("Create ", stringify!($name), " from a UUID")]
            pub fn from_uuid(uuid: uuid::Uuid) -> Self {
                Self(uuid)
            }

            #[doc = concat!("Get the inner UUID")]
            pub fn uuid(&self) -> uuid::Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}-{}", $display_prefix, self.0)
            }
        }

        impl From<uuid::Uuid> for $name {
            fn from(uuid: uuid::Uuid) -> Self {
                Self(uuid)
            }
        }

        impl From<$name> for uuid::Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }
    };
    ($name:ident) => {
        #[doc = concat!("Identifier for ", stringify!($name))]
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            serde::Serialize,
            serde::Deserialize,
        )]
        pub struct $name(pub uuid::Uuid);

        impl $name {
            #[doc = concat!("Create a new random ", stringify!($name))]
            #[allow(clippy::disallowed_methods)]
            pub fn new() -> Self {
                Self(uuid::Uuid::new_v4())
            }

            #[doc = concat!("Create ", stringify!($name), " from a UUID")]
            pub fn from_uuid(uuid: uuid::Uuid) -> Self {
                Self(uuid)
            }

            #[doc = concat!("Get the inner UUID")]
            pub fn uuid(&self) -> uuid::Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<uuid::Uuid> for $name {
            fn from(uuid: uuid::Uuid) -> Self {
                Self(uuid)
            }
        }

        impl From<$name> for uuid::Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }
    };
}

/// Define a string-based ID type with standard implementations
///
/// Generates a newtype wrapper around `String` with:
/// - `#[derive(Debug, Clone, ...)]` attributes
/// - `new()` method accepting `impl Into<String>`
/// - `as_str()` accessor
/// - `Display` implementation with optional prefix
/// - `From<String>` and `From<&str>` conversions
///
/// # Examples
///
/// ```ignore
/// define_string_id!(MemberId, "member");
/// // Generates: MemberId with Display format "member-{string}"
///
/// define_string_id!(ContextId);
/// // Generates: ContextId with Display format "{string}" (no prefix)
/// ```
#[macro_export]
macro_rules! define_string_id {
    ($name:ident, $display_prefix:expr) => {
        #[doc = concat!("Identifier for ", stringify!($name))]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
        pub struct $name(pub String);

        impl $name {
            #[doc = concat!("Create a new ", stringify!($name))]
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            #[doc = concat!("Get the inner string")]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}-{}", $display_prefix, self.0)
            }
        }

        impl From<String> for $name {
            fn from(id: String) -> Self {
                Self(id)
            }
        }

        impl From<&str> for $name {
            fn from(id: &str) -> Self {
                Self(id.to_string())
            }
        }
    };
    ($name:ident) => {
        #[doc = concat!("Identifier for ", stringify!($name))]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
        pub struct $name(pub String);

        impl $name {
            #[doc = concat!("Create a new ", stringify!($name))]
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            #[doc = concat!("Get the inner string")]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<String> for $name {
            fn from(id: String) -> Self {
                Self(id)
            }
        }

        impl From<&str> for $name {
            fn from(id: &str) -> Self {
                Self(id.to_string())
            }
        }
    };
}

/// Define a numeric ID type with standard implementations
///
/// Generates a newtype wrapper around a numeric type with:
/// - `#[derive(Debug, Clone, Copy, ...)]` attributes
/// - `new()` constructor
/// - `value()` accessor
/// - `Display` implementation with optional prefix
/// - `From<T>` and `From<Self> -> T` conversions
///
/// # Examples
///
/// ```ignore
/// define_numeric_id!(EventNonce, u64, "nonce");
/// // Generates: EventNonce(u64) with Display format "nonce-{u64}"
/// ```
#[macro_export]
macro_rules! define_numeric_id {
    ($name:ident, $inner:ty, $display_prefix:expr) => {
        #[doc = concat!("Identifier for ", stringify!($name))]
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            serde::Serialize,
            serde::Deserialize,
        )]
        pub struct $name(pub $inner);

        impl $name {
            #[doc = concat!("Create a new ", stringify!($name))]
            pub fn new(value: $inner) -> Self {
                Self(value)
            }

            #[doc = concat!("Get the inner value")]
            pub fn value(&self) -> $inner {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}-{}", $display_prefix, self.0)
            }
        }

        impl From<$inner> for $name {
            fn from(value: $inner) -> Self {
                Self(value)
            }
        }

        impl From<$name> for $inner {
            fn from(id: $name) -> Self {
                id.0
            }
        }
    };
    ($name:ident, $inner:ty) => {
        #[doc = concat!("Identifier for ", stringify!($name))]
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            serde::Serialize,
            serde::Deserialize,
        )]
        pub struct $name(pub $inner);

        impl $name {
            #[doc = concat!("Create a new ", stringify!($name))]
            pub fn new(value: $inner) -> Self {
                Self(value)
            }

            #[doc = concat!("Get the inner value")]
            pub fn value(&self) -> $inner {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<$inner> for $name {
            fn from(value: $inner) -> Self {
                Self(value)
            }
        }

        impl From<$name> for $inner {
            fn from(id: $name) -> Self {
                id.0
            }
        }
    };
}
