{
  description = "Aura - Threshold Identity and Storage Platform";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
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
      in
      {
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

            # Nix formatter
            nixpkgs-fmt
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
              echo "  just --list      Show all available tasks"
              echo "  just build       Build all crates"
              echo "  just test        Run all tests"
              echo "  just check       Run clippy and format check"
              echo "  just quint-parse Parse Quint files to JSON"
              echo "  trunk serve      Serve console with hot reload (in console/)"
              echo "  quint --help     Formal verification with Quint"
              echo ""
            fi

            export RUST_BACKTRACE=1
            export RUST_LOG=info
          '';
        };
      }
    );
}
