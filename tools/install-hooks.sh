#!/usr/bin/env bash
# Copies this repository's git hooks into `.git/hooks`.
#
# Hooks cannot be version-controlled where git looks for them, so the
# originals live in `tools/` and this script puts them where they run.
# It is safe to run twice.
#
#   ./tools/install-hooks.sh

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
hooks="$(git -C "$root" rev-parse --git-common-dir)/hooks"
mkdir -p "$hooks"

for hook in pre-commit; do
    cp "$root/tools/$hook" "$hooks/$hook"
    chmod +x "$hooks/$hook"
    echo "installed $hook"
done

# A worktree shares `.git/hooks` with the repository it was made from
# (`--git-common-dir` above is that shared directory), so an agent
# working in one is covered by this without installing anything itself.
echo "hooks live in $hooks"
