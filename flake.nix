{
  description = "Aura - Threshold Identity and Storage Platform";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crate2nix = {
      url = "github:timewave-computer/crate2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
      crate2nix,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
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

        # Import generated Cargo.nix with CC crate fix and other overrides
        cargoNix = import ./Cargo.nix {
          inherit pkgs;
          buildRustCrateForPkgs = pkgs: pkgs.buildRustCrate;
          # Apply CC crate fix for macOS builds and other common overrides
          defaultCrateOverrides = pkgs.defaultCrateOverrides // {
            # Override for cc crate - fix Apple target detection on macOS
            cc =
              attrs:
              pkgs.lib.optionalAttrs pkgs.stdenv.isDarwin {
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
        packages = {
          # Main CLI application
          default = cargoNix.workspaceMembers.aura-cli.build;
          aura-cli = cargoNix.workspaceMembers.aura-cli.build;

          # Core applications
          aura-agent = cargoNix.workspaceMembers.aura-agent.build;
          aura-simulator = cargoNix.workspaceMembers.aura-simulator.build;

          # Development tools
          regenerate-cargo-nix = pkgs.writeScriptBin "regenerate-cargo-nix" ''
            #!${pkgs.bash}/bin/bash
            echo "Regenerating Cargo.nix with crate2nix..."
            ${crate2nix.packages.${system}.default}/bin/crate2nix generate
            echo "Cargo.nix regenerated successfully!"
          '';
        };

        checks = {
          # Run tests for key crates
          aura-core-tests = cargoNix.workspaceMembers.aura-core.build.override {
            runTests = true;
          };
          aura-crypto-tests = cargoNix.workspaceMembers.aura-crypto.build.override {
            runTests = true;
          };
          aura-protocol-tests = cargoNix.workspaceMembers.aura-protocol.build.override {
            runTests = true;
          };
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Rust toolchain
            rustToolchain
            cargo-watch
            cargo-edit
            cargo-audit

            # WASM tools
            wasm-pack
            wasm-bindgen-cli
            trunk

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

            # Documentation tools
            markdown-link-check

            # Nix tools and formatting
            nixpkgs-fmt
            crate2nix.packages.${system}.default
          ];

          shellHook = ''
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
              echo ""
              echo "Available commands:"
              echo "  just --list          Show all available tasks"
              echo "  just build           Build all crates"
              echo "  just test            Run all tests"
              echo "  just check           Run clippy and format check"
              echo "  just quint-parse     Parse Quint files to JSON"
              echo "  trunk serve          Serve console with hot reload (in console/)"
              echo "  quint --help         Formal verification with Quint"
              echo "  apalache-mc --help   Model checking with Apalache"
              echo "  crate2nix --help     Generate hermetic Nix builds"
              echo ""
              echo "Hermetic builds:"
              echo "  nix build            Build with crate2nix (hermetic)"
              echo "  nix build .#aura-cli Build specific package"
              echo "  nix run              Run aura CLI hermetically"
              echo "  nix flake check      Run hermetic tests"
              echo ""
            fi

            export RUST_BACKTRACE=1
            export RUST_LOG=info
            export MACOSX_DEPLOYMENT_TARGET=11.0
          '';
        };
      }
    );
}
