#!/bin/sh

# Records on the forge that the integration flow is about to merge this exact
# commit, as the account this tree declares. The trunk requires the status, so
# a merge made outside the flow has nothing to show for it.

set -u

root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 2
ref=${1:?"a ref is required"}
run=${2:-}
say() { echo "integrated[$ref]: $*"; }
[ "$#" -eq 2 ] || { say "a commit and the run that integrates it, nothing else"; exit 2; }
[ -n "$run" ] || { say "no run names this integration: nothing is posted"; exit 2; }

sha=$(git -C "$root" rev-parse --verify "$ref^{commit}" 2>/dev/null) || { say "cannot read the ref"; exit 2; }
account=$(git -C "$root" config --get sailor.forgeAs) || { say "sailor.forgeAs is not declared"; exit 2; }
url=$(git -C "$root" remote get-url origin 2>/dev/null) || { say "no origin to post to"; exit 2; }
slug=$(printf '%s\n' "$url" | sed -E 's#^(https://github\.com/|git@github\.com:)##; s#\.git$##')
case "$slug" in
    */*) ;;
    *) say "origin is not a GitHub repository"; exit 2 ;;
esac

description="run $run; verdict on $(printf '%.12s' "$sha")"
token=$(gh auth token --user "$account") || { say "no token for the tree's account"; exit 2; }
GH_TOKEN="$token" gh api -X POST "repos/$slug/statuses/$sha" \
    -f state=success -f context=sailor/integrated -f description="$description" >/dev/null \
    || { say "the forge refused the status, so nothing is merged"; exit 2; }
say "posted on $sha: $description"
