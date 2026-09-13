#!/bin/sh

set -u

root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 2
identity=$(git -C "$root" config --get sailor.pushAs 2>/dev/null) || { echo "push: sailor.pushAs is not declared"; exit 2; }
helper='!f() { printf "%s\\n" username=x-access-token; printf "password=%s\\n" "$(gh auth token --user "'"$identity"'")"; }; f'

git -C "$root" for-each-ref --format='%(refname:short)' refs/heads | while read -r branch; do
    case "$branch" in
        main|matteodimattia/*|crew/*|work/*) ;;
        *) continue ;;
    esac
    if [ "$branch" != main ] && ! git -C "$root" merge-base main "$branch" >/dev/null; then
        echo "push[$branch]: skipped because it has no merge base with main"
        continue
    fi
    if ! "$root/scripts/privacy-scan.sh" "$branch"; then
        echo "push[$branch]: skipped because privacy could not be proved"
        continue
    fi
    tip=$(git -C "$root" rev-parse "$branch") || { echo "push[$branch]: skipped because its tip cannot be read"; continue; }
    short=$(git -C "$root" rev-parse --short "$tip") || exit 2
    pin="refs/sailor/pins/$branch/$short"
    if ! git -C "$root" show-ref --verify --quiet "$pin" && ! git -C "$root" update-ref "$pin" "$tip" ""; then
        echo "push[$branch]: skipped because its pin could not be written"
        continue
    fi
    if [ "$branch" = main ]; then
        git -C "$root" -c credential.helper= -c "credential.helper=$helper" push origin "$branch:refs/heads/$branch" || exit $?
    else
        git -C "$root" -c credential.helper= -c "credential.helper=$helper" push --force-with-lease origin "$branch:refs/heads/$branch" || exit $?
    fi
    echo "push[$branch]: pinned $short and pushed"
done
