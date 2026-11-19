#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# Unified Linux Hardware, Wi-Fi, NVIDIA GTX 1650, Audio & Desktop Setup
# Supports: Ubuntu Server / Desktop 24.04 LTS, Debian, Omarchy 4.5.0 / Arch
# ==============================================================================

echo "======================================================================"
echo "  Linux Hardware Driver & Core System Setup (Wi-Fi + NVIDIA GTX 1650)"
echo "======================================================================"

if [ -r /etc/os-release ]; then
    # shellcheck disable=SC1091
    . /etc/os-release
else
    echo "Error: cannot read /etc/os-release" >&2
    exit 1
fi

OS_ID="${ID:-unknown}"
HAS_NVIDIA=0
if command -v lspci >/dev/null 2>&1 && lspci -nn | grep -iE 'vga|3d|display' | grep -qi nvidia; then
    HAS_NVIDIA=1
fi

case "$OS_ID" in
    ubuntu|debian)
        echo "[1/4] Detected ${PRETTY_NAME:-Ubuntu/Debian}. Installing firmware, Wi-Fi, audio & headers..."
        sudo apt-get update
        sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
            linux-firmware \
            linux-headers-generic \
            "linux-headers-$(uname -r)" \
            dkms \
            build-essential \
            pkg-config \
            pciutils \
            usbutils \
            rfkill \
            iw \
            wireless-tools \
            wpasupplicant \
            network-manager \
            bluez \
            pipewire \
            pipewire-pulse \
            wireplumber \
            wl-clipboard \
            xclip \
            xsel \
            mesa-utils \
            vulkan-tools \
            vainfo \
            vdpauinfo \
            libva-drm2 \
            libva-x11-2 \
            libva-wayland2 \
            htop \
            jq \
            unzip \
            xdg-utils \
            ca-certificates \
            curl \
            git || true

        if lspci -nn | grep -iE 'vga|3d|display' | grep -qi nvidia; then
            HAS_NVIDIA=1
        fi

        sudo rfkill unblock wifi all 2>/dev/null || true

        WIFI_PCI="$(lspci -nn | grep -iE 'network|wireless|wi-fi|802\.11' || true)"
        echo "  Detected Wi-Fi hardware:"
        if [ -n "$WIFI_PCI" ]; then
            printf '    %s\n' "$WIFI_PCI"
        else
            echo "    (none detected on PCIe; USB Wi-Fi adapters also supported via linux-firmware)"
        fi
        if echo "$WIFI_PCI" | grep -qi '14e4:'; then
            echo "  -> Broadcom Wi-Fi detected; installing broadcom-sta-dkms..."
            sudo DEBIAN_FRONTEND=noninteractive apt-get install -y broadcom-sta-dkms || true
        fi
        if echo "$WIFI_PCI" | grep -qi '10ec:c821'; then
            echo "  -> Realtek RTL8821CE detected; installing rtl8821ce-dkms..."
            sudo DEBIAN_FRONTEND=noninteractive apt-get install -y rtl8821ce-dkms || true
        fi

        if [ -d /etc/netplan ] && ! grep -rq "renderer: NetworkManager" /etc/netplan 2>/dev/null; then
            sudo tee /etc/netplan/01-network-manager-all.yaml >/dev/null << 'EOF'
network:
  version: 2
  renderer: NetworkManager
EOF
            sudo chmod 600 /etc/netplan/01-network-manager-all.yaml
            sudo netplan apply 2>/dev/null || true
        fi

        sudo mkdir -p /etc/NetworkManager/conf.d
        sudo tee /etc/NetworkManager/conf.d/default-wifi-powersave-off.conf >/dev/null << 'EOF'
[connection]
wifi.powersave = 2
EOF
        sudo systemctl enable --now NetworkManager 2>/dev/null || true

        if ! systemctl list-unit-files 2>/dev/null | grep -qE '^(lightdm|gdm3|sddm)\.service'; then
            echo "[2/4] Ubuntu Server detected without GUI. Installing lightweight XFCE4 + LightDM desktop..."
            sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
                xfce4 \
                xfce4-goodies \
                xfce4-terminal \
                lightdm \
                network-manager-gnome \
                pavucontrol \
                dbus-x11 \
                firefox
            sudo systemctl enable lightdm || true
        else
            echo "[2/4] Graphical desktop manager already present; keeping existing desktop."
        fi

        if [ "$HAS_NVIDIA" -eq 1 ]; then
            echo "[3/4] NVIDIA GPU detected (GTX 1650 / Turing compatible). Installing NVIDIA driver stack..."
            sudo DEBIAN_FRONTEND=noninteractive apt-get install -y ubuntu-drivers-common || true
            NV_INSTALLED=0
            for pkg in nvidia-driver-570-open nvidia-driver-570 nvidia-driver-550-open nvidia-driver-550; do
                if apt-cache show "$pkg" >/dev/null 2>&1; then
                    echo "  -> Installing $pkg (with NVENC/NVDEC & nvidia-prime)..."
                    if sudo DEBIAN_FRONTEND=noninteractive apt-get install -y "$pkg" nvidia-settings nvidia-prime; then
                        NV_INSTALLED=1
                        break
                    fi
                fi
            done
            if [ "$NV_INSTALLED" -eq 0 ]; then
                echo "  -> Falling back to ubuntu-drivers install..."
                sudo ubuntu-drivers install || true
            fi

            # Enable KMS modesetting & fbdev for smooth desktop + RAC Host KMS capture
            sudo tee /etc/modprobe.d/nvidia-drm.conf >/dev/null << 'EOF'
