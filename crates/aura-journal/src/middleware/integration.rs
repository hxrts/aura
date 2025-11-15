//! Middleware integration patterns removed - migrated to effect system
//!
//! **MIGRATION NOTE**: All middleware composition, builders, and effect processors
//! have been removed in favor of Aura's unified effect system. Use JournalEffects
//! trait in the effect system instead.
//!
//! This file previously contained:
//! - EffectSystemHandler: 100 lines removed - replaced by JournalEffects
//! - JournalHandlerBuilder: 120 lines removed - replaced by effect dependency injection  
//! - DefaultEffectProcessor: 87 lines removed - replaced by JournalHandler in aura-effects
//! - Middleware stack composition: 68 lines removed - replaced by explicit effect composition
//!
//! TODO: Complete migration by implementing JournalEffects trait and handlers in aura-effects
