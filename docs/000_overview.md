# Aura Project Overview

## 1. High-Level Summary

Aura is a threshold identity and encrypted storage platform built on the principle of "relational security". It aims to solve the problem of single points of failure in digital identity by distributing trust among a user's social network and personal devices.

The core ideas are:
- **Threshold Cryptography**: Security is not dependent on a single key or device. Multiple parties (guardians) or devices must cooperate to perform sensitive actions.
- **Social Recovery**: Friends and family can act as "guardians" to help recover an identity, eliminating reliance on centralized providers.
- **Data Replication**: Data is replicated across social relationships, providing natural redundancy and availability.
- **Choreographic Programming**: The complex, distributed protocols are defined from a global viewpoint and then projected onto individual devices, ensuring protocol correctness and preventing deadlocks at compile-time.
- **Formal Verification**: The use of the Quint specification language (`aura-quint-api`) indicates a strong emphasis on proving the correctness of the distributed protocols.

## 2. Workspace Structure

The Aura project is a Rust workspace composed of many specialized crates. The main directories are:

- `crates/`: Contains all the individual Rust crates that make up the Aura platform.
- `docs/`: Project documentation, architecture decision records (ADRs), and specifications.
- `scenarios/`: Configuration files (`.toml`) that define various testing and simulation scenarios for the `aura-simulator`.
- `specs/`: Formal specifications, particularly those written in Quint.
- `ext/`: External or third-party dependencies that are included directly in the source tree.

## 3. Crate Breakdown

The project is divided into several logical groups of crates:

### Core Aura Crates

These crates form the backbone of the Aura platform's logic.

- **`aura-agent`**: The main entry point for Aura users. It manages an agent's lifecycle, services, and interactions with the underlying protocol, storage, and transport layers.
- **`aura-protocol`**: Coordinates the distributed cryptographic protocols like Distributed Key Derivation (DKD), resharing, and recovery. This is the heart of the system's security logic.
- **`aura-journal`**: Implements an event-sourced, CRDT-based log for each agent. This journal is the source of truth for an agent's state, and its CRDT nature ensures eventual consistency across devices.
- **`aura-crypto`**: A dedicated crate for common cryptographic primitives, providing a consistent and secure interface for hashing, encryption (sealing), and key derivation.
- **`aura-store`**: An encrypted blob store that orchestrates the storage of encrypted data chunks, likely interacting with the transport layer to replicate data across the network.
- **`aura-transport`**: A pluggable transport layer responsible for peer-to-peer communication. It appears to handle secure channel establishment (using the Noise protocol via `snow`) and message routing.
- **`aura-choreography`**: Contains the concrete implementations of the choreographic protocols using the `rumpsteak` framework. This crate translates the high-level protocol descriptions into executable state machines for each participant.
- **`aura-authentication`**: Handles authentication-related logic and data structures.
- **`aura-messages`**: Defines the wire formats for all messages exchanged between Aura agents, ensuring all parts of the system speak the same language.
- **`aura-types`**: A foundational crate that defines the core data structures, identifiers (`NodeId`, `AgentId`, etc.), and error types used across the entire workspace.
- **`aura-macros`**: Provides procedural macros to reduce boilerplate, likely for middleware patterns or other common code structures.

### Application & Simulation Crates

These crates are focused on building user-facing applications, developer tools, and simulations.

- **`aura-simulator`**: A powerful tool for running and testing complex, multi-agent scenarios defined in `.toml` files. It simulates the network and agent interactions, providing a controlled environment for development and verification.
- **`aura-cli`**: A command-line interface for interacting with the Aura network, likely used for operator tasks, smoke tests, and administrative functions.
- **`app-console`**: A web-based developer console, built with Leptos (a Rust WASM framework), for visualizing and interacting with Aura simulations or live agents.
- **`app-sim-server`**: The backend server that drives the simulation, communicating with the `app-console` frontend over WebSockets.
- **`app-sim-client`**: A WASM client that connects the web-based console to the simulation server.
- **`app-live-client`**: A WASM client for connecting the web console to a live, running Aura agent (as opposed to a simulated one).
- **`app-analysis-client`**: A WASM client focused on analyzing trace data from simulations, likely for performance profiling and formal verification checks against Quint specifications.
- **`app-wasm`**: A shared library for common WASM-related utilities used by the other `app-*` crates.
- **`app-console-types`**: Shared data types for communication between the simulation/live backend and the console frontend.

### Verification & Testing Crates

- **`aura-quint-api`**: Provides a native Rust interface to the Quint formal verification tool. This allows the simulation and analysis tools to check if the execution traces conform to the formal specification.
- **`aura-test-utils`**: A crate containing shared utilities and helpers to facilitate testing across the workspace.

## 4. Key Technologies and Concepts

- **Rust**: The entire project is built in Rust, leveraging its safety, performance, and concurrency features.
- **Tokio**: The asynchronous runtime used for all I/O-bound and concurrent operations.
- **FROST**: Implements FROST (Flexible Round-Optimized Schnorr Threshold) signatures for threshold cryptography.
- **Automerge**: A CRDT library used in `aura-journal` to manage the distributed, eventually consistent state of an agent.
- **Rumpsteak**: A framework for choreographic programming in Rust, used to implement the distributed protocols.
- **Quint**: A formal specification language used to verify the correctness of the distributed protocols.
- **WASM (WebAssembly)**: Used extensively to build the web-based developer console and clients, allowing Rust code to run directly in the browser.
- **Leptos**: A modern Rust framework for building reactive web applications, used for the `app-console`.
