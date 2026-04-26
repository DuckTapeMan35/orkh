#!/bin/bash

# orkh Setup Script – dedicated user, config stays in user's home

set -e

# --------------------------------------------
# 1. Build and install binary
# --------------------------------------------
echo "Building release binary..."
cargo build --release

echo "Installing binary to /usr/bin/..."
sudo cp target/release/orkh /usr/bin/

# --------------------------------------------
# 2. Config directory (user's original location)
# --------------------------------------------
ORKH_USER=$(whoami)
ORKH_UID=$(id -u)
CONFIG_DIR="$HOME/.config/orkh"
CONFIG_FILE="$CONFIG_DIR/config.yaml"

# Create config directory if missing (as the original user)
mkdir -p "$CONFIG_DIR"

# Create default config if missing
if [ ! -f "$CONFIG_FILE" ]; then
    echo "Creating default config in $CONFIG_FILE..."
    cat > "$CONFIG_FILE" << 'EOL'
pywal: false
modes:
  base:
    rules:
      - keys: ['all']
        color: '[255,0,0]'
EOL
fi

# --------------------------------------------
# 3. Create systemd service
# --------------------------------------------
SERVICE_FILE="/etc/systemd/system/orkh.service"
echo "Creating systemd service: $SERVICE_FILE"

sudo tee "$SERVICE_FILE" > /dev/null << EOF
[Unit]
Description=Keyboard Highlighter
After=graphical.target display-manager.service
Wants=graphical.target

[Service]
Type=simple
User=root
ExecStart=/usr/bin/orkh
Environment=DISPLAY=:0
Environment=XDG_RUNTIME_DIR=/run/user/$ORKH_UID
Environment=ORKH_USER=$ORKH_USER
Environment=HOME=/root

# Create /run/orkh-openrgb-config automatically
RuntimeDirectory=orkh-openrgb-config

# Capabilities
CapabilityBoundingSet=CAP_DAC_READ_SEARCH CAP_SYS_RAWIO CAP_SETUID CAP_SETGID
AmbientCapabilities=CAP_DAC_READ_SEARCH CAP_SYS_RAWIO CAP_SETUID CAP_SETGID
NoNewPrivileges=no

# Filesystem restrictions
ProtectSystem=full
ReadWritePaths=/home/$ORKH_USER/.config/orkh /tmp/.X11-unix /run/user/$ORKH_UID
ProtectHome=read-only
PrivateTmp=no

# Device policy - allow input devices
PrivateDevices=no
DevicePolicy=closed
DeviceAllow=/dev/null rw
DeviceAllow=/dev/zero rw
DeviceAllow=/dev/random r
DeviceAllow=/dev/urandom r
DeviceAllow=/dev/input rw
DeviceAllow=/dev/input/event* rw
DeviceAllow=char-usb_device rw
DeviceAllow=char-hidraw rw
DeviceAllow=char-input rw

# Hardening
SystemCallFilter=~@cpu-emulation @obsolete @resources @keyring @module
SystemCallArchitectures=native
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
RestrictRealtime=yes
RestrictNamespaces=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
RestrictAddressFamilies=AF_UNIX AF_NETLINK AF_INET AF_INET6

StandardInput=null
StandardOutput=journal
StandardError=journal
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
EOF

# --------------------------------------------
# 4. Enable the service
# --------------------------------------------
echo "Reloading systemd and starting service..."
sudo systemctl daemon-reload
sudo systemctl enable orkh.service

echo ""
echo "Installation complete"
