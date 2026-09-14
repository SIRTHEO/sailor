#!/bin/sh
# Runs, in this tree, the lines of a gate manifest that apply to base..HEAD: letter A
# always, B to E when the paths they name changed. F is held by the delivery flows.
# Prints one JSON summary on stdout; each command's own output goes to stderr.
# Exit: 0 no line red, 1 a line red, 2 cannot run. A line that is not a command, or
# needs a value only a person has, is counted in "manual" and never passes for green.

set -u

no_fail_fast=0
list_only=0
base=""
manifest=""
while [ $# -gt 0 ]; do
    case "$1" in
        --no-fail-fast) no_fail_fast=1 ;;
        --list) list_only=1 ;;
        --base) base=${2:-}; [ -n "$base" ] || { echo "gates: --base needs a commit" >&2; exit 2; }; shift ;;
        --manifest) manifest=${2:-}; [ -n "$manifest" ] || { echo "gates: --manifest needs a file" >&2; exit 2; }; shift ;;
        *) echo "gates: unknown argument $1" >&2; exit 2 ;;
    esac
    shift
done

root=$(git rev-parse --show-toplevel 2>/dev/null) || { echo "gates: not inside a git tree" >&2; exit 2; }
manifest=${manifest:-"$root/docs/gates.md"}
[ -r "$manifest" ] || { echo "gates: cannot read the manifest $manifest" >&2; exit 2; }
if [ -z "$base" ]; then
    base=$(git -C "$root" merge-base main HEAD 2>/dev/null) || { echo "gates: no base to compare against" >&2; exit 2; }
fi
git -C "$root" rev-parse --verify "$base^{commit}" >/dev/null 2>&1 || { echo "gates: cannot read the base $base" >&2; exit 2; }
changed=$(git -C "$root" diff --name-only "$base" HEAD) || { echo "gates: cannot read what changed" >&2; exit 2; }

applies() {
    case "$1" in
        A) return 0 ;;
        B) printf '%s\n' "$changed" | grep -E -q '\.flow\.json$|(^|/)token-seeds\.json$' ;;
        C) printf '%s\n' "$changed" | grep -E -q '^crates/actions/|brake' ;;
        D) printf '%s\n' "$changed" | grep -E -q '^crates/profiles/' ;;
        E) printf '%s\n' "$changed" | grep -E -q '^desktop/' ;;
        *) return 1 ;;
    esac
}

crates=$(printf '%s\n' "$changed" | sed -n 's#^crates/\([^/]*\)/.*#-p \1#p' | sort -u | tr '\n' ' ')

# One line per bullet or command: letter, a tab, "empty", "exit" or "manual", a tab, the text.
plan=$(awk '
    /^## [A-Z]\./ { letter = substr($2, 1, 1); next }
    /^## / { letter = ""; next }
    letter != "" && /^- / {
        line = $0; prefix = ""; judge = (line ~ /— empty/) ? "empty" : "exit"; found = 0
        while (match(line, /`[^`]+`/)) {
            span = substr(line, RSTART + 1, RLENGTH - 2)
            line = substr(line, RSTART + RLENGTH)
            if (span !~ /^(cargo |git |cd |npx |npm |SAILOR_)/) continue
            found = 1
            if (span ~ /^cd [^ ]+ && /) { split(span, parts, " "); prefix = "cd " parts[2] " && " }
            else if (prefix != "") span = prefix span
            print letter "\t" judge "\t" span
        }
        if (!found) { text = substr($0, 3); gsub(/\t/, " ", text); print letter "\tmanual\t" text }
    }' "$manifest")

printf '%s\n' "$plan" | grep -q "^A$(printf '\t')" || { echo "gates: letter A resolves to nothing in $manifest" >&2; exit 2; }

export CARGO_TARGET_DIR="$root/target/own"
letters=""
ran=0
red=0
manual=0
tab=$(printf '\t')
while IFS="$tab" read -r letter judge command; do
    [ -n "$letter" ] || continue
    applies "$letter" || continue
    case " $letters " in *" $letter "*) ;; *) letters="${letters:+$letters }$letter" ;; esac
    if [ "$judge" = manual ]; then
        echo "gates[$letter] needs a person: $command" >&2
        manual=$((manual + 1))
        continue
    fi
    case "$command" in
        *"<every crate touched>"*)
            [ -n "$crates" ] || { echo "gates[$letter] no crate touched: $command" >&2; continue; }
            command=$(printf '%s' "$command" | sed "s#-p <every crate touched>#$crates#")
            command="$command -- -D warnings" ;;
        *"<"*">"*)
            echo "gates[$letter] needs a person: $command" >&2
            manual=$((manual + 1))
            continue ;;
    esac
    command=$(printf '%s' "$command" | sed "s#main\.\.HEAD#$base..HEAD#g")
    case "$command" in
        "cargo test "*|*"&& cargo test "*)
            [ "$no_fail_fast" -eq 1 ] && case "$command" in *--no-fail-fast*) ;; *) command="$command --no-fail-fast" ;; esac ;;
    esac
    if [ "$list_only" -eq 1 ]; then
        echo "gates[$letter] $command" >&2
        continue
    fi
    echo "gates[$letter] \$ $command" >&2
    ran=$((ran + 1))
    if [ "$judge" = empty ]; then
        said=$(cd "$root" && sh -c "$command" </dev/null 2>&1)
        if [ -n "$said" ]; then
            printf '%s\n' "$said" >&2
            echo "gates[$letter] red: expected no output" >&2
            red=$((red + 1))
        fi
    elif ! (cd "$root" && sh -c "$command" </dev/null) >&2 2>&1; then
        echo "gates[$letter] red: $command" >&2
        red=$((red + 1))
    fi
done <<PLAN
$plan
PLAN

green=false
[ "$red" -eq 0 ] && [ "$manual" -eq 0 ] && green=true
manual_pending=false
[ "$manual" -gt 0 ] && manual_pending=true
printf '{"letters":"%s","ran":%s,"red":%s,"manual":%s,"manual_pending":%s,"green":%s}\n' \
    "$letters" "$ran" "$red" "$manual" "$manual_pending" "$green"
[ "$red" -eq 0 ] || exit 1
exit 0
