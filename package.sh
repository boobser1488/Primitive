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
# Two invocations, not one `--workspace`: cargo unifies features across a
# single build, and the client asks for the server without its `plugins`
# feature precisely so the scripting engine stays out of the game binary.
# Built together, the union wins and the client ships rhai anyway.
(cd "$root" && cargo build --release -p primitive_client)
(cd "$root" && cargo build --release -p primitive_server)

rm -rf "$target"
mkdir -p "$target"

for exe in primitive_client primitive_server; do
    cp "$root/target/release/$exe" "$target/"
done

# Assets sit next to the executable; `resolve_assets_dir` looks there
# first, so a packaged build finds them without any configuration.
cp -r "$root/assets" "$target/assets"
cp -r "$root/plugins" "$target/plugins"
cp "$root/GUIDE.md" "$root/README.md" "$root/CHANGELOG.md" "$root/LICENSE" "$target/"

echo "packaged $target ($(du -sh "$target" | cut -f1))"

if [ "${1:-}" = "--tar" ]; then
    archive="$root/dist/$name.tar.gz"
    tar -czf "$archive" -C "$root/dist" "$name"
    echo "wrote $archive"
fi
