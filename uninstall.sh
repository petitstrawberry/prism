#!/bin/bash
set -e

DRIVER_PATH="/Library/Audio/Plug-Ins/HAL/Prism.driver"

if [ -d "$DRIVER_PATH" ]; then
    echo "Removing $DRIVER_PATH..."
    sudo rm -rf "$DRIVER_PATH"
else
    echo "$DRIVER_PATH not found."
fi

echo "Please reboot your system to complete the uninstallation of the Prism driver."