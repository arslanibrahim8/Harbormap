#!/usr/bin/env bash
# Fetches the pinned public evaluation repositories listed in tests/eval/repos.toml.
# The fetched trees are untrusted data: this script only runs git against them.
# It never runs npm, a build, or any other code from the fetched trees.
#
# Usage: scripts/eval/fetch.sh <dest>
# Idempotent: a repository whose <dest>/<name>/.git already exists is left alone.
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <dest>" >&2
  exit 2
fi
dest=$1
root=$(cd "$(dirname "$0")/../.." && pwd)
manifest="$root/tests/eval/repos.toml"

# Line-based parse: one "name url commit" line per [[repo]] table.
awk -F' *= *' '
  /^\[\[repo\]\]/ { if (name != "") print name, url, commit; name = url = commit = "" }
  /^name *=/      { gsub(/"/, "", $2); name = $2 }
  /^url *=/       { gsub(/"/, "", $2); url = $2 }
  /^commit *=/    { gsub(/"/, "", $2); commit = $2 }
  END { if (name != "") print name, url, commit }
' "$manifest" | while read -r name url commit; do
  repo="$dest/$name"
  mkdir -p "$repo"
  if [ ! -e "$repo/.git" ]; then
    git -C "$repo" init -q
    git -C "$repo" fetch -q --depth 1 "$url" "$commit"
    git -C "$repo" checkout -q FETCH_HEAD
  fi
  echo "$name $(git -C "$repo" rev-parse HEAD)"
done
