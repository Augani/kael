#!/usr/bin/env bash
set -euo pipefail

if ! command -v apt-get >/dev/null 2>&1; then
  echo "scripts/ci/install-linux-deps.sh currently supports Debian/Ubuntu runners only." >&2
  exit 1
fi

sudo_cmd=()
if [[ "$(id -u)" -ne 0 ]]; then
  if ! command -v sudo >/dev/null 2>&1; then
    echo "sudo is required when not running as root." >&2
    exit 1
  fi
  sudo_cmd=(sudo)
fi

packages=(
  build-essential
  clang
  libasound2-dev
  libavcodec-dev
  libavdevice-dev
  libavfilter-dev
  libavformat-dev
  libavutil-dev
  libdbus-1-dev
  libdav1d-dev
  libegl1-mesa-dev
  libfontconfig1-dev
  libfreetype6-dev
  libgl1-mesa-dev
  libgles2-mesa-dev
  libgtk-4-dev
  libpipewire-0.3-dev
  libswresample-dev
  libswscale-dev
  libudev-dev
  libvulkan-dev
  libwayland-dev
  libwebkitgtk-6.0-dev
  libx11-dev
  libx11-xcb-dev
  libxcb-composite0-dev
  libxcb-damage0-dev
  libxcb-icccm4-dev
  libxcb-image0-dev
  libxcb-keysyms1-dev
  libxcb-randr0-dev
  libxcb-render-util0-dev
  libxcb-shape0-dev
  libxcb-util-dev
  libxcb-xfixes0-dev
  libxcb-xinput-dev
  libxcb-xkb-dev
  libxkbcommon-dev
  libxkbcommon-x11-dev
  libnotify-dev
  pkg-config
  wayland-protocols
)

"${sudo_cmd[@]}" apt-get update
"${sudo_cmd[@]}" apt-get install --yes --no-install-recommends "${packages[@]}"
