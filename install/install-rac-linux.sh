#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# Remote Access Core (RAC) Complete Linux Installer
# Installs & brands every component exclusively as Remote Access Core (RAC):
#   1. Remote Access Core (Host)   - NVENC/KMS/uinput + LAN-only UI + RAC logo
#   2. Remote Access Core (Bridge) - v0.7.0 clipboard/files/consent + RAC logo
#   3. Remote Access Core (Client) - Qt6 desktop client + RAC logo
#   4. Security                    - Strict RFC1918 LAN + Tailscale UFW firewall
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
HOST_PKG_DIR="$SCRIPT_DIR/host"
BRAND_DIR="$SCRIPT_DIR/branding"
BRIDGE_BUNDLE="$SCRIPT_DIR/rac-bridge-latest.bundle"
BRIDGE_SHA="$SCRIPT_DIR/rac-bridge-latest.bundle.sha256"
CLIENT_TAR="$SCRIPT_DIR/rac-client-src.tar.gz"
CLIENT_SHA="$SCRIPT_DIR/rac-client-src.tar.gz.sha256"

BRIDGE_DIR="${RAC_BRIDGE_DIR:-$HOME/Projects/Remote-Access-Core-Bridge}"
CLIENT_DIR="${RAC_CLIENT_DIR:-$HOME/Projects/Remote-Access-Core-Client}"

BIN_DIR="$HOME/.local/bin"
SHARE_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
APP_DIR="$SHARE_DIR/applications"
ICON_PNG_DIR="$SHARE_DIR/icons/hicolor/256x256/apps"
ICON_SVG_DIR="$SHARE_DIR/icons/hicolor/scalable/apps"

export PATH="$BIN_DIR:$HOME/.cargo/bin:$PATH"
mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_PNG_DIR" "$ICON_SVG_DIR" "$HOME/.config/systemd/user"

echo "======================================================================"
echo "  Remote Access Core (RAC) Full Stack Installer"
echo "  Components: RAC Host + RAC Bridge v0.7.0 + RAC Client"
echo "======================================================================"

if [ -r /etc/os-release ]; then
    # shellcheck disable=SC1091
    . /etc/os-release
else
    echo "Error: cannot read /etc/os-release" >&2
    exit 1
fi
OS_ID="${ID:-unknown}"

echo "[0/4] Installing official Remote Access Core (RAC) logos and icons..."
if [ -d "$BRAND_DIR" ]; then
    for name in rac-host rac-client rac-bridge; do
        [ -f "$BRAND_DIR/$name.png" ] && install -m 0644 "$BRAND_DIR/$name.png" "$ICON_PNG_DIR/$name.png"
        [ -f "$BRAND_DIR/$name.svg" ] && install -m 0644 "$BRAND_DIR/$name.svg" "$ICON_SVG_DIR/$name.svg"
    done
    [ -f "$BRAND_DIR/rac-bridge.png" ] && install -m 0644 "$BRAND_DIR/rac-bridge.png" "$ICON_PNG_DIR/remote-access-crew.png"
    [ -f "$BRAND_DIR/rac-bridge.svg" ] && install -m 0644 "$BRAND_DIR/rac-bridge.svg" "$ICON_SVG_DIR/remote-access-crew.svg"
fi

echo "[1/4] Installing Remote Access Core (Host)..."

install_rac_host_appimage() {
    local appimg="$HOST_PKG_DIR/rac-host-linux-x86_64.AppImage"
    if [ -f "$appimg" ]; then
        (cd "$HOST_PKG_DIR" && grep 'rac-host-linux-x86_64.AppImage$' SHA256SUMS | sha256sum -c -)
        install -m 0755 "$appimg" "$BIN_DIR/rac-host-bin"
        echo "  -> Installed Remote Access Core (Host) portable binary to $BIN_DIR/rac-host-bin"
    else
        echo "  WARNING: RAC Host portable image not found at $appimg"
    fi
}

