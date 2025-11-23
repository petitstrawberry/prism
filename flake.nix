{
  description = "A Nix flake for a development shell for prism";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, fenix }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        darwinOutputs =
          if pkgs.stdenv.isDarwin then
            let
              toolchain = fenix.packages.${system}.fromToolchainFile {
                file = ./rust-toolchain.toml;
                sha256 = "sha256-akA93eWREhUXpWuhsOYqv0B4ZHuTRhQOYjZRcbrxXKg=";
              };

              rustPlatform = pkgs.makeRustPlatform {
                cargo = toolchain;
                rustc = toolchain;
              };

              prismDriver = rustPlatform.buildRustPackage rec {
                pname = "prism-driver";
                version = "0.1.0";
                src = ./.;

                cargoLock.lockFile = ./Cargo.lock;

                RUSTFLAGS = "-Zunstable-options";

                nativeBuildInputs = with pkgs; [ darwin.cctools ];
                postInstall = ''
                  bundle=$out/Prism.driver
                  mkdir -p "$bundle/Contents/MacOS"
                  cp ${./driver_bundle/Info.plist} "$bundle/Contents/Info.plist"
                  cp "$out/lib/libprism.dylib" "$bundle/Contents/MacOS/Prism"
                  chmod +x "$bundle/Contents/MacOS/Prism"

                  iconv_path=$(otool -L "$bundle/Contents/MacOS/Prism" | awk '/libiconv/{print $1; exit}')
                  if [ -n "$iconv_path" ]; then
                    install_name_tool -change "$iconv_path" /usr/lib/libiconv.2.dylib "$bundle/Contents/MacOS/Prism"
                  fi

                  if command -v codesign >/dev/null 2>&1; then
                    /usr/bin/codesign --force --deep --sign - "$bundle"
                  fi
                '';
              };

              installScript = pkgs.writeShellApplication {
                name = "install-prism-driver";
                text = ''
                  set -euo pipefail

                  bundle="${prismDriver}/Prism.driver"
                  if [ ! -d "$bundle" ]; then
                    echo "error: bundle not found: $bundle" >&2
                    exit 1
                  fi

                  dest="/Library/Audio/Plug-Ins/HAL"
                  echo "Installing Prism.driver to $dest (sudo may prompt for your password)..."
                  sudo mkdir -p "$dest"
                  if [ -d "$dest/Prism.driver" ]; then
                    echo "Removing existing Prism.driver from $dest"
                    sudo rm -rf "$dest/Prism.driver"
                  fi
                  sudo cp -R "$bundle" "$dest/"

                  echo "Installation complete. Please reboot to activate the driver."
                '';
              };
            in
            {
              packages = {
                default = prismDriver;
                prism-driver = prismDriver;
              };

              apps.install = {
                type = "app";
                program = "${installScript}/bin/install-prism-driver";
              };
            }
          else {
            packages = { };
            apps = { };
          };
      in
      darwinOutputs // {

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustup
          ];

          # Ensure common Rust tooling is available in the shell.
          # If a `rust-toolchain.toml` exists in the project root, parse the
          # `channel` and ensure that toolchain and components are installed
          # via rustup so `cargo`, `clippy`, and `rustfmt` behave as expected.
          shellHook = ''
            # Prefer rustup-managed toolchains over the Nix-provided cargo/rustc
            export PATH="$HOME/.cargo/bin:$PATH"

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