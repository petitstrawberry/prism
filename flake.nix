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
          # If a `rust-toolchain.toml` exists in the project root, parse the
          # `channel` and ensure that toolchain and components are installed
          # via rustup so `cargo`, `clippy`, and `rustfmt` behave as expected.
          shellHook = ''
            # Default to stable if no toolchain file found or parsing fails.
            channel=stable
            if [ -f rust-toolchain.toml ]; then
              channel=$(awk -F\" '/channel/ {print $2; exit}' rust-toolchain.toml || echo stable)
            fi

            if command -v rustup >/dev/null 2>&1; then
              # Install the toolchain specified in rust-toolchain.toml (no-self-update to avoid prompting)
              rustup toolchain install "$channel" --no-self-update >/dev/null 2>&1 || true
              # Ensure common components exist for the pinned toolchain
              rustup component add rustfmt clippy rust-src --toolchain "$channel" >/dev/null 2>&1 || true
              # Set a directory override so cargo/rustc inside the project use the pinned toolchain
              rustup override set "$channel" >/dev/null 2>&1 || true
            fi
          '';
        };
      }
    );
}