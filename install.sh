#!/bin/bash
set -e

DRIVER_NAME="Prism.driver"
DEST="/Library/Audio/Plug-Ins/HAL"

if [ ! -d "$DRIVER_NAME" ]; then
    echo "Error: $DRIVER_NAME not found. Run ./build_driver.sh first."
    exit 1
fi

echo "Installing $DRIVER_NAME to $DEST..."
sudo cp -r "$DRIVER_NAME" "$DEST/"

echo "Restarting coreaudiod..."
sudo launchctl kickstart -k system/com.apple.audio.coreaudiod

echo "Installation complete. Check logs with:"
echo "log stream --predicate 'process == \"coreaudiod\"' | grep Prism"
