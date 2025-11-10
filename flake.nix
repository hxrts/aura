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
      url = "path:./ext/crate2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, crate2nix }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
          targets = [ "wasm32-unknown-unknown" ];
        };

        # Import generated Cargo.nix with crate2nix's built-in overrides
        cargoNix = import ./Cargo.nix {
          inherit pkgs;
          buildRustCrateForPkgs = pkgs: pkgs.buildRustCrate;
          # Use crate2nix's default overrides which now include the CC crate fix
          defaultCrateOverrides = pkgs.defaultCrateOverrides;
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
            ${crate2nix.packages.${system}.default}/bin/crate2nix generate \
              --offline \
              --no-cargo-build-std
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

            # Development tools
            git
            jq

            # Formal verification and protocol modeling
            quint
            nodejs_20
            jre  # Java Runtime Environment for ANTLR4TS

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