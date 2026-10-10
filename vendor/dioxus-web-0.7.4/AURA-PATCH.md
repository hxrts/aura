# Renderer source provenance

Base: the published crates.io dioxus-web 0.7.4 archive.
Archive SHA256: eac92ef863bc5333440021e8ec3e538a39598c9c960daeaab66ab10ba940b5e0.
The published archive records VCS commit 1eb00b5e0080ab4bd6a11ddd0a01c97f28493e04,
path packages/web, dirty=true; the released archive, rather than a presumed clean
Git tree, is the source of truth. Original MIT and Apache-2.0 licenses are copied
from that exact repository commit because the archive omits those files.

The source patch adds optional generic synchronous renderer batch observations
in src/cfg.rs and src/lib.rs. It contains no Aura types or workflow behavior.
Hydration and external asset mutation explicitly withhold this commit contract.
Cargo and generated Cargo.nix must resolve this same checked-in source.

The normalized package manifest has an empty local workspace boundary so direct
Cargo tooling does not discover an enclosing Aura checkout from nested worktrees.
This packaging change does not alter the upstream dependency or feature contract.
Its standalone lockfile is pruned from Aura's resolved lockfile; all registry
package versions and checksums match that graph. Normal staged-source checks use
the checkout's explicit budgeted target directory, including this standalone owner.

Remove this patch and vendor when an audited released renderer exposes the same
commit/invalidation contract, updating Cargo.lock and regenerating Cargo.nix.
