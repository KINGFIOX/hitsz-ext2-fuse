{
  description = "A flake to provide an environment for xv6 riscv";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        libPath = pkgs.lib.makeLibraryPath [ ];
      in {
        devShell = pkgs.mkShell {
          buildInputs = with pkgs; [
            libxml2
            pkg-config
            clang_16
            llvm_16
            fuse
            cmake
          ];
          MAKEFLAGS = "-j$(nproc)";
        };
      });
}

