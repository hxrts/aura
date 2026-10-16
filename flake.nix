{
  description = "Aura";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    nixpkgs-cargo-deny.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # Same pinned crane revision already present in the lock through Charon.
    crane.url = "github:ipetkov/crane/e316f19ee058e6db50075115783be57ac549c389";
    crate2nix = {
      url = "github:timewave-computer/crate2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    toolkit = {
      url = "github:hxrts/toolkit";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-overlay.follows = "rust-overlay";
      inputs.flake-utils.follows = "flake-utils";
    };
    # Aeneas: Rust-to-Lean/Coq/F*/HOL4 verification toolchain
    aeneas = {
      url = "github:AeneasVerif/aeneas";
      # Don't follow nixpkgs - Aeneas has specific OCaml/Coq version requirements
    };
    # Patchbay: Network simulation for holepunch testing. Pinned to the same
    # upstream revision as crates/aura-harness/Cargo.toml and
    # scripts/ci/holepunch.sh audit (the patchbay-v0.7.0 tag).
    patchbay = {
      url = "github:n0-computer/patchbay/5cc60db1eb190360dfcd64eaf2f829bce4072551";
      flake = false; # Use as source, not as flake (no packages exported)
    };
  };

  outputs =
    { self
    , nixpkgs
    , nixpkgs-cargo-deny
    , flake-utils
    , rust-overlay
    , crane
    , crate2nix
    , toolkit
    , aeneas
    , patchbay
    ,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        cargoDenyPkgs = import nixpkgs-cargo-deny {
          inherit system;
        };
        cargoDeny = cargoDenyPkgs.cargo-deny;
        binaryen123Archive = {
          "aarch64-darwin" = { platform = "arm64-macos"; sha256 = "74428be348c1a09863e7b642a1fa948cabf8ec9561052233d8288e941951725b"; };
          "x86_64-darwin" = { platform = "x86_64-macos"; sha256 = "cc18b14d2b673d9c66bf54f31ff2b0ceb23ba5132455b893965ae2792f9e00dd"; };
          "aarch64-linux" = { platform = "aarch64-linux"; sha256 = "4b6bd61ba6cd3b18c993b4657d93426c782f9b91b74be0d38018cd8be1319376"; };
          "x86_64-linux" = { platform = "x86_64-linux"; sha256 = "e959f2170af4c20c552e9de3a0253704d6a9d2766e8fdb88e4d6ac4bae9388fe"; };
        }.${system};
        binaryen123Src = pkgs.fetchurl {
          url = "https://github.com/WebAssembly/binaryen/releases/download/version_123/binaryen-version_123-${binaryen123Archive.platform}.tar.gz";
          inherit (binaryen123Archive) sha256;
        };
        # Keep wasm-opt and its sibling dynamic library in one immutable output.
        binaryen123 = pkgs.runCommand "binaryen-version_123" {
          nativeBuildInputs = [ pkgs.gnutar pkgs.gzip ];
        } ''
          mkdir -p "$out"
          tar -xzf ${binaryen123Src} -C "$out" --strip-components=1
          test -x "$out/bin/wasm-opt"
          test -d "$out/lib"
        '';
        # Lifecycle custody needs the portable binary, not optional Ruby-generated manuals.
        lifecycleFlock = pkgs.flock.overrideAttrs (old: {
          nativeBuildInputs = builtins.filter (input: input != pkgs.ronn) old.nativeBuildInputs;
          buildFlags = [ "flock" ];
          installTargets = [ "install-binPROGRAMS" ];
        });
        toolkitSupport = toolkit.lib.${system}.consumerShellSupport;
        toolkitCommands = with toolkit.packages.${system}; [
          toolkit-clippy
          toolkit-dylint
          toolkit-dylint-link
          toolkit-install-dylint
        ];

        # Pin to specific Rust version for reproducible CI behavior
        rustToolchain = pkgs.rust-bin.stable."1.93.0".default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
            "clippy"
          ];
          targets = [ "wasm32-unknown-unknown" ];
        };

        # Inert warm-tree producer: Cargo in the dev shell still uses its
        # existing dispatcher and registry until a separate consumer packet.
        craneVendorLib = crane.mkLib pkgs;
        craneLib = craneVendorLib.overrideToolchain (_: rustToolchain);
        warmSrc = craneLib.cleanCargoSource ./.;
        warmDummySrc = craneLib.mkDummySrc { src = warmSrc; cargoLock = ./Cargo.lock; };
        warmVendor = craneVendorLib.vendorCargoDeps { cargoLock = ./Cargo.lock; };
        warmVendorPath = warmVendor.outPath;
        warmCC = if pkgs.stdenv.isDarwin then "clang" else "gcc";
        warmWrapper = "${pkgs.sccache}/bin/sccache";
        warmDebugArgs = {
          dummySrc = warmDummySrc;
          cargoLock = ./Cargo.lock;
          cargoVendorDir = warmVendor;
          pname = "aura-core-debug-warm";
          version = "0.1.0";
          cargoExtraArgs = "--locked -p hxrts-aura-core";
          CARGO_PROFILE = "dev";
          CC = warmCC;
          CARGO_INCREMENTAL = "0";
          RUSTC_WRAPPER = warmWrapper;
          nativeBuildInputs = [ pkgs.sccache pkgs.jq ];
          preBuild = ''
            # A sandbox compiler must never reuse the developer-owned daemon:
            # that process cannot write the sandbox's target directory.
            export SCCACHE_SERVER_UDS="$TMPDIR/aura-sccache.sock"
            export SCCACHE_DIR="$TMPDIR/aura-sccache"
            mkdir -m 700 -p "$SCCACHE_DIR"
            trap 'sccache --stop-server >/dev/null 2>&1 || true' EXIT
          '';
          postBuild = ''
            if [ -f target/.rustc_info.json ]; then
              jq -S . target/.rustc_info.json > target/.rustc_info.json.canonical
              mv target/.rustc_info.json.canonical target/.rustc_info.json
            fi
          '';
        };
        warmDebug = craneLib.buildDepsOnly warmDebugArgs;
        warmDebugKeyInputs = {
          platform = system;
          devShellPath = self.devShells.${system}.default.drvPath;
          toolchainPath = rustToolchain.outPath;
          vendorPath = warmVendorPath;
          lockHash = builtins.hashFile "sha256" ./Cargo.lock;
          dummyManifestPath = "${warmDummySrc}/Cargo.toml";
          dummyConfigPath = "${warmVendorPath}/config.toml";
          package = "hxrts-aura-core";
          profile = "dev";
          features = [ "default" ];
          target = pkgs.stdenv.hostPlatform.config;
          rustflags = "";
          cc = warmCC;
          wrapper = warmWrapper;
          incremental = false;
          buildEnv = {
            CARGO_PROFILE = "dev";
            CC = warmCC;
            CARGO_INCREMENTAL = "0";
            RUSTC_WRAPPER = warmWrapper;
            SCCACHE_SERVER_UDS_POLICY = "sandbox-local";
          };
        };

        auraCargo = pkgs.writeShellScriptBin "cargo" ''
          export AURA_NIX_CARGO_BIN="${rustToolchain}/bin/cargo"
          export AURA_NIX_CLIPPY_BIN="${rustToolchain}/bin/cargo-clippy"
          exec ${pkgs.bash}/bin/bash ${./scripts/dev/cargo-in-nix.sh} "$@"
        '';

        rustToolchainNightly = pkgs.rust-bin.nightly.latest.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
            "rustc-dev"
            "llvm-tools-preview"
            "rustfmt"
          ];
          targets = [ "wasm32-unknown-unknown" ];
        };

        # Apalache package from pre-built release
        apalache = pkgs.stdenv.mkDerivation rec {
          pname = "apalache";
          version = "0.45.4";

          src = pkgs.fetchurl {
            url = "https://github.com/apalache-mc/apalache/releases/download/v${version}/apalache-${version}.tgz";
            sha256 = "sha256-neJbVEIAqOrfuP/4DR2MEhLJdnthxNx4KVoCyoGVdQ4=";
          };

          nativeBuildInputs = with pkgs; [ makeWrapper ];
          buildInputs = with pkgs; [ jre ];

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin

            # Copy the entire apalache directory
            cp -r . $out/lib-apalache

            # Make the apalache-mc script executable
            chmod +x $out/lib-apalache/bin/apalache-mc

            # Create wrapper script
            makeWrapper $out/lib-apalache/bin/apalache-mc $out/bin/apalache-mc \
              --set JAVA_HOME ${pkgs.jre} \
              --prefix PATH : ${pkgs.jre}/bin

            runHook postInstall
          '';

          meta = with pkgs.lib; {
            description = "Symbolic model checker for TLA+ specifications";
            homepage = "https://apalache-mc.github.io/apalache/";
            license = licenses.asl20;
            platforms = platforms.unix;
          };
        };

        # patchbay: unified upstream CLI whose `vm` subcommand drives patchbay
        # network simulations in a QEMU guest (`patchbay vm run`), with its
        # default `serve` (devtools UI) and `upload` features.
        # Only available on Linux (requires QEMU and network namespace support)
        # The patchbay devtools UI is built by patchbay-server's build script
        # through npm; prefetch its lockfile so that runs offline in the sandbox.
        patchbayUiDeps = pkgs.fetchNpmDeps {
          name = "patchbay-ui-npm-deps";
          src = "${patchbay}/ui";
          hash = "sha256-Czu0fqb1Kls7sdm1fwEzNIJsAkSXsUK4lmC9iUJN/E4=";
        };
        patchbayCli = pkgs.rustPlatform.buildRustPackage {
            pname = "patchbay";
            version = "0.7.0";
            src = patchbay;
            cargoLock = {
              lockFile = "${patchbay}/Cargo.lock";
            };
            cargoBuildFlags = [ "-p" "patchbay-cli" ];
            nativeBuildInputs = with pkgs; [ pkg-config nodejs npmHooks.npmConfigHook ];
            npmDeps = patchbayUiDeps;
            npmRoot = "ui";
            buildInputs = with pkgs; [ openssl ];
            # Skip tests as they require network capabilities
            doCheck = false;
            meta = with pkgs.lib; {
              description = "Patchbay CLI with the QEMU VM backend for network simulations";
              license = with licenses; [ mit asl20 ];
              platforms = platforms.linux;
            };
          };

        # Import generated Cargo.nix with CC crate fix and other overrides
        cargoNix = import ./Cargo.nix {
          inherit pkgs;
          buildRustCrateForPkgs = pkgs: pkgs.buildRustCrate;
          # Apply CC crate fix for macOS builds and other common overrides
          defaultCrateOverrides = pkgs.defaultCrateOverrides // {
            # Override for cc crate - fix Apple target detection on macOS
            cc =
              attrs:
              pkgs.lib.optionalAttrs pkgs.stdenv.isDarwin
                {
                  # Fix CC crate Apple target detection: it expects "darwin" but Nix reports "macos"
                  preBuild = ''
                    export CARGO_CFG_TARGET_OS="darwin"
                  '';
                }
              // attrs;

            # Override for ring (crypto library)
            ring = attrs: {
              nativeBuildInputs = [ pkgs.perl ];
              # Ensure consistent deployment target to avoid CC crate issues
              MACOSX_DEPLOYMENT_TARGET = "11.0";
              # Set explicit Rust target for CC crate compatibility
              TARGET_OS = "darwin";
              CARGO_CFG_TARGET_OS = "darwin";
              # Override Rust target detection
              preBuild = ''
                export TARGET_OS="darwin"
                export CARGO_CFG_TARGET_OS="darwin"
                # Ensure cargo sees darwin target
                export RUST_TARGET_PATH=${pkgs.stdenv.targetPlatform.config}
              '';
            };
          };
        };
      in
      {
        # Shared pure definition only. Vendor/deps outputs and rehydration are
        # deliberately absent until the dependency tree is proven.
        lib.buildCacheKey = import ./nix/build-cache-key.nix;
        lib.debugCacheKeyInputs = warmDebugKeyInputs;
        lib.debugCacheKey = (import ./nix/build-cache-key.nix) warmDebugKeyInputs;

        packages = {
          binaryen123 = binaryen123;
          vendor-deps = warmVendor;
          deps-debug = warmDebug;
          # Main CLI application (terminal interface)
          default = cargoNix.workspaceMembers.aura-terminal.build;
          aura-terminal = cargoNix.workspaceMembers.aura-terminal.build;

          # Terminal with development features (demo mode, simulator integration)
          aura-terminal-dev = cargoNix.workspaceMembers.aura-terminal.build.override {
            features = [ "development" ];
          };

          # Core applications
          aura-agent = cargoNix.workspaceMembers.hxrts-aura-agent.build;
          aura-simulator = cargoNix.workspaceMembers.aura-simulator.build;

          # Development tools
          regenerate-cargo-nix = pkgs.writeScriptBin "regenerate-cargo-nix" ''
            #!${pkgs.bash}/bin/bash
            echo "Regenerating Cargo.nix with crate2nix..."
            ${crate2nix.packages.${system}.default}/bin/crate2nix generate
            echo "Cargo.nix regenerated successfully!"
          '';
        } // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          # Upstream patchbay CLI: `patchbay serve` inspects a harness lab run dir.
          patchbay = patchbayCli;
        };

        checks = {
          # Run tests for key crates
          # Note: aura-protocol-tests disabled due to circular dev-dependency with aura-testkit
          # Tests still run via `cargo test` - hermetic checks are optional
          aura-core-tests = cargoNix.workspaceMembers.aura-core.build.override {
            runTests = true;
          };
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Rust toolchain
            auraCargo
            rustToolchain
            cargo-watch
            cargo-edit
            cargo-audit
            cargoDeny
            cargo-sweep
            # Shared compiler cache for every Aura worktree (opt out: AURA_NO_SCCACHE=1)
            sccache
            # Local test runner (not used by gates; their parsers read libtest output)
            cargo-nextest

            # WASM tools
            wasm-pack
            wasm-bindgen-cli

            # Build tools
            pkg-config
            openssl

            # Task runner
            just

            # Documentation
            mdbook
            mdbook-mermaid
            mdbook-katex

            # Development tools
            git
            jq
            ripgrep
            rustup

            # POSIX tools (for Justfile scripts)
            coreutils
            findutils
            gawk
            gnused

            # Formal verification and protocol modeling
            quint
            apalache
            tlaplus # TLA+ tools from nixpkgs
            nodejs_20
            jre # For ANTLR4TS and Apalache
            lean4 # Lean 4 for kernel verification
            aeneas.packages.${system}.aeneas # Rust-to-Lean translator

            # Documentation tools
            markdown-link-check
            lifecycleFlock # portable inherited-FD kernel custody for LAN lifecycle

            # Nix tools and formatting
            nixpkgs-fmt
            crate2nix.packages.${system}.default
          ]
          ++ toolkitSupport.packages
          ++ toolkitCommands
          ++ toolkitSupport.buildInputs
          # Linux-only: patchbay network simulation dependencies
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            chromium
            chromedriver
            iproute2 # tc command for traffic control
            nftables # nft command for NAT rules
            iptables # iptables-legacy for some nftables compat
            conntrack-tools # conntrack, used by patchbay to flush NAT state
            tcpdump # per-device packet capture in harness network labs
          ];

          shellHook = ''
            ${toolkitSupport.shellHook}
            ${pkgs.lib.optionalString pkgs.stdenv.isLinux ''
              export AURA_TEST_CHROMIUM="${pkgs.chromium}/bin/chromium"
              export AURA_TEST_CHROMEDRIVER="${pkgs.chromedriver}/bin/chromedriver"
            ''}
            if [ -d "$PWD/../toolkit/xtask" ]; then
              export TOOLKIT_ROOT="$PWD/../toolkit"
            fi
            [[ -r "$HOME/.local/state/secrets/cargo-registry-token" ]] && export CARGO_REGISTRY_TOKEN="$(cat "$HOME/.local/state/secrets/cargo-registry-token")"
            export AURA_SUPPRESS_NIX_WELCOME=1
            # No CARGO_TARGET_DIR export: each checkout uses its own target/, and
            # sccache hashes CARGO_* variables, so a per-checkout value would
            # defeat cache sharing between worktrees.
            export AURA_WORKSPACE_ROOT="$PWD"
            export AURA_CARGO_DENY_BIN="${cargoDeny}/bin/cargo-deny"
            # One sccache store shared by every Aura worktree and checkout.
            # Non-incremental builds (the default) are cached; with
            # CARGO_INCREMENTAL=1 sccache passes compilations through.
            # AURA_NO_SCCACHE=1 opts out, at shell entry or per cargo command.
            if [ "''${AURA_NO_SCCACHE:-0}" != 1 ]; then
              export RUSTC_WRAPPER=sccache
              export SCCACHE_DIR="''${SCCACHE_DIR:-$HOME/.cache/aura-sccache}"
              export SCCACHE_CACHE_SIZE="''${SCCACHE_CACHE_SIZE:-10G}"
            fi
            if [ -z "$AURA_SUPPRESS_NIX_WELCOME" ]; then
              echo "Aura Development Environment"
              echo "============================"
              echo ""
              echo "Rust version: $(rustc --version)"
              echo "Cargo version: $(cargo --version)"
              echo "Quint version: $(quint --version 2>/dev/null || echo 'available')"
              echo "Apalache version: $(apalache-mc version 2>/dev/null | head -1 || echo 'available')"
              echo "TLA+ tools: $(tlc2 2>&1 | head -1 | grep -o 'Version.*' || echo 'available')"
              echo "Node.js version: $(node --version)"
              echo "Lean version: $(lean --version 2>/dev/null || echo 'available')"
              echo "Aeneas version: $(aeneas --version 2>/dev/null || echo 'available')"
              echo ""
              echo "Available commands:"
              echo "  just --list          Show all available tasks"
              echo "  just build           Build all crates"
              echo "  just test            Run all tests"
              echo "  just check           Check workspace (cargo check)"
              echo "  just quint-parse     Parse Quint files to JSON"
              echo "  just serve-console   Serve console with hot reload (crates/console)"
              echo "  quint --help         Formal verification with Quint"
              echo "  apalache-mc --help   Model checking with Apalache"
              echo "  lean --help          Kernel verification with Lean 4"
              echo "  aeneas --help        Rust-to-Lean translation"
              echo "  crate2nix --help     Generate hermetic Nix builds"
              echo ""
              echo "Hermetic builds:"
              echo "  nix build            Build with crate2nix (hermetic)"
              echo "  nix build .#aura-terminal Build specific package"
              echo "  nix run              Run aura CLI hermetically"
              echo "  nix flake check      Run hermetic tests"
              echo ""
            fi

            export RUST_BACKTRACE=1
            export RUST_LOG=info
            export MACOSX_DEPLOYMENT_TARGET=11.0

            # Custom PS1 prompt matching user's style: user@host path (branch) [nix] >
            # Colors: darkgrey=user@host, purple=path, greenish-yellow=branch, orange=[nix] >
            # Note: \001 and \002 mark non-printing chars in command substitutions (like \[ \] in literal PS1)
            __git_branch() {
              local branch
              branch=$(git branch 2>/dev/null | grep '^*' | colrm 1 2)
              if [ -n "$branch" ]; then
                printf ' \001\033[38;5;148m\002(%s)\001\033[0m\002' "$branch"
              fi
            }
            PS1='\[\033[90m\]\u@\h\[\033[0m\] \[\033[35m\]\w\[\033[0m\]$(__git_branch)\[\033[38;5;208m\][nix] >\[\033[0m\] '

            # Set AURA_PATH to the project directory for development.
            # This ensures TUI data files (.aura/, .aura-demo/) are created in the
            # project root rather than ~/.aura, making them visible and easy to
            # inspect/delete during development. In production, AURA_PATH defaults
            # to ~ (home directory).
            export AURA_PATH="$PWD"
          '';
        };

        # Minimal CI shell - excludes heavy verification tools (Aeneas, Lean) that have
        # flaky OCaml dependencies. Use for CI jobs that only need Rust + basic tools.
        devShells.ci = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Rust toolchain
            auraCargo
            rustToolchain
            rustToolchainNightly
            # Cargo.nix drift check
            crate2nix.packages.${system}.default

            # Build tools
            pkg-config
            openssl
            cargoDeny
            cargo-sweep

            # Task runner
            just

            # Development tools
            git
            jq
            ripgrep
            rustup

            # POSIX tools (for Justfile scripts)
            coreutils
            findutils
            gawk
            gnused

            # WASM tools (for agent-wasm CI)
            wasm-pack
            wasm-bindgen-cli

            # Browser/build tooling used by harness and web checks
            nodejs_20

            # Documentation link checking without the verification toolchain
            markdown-link-check
            lifecycleFlock # portable inherited-FD kernel custody for LAN lifecycle

            # Conformance ITF generation
            quint
          ]
          ++ toolkitSupport.packages
          ++ toolkitCommands
          ++ toolkitSupport.buildInputs
          # Linux-only: browser harness plus the patchbay network lab tools
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            pkgs.chromium
            pkgs.chromedriver
            pkgs.iproute2
            pkgs.nftables
            pkgs.iptables
            pkgs.conntrack-tools
            pkgs.tcpdump
          ]
          ;

          shellHook = ''
            ${toolkitSupport.shellHook}
            ${pkgs.lib.optionalString pkgs.stdenv.isLinux ''
              export AURA_TEST_CHROMIUM="${pkgs.chromium}/bin/chromium"
              export AURA_TEST_CHROMEDRIVER="${pkgs.chromedriver}/bin/chromedriver"
            ''}
            if [ -d "$PWD/../toolkit/xtask" ]; then
              export TOOLKIT_ROOT="$PWD/../toolkit"
            fi
            export RUST_BACKTRACE=1
            export RUST_LOG=info
            export MACOSX_DEPLOYMENT_TARGET=11.0
            export CARGO_TARGET_DIR="$PWD/target"
            export AURA_WORKSPACE_ROOT="$PWD"
            export AURA_TOOLKIT_NIGHTLY_BIN="${rustToolchainNightly}/bin"
            export AURA_CARGO_DENY_BIN="${cargoDeny}/bin/cargo-deny"
          '';
        };

        devShells.nightly = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Nightly Rust toolchain (required for Kani)
            rustToolchainNightly
            cargo-udeps

            # Essential development tools
            just
            git
            ripgrep
            jq

            # Build tools
            pkg-config
            openssl

            # POSIX tools (for Justfile scripts)
            coreutils
            findutils
            gawk
            gnused

            # Kani dependencies
            # Kani itself is bootstrapped by `just _ensure-kani` into a repo-local tool root.
            # Keep z3 in the shell because Kani can delegate to it at verification time.
            z3 # SMT solver for Kani
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            dbus
          ];

          shellHook = ''
            export CARGO_TARGET_DIR="$PWD/target"
            export AURA_WORKSPACE_ROOT="$PWD"
            echo "Aura Nightly Development Environment"
            echo "====================================="
            echo ""
            echo "Rust version: $(rustc --version)"
            echo "Cargo version: $(cargo --version)"
            echo ""
            echo "Available commands:"
            echo "  cargo udeps              Check for unused dependencies"
            echo "  just --list              Show all available tasks"
            echo ""
            echo "Kani Setup (first time only):"
            echo "  just kani-setup"
            echo ""
            echo "Kani Usage:"
            echo "  just kani                    Run Kani on aura-protocol"
            echo "  just kani-harness <name>     Run a specific harness"
            echo ""
            export RUST_BACKTRACE=1
            export RUST_LOG=info
            export MACOSX_DEPLOYMENT_TARGET=11.0
            export CARGO_TARGET_DIR="$PWD/target"

            # Custom PS1 prompt (nightly variant)
            # Colors: darkgrey=user@host, purple=path, greenish-yellow=branch, orange=[nix-nightly] >
            # Note: \001 and \002 mark non-printing chars in command substitutions (like \[ \] in literal PS1)
            __git_branch() {
              local branch
              branch=$(git branch 2>/dev/null | grep '^*' | colrm 1 2)
              if [ -n "$branch" ]; then
                printf ' \001\033[38;5;148m\002(%s)\001\033[0m\002' "$branch"
              fi
            }
            PS1='\[\033[90m\]\u@\h\[\033[0m\] \[\033[35m\]\w\[\033[0m\]$(__git_branch)\[\033[38;5;208m\][nix-nightly] >\[\033[0m\] '

            # Set AURA_PATH to the project directory for development.
            # See default devShell for full explanation.
            export AURA_PATH="$PWD"
          '';
        };
      }
    );
}
