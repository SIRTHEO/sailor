#!/bin/sh
# Runs, in this tree, the lines of a gate manifest that apply to base..HEAD: letter A
# always, B to E when the paths they name changed. F is held by the delivery flows.
# No manifest text reaches a shell: a command runs only when it matches one of the
# shapes below, as an argument list. Prints one JSON summary on stdout.
# Exit: 0 no line red, 1 a line red, 2 cannot run. "manual" counts what nobody ran:
# a command of no known shape, a value only a person has, or a bullet with no command
# that the review did not check by its exact text (--covered). Manual is never green.
# A line a person already approved for this change (--approved) counts as covered, and
# every line still left to a person is written, one per line, to --pending.

set -u

no_fail_fast=0
list_only=0
base=""
manifest=""
covered=""
approved=""
pending=""
while [ $# -gt 0 ]; do
    case "$1" in
        --no-fail-fast) no_fail_fast=1 ;;
        --list) list_only=1 ;;
        --base) base=${2:-}; [ -n "$base" ] || { echo "gates: --base needs a commit" >&2; exit 2; }; shift ;;
        --manifest) manifest=${2:-}; [ -n "$manifest" ] || { echo "gates: --manifest needs a file" >&2; exit 2; }; shift ;;
        --covered) covered=${2:-}; [ -r "$covered" ] || { echo "gates: --covered needs a readable file" >&2; exit 2; }; shift ;;
        --approved) approved=${2:-}; [ -r "$approved" ] || { echo "gates: --approved needs a readable file" >&2; exit 2; }; shift ;;
        --pending) pending=${2:-}; [ -n "$pending" ] || { echo "gates: --pending needs a file" >&2; exit 2; }; : > "$pending" || exit 2; shift ;;
        *) echo "gates: unknown argument $1" >&2; exit 2 ;;
    esac
    shift
done

root=$(git rev-parse --show-toplevel 2>/dev/null) || { echo "gates: not inside a git tree" >&2; exit 2; }

# Whoever creates a thing writes its end. The battery names the temporary
# directory in 278 places, and inherited that is the one every session shares:
# 16.032 directories weighing 807 MB had piled up there, one set per run and
# per pid, because a run wrote an end for nothing it made. Measured 22/09/2026:
# eleven tests of one module leave eleven behind. A root of this run's own dies
# with the run, and none of the 278 places has to be touched for that.
gates_tmp=$(mktemp -d "${TMPDIR:-/tmp}/sailor-gates-XXXXXX") || { echo "gates: no temporary root" >&2; exit 2; }
trap 'rm -rf "$gates_tmp"' EXIT INT TERM
TMPDIR="$gates_tmp"
export TMPDIR
manifest=${manifest:-"$root/docs/gates.md"}
[ -r "$manifest" ] || { echo "gates: cannot read the manifest $manifest" >&2; exit 2; }
if [ -z "$base" ]; then
    base=$(git -C "$root" merge-base main HEAD 2>/dev/null) || { echo "gates: no base to compare against" >&2; exit 2; }
fi
git -C "$root" rev-parse --verify "$base^{commit}" >/dev/null 2>&1 || { echo "gates: cannot read the base $base" >&2; exit 2; }
fork=$(git -C "$root" merge-base "$base" HEAD) || { echo "gates: $base shares no history with HEAD" >&2; exit 2; }
changed=$(git -C "$root" diff --name-only "$fork" HEAD) || { echo "gates: cannot read what changed" >&2; exit 2; }

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

crates=$(printf '%s\n' "$changed" | sed -n 's#^crates/\([a-z0-9_-]*\)/.*#-p \1#p' | sort -u | tr '\n' ' ')

