#!/usr/bin/env bash
# Install MyMediathek for the current user on Linux, plus a launcher and icon
# so it shows up in the app menu and the dock with its icon (on Wayland the
# compositor finds the icon through the desktop entry matching the window's
# app id, "mymediathek").
#
# From an unpacked release tarball, the whole folder (the app with its own
# ffmpeg) goes to ~/.local/share/mymediathek. From the repo, after
# `cargo build --release`, only the binary is installed and the system's
# ffmpeg is used.
set -euo pipefail
cd "$(dirname "$0")"

BIN_DIR="$HOME/.local/bin"
ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
APP_DIR="$HOME/.local/share/applications"
mkdir -p "$BIN_DIR" "$ICON_DIR" "$APP_DIR"

if [[ -x ./mymediathek && -x ./ffmpeg ]]; then
  DEST="$HOME/.local/share/mymediathek"
  rm -rf "$DEST"
  mkdir -p "$DEST"
  cp -a mymediathek ffmpeg ffprobe lib "$DEST/"
  EXE="$DEST/mymediathek"
  # A link, not a copy: the app finds its ffmpeg next to where it really lives.
  ln -sf "$EXE" "$BIN_DIR/mymediathek"
  ICON_SRC=mymediathek.png
  DESKTOP_SRC=mymediathek.desktop
elif [[ -x target/release/mymediathek ]]; then
  command -v ffmpeg >/dev/null || echo "warning: ffmpeg is not installed — install it (e.g. sudo apt install ffmpeg) to play and download videos" >&2
  EXE="$BIN_DIR/mymediathek"
  rm -f "$EXE"
  install -m 755 target/release/mymediathek "$EXE"
  ICON_SRC=assets/icon-256.png
  DESKTOP_SRC=assets/linux/mymediathek.desktop
else
  echo "error: mymediathek not found — run 'cargo build --release' first" >&2
  exit 1
fi

install -m 644 "$ICON_SRC" "$ICON_DIR/mymediathek.png"
# Absolute Exec path, so the launcher works even if ~/.local/bin isn't in PATH.
sed "s|^Exec=.*|Exec=$EXE|" "$DESKTOP_SRC" > "$APP_DIR/mymediathek.desktop"
chmod 644 "$APP_DIR/mymediathek.desktop"
echo "installed — MyMediathek is in your app menu"
