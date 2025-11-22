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

if [ ! -f "$CONFIG_FILE" ]; then
    echo "Creating default config at $CONFIG_FILE"
    # Create a temporary file first to avoid permission issues with redirection
    TMP_CONFIG=$(mktemp)
    cat <<EOF > "$TMP_CONFIG"
# Prism Driver Configuration
# Restart coreaudiod after changing these values:
# sudo launchctl kickstart -k system/com.apple.audio.coreaudiod

buffer_frame_size=1024
safety_offset=256
ring_buffer_frame_size=1024
zero_timestamp_period=1024
num_channels=16
EOF
    sudo mv "$TMP_CONFIG" "$CONFIG_FILE"
    sudo chmod 666 "$CONFIG_FILE" # Allow everyone to edit
else
    echo "Config file already exists at $CONFIG_FILE"
fi

echo "Restarting coreaudiod..."
sudo launchctl kickstart -k system/com.apple.audio.coreaudiod

echo "Installation complete. Check logs with:"
echo "log stream --predicate 'process == \"coreaudiod\"' | grep Prism"
