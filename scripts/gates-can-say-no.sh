#!/bin/sh

# Proves that every check the trunk requires is a check that can refuse.
#
# A required check that cannot go red is worse than no check: the ruleset
# counts it, the tree believes it, and the belief costs nothing to hold. Two
# of the six were exactly that — one could not fail, one could not pass — and
# neither was noticed by anything, because nothing ever asked a gate to prove
# it had said no.
#
# So each of the six ends in one of two states, never a third:
#   proved red  a real commit and the receipt of the run that concluded
#               `failure` on it, reverifiable here against the forge
#   refused     it cannot be made red without touching the product; the
#               reason is written and a fault number holds it
#
# The six are read from the ruleset, never from a list in here: a list would
# go stale the day a check is added and say nothing. Exit 0 only when the
# table and the ruleset name the same checks and every receipt holds.

set -u

root=$(git rev-parse --show-toplevel 2>/dev/null) || { echo "gates-can-say-no: not inside a git tree" >&2; exit 2; }
manifest=${1:-"$root/docs/gates.md"}
[ -r "$manifest" ] || { echo "gates-can-say-no: cannot read the manifest $manifest" >&2; exit 2; }

account=$(git -C "$root" config --get sailor.forgeAs) || { echo "gates-can-say-no: sailor.forgeAs is not declared" >&2; exit 2; }
url=$(git -C "$root" remote get-url origin 2>/dev/null) || { echo "gates-can-say-no: no origin" >&2; exit 2; }
slug=$(printf '%s\n' "$url" | sed -E 's#^(https://github\.com/|git@github\.com:)##; s#\.git$##')
case "$slug" in */*) ;; *) echo "gates-can-say-no: origin is not a GitHub repository" >&2; exit 2 ;; esac

GH_TOKEN=$(gh auth token --user "$account") || { echo "gates-can-say-no: no token for $account" >&2; exit 2; }
export GH_TOKEN

# The checks the trunk actually demands, from every active branch ruleset.
required=$(
    for id in $(gh api "repos/$slug/rulesets" --jq '.[] | select(.target == "branch" and .enforcement == "active") | .id' 2>/dev/null); do
        gh api "repos/$slug/rulesets/$id" \
            --jq '.rules[] | select(.type == "required_status_checks") | .parameters.required_status_checks[].context' 2>/dev/null
    done | sort -u
)
[ -n "$required" ] || { echo "gates-can-say-no: the ruleset named no required check; refusing to call that proof" >&2; exit 2; }

# One row per line: check, tab, state, tab, commit, tab, receipt. Backticks and
# surrounding spaces are dressing, and an em dash is an empty cell.
rows=$(awk -F'|' '
    /^## / { inside = ($0 ~ /^## The six the trunk requires/) ; next }
    !inside || $0 !~ /^\|/ { next }
    NF < 6 { next }
    {
        for (field = 2; field <= 5; field += 1) {
            gsub(/`/, "", $field); gsub(/^[ \t]+|[ \t]+$/, "", $field)
            if ($field == "—") $field = ""
        }
        if ($2 == "check" || $2 ~ /^-+$/) next
        print $2 "\t" $3 "\t" $4 "\t" $5
    }
' "$manifest")
[ -n "$rows" ] || { echo "gates-can-say-no: the manifest holds no receipts table" >&2; exit 2; }

fails=$(mktemp "${TMPDIR:-/tmp}/gates-can-say-no.XXXXXX") || exit 2
trap 'rm -f "$fails"' EXIT HUP INT TERM
say() { echo "gates-can-say-no: $*"; }
# **IT REPORTS ALL SIX, NOT THE FIRST.** Stopping at the first gap answers one
# sixth of the question asked and costs a full round trip to learn the rest.
no() { say "$*"; echo x >> "$fails"; }

# A row naming a check the trunk does not require is a row that has gone stale,
# and a stale receipt is the thing this script exists to catch.
for named in $(printf '%s\n' "$rows" | cut -f1 | sort -u | tr ' ' '\001'); do
    named=$(printf '%s\n' "$named" | tr '\001' ' ')
    printf '%s\n' "$required" | grep -q -x -F -- "$named" \
        || no "the table names «${named}», which the trunk does not require"
done

printf '%s\n' "$required" > "$fails.required"
while IFS= read -r check; do
    [ -n "$check" ] || continue
    row=$(printf '%s\n' "$rows" | awk -F'\t' -v want="$check" '$1 == want { print; exit }')
    [ -n "$row" ] || { no "«${check}» is required and has no row: it has never been asked to refuse"; continue; }

    state=$(printf '%s\n' "$row" | cut -f2)
    commit=$(printf '%s\n' "$row" | cut -f3)
    receipt=$(printf '%s\n' "$row" | cut -f4)

    case "$state" in
        refused)
            case "$receipt" in
                fault\ [0-9]*) say "«${check}»: refused, held by $receipt" ;;
                *) no "«${check}»: refused with no fault number holding it" ;;
            esac
            ;;
        "proved red")
            [ -n "$commit" ] && [ -n "$receipt" ] \
                || { no "«${check}»: proved red with no commit or no receipt"; continue; }
            case "$check" in
                */*)
                    # A commit status: there is no endpoint for one by id, so
                    # the whole list of the commit is read and the id matched.
                    verdict=$(gh api --paginate "repos/$slug/commits/$commit/statuses" \
                        --jq "[.[] | select(.id == $receipt)] | .[0] | \"\(.context)\t\(.state)\"" 2>/dev/null)
                    ;;
                *)
                    # A check run, read by its id, which carries its own sha:
                    # a receipt that names another commit is not a receipt.
                    verdict=$(gh api "repos/$slug/check-runs/$receipt" \
                        --jq '"\(.name)\t\(.conclusion)\t\(.head_sha)"' 2>/dev/null)
                    ;;
            esac
            [ -n "$verdict" ] && [ "$verdict" != "null" ] \
                || { no "«${check}»: the forge does not know receipt $receipt"; continue; }

            name=$(printf '%s\n' "$verdict" | cut -f1)
            concluded=$(printf '%s\n' "$verdict" | cut -f2)
            sha=$(printf '%s\n' "$verdict" | cut -f3)
            [ "$name" = "$check" ] \
                || { no "«${check}»: receipt $receipt belongs to «${name}»"; continue; }
            [ "$concluded" = failure ] \
                || { no "«${check}»: receipt $receipt concluded «${concluded}», not failure"; continue; }
            case "$sha" in
                ''|"$commit"*) ;;
                *) no "«${check}»: receipt $receipt was run on $sha, and the table says $commit"; continue ;;
            esac
            say "«${check}»: said no on $commit, receipt $receipt"
            ;;
        *)
            no "«${check}»: the state «${state}» is neither «proved red» nor «refused»"
            ;;
    esac
done < "$fails.required"
rm -f "$fails.required"

[ ! -s "$fails" ] || { say "$(grep -c . "$fails") of the checks the trunk requires cannot be shown to say no"; exit 1; }
say "all $(printf '%s\n' "$required" | grep -c .) checks the trunk requires can say no"
