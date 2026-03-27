{
  description = "xv6fs-fuse - xv6 filesystem implemented in userspace via FUSE";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        isDarwin = pkgs.stdenv.isDarwin;
      in
      {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            cmake
            ninja
            pkg-config
            gcc
          ];

          buildInputs = with pkgs; [
            gtest
          ] ++ pkgs.lib.optionals (!isDarwin) [
            fuse
          ] ++ pkgs.lib.optionals isDarwin [
            macfuse-stubs
          ];

          shellHook = ''
            echo "xv6fs-fuse dev environment loaded"
          '' + pkgs.lib.optionalString isDarwin ''
            echo "NOTE: macFUSE must be installed system-wide (https://osxfuse.github.io/)"
            echo "      Nix only provides the headers for compilation."
          '';
        };
      }
    );
}
