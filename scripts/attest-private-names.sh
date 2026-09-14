#!/bin/sh

# Runs the armed privacy check on one commit and records its verdict on the
# forge as the account this tree declares, so a check without the list can
# trust it.

set -u

root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 2
# The gate is the one beside this script, never the candidate's own copy.
gate=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/privacy-scan.sh
ref=${1:?"a ref is required"}
base=${2:-}
say() { echo "attest[$ref]: $*"; }

sha=$(git -C "$root" rev-parse --verify "$ref^{commit}" 2>/dev/null) || { say "cannot read the ref"; exit 2; }
account=$(git -C "$root" config --get sailor.forgeAs) || { say "sailor.forgeAs is not declared"; exit 2; }
url=$(git -C "$root" remote get-url origin 2>/dev/null) || { say "no origin to attest to"; exit 2; }
slug=$(printf '%s\n' "$url" | sed -E 's#^(https://github\.com/|git@github\.com:)##; s#\.git$##')
case "$slug" in
    */*) ;;
    *) say "origin is not a GitHub repository"; exit 2 ;;
esac

"$gate" "$ref" ${base:+"$base"}
verdict=$?
[ "$(git -C "$root" rev-parse --verify "$ref^{commit}" 2>/dev/null)" = "$sha" ] || { say "the ref moved during the check"; exit 2; }
case "$verdict" in
    0) state=success; description="no declared private name, home path or workshop shape" ;;
    1) state=failure; description="the armed check refused this commit" ;;
    *) say "nothing attested: the check could not run"; exit 2 ;;
esac

token=$(gh auth token --user "$account") || { say "no token for the tree's account"; exit 2; }
GH_TOKEN="$token" gh api -X POST "repos/$slug/statuses/$sha" \
    -f state="$state" -f context=sailor/private-names -f description="$description" >/dev/null \
    || { say "the forge refused the status"; exit 2; }
say "$state on $sha"
[ "$state" = success ]
