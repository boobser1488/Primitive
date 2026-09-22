#!/usr/bin/env bash
# Builds a release and lays out a folder ready to tar and hand to someone.
# The Linux/macOS twin of package.ps1 -- see that file for why the
# settings files are deliberately *not* included.
#
#   ./package.sh            -> dist/Primitive-<version>
#   ./package.sh --tar      -> also dist/primitive_linux64.tar.gz

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

# No `assets` folder and no README or CHANGELOG: the assets are compiled
# into the client, and a loose copy would both double them and override
# the next release's -- see package.ps1 for the whole argument.
cp -r "$root/plugins" "$target/plugins"
cp "$root/GUIDE.md" "$root/LICENSE" "$root/LICENSE-APACHE" "$root/LICENSE-ASSETS" "$root/NOTICE" "$target/"

echo "packaged $target ($(du -sh "$target" | cut -f1))"

if [ "${1:-}" = "--tar" ]; then
    # Named by platform rather than by version, exactly as the Windows
    # script names its zip: this is the file people link to and
    # download, and a name that changes every release breaks every link
    # to it. The version is inside, in the folder name and in the game's
    # own menu.
    archive="$root/dist/primitive_linux64.tar.gz"
    rm -f "$archive"
    # The folder goes in with it, so extracting does not scatter eight
    # files into whatever directory the user happened to be in.
    tar -czf "$archive" -C "$root/dist" "$name"
    echo "wrote $archive ($(du -h "$archive" | cut -f1))"
fi