case "$OS_ID" in
    ubuntu|debian)
        DEB="$HOST_PKG_DIR/rac-host-ubuntu24.04-amd64.deb"
        if [ -f "$DEB" ]; then
            (cd "$HOST_PKG_DIR" && grep 'rac-host-ubuntu24.04-amd64.deb$' SHA256SUMS | sha256sum -c -)
            sudo apt-get update
            if ! sudo DEBIAN_FRONTEND=noninteractive apt-get install -y "$DEB"; then
                sudo DEBIAN_FRONTEND=noninteractive apt-get -f install -y || install_rac_host_appimage
            fi
        else
            install_rac_host_appimage
        fi
        ;;
    arch|omarchy)
        PKG="$HOST_PKG_DIR/rac-host-arch-x86_64.pkg.tar.zst"
        if [ -f "$PKG" ]; then
            (cd "$HOST_PKG_DIR" && grep 'rac-host-arch-x86_64.pkg.tar.zst$' SHA256SUMS | sha256sum -c -)
            if ! sudo pacman -U --noconfirm --needed "$PKG"; then
                echo "  -> Falling back to portable Remote Access Core (Host) image..."
                install_rac_host_appimage
            fi
        else
            install_rac_host_appimage
        fi
        ;;
    *)
        install_rac_host_appimage
        ;;
esac

HOST_EXEC=""
if [ -x "$BIN_DIR/rac-host-bin" ]; then
    HOST_EXEC="$BIN_DIR/rac-host-bin"
elif command -v sunshine >/dev/null 2>&1; then
    HOST_EXEC="$(readlink -f "$(command -v sunshine)")"
fi

if [ -n "$HOST_EXEC" ]; then
    ln -sf "$HOST_EXEC" "$BIN_DIR/rac-host"
    sudo setcap cap_sys_admin+p "$HOST_EXEC" 2>/dev/null || true
fi

sudo usermod -aG input,video,render "$USER" 2>/dev/null || true
sudo tee /etc/udev/rules.d/60-rac-host-uinput.rules >/dev/null << 'EOF'
KERNEL=="uinput", SUBSYSTEM=="misc", OPTIONS+="static_node=uinput", TAG+="uaccess", GROUP="input", MODE="0660"
EOF
sudo udevadm control --reload-rules 2>/dev/null || true
sudo udevadm trigger /dev/uinput 2>/dev/null || true

if [ -f "$BRAND_DIR/rac-host.png" ]; then
    for icon_slot in \
        /usr/share/icons/hicolor/scalable/apps/sunshine.svg \
        /usr/share/icons/hicolor/scalable/status/sunshine-tray.svg \
        /usr/share/icons/hicolor/scalable/status/sunshine-playing.svg \
        /usr/share/icons/hicolor/scalable/status/sunshine-pausing.svg \
        /usr/share/icons/hicolor/scalable/status/sunshine-locked.svg; do
        if [ -f "$icon_slot" ] && [ -f "$BRAND_DIR/rac-host.svg" ]; then
            sudo install -m 0644 "$BRAND_DIR/rac-host.svg" "$icon_slot" 2>/dev/null || true
        fi
    done
    for png_slot in \
        /usr/share/sunshine/assets/web/images/sunshine.ico \
        /usr/share/sunshine/assets/web/images/logo-sunshine-45.png \
        /usr/share/sunshine/assets/web/images/logo-sunshine-256.png; do
        if [ -f "$png_slot" ]; then
            case "$png_slot" in
                *.ico) [ -f "$BRAND_DIR/rac-host.ico" ] && sudo install -m 0644 "$BRAND_DIR/rac-host.ico" "$png_slot" 2>/dev/null || true ;;
                *.png) sudo install -m 0644 "$BRAND_DIR/rac-host.png" "$png_slot" 2>/dev/null || true ;;
            esac
        fi
    done
fi

HOST_CONF_DIR="$HOME/.config/sunshine"
mkdir -p "$HOST_CONF_DIR"
if [ ! -f "$HOST_CONF_DIR/sunshine.conf" ]; then
    cat > "$HOST_CONF_DIR/sunshine.conf" << EOF
