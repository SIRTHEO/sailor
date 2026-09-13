#!/bin/sh
# Builds the two throwaway projects `take-the-next-work` is checked against,
# under target/fixtures (gitignored, remade on every call). alpha's check.sh
# passes once TASK_OK exists; beta's looks for a file the cheap worker never
# writes, so its task can never pass — the fixture the flow is meant to park.
set -eu

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
  git -C "$dir" config user.email "queue-flow-fixture@example"
  git -C "$dir" config user.name "queue-flow-fixture"
  printf '%s\n' "$check_body" > "$dir/check.sh"
  chmod +x "$dir/check.sh"
  git -C "$dir" add check.sh
  git -C "$dir" commit -q -m "first"
}

make_project alpha '#!/bin/sh
test -f TASK_OK'

make_project beta '#!/bin/sh
test -f MARKER_THE_CHEAP_WORKER_NEVER_WRITES'

echo "fixtures written under $fixtures"
echo "seed their ledger with: cargo run -p sailor --example seed_take_the_next_work -- $fixtures"
