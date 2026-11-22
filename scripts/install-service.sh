#!/usr/bin/env bash
# Instala Roost como servicio 24/7 en macOS (launchd) o Linux (systemd).
set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$REPO_DIR/target/release/roost"
DATA_DIR="${ROOST_DATA_DIR:-$HOME/.local/share/roost}"

if [ ! -x "$BIN" ]; then
  echo "Compilando Roost en modo release..."
  (cd "$REPO_DIR/web" && npm ci && npm run build)
  (cd "$REPO_DIR" && cargo build --release --bin roost)
fi

mkdir -p "$DATA_DIR"

if [ "$(uname -s)" = "Darwin" ]; then
  PLIST="$HOME/Library/LaunchAgents/com.roost.server.plist"
  mkdir -p "$(dirname "$PLIST")"
  cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.roost.server</string>
  <key>ProgramArguments</key>
  <array>
    <string>$BIN</string>
  </array>
  <key>WorkingDirectory</key><string>$REPO_DIR</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>ROOST_ADDR</key><string>0.0.0.0:8420</string>
    <key>ROOST_DATA_DIR</key><string>$DATA_DIR</string>
    <key>ROOST_APPS_DIR</key><string>$REPO_DIR/apps</string>
    <key>ROOST_SKILLS_DIR</key><string>$REPO_DIR/skills</string>
  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>$DATA_DIR/roost.log</string>
  <key>StandardErrorPath</key><string>$DATA_DIR/roost.err.log</string>
</dict>
</plist>
EOF
  launchctl unload "$PLIST" 2>/dev/null || true
  launchctl load -w "$PLIST"
  echo "Roost instalado como servicio 24/7 en macOS (http://localhost:8420)."
else
  UNIT="$HOME/.config/systemd/user/roost.service"
  mkdir -p "$(dirname "$UNIT")"
  cat > "$UNIT" <<EOF
[Unit]
Description=Roost Family Home Server
After=network-online.target

[Service]
Type=simple
WorkingDirectory=$REPO_DIR
Environment=ROOST_ADDR=0.0.0.0:8420
Environment=ROOST_DATA_DIR=$DATA_DIR
Environment=ROOST_APPS_DIR=$REPO_DIR/apps
Environment=ROOST_SKILLS_DIR=$REPO_DIR/skills
ExecStart=$BIN
Restart=always
RestartSec=3

[Install]
WantedBy=default.target
EOF
  systemctl --user daemon-reload
  systemctl --user enable --now roost.service
  echo "Roost instalado como servicio 24/7 en Linux (http://localhost:8420)."
fi
