# Aura

Threshold identity and encrypted storage platform built on relational security principles.

## Overview

Current identity systems force an impossible choice: trust a single device that can be lost or compromised, or trust a corporation that can lock you out. Aura eliminates this choice by building on the trust you already have. Friends, family, and your own devices work together through threshold cryptography and social recovery to create resilient digital identity.

Aura combines threshold signatures with social networks to create a new model for digital ownership. Your identity is distributed across trusted guardians who can help you recover access. Your data is replicated across social relationships that provide natural redundancy. No single point of failure can lock you out or compromise your security.

## Architecture

Aura uses choreographic programming with session types to coordinate distributed protocols across peer devices. Threshold cryptography eliminates single points of failure while social networks provide natural trust relationships for recovery and storage replication.

The system coordinates multi-device threshold protocols through choreographic programming. Session types provide compile-time safety for distributed state machines while runtime witnesses verify global conditions like quorum thresholds. A unified CRDT ledger maintains eventual consistency across all devices without requiring centralized coordination.

### Session Types & Choreographic Programming:

These complementary techniques provide both local and global protocol safety. Choreographic programming describes protocols from a global viewpoint across all participants, automatically generating deadlock-free coordination patterns and local projections for each device. Session types then enforce local protocol correctness through typestate, ensuring individual devices follow their projected protocol steps correctly at compile-time (e.g., preventing message sends before prerequisite states are reached). Runtime witnesses verify distributed invariants that span multiple participants, such as threshold quorum requirements or epoch synchronization.