sunshine_name = $(hostname)
origin_web_ui_allowed = lan
upnp = disabled
channels = 1
EOF
    chmod 600 "$HOST_CONF_DIR/sunshine.conf"
fi

# Deduplicate stock Linux desktop tiles (remove redundant "Low Res Desktop" / duplicate "Desktop")
HOST_APPS_JSON="$HOST_CONF_DIR/apps.json"
if [ -f "$HOST_APPS_JSON" ] && command -v python3 >/dev/null 2>&1; then
    python3 - "$HOST_APPS_JSON" << 'PYEOF'
import json, pathlib, sys
p = pathlib.Path(sys.argv[1])
try:
    data = json.loads(p.read_text(encoding="utf-8"))
except Exception:
    sys.exit(0)
apps = data.get("apps")
if not isinstance(apps, list):
    sys.exit(0)
has_desktop = any((a.get("name") or "").strip().lower() == "desktop" for a in apps if isinstance(a, dict))
if not has_desktop:
    sys.exit(0)
kept = []
saw_desktop = False
for a in apps:
    if not isinstance(a, dict):
        kept.append(a)
        continue
    nm = (a.get("name") or "").strip().lower()
    if nm == "low res desktop":
        a["name"] = "Low Res Desktop"
        a.pop("image-path", None)
        kept.append(a)
        continue
    if nm == "desktop":
        if saw_desktop:
            continue
        saw_desktop = True
    kept.append(a)
if kept != apps:
    data["apps"] = kept
    p.write_text(json.dumps(data, indent=4) + "\n", encoding="utf-8")
PYEOF
    chmod 600 "$HOST_APPS_JSON" 2>/dev/null || true
elif [ ! -f "$HOST_APPS_JSON" ]; then
    cat > "$HOST_APPS_JSON" << 'EOF'
{
    "env": {
        "PATH": "$(PATH):$(HOME)/.local/bin"
    },
    "apps": [
        {
            "name": "Desktop",
            "image-path": "desktop.png"
        }
    ]
}
EOF
    chmod 600 "$HOST_APPS_JSON"
fi

systemctl --user disable --now sunshine 2>/dev/null || true
cat > "$HOME/.config/systemd/user/rac-host.service" << EOF
[Unit]
Description=Remote Access Core (Host)
After=graphical-session.target network-online.target

[Service]
ExecStart=$BIN_DIR/rac-host
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
EOF
systemctl --user daemon-reload 2>/dev/null || true
systemctl --user enable rac-host.service 2>/dev/null || true
sudo loginctl enable-linger "$USER" 2>/dev/null || true

echo "[2/4] Building & installing Remote Access Core (Bridge) v0.7.0..."

NEED_RUSTUP=1
if command -v rustc >/dev/null 2>&1; then
    RUST_VER="$(rustc --version | awk '{print $2}')"
    if [ "$(printf '%s\n1.85.0\n' "$RUST_VER" | sort -V | head -n1)" = "1.85.0" ]; then
        NEED_RUSTUP=0
        echo "  -> Using existing rustc $RUST_VER"
    fi
fi
if [ "$NEED_RUSTUP" -eq 1 ]; then
    echo "  -> Installing Rust toolchain (>= 1.85) via rustup for RAC Bridge..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path
    export PATH="$HOME/.cargo/bin:$PATH"
fi

case "$OS_ID" in
    ubuntu|debian)
        sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
            build-essential pkg-config libx11-dev libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libwayland-dev || true
        ;;
    arch|omarchy)
        sudo pacman -S --needed --noconfirm base-devel pkgconf libx11 libxcb wayland || true
        ;;
esac

if [ -f "$BRIDGE_BUNDLE" ] && [ -f "$BRIDGE_SHA" ]; then
    (cd "$SCRIPT_DIR" && sha256sum -c "$(basename "$BRIDGE_SHA")")
    mkdir -p "$(dirname "$BRIDGE_DIR")"
    if [ ! -d "$BRIDGE_DIR/.git" ]; then
        git clone "$BRIDGE_BUNDLE" "$BRIDGE_DIR"
        git -C "$BRIDGE_DIR" remote set-url origin "https://github.com/PurpleIndustriesAI/Remote-Access-Core-Bridge.git"
    fi
    (
        cd "$BRIDGE_DIR"
        cargo build --release --locked
        sh ./install/install.sh
    )
    ln -sf "$BIN_DIR/remote-access-crew" "$BIN_DIR/rac-bridge"
    cat > "$HOME/.config/systemd/user/rac-bridge.service" << EOF
