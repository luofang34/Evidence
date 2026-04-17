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
          };
      });
}
