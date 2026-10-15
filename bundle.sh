#!/usr/bin/env bash
# Build MyMediathek.app (requires `cargo install cargo-bundle`).
#
# The player and HLS downloads run ffmpeg and ffprobe. An app started from
# Finder doesn't see Homebrew's PATH, so put static builds of both into
# Contents/Helpers, where the app looks first. Pass a directory holding them,
# or leave it out to bundle without (the app then falls back to Homebrew's).
#
# Usage: ./bundle.sh [dir-with-ffmpeg-and-ffprobe]
set -euo pipefail
cd "$(dirname "$0")"

cargo bundle --release

APP="target/release/bundle/osx/MyMediathek.app"
if [[ -n "${1:-}" ]]; then
  mkdir -p "$APP/Contents/Helpers"
  install -m 755 "$1/ffmpeg" "$1/ffprobe" "$APP/Contents/Helpers/"
fi

# ad-hoc, so a local build opens; the release workflow signs properly after this
codesign --force --deep -s - "$APP"

echo "bundled $APP"