# One line per span or bullet: letter, a tab, "command" or "bullet", a tab, the text.
plan=$(awk '
    /^## [A-Z]\./ { letter = substr($2, 1, 1); next }
    /^## / { letter = ""; next }
    letter != "" && /^- / {
        line = $0; prefix = ""; found = 0
        while (match(line, /`[^`]+`/)) {
            span = substr(line, RSTART + 1, RLENGTH - 2)
            line = substr(line, RSTART + RLENGTH)
            if (span !~ /^(cargo |git |cd |npx |npm |SAILOR_)/) continue
            found = 1
            if (span ~ /^cd [^ ]+ && /) { split(span, parts, " "); prefix = "cd " parts[2] " && " }
            else if (prefix != "") span = prefix span
            print letter "\tcommand\t" span
        }
        if (!found) { text = substr($0, 3); gsub(/\t/, " ", text); print letter "\tbullet\t" text }
    }' "$manifest")

printf '%s\n' "$plan" | grep -q "^A$(printf '\t')command$(printf '\t')" || { echo "gates: letter A resolves to no command in $manifest" >&2; exit 2; }

export CARGO_TARGET_DIR="$root/target/own"
TRAILERS="git log main..HEAD --format=%B | grep -E '^(Co-Authored-By|Claude-Session):'"
letters=""
ran=0
red=0
manual=0
covered_count=0
tab=$(printf '\t')

say() { echo "gates[$letter] $*" >&2; }
left_to_a_person() {
    if [ -n "$approved" ] && grep -F -x -q -- "$1" "$approved"; then
        say "approved by a person for this change: $1"
        covered_count=$((covered_count + 1))
        return
    fi
    say "$2: $1"
    manual=$((manual + 1))
    [ -z "$pending" ] || printf '%s\n' "$1" >> "$pending"
}
count_red() { say "red: $1"; red=$((red + 1)); }
run_argv() {
    # The shape was matched first, so every word is from a closed alphabet: no quote,
    # no operator, no glob, and word splitting cannot change what runs.
    shape=$1
    set -f
    # shellcheck disable=SC2086
    set -- $shape
    set +f
    if [ "$list_only" -eq 1 ]; then say "$*"; return; fi
    say "\$ $*"
    ran=$((ran + 1))
    (cd "$root" && "$@" </dev/null) >&2 2>&1 || count_red "$*"
}

while IFS="$tab" read -r letter kind text; do
    [ -n "$letter" ] || continue
    applies "$letter" || continue
    case " $letters " in *" $letter "*) ;; *) letters="${letters:+$letters }$letter" ;; esac
    if [ "$kind" = bullet ]; then
        if [ -n "$covered" ] && grep -F -x -q -- "$text" "$covered"; then
            say "checked by the review: $text"
            covered_count=$((covered_count + 1))
        else
            left_to_a_person "$text" "needs a person, the review did not check it"
        fi
        continue
    fi
    if [ "$text" = "$TRAILERS" ]; then
        if [ "$list_only" -eq 1 ]; then say "trailer check over $base..HEAD"; continue; fi
        say "\$ trailer check over $base..HEAD"
        ran=$((ran + 1))
        found=$(git -C "$root" log "$base..HEAD" --format=%B | grep -E '^(Co-Authored-By|Claude-Session):' || true)
        [ -z "$found" ] || { printf '%s\n' "$found" >&2; count_red "a commit carries a tool trailer"; }
        continue
    fi
    if printf '%s' "$text" | grep -E -q '^cargo clippy -p <every crate touched> --tests -j 1$'; then
        [ -n "$crates" ] || { say "no crate touched: $text"; continue; }
        run_argv "cargo clippy $crates--tests -j 1 -- -D warnings"
        continue
    fi
    if printf '%s' "$text" | grep -E -q '^cargo test (-p [a-z0-9_-]+ )?-j 1( --test [a-z0-9_]+)*( --no-fail-fast)?$' ||
        [ "$text" = "cargo test --manifest-path desktop/src-tauri/Cargo.toml -j 1" ]; then
        command=$text
        [ "$no_fail_fast" -eq 1 ] && case "$command" in *--no-fail-fast*) ;; *) command="$command --no-fail-fast" ;; esac
        run_argv "$command"
        continue
    fi
    if [ "$text" = "git diff --check HEAD~1 HEAD" ]; then
        run_argv "$text"
        continue
    fi
    case "$text" in
        "cd desktop && npm test"|"cd desktop && npx tsc --noEmit")
            words=${text#cd desktop && }
            if [ "$list_only" -eq 1 ]; then say "(in desktop) $words"; continue; fi
            # A tree cut for the gates has never installed the window's packages.
            if [ ! -d "$root/desktop/node_modules" ]; then
                say "\$ (in desktop) npm ci"
                (cd "$root/desktop" && npm ci </dev/null) >&2 2>&1 || { count_red "cd desktop && npm ci"; continue; }
            fi
            say "\$ (in desktop) $words"
            ran=$((ran + 1))
            set -f
            # shellcheck disable=SC2086
            (cd "$root/desktop" && $words </dev/null) >&2 2>&1 || count_red "$text"
            set +f
            continue ;;
    esac
    left_to_a_person "$text" "needs a person, no known shape runs it"
done <<PLAN
$plan
PLAN

green=false
[ "$red" -eq 0 ] && [ "$manual" -eq 0 ] && green=true
manual_pending=false
[ "$manual" -gt 0 ] && manual_pending=true
printf '{"letters":"%s","ran":%s,"red":%s,"manual":%s,"covered":%s,"manual_pending":%s,"green":%s}\n' \
    "$letters" "$ran" "$red" "$manual" "$covered_count" "$manual_pending" "$green"
[ "$red" -eq 0 ] || exit 1
exit 0
