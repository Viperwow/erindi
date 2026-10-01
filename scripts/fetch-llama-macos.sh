#!/bin/sh
# Downloads llama-server for macOS into apps/desktop/src-tauri/llama/, verifies its SHA-256
# and signs it ad hoc, so the bundle carries it under Contents/Resources/llama/.
# Usage: scripts/fetch-llama-macos.sh <arm64|x64>
set -eu

arch="${1:?usage: $0 <arm64|x64>}"
case "$arch" in
  arm64) sha=67a1b119fa4b495a1f17f5b3950de24fd0bfeecce73b1619559f85ca1e740bac ;;
  x64) sha=a46abc97f47d23e6bfa8e42a250f1f42afb883bb38dd3c3534e9859e9f81d3de ;;
  *) echo "unknown arch: $arch" >&2; exit 1 ;;
esac

root="$(cd "$(dirname "$0")/.." && pwd)"
dest="$root/apps/desktop/src-tauri/llama"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

archive="$tmp/llama.tar.gz"
curl -fsSL -o "$archive" "https://github.com/ggml-org/llama.cpp/releases/download/b11095/llama-b11095-bin-macos-$arch.tar.gz"
echo "$sha  $archive" | shasum -a 256 -c - >/dev/null || { echo "llama.cpp: SHA-256 mismatch" >&2; exit 1; }
tar xzf "$archive" -C "$tmp"

# llama-server loads its libraries through @loader_path by their .0 names; the other
# names in the archive are symlinks, and app bundles copy them as duplicate files.
rm -rf "$dest"
mkdir -p "$dest"
src="$tmp/llama-b11095"
cp "$src/llama-server" "$src/libllama-server-impl.dylib" "$src/LICENSE" "$dest/"
for lib in "$src"/lib*.0.dylib; do
  if [ -L "$lib" ]; then cp -L "$lib" "$dest/"; fi
done
for bin in "$dest/llama-server" "$dest"/*.dylib; do
  codesign -s - -f "$bin" 2>/dev/null
done
# An Intel build cannot start on Apple Silicon without Rosetta.
case "$arch-$(uname -m)" in
  arm64-arm64 | x64-x86_64)
    "$dest/llama-server" --version >/dev/null 2>&1 || { echo "llama-server does not start" >&2; exit 1; } ;;
esac
echo "llama-server ($arch) in $dest"
