//! Journal operation handler patterns removed - migrated to effect system
//!
//! **MIGRATION NOTE**: Handler traits and implementations removed in favor of unified effect system.
//! The StateHandler and JournalHandler patterns have been replaced by JournalEffects trait.
//!
//! This file previously contained:
//! - StateHandler: 86 lines removed - replaced by JournalEffects implementation in aura-effects
//! - JournalHandler trait: migrated to effect trait in aura-core
//! - NoOpHandler: 15 lines removed - replaced by MockJournalHandler in aura-effects
//!
//! Essential state manipulation logic has been preserved and moved to the effect system where
//! it follows Aura's Layer 3 principles (stateless, single-party, context-free operations).
//!
//! TODO: Complete migration by implementing JournalHandler in aura-effects using JournalEffects trait
