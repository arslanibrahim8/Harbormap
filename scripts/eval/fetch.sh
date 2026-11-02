#!/usr/bin/env bash
# Fetches the pinned public evaluation repositories listed in tests/eval/repos.toml.
# The fetched trees are untrusted data: this script only runs git against them.
# It never runs npm, a build, or any other code from the fetched trees.
#
# Usage: scripts/eval/fetch.sh <dest>
# Idempotent: a repository already at <dest>/<name> whose HEAD is the pinned commit
# is left alone. A checkout at a different commit is an error, not a silent skip.
# A fresh clone is built in a hidden sibling folder and moved into place only
# after it checked out the pinned commit, so a failed fetch leaves no repository.
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <dest>" >&2
  exit 2
fi
dest=$1
root=$(cd "$(dirname "$0")/../.." && pwd)
manifest="$root/tests/eval/repos.toml"
tmp=""

cleanup() {
  if [ -n "$tmp" ] && [ -e "$tmp" ]; then
    rm -rf -- "$tmp"
  fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

mkdir -p "$dest"

# Line-based parse: one "name url commit" line per [[repo]] table.
# Read from process substitution, not a pipe, so `exit` below ends the script.
while read -r name url commit; do
  repo="$dest/$name"

  if [ -e "$repo" ]; then
    head_sha=""
    if [ -e "$repo/.git" ]; then
      head_sha=$(git -C "$repo" rev-parse --verify -q 'HEAD^{commit}' 2>/dev/null) || head_sha=""
    fi
    if [ "$head_sha" != "$commit" ]; then
      echo "fetch.sh: $name is at ${head_sha:-nothing}, expected $commit; delete $repo and run again" >&2
      exit 1
    fi
  else
    tmp="$dest/.$name.tmp.$$"
    rm -rf -- "$tmp"
    mkdir -p "$tmp"
    git -C "$tmp" init -q
    git -C "$tmp" fetch -q --depth 1 "$url" "$commit" </dev/null
    git -C "$tmp" checkout -q FETCH_HEAD
    got=$(git -C "$tmp" rev-parse --verify 'HEAD^{commit}')
    if [ "$got" != "$commit" ]; then
      echo "fetch.sh: $name checked out $got, expected $commit" >&2
      exit 1
    fi
    mv "$tmp" "$repo"
    tmp=""
  fi

  sha=$(git -C "$repo" rev-parse --verify 'HEAD^{commit}')
  echo "$name $sha"
done < <(awk -F' *= *' '
  /^\[\[repo\]\]/ { if (name != "") print name, url, commit; name = url = commit = "" }
  /^name *=/      { gsub(/"/, "", $2); name = $2 }
  /^url *=/       { gsub(/"/, "", $2); url = $2 }
  /^commit *=/    { gsub(/"/, "", $2); commit = $2 }
  END { if (name != "") print name, url, commit }
' "$manifest")