options nvidia-drm modeset=1 fbdev=1
EOF
            sudo update-initramfs -u || true
        else
            echo "[3/4] No NVIDIA GPU detected on this machine (skipping NVIDIA driver install)."
        fi
        ;;

    arch|omarchy)
        echo "[1/4] Detected ${PRETTY_NAME:-Arch/Omarchy}. Installing firmware, Wi-Fi, audio & kernel headers..."
        sudo pacman -Sy --noconfirm

        HEADER_PKGS=()
        pacman -Q linux >/dev/null 2>&1 && HEADER_PKGS+=(linux-headers)
        pacman -Q linux-lts >/dev/null 2>&1 && HEADER_PKGS+=(linux-lts-headers)
        pacman -Q linux-zen >/dev/null 2>&1 && HEADER_PKGS+=(linux-zen-headers)
        pacman -Q linux-hardened >/dev/null 2>&1 && HEADER_PKGS+=(linux-hardened-headers)
        if [ "${#HEADER_PKGS[@]}" -eq 0 ]; then
            HEADER_PKGS+=(linux-headers)
        fi

        sudo pacman -S --needed --noconfirm \
            linux-firmware \
            "${HEADER_PKGS[@]}" \
            dkms \
            base-devel \
            pciutils \
            usbutils \
            rfkill \
            iw \
            wireless_tools \
            pipewire \
            pipewire-pulse \
            wireplumber \
            wl-clipboard \
            xclip \
            mesa-utils \
            vulkan-tools \
            libva-utils \
            htop \
            jq \
            unzip \
            xdg-utils

        if lspci -nn | grep -iE 'vga|3d|display' | grep -qi nvidia; then
            HAS_NVIDIA=1
        fi

        sudo rfkill unblock wifi all 2>/dev/null || true
        if systemctl is-active --quiet iwd 2>/dev/null; then
            echo "[2/4] Omarchy/Arch iwd wireless daemon is active; preserving native iwd stack."
        else
            echo "[2/4] Ensuring NetworkManager is installed and active for Wi-Fi..."
            sudo pacman -S --needed --noconfirm networkmanager wpa_supplicant
            sudo mkdir -p /etc/NetworkManager/conf.d
            sudo tee /etc/NetworkManager/conf.d/default-wifi-powersave-off.conf >/dev/null << 'EOF'
[connection]
wifi.powersave = 2
EOF
            sudo systemctl enable --now NetworkManager 2>/dev/null || true
        fi

        if [ "$HAS_NVIDIA" -eq 1 ]; then
            echo "[3/4] NVIDIA GPU detected (GTX 1650 / Turing compatible). Installing nvidia-open-dkms & Wayland stack..."
            NV_PKGS=(nvidia-open-dkms nvidia-utils nvidia-settings libva-nvidia-driver egl-wayland vulkan-icd-loader)
            if grep -qE '^\[multilib\]' /etc/pacman.conf 2>/dev/null; then
                NV_PKGS+=(lib32-nvidia-utils)
            fi
            sudo pacman -S --needed --noconfirm "${NV_PKGS[@]}"

            sudo tee /etc/modprobe.d/nvidia-drm.conf >/dev/null << 'EOF'
options nvidia_drm modeset=1 fbdev=1
EOF
            sudo mkdir -p /etc/environment.d
            sudo tee /etc/environment.d/90-nvidia-wayland.conf >/dev/null << 'EOF'
LIBVA_DRIVER_NAME=nvidia
NVD_BACKEND=direct
GBM_BACKEND=nvidia-drm
__GLX_VENDOR_LIBRARY_NAME=nvidia
EOF

            if command -v limine-mkinitcpio >/dev/null 2>&1; then
                sudo limine-mkinitcpio || true
            elif command -v mkinitcpio >/dev/null 2>&1; then
                sudo mkinitcpio -P || true
            fi
        else
            echo "[3/4] No NVIDIA GPU detected on this machine (skipping NVIDIA driver install)."
        fi
        ;;

    *)
        echo "[1/4] Generic Linux (${PRETTY_NAME:-unknown}) detected. Unblocking Wi-Fi radios..."
        sudo rfkill unblock wifi all 2>/dev/null || true
        ;;
esac

echo "[4/4] Hardware verification summary:"
echo "  Wi-Fi PCI devices & bound drivers:"
lspci -nnk | grep -A3 -iE 'network|wireless|wi-fi|802\.11' || echo "    (none on PCIe)"
echo "  RFKill status:"
rfkill list 2>/dev/null || true
echo "  GPU devices & bound drivers:"
lspci -nnk | grep -A3 -iE 'vga|3d|display' || true
if command -v nvidia-smi >/dev/null 2>&1; then
    nvidia-smi --query-gpu=name,driver_version --format=csv,noheader 2>/dev/null || \
        echo "  NOTE: NVIDIA driver installed; reboot required to load the kernel module."
fi
echo "======================================================================"
echo "  Hardware Driver Setup Complete."
echo "======================================================================"
