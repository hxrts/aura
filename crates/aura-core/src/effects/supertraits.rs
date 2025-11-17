//! Sealed supertraits for common effect combinations
//!
//! This module provides sealed supertraits that group commonly used effect trait
//! combinations to improve type signature readability and maintainability.

use super::{
    ConsoleEffects, CryptoEffects, JournalEffects, NetworkEffects, RandomEffects, StorageEffects,
    TimeEffects,
};

/// Sealed supertrait for FROST threshold signing operations
///
/// Combines effects needed for cryptographic threshold signing protocols:
/// network communication, cryptographic operations, time tracking, and logging.
pub trait SigningEffects: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects {
    // Sealed trait - users cannot implement this directly
}

/// Automatic implementation for types that satisfy the required bounds
impl<T> SigningEffects for T where T: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects {}

/// Sealed supertrait for CRDT synchronization operations
///
/// Combines effects needed for CRDT state management and synchronization:
/// storage access, journal operations, and logging.
pub trait CrdtEffects: StorageEffects + JournalEffects + ConsoleEffects {
    // Sealed trait - users cannot implement this directly
}

/// Automatic implementation for types that satisfy the required bounds
impl<T> CrdtEffects for T where T: StorageEffects + JournalEffects + ConsoleEffects {}

/// Sealed supertrait for choreography coordination
///
/// Combines effects needed for multi-party protocol coordination:
/// network communication, cryptographic operations, randomness, time tracking,
/// storage access, journal operations, and logging.
pub trait ChoreographyEffects:
    NetworkEffects
    + CryptoEffects
    + RandomEffects
    + TimeEffects
    + StorageEffects
    + JournalEffects
    + ConsoleEffects
{
    // Sealed trait - users cannot implement this directly
}

/// Automatic implementation for types that satisfy the required bounds
impl<T> ChoreographyEffects for T where
    T: NetworkEffects
        + CryptoEffects
        + RandomEffects
        + TimeEffects
        + StorageEffects
        + JournalEffects
        + ConsoleEffects
{
}

/// Sealed supertrait for anti-entropy synchronization
///
/// Combines effects needed for anti-entropy protocols:
/// network communication, cryptographic operations, randomness, and logging.
pub trait AntiEntropyEffects:
    NetworkEffects + CryptoEffects + RandomEffects + ConsoleEffects
{
    // Sealed trait - users cannot implement this directly
}

/// Automatic implementation for types that satisfy the required bounds
impl<T> AntiEntropyEffects for T where
    T: NetworkEffects + CryptoEffects + RandomEffects + ConsoleEffects
{
}

/// Sealed supertrait for tree operations
///
/// Combines effects needed for tree coordination protocols:
/// network communication, cryptographic operations, time tracking,
/// storage access, and logging.
pub trait TreeEffects:
    NetworkEffects + CryptoEffects + TimeEffects + StorageEffects + ConsoleEffects
{
    // Sealed trait - users cannot implement this directly
}

/// Automatic implementation for types that satisfy the required bounds
impl<T> TreeEffects for T where
    T: NetworkEffects + CryptoEffects + TimeEffects + StorageEffects + ConsoleEffects
{
}

/// Sealed supertrait for minimal effect operations
///
/// Combines basic effects needed for simple operations:
/// cryptographic operations, randomness, and logging.
pub trait MinimalEffects: CryptoEffects + RandomEffects + ConsoleEffects {
    // Sealed trait - users cannot implement this directly
}

/// Automatic implementation for types that satisfy the required bounds
impl<T> MinimalEffects for T where T: CryptoEffects + RandomEffects + ConsoleEffects {}

/// Sealed supertrait for snapshot coordination
///
/// Combines effects needed for snapshot coordination protocols:
/// network communication, cryptographic operations, time tracking,
/// storage access, journal operations, and logging.
pub trait SnapshotEffects:
    NetworkEffects + CryptoEffects + TimeEffects + StorageEffects + JournalEffects + ConsoleEffects
{
    // Sealed trait - users cannot implement this directly
}

/// Automatic implementation for types that satisfy the required bounds
impl<T> SnapshotEffects for T where
    T: NetworkEffects
        + CryptoEffects
        + TimeEffects
        + StorageEffects
        + JournalEffects
        + ConsoleEffects
{
}
