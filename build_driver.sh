#!/bin/bash
set -e

# Build the rust project
cargo build

# Fix libiconv dependency to use system library instead of Nix store library
NIX_ICONV=$(otool -L target/debug/libprism.dylib | grep libiconv | awk '{print $1}')
if [ ! -z "$NIX_ICONV" ]; then
    echo "Fixing libiconv path: $NIX_ICONV -> /usr/lib/libiconv.2.dylib"
    install_name_tool -change "$NIX_ICONV" /usr/lib/libiconv.2.dylib target/debug/libprism.dylib
fi

# Create bundle structure
mkdir -p Prism.driver/Contents/MacOS

# Copy Info.plist
cp driver_bundle/Info.plist Prism.driver/Contents/

# Copy the dylib and rename it to the executable name specified in Info.plist
cp target/debug/libprism.dylib Prism.driver/Contents/MacOS/Prism

# Ad-hoc sign the bundle (required for Apple Silicon)
codesign --force --deep --sign - Prism.driver

echo "Prism.driver bundle created successfully."
