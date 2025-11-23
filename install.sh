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

# Create default config in /Library/Application Support/Prism/
CONFIG_DIR="/Library/Application Support/Prism"
CONFIG_FILE="$CONFIG_DIR/config.txt"

if [ ! -d "$CONFIG_DIR" ]; then
    echo "Creating config directory at $CONFIG_DIR"
    sudo mkdir -p "$CONFIG_DIR"
    sudo chmod 777 "$CONFIG_DIR" # Allow everyone to write to the dir for now
fi

echo "Please reboot your system to complete the installation of the Prism driver."