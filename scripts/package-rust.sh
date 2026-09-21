#!/usr/bin/env sh
set -eu
ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"
TARGET="${1:?target triple required}"
BINARY="${2:?binary path required}"
case "$TARGET" in
  x86_64-unknown-linux-gnu) NAME=smartaudio-linux-x86_64; EXT=tar.gz ;;
  aarch64-unknown-linux-gnu) NAME=smartaudio-linux-arm64; EXT=tar.gz ;;
  x86_64-pc-windows-gnu) NAME=smartaudio-windows-x86_64; EXT=zip ;;
  aarch64-pc-windows-gnullvm) NAME=smartaudio-windows-arm64; EXT=zip ;;
  x86_64-apple-darwin) NAME=smartaudio-macos-x86_64; EXT=zip ;;
  aarch64-apple-darwin) NAME=smartaudio-macos-arm64; EXT=zip ;;
  *) printf 'Unsupported release target: %s\n' "$TARGET" >&2; exit 1 ;;
esac
if [ ! -s "$BINARY" ]; then
  printf 'Missing or empty release binary: %s\n' "$BINARY" >&2
  exit 1
fi
mkdir -p dist package-tmp
STAGE="$(mktemp -d "$ROOT_DIR/package-tmp/release.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT HUP INT TERM
mkdir -p "$STAGE/binary"
cp "$BINARY" LICENSE NOTICE THIRD_PARTY_NOTICES.md "$STAGE/binary/"
cp -R THIRD_PARTY_LICENSES "$STAGE/binary/"
if [ "$EXT" = zip ]; then
  (cd "$STAGE/binary" && zip -q -r "$STAGE/$NAME.$EXT" .)
else
  tar -C "$STAGE/binary" -czf "$STAGE/$NAME.$EXT" .
fi
mv "$STAGE/$NAME.$EXT" "dist/$NAME.$EXT"
(cd dist && sha256sum "$NAME.$EXT" > "$NAME.$EXT.sha256")
