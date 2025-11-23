{
  description = "A Nix flake for a development shell for prism";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustup
          ];

          # Ensure common Rust tooling is available in the shell.
          # In Nix environments, adding components via rustup in a shellHook
          # is a practical way to get `rustfmt` and `clippy` available.
          shellHook = ''
            rustup component add rustfmt clippy --toolchain stable >/dev/null 2>&1 || true
          '';
        };
      }
    );
}