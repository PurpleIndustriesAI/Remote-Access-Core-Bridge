#!/bin/sh
# Install Remote Access Crew for the current Linux user (no root needed).
# Copies the binaries to ~/.local/bin, adds a launcher and autostart entry with the app icon,
# installs the Crew app when Crew is present, and starts the bridge.
set -eu
SRC=$(cd "$(dirname "$0")" && pwd)
BIN="$HOME/.local/bin"
SHARE="${XDG_DATA_HOME:-$HOME/.local/share}"
CONF="${XDG_CONFIG_HOME:-$HOME/.config}"

find_file() {
  for p in "$SRC/$1" "$SRC/bin/$1" "$SRC/../target/release/$1" "$SRC/../assets/$1"; do
    [ -f "$p" ] && { echo "$p"; return 0; }
  done
  echo "cannot find $1 next to this installer" >&2
  exit 1
}

mkdir -p "$BIN" "$SHARE/applications" "$SHARE/icons/hicolor/256x256/apps" "$CONF/autostart"
# Process names are truncated to 15 characters, so match the full binary path instead.
pkill -f "$BIN/remote-access-crew" 2>/dev/null || true
install -m 0755 "$(find_file rac)" "$BIN/rac"
install -m 0755 "$(find_file remote-access-crew)" "$BIN/remote-access-crew"
install -m 0644 "$(find_file remote-access-crew.png)" "$SHARE/icons/hicolor/256x256/apps/remote-access-crew.png"

DESKTOP="[Desktop Entry]
Type=Application
Name=Remote Access Crew
Comment=Copy on one machine, paste on the other (LAN and Tailscale)
Exec=$BIN/remote-access-crew
Icon=remote-access-crew
Terminal=false
Categories=Network;Utility;
StartupNotify=false"
printf '%s\n' "$DESKTOP" > "$SHARE/applications/remote-access-crew.desktop"
printf '%s\nX-GNOME-Autostart-enabled=true\n' "$DESKTOP" > "$CONF/autostart/remote-access-crew.desktop"

if [ -d "$HOME/.codex-crew/apps" ] && [ -d "$SRC/crew-app" -o -d "$SRC/../crew-app" ]; then
  APP=$( [ -d "$SRC/crew-app" ] && echo "$SRC/crew-app" || echo "$SRC/../crew-app" )
  STAGE=$(mktemp -d)
  cp -R "$APP/." "$STAGE/"
  mkdir -p "$STAGE/bin"
  cp "$BIN/remote-access-crew" "$BIN/rac" "$STAGE/bin/"
  if command -v codexcrew >/dev/null 2>&1; then
    if codexcrew app install "$STAGE"; then
      echo "Crew app installed. Enable it from Crew > Apps (or: codexcrew app enable remote-access-crew --live)."
    else
      echo "Crew did not install the app (see above). The bridge works without it. App files: $STAGE"
    fi
  else
    echo "Crew CLI not found; install the app from $STAGE in Crew > Apps."
  fi
fi

nohup "$BIN/remote-access-crew" >/dev/null 2>&1 &
sleep 1
"$BIN/rac" status || true
echo
echo "Installed. Pair with: rac pair-code (one machine) and rac pair <address> <code> (the other)."
if command -v ufw >/dev/null 2>&1 && ufw status 2>/dev/null | grep -q "Status: active"; then
  echo "ufw is active. To allow peers on your LAN and Tailscale, run:"
  echo "  sudo ufw allow from 192.168.0.0/16 to any port 48765 proto tcp"
  echo "  sudo ufw allow from 10.0.0.0/8 to any port 48765 proto tcp"
  echo "  sudo ufw allow in on tailscale0 to any port 48765 proto tcp"
fi
