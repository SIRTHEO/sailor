#!/bin/sh

set -u

root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 2
names=${SAILOR_PRIVATE_NAMES:-"$HOME/personal/.sailor-notes/private-names"}
ref=${1:?"a ref is required"}
base=${2:-}

[ -r "$names" ] || { echo "privacy[$ref]: cannot read the declared list"; exit 2; }
clean=$(mktemp "${TMPDIR:-/tmp}/sailor-private-names.XXXXXX") || exit 2
trap 'rm -f "$clean"' EXIT HUP INT TERM
awk 'NF && $1 !~ /^#/ { sub(/^[[:space:]]+/, ""); sub(/[[:space:]]+$/, ""); print }' "$names" > "$clean"

if [ -z "$base" ]; then
    if [ "$ref" = main ] || [ "$ref" = refs/heads/main ]; then
        base=$(git -C "$root" rev-parse origin/main 2>/dev/null) || base=""
    else
        base=$(git -C "$root" merge-base main "$ref" 2>/dev/null) || base=""
    fi
fi
range=${base:+$base..}$ref
git -C "$root" rev-parse --verify "$ref^{commit}" >/dev/null 2>&1 || { echo "privacy[$ref]: cannot read the ref"; exit 2; }
git -C "$root" rev-list --count "$range" >/dev/null 2>&1 || { echo "privacy[$ref]: cannot read its range"; exit 2; }

red=0
say() { echo "privacy[$ref]: $*"; }
refuse_paths() {
    count=$(printf '%s\n' "$2" | awk 'NF { count += 1 } END { print count + 0 }')
    [ "$count" -eq 0 ] && return
    say "$1: $count"
    printf '%s\n' "$2" | awk 'NF { print "  " $0 }'
    red=1
}

flows=$(git -C "$root" ls-tree -r --name-only "$ref" | grep -E '\.flow\.json$' | grep -v -E '^crates/flow/system/|^crates/[^/]+/(tests?|fixtures?)/|^docs/' || true)
refuse_paths "flows outside shipped folders" "$flows"
reserved=$(git -C "$root" ls-tree -r --name-only "$ref" | grep -E '(^|/)(\.sailor-notes|\.env|\.envrc|profiles-homes|\.claude/settings\.local\.json|\.codex|\.gemini)(/|$)' || true)
refuse_paths "reserved paths" "$reserved"
private=$(git -C "$root" grep -I -i -w -l -f "$clean" "$ref" -- . 2>/dev/null | awk -F: '{ print $2 }')
refuse_paths "tracked files with a private name" "$private"
private_messages=$(git -C "$root" log --format=%B "$range" | grep -i -w -c -f "$clean" || true)
[ "$private_messages" -eq 0 ] || { say "commit messages with a private name: $private_messages"; red=1; }
homes=$(git -C "$root" grep -I -l -F "$HOME" "$ref" -- . ':!**/*.lock' 2>/dev/null | awk -F: '{ print $2 }')
refuse_paths "tracked files with this machine's home path" "$homes"
home_messages=$(git -C "$root" log --format=%B "$range" | grep -F -c "$HOME" || true)
[ "$home_messages" -eq 0 ] || { say "commit messages with this machine's home path: $home_messages"; red=1; }
signatures=$(git -C "$root" log --format=%B "$range" | grep -E -c '^(Co-Authored-By|Claude-Session|Signed-off-by: .*(claude|codex|gemini))' || true)
[ "$signatures" -eq 0 ] || { say "tool-signature trailers: $signatures"; red=1; }
credentials=$(git -C "$root" diff --name-only -G'sk-ant-[A-Za-z0-9_-]{20,}|ghp_[A-Za-z0-9]{30,}|gho_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,}|AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY|xox[bap]-[A-Za-z0-9-]{20,}|AIza[0-9A-Za-z_-]{35}' "$range" -- . ':!**/*.lock')
non_test_credentials=$(printf '%s\n' "$credentials" | awk 'NF && $0 !~ /(^|\/)(tests?|fixtures?)\// && $0 !~ /test/ { print }')
refuse_paths "credential-shaped additions outside tests" "$non_test_credentials"

count=$(git -C "$root" rev-list --count "$range")
if [ "$red" -eq 0 ]; then
    say "clean: $count commit(s) may be pushed"
    exit 0
fi
say "refused: $count commit(s) cannot be pushed"
exit 1
