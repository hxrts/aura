//! Sealed supertraits for common effect combinations
//!
//! This module provides sealed supertraits that group commonly used effect trait
//! combinations to improve type signature readability and maintainability.
//!
//! # Module Classification
//!
//! - **Category**: Composite Effect Traits
//! - **Purpose**: Convenience wrappers combining multiple effect traits
//!
//! This module provides sealed supertraits (e.g., `SigningEffects`, `ChoreographyEffects`)
//! that combine commonly-used effect trait combinations. These are blanket implementations
//! that simplify type signatures. No handlers needed - these are pure trait composition.

use super::{
    ConsoleEffects, CryptoEffects, JournalEffects, NetworkEffects, PhysicalTimeEffects,
    RandomEffects, StorageEffects,
};

macro_rules! composite_effect {
    ($(#[$meta:meta])* $name:ident: $($bounds:path),+ $(,)?) => {
        $(#[$meta])*
        pub trait $name: $( $bounds + )* {}

        impl<T> $name for T where T: $( $bounds + )* {}
    };
}

composite_effect!(
    /// Sealed supertrait for FROST threshold signing operations
    ///
    /// Combines effects needed for cryptographic threshold signing protocols:
    /// network communication, cryptographic operations, time tracking, and logging.
    SigningEffects: NetworkEffects, CryptoEffects, PhysicalTimeEffects, ConsoleEffects
);

composite_effect!(
    /// Sealed supertrait for CRDT synchronization operations
    ///
    /// Combines effects needed for CRDT state management and synchronization:
    /// storage access, journal operations, and logging.
    CrdtEffects: StorageEffects, JournalEffects, ConsoleEffects
);

composite_effect!(
    /// Sealed supertrait for choreography coordination
    ///
    /// Combines effects needed for multi-party protocol coordination:
    /// network communication, cryptographic operations, randomness, time tracking,
    /// storage access, journal operations, and logging.
    ChoreographyEffects:
        NetworkEffects,
        CryptoEffects,
        RandomEffects,
        PhysicalTimeEffects,
        StorageEffects,
        JournalEffects,
        ConsoleEffects
);

composite_effect!(
    /// Sealed supertrait for anti-entropy synchronization
    ///
    /// Combines effects needed for anti-entropy protocols:
    /// network communication, cryptographic operations, randomness, and logging.
    AntiEntropyEffects: NetworkEffects, CryptoEffects, RandomEffects, ConsoleEffects
);

composite_effect!(
    /// Sealed supertrait for tree operations
    ///
    /// Combines effects needed for tree coordination protocols:
    /// network communication, cryptographic operations, time tracking,
    /// storage access, and logging.
    TreeEffects:
        NetworkEffects,
        CryptoEffects,
        PhysicalTimeEffects,
        StorageEffects,
        ConsoleEffects
);

composite_effect!(
    /// Sealed supertrait for minimal effect operations
    ///
    /// Combines basic effects needed for simple operations:
    /// cryptographic operations, randomness, and logging.
    MinimalEffects: CryptoEffects, RandomEffects, ConsoleEffects
);

composite_effect!(
    /// Sealed supertrait for snapshot coordination
    ///
    /// Combines effects needed for snapshot coordination protocols:
    /// network communication, cryptographic operations, time tracking,
    /// storage access, journal operations, and logging.
    SnapshotEffects:
        NetworkEffects,
        CryptoEffects,
        PhysicalTimeEffects,
        StorageEffects,
        JournalEffects,
        ConsoleEffects
);