[Unit]
Description=Remote Access Core (Bridge)
After=network-online.target

[Service]
ExecStart=$BIN_DIR/rac-bridge
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
EOF
    systemctl --user daemon-reload 2>/dev/null || true
    systemctl --user enable --now rac-bridge.service 2>/dev/null || true
else
    echo "  WARNING: RAC Bridge bundle missing in $SCRIPT_DIR"
fi

echo "[3/4] Building & installing Remote Access Core (Client)..."

case "$OS_ID" in
    ubuntu|debian)
        sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
            build-essential pkg-config git \
            qt6-base-dev qt6-declarative-dev qt6-svg-dev qt6-wayland-dev \
            qml6-module-qtquick-controls qml6-module-qtquick-layouts \
            qml6-module-qtquick-window qml6-module-qtquick-templates \
            libegl1-mesa-dev libgl1-mesa-dev libdrm-dev \
            libopus-dev libsdl2-dev libsdl2-ttf-dev libssl-dev \
            libavcodec-dev libavformat-dev libavutil-dev libswscale-dev \
            libva-dev libvdpau-dev libxkbcommon-dev libwayland-dev wayland-protocols || true
        ;;
    arch|omarchy)
        sudo pacman -S --needed --noconfirm \
            base-devel pkgconf git \
            qt6-base qt6-declarative qt6-svg qt6-wayland \
            ffmpeg sdl2 sdl2_ttf opus openssl libva libvdpau libdrm wayland libxkbcommon || true
        ;;
esac

if [ -f "$CLIENT_TAR" ] && [ -f "$CLIENT_SHA" ]; then
    (cd "$SCRIPT_DIR" && sha256sum -c "$(basename "$CLIENT_SHA")")
    mkdir -p "$(dirname "$CLIENT_DIR")"
    if [ ! -f "$CLIENT_DIR/moonlight-qt.pro" ]; then
        TMP_EXTRACT="$(mktemp -d)"
        tar -xzf "$CLIENT_TAR" -C "$TMP_EXTRACT"
        rm -rf "$CLIENT_DIR"
        mv "$TMP_EXTRACT/rac-client-stage" "$CLIENT_DIR"
        rmdir "$TMP_EXTRACT" 2>/dev/null || true
    fi
    QMAKE_BIN="$(command -v qmake6 || command -v qmake || true)"
    if [ -n "$QMAKE_BIN" ]; then
        (
            cd "$CLIENT_DIR"
            mkdir -p build
            cd build
            "$QMAKE_BIN" ../moonlight-qt.pro CONFIG+=release
            make -j"$(nproc)"
            sudo make install
        )
        CLIENT_REAL="$(command -v RemoteAccessCoreClient || command -v moonlight || echo /usr/local/bin/moonlight)"
        if [ -x "$CLIENT_REAL" ]; then
            ln -sf "$CLIENT_REAL" "$BIN_DIR/rac-client"
        fi
        echo "  -> Remote Access Core (Client) built and linked to $BIN_DIR/rac-client"
    else
        echo "  WARNING: qmake6 not found; RAC Client source unpacked at $CLIENT_DIR"
    fi
else
    echo "  WARNING: RAC Client source tarball missing in $SCRIPT_DIR"
fi

