#!/usr/bin/env bash
# Install MyMediathek for the current user on Linux: the binary into
# ~/.local/bin, plus a launcher and icon so it shows up in the app menu and
# the dock with its icon (on Wayland the compositor finds the icon through
# the desktop entry matching the window's app id, "mymediathek").
#
# Run from the repo after `cargo build --release`, or from an unpacked
# release tarball (binary next to this script). Needs ffmpeg installed.
set -euo pipefail
cd "$(dirname "$0")"

if [[ -x target/release/mymediathek ]]; then
  BIN_SRC=target/release
  ICON_SRC=assets/icon-256.png
  DESKTOP_SRC=assets/linux/mymediathek.desktop
elif [[ -x ./mymediathek ]]; then
  BIN_SRC=.
  ICON_SRC=mymediathek.png
  DESKTOP_SRC=mymediathek.desktop
else
  echo "error: mymediathek not found — run 'cargo build --release' first" >&2
  exit 1
fi

command -v ffmpeg >/dev/null || echo "warning: ffmpeg is not installed — install it (e.g. sudo apt install ffmpeg) to play and download videos" >&2

BIN_DIR="$HOME/.local/bin"
ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
APP_DIR="$HOME/.local/share/applications"
mkdir -p "$BIN_DIR" "$ICON_DIR" "$APP_DIR"

install -m 755 "$BIN_SRC/mymediathek" "$BIN_DIR/mymediathek"
install -m 644 "$ICON_SRC" "$ICON_DIR/mymediathek.png"
# Absolute Exec path, so the launcher works even if ~/.local/bin isn't in PATH.
sed "s|^Exec=.*|Exec=$BIN_DIR/mymediathek|" "$DESKTOP_SRC" > "$APP_DIR/mymediathek.desktop"
chmod 644 "$APP_DIR/mymediathek.desktop"
echo "installed — MyMediathek is in your app menu"
