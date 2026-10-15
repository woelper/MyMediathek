# MyMediathek

A small desktop app for the German public broadcasters' media libraries (ARD, ZDF, arte, 3sat, KiKA, ORF, SRF, …). Search, download a programme with one click, and watch it offline in the built-in player. It's meant to be simple enough for kids to use, and it comes in German and English.

The search uses the [MediathekViewWeb](https://github.com/mediathekview/mediathekviewweb) API.

![icon](assets/icon-256.png)

## Features

- One search box. Each result is one line, and hovering it shows the full description.
- **Downloads** run in the background, and the files land in `~/Videos/MyMediathek`.
- **Favourites:** click the star to keep a programme for later.
- A built-in player with fullscreen, seeking and volume, so no other app is needed.
- Picture quality (high, medium, low) and language are set in the settings.

## Download

Ready-made builds for macOS (Apple Silicon), Windows and Linux are on the [releases page](https://github.com/woelper/MyMediathek/releases).

- **macOS and Windows:** ffmpeg is included.
- **Linux:** install ffmpeg from your distribution (`sudo apt install ffmpeg`), unpack the tarball and run `./install-linux.sh` to add MyMediathek to the app menu.

## Building

```sh
cargo run --release
```

You need `ffmpeg` and `ffprobe` on the PATH. The app runs them for playback and for HLS streams.

The tests run with `cargo test --release`. Tests that need the network or a GPU are ignored by default; run them with `cargo test --release -- --ignored`.

## Releasing

`./release.sh [patch|minor|major|X.Y.Z]` bumps the version and tags it, with release notes in the tag. Pushing the tag (`git push origin HEAD --follow-tags`) starts the workflow, which builds, signs and notarises the macOS app, packages all three platforms and publishes the release.

## Licences

The fonts are IBM Plex (SIL Open Font License, see `assets/fonts/LICENSE.txt`). The release builds bundle ffmpeg from [martin-riedl.de](https://ffmpeg.martin-riedl.de) (macOS) and [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds) (Windows). ffmpeg is licensed under the LGPL/GPL, and its source code is available at [ffmpeg.org](https://ffmpeg.org).
