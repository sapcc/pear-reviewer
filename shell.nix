{ pkgs ? import <nixpkgs> { } }:

with pkgs;

let
  fenix = import (fetchTarball "https://github.com/nix-community/fenix/archive/monthly.tar.gz") { };
in mkShell {
  nativeBuildInputs = [
    cargo
    cargo-deny
    clippy
    pkg-config
    rustc
    fenix.default.rustfmt

    # keep this line if you use bash
    bashInteractive
  ];

  buildInputs =  [
    # https://github.com/NixOS/nixpkgs/pull/317275
    (libgit2.overrideAttrs (oldAttrs: rec {
      version = "1.8.1";
      src = fetchFromGitHub {
        owner = "libgit2";
        repo = "libgit2";
        rev = "v${version}";
        hash = "sha256-J2rCxTecyLbbDdsyBWn9w7r3pbKRMkI9E7RvRgAqBdY=";
      };
    }))
    openssl
  ];

  LIBGIT2_NO_VENDOR = 1;
  OPENSSL_NO_VENDOR = 1;
  # required for rusta-analyzer
  RUST_SRC_PATH = pkgs.rust.packages.stable.rustPlatform.rustLibSrc;
}
