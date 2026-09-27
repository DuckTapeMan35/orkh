#!/usr/bin/env bash
# orkh setup: installs the binary and a root systemd service.
# orkh sandboxes itself with bubblewrap at startup, so the unit stays simple.
set -euo pipefail

if [[ $EUID -eq 0 ]]; then
  echo "Run this as your normal user; it uses sudo where needed." >&2
  exit 1
fi
if [[ -e /etc/NIXOS ]]; then
  echo "On NixOS, use the flake instead of this script." >&2
  exit 1
fi

ORKH_USER=$(id -un)
ORKH_UID=$(id -u)
RUNDIR=/run/user/$ORKH_UID

# --------------------------------------------
# 1. Dependencies
# --------------------------------------------
# Non-Nix builds expect bwrap at /usr/bin/bwrap, and the sandbox PATH is /usr/bin.
missing=()
[[ -x /usr/bin/bwrap ]] || missing+=(bubblewrap)
[[ -x /usr/bin/openrgb ]] || missing+=(openrgb)
if ((${#missing[@]})); then
  echo "Missing: ${missing[*]}. Install them with your package manager." >&2
  exit 1
fi

if systemctl is-enabled --quiet openrgb.service 2>/dev/null; then
  echo "Warning: openrgb.service is enabled and will fight orkh over the keyboard." >&2
  echo "         Disable it with: sudo systemctl disable --now openrgb.service" >&2
fi

# --------------------------------------------
# 2. Build and install
# --------------------------------------------
echo "Building release binary..."
cargo build --release
sudo install -Dm755 target/release/orkh /usr/bin/orkh

# --------------------------------------------
# 3. Default config
# --------------------------------------------
CONFIG_DIR="$HOME/.config/orkh"
CONFIG_FILE="$CONFIG_DIR/config.yaml"
mkdir -p "$CONFIG_DIR"

if [[ ! -f "$CONFIG_FILE" ]]; then
  echo "Creating default config in $CONFIG_FILE..."
  cat >"$CONFIG_FILE" <<'EOL'
pywal: false
openrgb:
  i2c: false  # true for RGB RAM / motherboard / GPU lighting (needs i2c-dev + SMBus driver)
modes:
  base:
    rules:
      - keys: ['all']
        color: [255, 0, 0]
EOL
fi

# --------------------------------------------
# 4. systemd units
# --------------------------------------------
echo "Installing systemd units..."

sudo tee /etc/systemd/system/orkh.service >/dev/null <<EOF
[Unit]
Description=orkh keyboard highlighter

[Service]
Type=simple
ExecStart=/usr/bin/orkh
Environment=ORKH_USER=$ORKH_USER
Environment=ORKH_WAYLAND_DISPLAY=wayland-0
StateDirectory=orkh
Restart=on-failure
RestartSec=5

# Hardening that doesn't interfere with bwrap; the sandbox does the rest
NoNewPrivileges=yes
LockPersonality=yes
RestrictRealtime=yes
SystemCallArchitectures=native

[Install]
WantedBy=multi-user.target
EOF

# The sandbox binds WM sockets once at startup, so restart orkh when one appears
sudo tee /etc/systemd/system/orkh-rebind.path >/dev/null <<EOF
[Unit]
Description=Watch for window manager sockets for orkh

[Path]
PathExists=$RUNDIR/wayland-0
PathExists=$RUNDIR/duckwm.sock
PathExists=/tmp/duckwm-$ORKH_UID.sock
Unit=orkh-rebind.service

[Install]
WantedBy=multi-user.target
EOF

# Tied to the runtime dir, so it resets on logout and fires again on the next login
sudo tee /etc/systemd/system/orkh-rebind.service >/dev/null <<EOF
[Unit]
Description=Restart orkh to bind the window manager socket
BindsTo=user-runtime-dir@$ORKH_UID.service
After=user-runtime-dir@$ORKH_UID.service

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/bin/systemctl try-restart orkh.service
EOF

# --------------------------------------------
# 5. Enable
# --------------------------------------------
sudo systemctl daemon-reload
sudo systemctl enable --now orkh.service orkh-rebind.path

echo ""
echo "Installation complete. Logs: journalctl -u orkh -f"
