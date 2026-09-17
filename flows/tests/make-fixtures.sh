#!/bin/sh
# Builds alpha (passes once TASK_OK exists) and beta (never passes: the
# fixture the flow is meant to park) under target/fixtures, remade on every call.
set -eu
# A git hook exports these, and they would send every command below to the
# repository the hook runs for instead of the fixture's own.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_COMMON_DIR GIT_OBJECT_DIRECTORY
export GIT_AUTHOR_NAME=queue-flow-fixture GIT_AUTHOR_EMAIL=queue-flow-fixture@example
export GIT_COMMITTER_NAME=queue-flow-fixture GIT_COMMITTER_EMAIL=queue-flow-fixture@example

root="$(cd "$(dirname "$0")/../.." && pwd)"
fixtures="$root/target/fixtures"
rm -rf "$fixtures"
mkdir -p "$fixtures"

make_project() {
  name="$1"
  check_body="$2"
  dir="$fixtures/$name"
  mkdir -p "$dir"
  git -C "$dir" init -q
  printf '%s\n' "$check_body" > "$dir/check.sh"
  chmod +x "$dir/check.sh"
  git -C "$dir" add check.sh
  git -C "$dir" commit -q -m "first"
}

make_project alpha '#!/bin/sh
test -f TASK_OK'

make_project beta '#!/bin/sh
test -f MARKER_THE_CHEAP_WORKER_NEVER_WRITES'

# Passes unconditionally: a worker that leaves the tree exactly as it found it
# must still have that tree standing when this runs.
make_project gamma '#!/bin/sh
true'

echo "fixtures written under $fixtures"
echo "seed their ledger with: cargo run -p sailor --example seed_take_the_next_work -- $fixtures"
echo "check the flow against it with: SAILOR_LEDGER=$fixtures/store cargo run -p sailor -- flow check take-the-next-work"
