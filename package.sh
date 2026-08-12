#!/usr/bin/env bash
# Builds a release and lays out a folder ready to tar and hand to someone.
# The Linux/macOS twin of package.ps1 -- see that file for why the
# settings files are deliberately *not* included.
#
#   ./package.sh            -> dist/Primitive-1.0.0
#   ./package.sh --tar      -> also dist/Primitive-1.0.0.tar.gz

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -n1)"
name="Primitive-$version"
target="$root/dist/$name"

echo "building $name (release)"
(cd "$root" && cargo build --release --workspace)

rm -rf "$target"
mkdir -p "$target"

for exe in primitive_client primitive_server; do
    cp "$root/target/release/$exe" "$target/"
done

# Assets sit next to the executable; `resolve_assets_dir` looks there
# first, so a packaged build finds them without any configuration.
cp -r "$root/assets" "$target/assets"
cp -r "$root/plugins" "$target/plugins"
cp "$root/README.md" "$root/CHANGELOG.md" "$root/LICENSE" "$target/"

echo "packaged $target ($(du -sh "$target" | cut -f1))"

if [ "${1:-}" = "--tar" ]; then
    archive="$root/dist/$name.tar.gz"
    tar -czf "$archive" -C "$root/dist" "$name"
    echo "wrote $archive"
fi