for sys_app_dir in /usr/share/applications /usr/local/share/applications; do
    if [ -d "$sys_app_dir" ]; then
        for f in "$sys_app_dir"/*sunshine*.desktop "$sys_app_dir"/*moonlight*.desktop; do
            [ -f "$f" ] || continue
            base="$(basename "$f")"
            cat > "$APP_DIR/$base" << 'EOF'
[Desktop Entry]
Type=Application
Name=Hidden Upstream Entry
NoDisplay=true
Hidden=true
EOF
        done
    fi
done

cat > "$APP_DIR/rac-host.desktop" << EOF
[Desktop Entry]
Type=Application
Name=Remote Access Core (Host)
Comment=Remote Access Core streaming host service
Exec=$BIN_DIR/rac-host
Icon=rac-host
Terminal=false
Categories=Network;RemoteAccess;
EOF

cat > "$APP_DIR/rac-host-ui.desktop" << EOF
[Desktop Entry]
Type=Application
Name=Remote Access Core (Host Settings)
Comment=Open Remote Access Core Host LAN dashboard
Exec=xdg-open https://localhost:47990
Icon=rac-host
Terminal=false
Categories=Network;RemoteAccess;Settings;
EOF

cat > "$APP_DIR/rac-client.desktop" << EOF
[Desktop Entry]
Type=Application
Name=Remote Access Core (Client)
Comment=Remote Access Core low-latency desktop client
Exec=$BIN_DIR/rac-client
Icon=rac-client
Terminal=false
Categories=Network;RemoteAccess;
EOF

cat > "$APP_DIR/rac-bridge.desktop" << EOF
[Desktop Entry]
Type=Application
Name=Remote Access Core (Bridge)
Comment=Secure two-way clipboard, image, and file sync over LAN and Tailscale
Exec=$BIN_DIR/rac-bridge
Icon=rac-bridge
Terminal=false
Categories=Network;Utility;
StartupNotify=false
EOF

if [ -f "$APP_DIR/remote-access-crew.desktop" ]; then
    rm -f "$APP_DIR/remote-access-crew.desktop"
fi

command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -f -t "$SHARE_DIR/icons/hicolor" 2>/dev/null || true
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APP_DIR" 2>/dev/null || true

echo "[4/4] Applying LAN-only + Tailscale UFW firewall rules for Remote Access Core..."
if command -v ufw >/dev/null 2>&1; then
    sudo ufw default deny incoming
    sudo ufw default allow outgoing

    for subnet in 192.168.0.0/16 10.0.0.0/8 172.16.0.0/12; do
        sudo ufw allow from "$subnet" to any port 47984,47989,47990,48010 proto tcp comment 'RAC Host TCP LAN' >/dev/null
        sudo ufw allow from "$subnet" to any port 47998,47999,48000,48002,48010 proto udp comment 'RAC Host UDP LAN' >/dev/null
        sudo ufw allow from "$subnet" to any port 48765 proto tcp comment 'RAC Bridge LAN' >/dev/null
    done

    sudo ufw allow in on tailscale0 to any port 47984,47989,47990,48010 proto tcp comment 'RAC Host TCP Tailscale' >/dev/null 2>&1 || true
    sudo ufw allow in on tailscale0 to any port 47998,47999,48000,48002,48010 proto udp comment 'RAC Host UDP Tailscale' >/dev/null 2>&1 || true
    sudo ufw allow in on tailscale0 to any port 48765 proto tcp comment 'RAC Bridge Tailscale' >/dev/null 2>&1 || true

    sudo ufw --force enable >/dev/null 2>&1 || true
    echo "  -> UFW active: Remote Access Core ports restricted strictly to RFC1918 LAN and tailscale0."
else
    echo "  WARNING: ufw not installed; ensure firewall restricts ports 47984-48010 and 48765 to LAN only."
fi

echo ""
echo "======================================================================"
echo "  Remote Access Core (RAC) Full Stack Verification:"
for b in rac-host rac-bridge rac rac-client; do
    if command -v "$b" >/dev/null 2>&1; then
        printf '    [OK] %-20s -> %s\n' "$b" "$(command -v "$b")"
    else
        printf '    [--] %-20s -> not found on PATH\n' "$b"
    fi
done
echo "  RAC Host LAN Dashboard: https://localhost:47990"
echo "  RAC Bridge pairing:     rac pair-code / rac pair <ip> <code>"
echo "======================================================================"
