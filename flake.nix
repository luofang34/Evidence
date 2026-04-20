{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            rustToolchain
            pkgs.cargo-watch
            pkgs.cargo-nextest
            # `jq` is required by
            # `scripts/deterministic-baseline-override-lint.sh`;
            # ship it in the devShell so contributors running
            # `./scripts/local-ci.sh` inside `nix develop` don't
            # hit a PATH miss.
            pkgs.jq
          ];
          shellHook = ''
            export IN_NIX_SHELL=1
          '';
        };

        packages.default =
          let
            # `pkgs.rustPlatform` uses nixpkgs' bundled rustc, which
            # lags the workspace MSRV. Build the package with the
            # same toolchain the devShell uses (driven by
            # rust-toolchain.toml) so the Nix build honors the
            # project's `rust-version = "1.95"` pin.
            rustPlatform = pkgs.makeRustPlatform {
              cargo = rustToolchain;
              rustc = rustToolchain;
            };
          in
          rustPlatform.buildRustPackage {
            pname = "cargo-evidence";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            # `cross_time_determinism.rs` integration tests spawn
            # the override lint, which runs `jq` against bundle
            # manifests. Without this, the sandbox hides `jq`
            # from PATH and the tests silently skip — which
            # defeats the Nix reproducibility gate's whole point.
            nativeBuildInputs = [ pkgs.jq ];
          };
      });
}
