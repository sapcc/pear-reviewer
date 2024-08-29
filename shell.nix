{ pkgs ? import <nixpkgs> { } }:

with pkgs;

let
  fenix = import (fetchTarball "https://github.com/nix-community/fenix/archive/monthly.tar.gz") { };
in mkShell {
  nativeBuildInputs = [
    cargo
    cargo-deny
    cargo-tarpaulin
    clippy
    pkg-config
    rustc
    fenix.default.rustfmt

    # keep this line if you use bash
    bashInteractive
  ];

  buildInputs =  [
    libgit2
    openssl
  ];

  LIBGIT2_NO_VENDOR = 1;
  OPENSSL_NO_VENDOR = 1;
  # required for rusta-analyzer
  RUST_SRC_PATH = pkgs.rust.packages.stable.rustPlatform.rustLibSrc;
}
