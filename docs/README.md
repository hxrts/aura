# Aura Documentation (Simplified Architecture)

Welcome to the refreshed documentation set for the streamlined Aura platform.
This version focuses on getting a lean-but-secure identity and storage stack in
place before layering on optional transports or advanced policy features.

## Reading Order

1. `010_motivation.md` – Why Aura exists and what the simplified MVP must deliver.
2. `020_architecture.md` – Layered system overview tying identity, CRDTs, and storage together.
3. `030_identity_spec.md` – Dual-root keys, deterministic key derivation, presence tickets, and device lifecycle.
4. `040_storage_mvp.md` – Minimal chunk store built on a single transport (iroh/HTTPS), inline metadata, and proof-of-storage.
5. `050_recovery_and_policy.md` – Guardian-based recovery with a single policy path.
6. `060_phased_roadmap.md` – Implementation milestones and gating criteria.
7. `101_example_app.md` – A lightweight end-to-end walk-through for an example client once the core is ready.

## Scope of This Doc Set

- Captures only the functionality required for the **Phase 0/1 MVP**.
- Anything marked “Future” is intentionally out of scope for the initial build.

## Feedback

Open a GitHub issue or start a discussion in `#aura-docs` if you spot gaps or unclear sections. The goal is for this doc set to stay tightly aligned with the code we are delivering in Phase 0/1.***
