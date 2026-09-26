#!/usr/bin/env bash
# Publishes the current build to itch.io with butler.
#
#   ./publish-itch.sh                 build what this machine can, then push
#   ./publish-itch.sh --dry-run       build and show what would be pushed
#   ./publish-itch.sh --no-build      push what is already in dist/
#   ./publish-itch.sh --only android  one channel (windows|linux|android)
#   ./publish-itch.sh --skip-checks   skip clippy and the test suite
#
# The itch target (user/game) is read from `.itch-target` beside this
# script, or from $ITCH_TARGET. It is not hardcoded: a typo in a target
# does not fail -- it silently creates a *new* page on the account, and
# the first anybody knows of it is a release nobody can find.
#
# **Why butler rather than the upload form.** It sends only the blocks
# that changed (a 16 MB APK where the assets did not move is a few
# hundred kilobytes on the wire), it keeps a channel's history so a bad
# build can be rolled back from the dashboard, and it records the version
# the player sees. Dragging a zip into the browser does none of that and
# has no record of which binary is live.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$root"

build=1
dry=0
checks=1
only=""

while [ $# -gt 0 ]; do
    case "$1" in
        --dry-run) dry=1 ;;
        --no-build) build=0 ;;
        --skip-checks) checks=0 ;;
        --only) shift; only="${1:-}" ;;
        -h|--help) sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
target="${ITCH_TARGET:-}"
if [ -z "$target" ] && [ -f .itch-target ]; then
    target="$(tr -d ' \r\n' < .itch-target)"
fi

say() { printf '\n== %s\n' "$*"; }
die() { printf 'publish-itch: %s\n' "$*" >&2; exit 1; }

# --- what is needed before anything is built -------------------------

command -v butler >/dev/null 2>&1 || die "butler is not on PATH.
    Download it from https://itch.io/docs/butler/installing.html, put
    butler.exe somewhere on PATH, and run 'butler login' once."

[ -n "$target" ] || die "no itch target.
    Write it into .itch-target beside this script (one line, e.g.
    yourname/primitive) or set ITCH_TARGET. A wrong target does not
    fail -- it creates a new page nobody is looking at."

butler status "$target" >/dev/null 2>&1 || die "butler cannot see $target.
    Run 'butler login' (a browser window asks for the key), or check the
    target: it is the URL of the page, without https://itch.io/."

# **Refuse to publish a tree that is not committed.** A build pushed from
# a dirty tree cannot be pointed at a commit afterwards, and the question
# "which source is this binary?" always gets asked eventually.
if [ -n "$(git status --porcelain 2>/dev/null)" ]; then
    die "the working tree has uncommitted changes. Commit them, or
    publish from a clean checkout."
fi

say "Primitive $version -> $target ($(git rev-parse --short HEAD))"

# --- the checks the house rule asks for ------------------------------

if [ "$checks" = 1 ]; then
    say "clippy"
    cargo clippy --workspace --all-targets 2>&1 | grep -E '^(warning|error)' && \
        die "clippy is not silent. It has been silent for the whole
    history of this repository; a release is not where that changes."
    say "tests (this takes a few minutes)"
    cargo test --workspace --no-fail-fast -- --test-threads=4 >/dev/null || \
        die "the test suite is red. Every release must pass the
    scenarios -- see CLAUDE.md."
fi

# --- build ------------------------------------------------------------

wants() { [ -z "$only" ] || [ "$only" = "$1" ]; }

if [ "$build" = 1 ]; then
    if wants windows && command -v powershell.exe >/dev/null 2>&1; then
        say "windows"
        powershell.exe -NoProfile -ExecutionPolicy Bypass -File ./package.ps1 -Zip
    fi
    if wants linux && [ "$(uname -s)" != "MINGW"* ]; then
        # Only where this machine can actually produce a Linux binary.
        # Cross-compiling from Windows needs a toolchain this script does
        # not install, and a half-built archive is worse than none.
        if cargo build --release -p primitive_client --target x86_64-unknown-linux-gnu 2>/dev/null; then
            say "linux"
            ./package.sh --tar
        else
            echo "skipping linux: no x86_64-unknown-linux-gnu toolchain here"
        fi
    fi
    if wants android; then
        say "android"
        ./package-android.sh
    fi
fi

# --- push -------------------------------------------------------------

# channel -> the file or folder butler sends. A channel name carrying a
# platform word is what makes itch offer the right download to the right
# visitor and what makes the itch app able to install it at all.
push() {
    local channel="$1" path="$2"
    wants "${channel%%-*}" || return 0
    [ -e "$path" ] || { echo "skipping $channel: no $path"; return 0; }
    if [ "$dry" = 1 ]; then
        echo "would push $path -> $target:$channel (version $version)"
        return 0
    fi
    say "pushing $channel"
    butler push "$path" "$target:$channel" --userversion "$version"
}

# The zip and the tarball are sent *unpacked* where there is a folder to
# send: butler diffs files, and a zip is one opaque file that changes
# entirely when one byte inside it does. The APK has no folder form, so
# it goes as the file it is.
push windows "dist/Primitive-$version"
push linux   "dist/primitive_linux64.tar.gz"
push android "dist/primitive-$version-android-arm64.apk"

if [ "$dry" = 0 ]; then
    say "done"
    butler status "$target"
    echo
    echo "The page itself -- description, screenshots, the cover -- is not"
    echo "butler's business and stays on itch.io/dashboard."
fi
