#!/bin/sh

set -u

root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 2
names=${SAILOR_PRIVATE_NAMES:-"$HOME/personal/.sailor-notes/private-names"}

# What a workshop leaves in a sentence and no list is needed to recognise: a
# home path of any machine, the loopback, a process number, a session id, a tty.
hard_shapes='/(Users|home)/[A-Za-z0-9._-]+/|127\.0\.0\.1|(^|[^A-Za-z0-9_])pid:? ?[0-9]{3,}|[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|ttys[0-9]{3}([^0-9]|$)'

mode=ref
case "${1:-}" in
    --text) mode=text; shift ;;
    --shapes-only) mode=shapes; shift ;;
esac

clean=""
arm() {
    [ -r "$names" ] || { echo "privacy[$label]: cannot read the declared list"; exit 2; }
    clean=$(mktemp "${TMPDIR:-/tmp}/sailor-private-names.XXXXXX") || exit 2
    trap 'rm -f "$clean"' EXIT HUP INT TERM
    awk 'NF && $1 !~ /^#/ { sub(/^[[:space:]]+/, ""); sub(/[[:space:]]+$/, ""); print }' "$names" > "$clean"
}

red=0
say() { echo "privacy[$label]: $*"; }

if [ "$mode" = text ]; then
    text=${1:?"a text file is required"}
    label=text
    [ -r "$text" ] || { say "cannot read the text"; exit 2; }
    arm
    if [ -s "$clean" ] && grep -i -w -q -f "$clean" "$text"; then say "a private name"; red=1; fi
    if [ -n "${HOME:-}" ] && grep -F -q "$HOME" "$text"; then say "this machine's home path"; red=1; fi
    if grep -E -q "$hard_shapes" "$text"; then say "a path, address, process or session of a machine"; red=1; fi
    if grep -i -q 'this machine' "$text"; then say "a passage about the machine it was written on"; red=1; fi
    if [ "$red" -eq 0 ]; then
        say "clean: the text may be published"
        exit 0
    fi
    say "refused: the text cannot be published"
    exit 1
fi

ref=${1:?"a ref is required"}
base=${2:-}
label=$ref
[ "$mode" = ref ] && arm

if [ -z "$base" ]; then
    if [ "$ref" = main ] || [ "$ref" = refs/heads/main ]; then
        base=$(git -C "$root" rev-parse origin/main 2>/dev/null) || base=""
    else
        base=$(git -C "$root" merge-base main "$ref" 2>/dev/null) || base=""
    fi
fi
range=${base:+$base..}$ref
git -C "$root" rev-parse --verify "$ref^{commit}" >/dev/null 2>&1 || { say "cannot read the ref"; exit 2; }
git -C "$root" rev-list --count "$range" >/dev/null 2>&1 || { say "cannot read its range"; exit 2; }

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
if [ "$mode" = ref ]; then
    private=""
    [ -s "$clean" ] && private=$(git -C "$root" grep -I -i -w -l -f "$clean" "$ref" -- . 2>/dev/null | awk -F: '{ print $2 }')
    refuse_paths "tracked files with a private name" "$private"
    private_messages=0
    [ -s "$clean" ] && private_messages=$(git -C "$root" log --format=%B "$range" | grep -i -w -c -f "$clean" || true)
    [ "$private_messages" -eq 0 ] || { say "commit messages with a private name: $private_messages"; red=1; }
    homes=$(git -C "$root" grep -I -l -F "$HOME" "$ref" -- . ':!**/*.lock' 2>/dev/null | awk -F: '{ print $2 }')
    refuse_paths "tracked files with this machine's home path" "$homes"
    home_messages=$(git -C "$root" log --format=%B "$range" | grep -F -c "$HOME" || true)
    [ "$home_messages" -eq 0 ] || { say "commit messages with this machine's home path: $home_messages"; red=1; }
fi
shaped_messages=$(git -C "$root" log --format=%B "$range" | grep -E -c "$hard_shapes" || true)
[ "$shaped_messages" -eq 0 ] || { say "commit messages with a machine's path, address, process or session: $shaped_messages"; red=1; }
signatures=$(git -C "$root" log --format=%B "$range" | grep -E -c '^(Co-Authored-By|Claude-Session|Signed-off-by: .*(claude|codex|gemini))' || true)
[ "$signatures" -eq 0 ] || { say "tool-signature trailers: $signatures"; red=1; }
credentials=$(git -C "$root" diff --name-only -G'sk-ant-[A-Za-z0-9_-]{20,}|ghp_[A-Za-z0-9]{30,}|gho_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,}|AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY|xox[bap]-[A-Za-z0-9-]{20,}|AIza[0-9A-Za-z_-]{35}' "$range" -- . ':!**/*.lock')
non_test_credentials=$(printf '%s\n' "$credentials" | awk 'NF && $0 !~ /(^|\/)(tests?|fixtures?)\// && $0 !~ /test/ { print }')
refuse_paths "credential-shaped additions outside tests" "$non_test_credentials"

shaped_prose=$(git -C "$root" grep -I -l -i -E -e "$hard_shapes" -e 'this machine' "$ref" -- '*.md' 2>/dev/null | awk -F: '{ print $2 }' | awk 'NF { count += 1 } END { print count + 0 }')
say "tracked prose carrying a machine's shapes, counted and not refused: $shaped_prose file(s)"

count=$(git -C "$root" rev-list --count "$range")
[ "$mode" = shapes ] && say "names: not measured here; the trusted check on this exact commit carries them"
if [ "$red" -eq 0 ]; then
    say "clean: $count commit(s) may be pushed"
    exit 0
fi
say "refused: $count commit(s) cannot be pushed"
exit 1
