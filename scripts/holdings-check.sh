#!/bin/sh

set -u

root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 2
red=0
git -C "$root" for-each-ref --format='%(refname:short)' refs/heads | while read -r branch; do
    case "$branch" in
        main|matteodimattia/*|crew/*|work/*) ;;
        *) continue ;;
    esac
    current=$(git -C "$root" rev-parse "$branch") || exit 2
    git -C "$root" for-each-ref --format='%(objectname)' "refs/sailor/pins/$branch" | while read -r pin; do
        if ! git -C "$root" merge-base --is-ancestor "$pin" "$current"; then
            echo "holding[$branch]: moved behind pinned $pin"
            exit 1
        fi
    done || red=1
done

[ "$red" -eq 0 ] || exit 1
echo "holdings: every protected branch contains its pins"
