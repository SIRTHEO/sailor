#!/bin/sh
# Builds alpha (passes once TASK_OK exists) and beta (never passes: the
# fixture the flow is meant to park) under target/fixtures, remade on every call.
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

# Passes unconditionally: a worker that leaves the tree exactly as it found it
# must still have that tree standing when this runs.
make_project gamma '#!/bin/sh
true'

# The cheap worker the journey runs: it acknowledges the mandate and writes
# TASK_OK, so walking the journey never reaches an engine that is paid for.
mkdir -p "$fixtures/bin"
cat > "$fixtures/bin/fake-cheap-worker" <<'WORKER'
#!/bin/sh
input=$(cat)
digest=$(printf '%s' "$input" | head -n 1 | sed -E 's/^Mandate ([^:]+):.*/\1/')
[ -n "$input" ] && touch TASK_OK
printf 'ack %s\nattempted' "$digest"
WORKER
chmod +x "$fixtures/bin/fake-cheap-worker"
printf '%s\n' '[{"id": "fake-cheap-worker", "family": "ai_cli", "label": "Fake Cheap Worker", "detect": {"command": "fake-cheap-worker"}, "ask": {"args": [], "prompt": "stdin"}}]' > "$fixtures/descriptors.json"

echo "fixtures written under $fixtures"
echo "seed their ledger with: cargo run -p sailor --example seed_take_the_next_work -- $fixtures fake-cheap-worker"
echo "then, for every command that reads them: export SAILOR_LEDGER=$fixtures/store SAILOR_TOOL_DESCRIPTORS=$fixtures/descriptors.json PATH=$fixtures/bin:\$PATH"
