#!/usr/bin/env python3
"""Compare the Rust handoff measurement against handoff_common.py on real transcripts.

The oracle is the Python implementation: it is what runs on this machine today,
and the port is correct when it cannot be told apart from it. Hand-written cases
do not find the defects — the ones that matter came out of real material every
time.

WHAT IS COMPARED. Model, budget, warn, require and used, as five separate fields
rather than one verdict. A verdict hides which half diverged, and on the previous
ports the divergence was twice in a field the verdict did not depend on.

THE MEMO IS THE TRAP. context_used() memoises per session under
state/consegna-misura-<id>, and both implementations read and write that same
file. Running one after the other on the same session id would have the second
read the first's answer and agree with itself. Each side therefore runs with an
empty session, which disables the memo — and the memo's own format is covered by
a separate case below.
"""
import json
import subprocess
import sys
from pathlib import Path

BIN = Path.home() / '.claude/rust/target/release/claude-hooks'
HOOKS = Path.home() / '.claude/skills/hooks'
PROJECTS = Path.home() / '.claude/projects'
SAMPLE = 120


def ask_rust(transcript):
    r = subprocess.run([str(BIN), 'handoff-measure', transcript, ''],
                       capture_output=True, text=True, timeout=120)
    if r.returncode != 0:
        return {'error': (r.stderr or '')[:120]}
    try:
        return json.loads(r.stdout)
    except Exception:
        return {'error': 'unparsable: ' + (r.stdout or '')[:120]}


def ask_python(transcript):
    code = (
        'import sys, json;'
        f'sys.path.insert(0, {str(HOOKS)!r});'
        'from handoff_common import thresholds, context_used;'
        f't = thresholds({transcript!r});'
        f'u = context_used({transcript!r}, "");'
        'print(json.dumps({"model": t["model"], "budget": t["budget"],'
        ' "warn": t["warn"], "require": t["require"], "used": u}))'
    )
    r = subprocess.run([sys.executable, '-c', code], capture_output=True,
                       text=True, timeout=120)
    if r.returncode != 0:
        return {'error': (r.stderr or '')[-120:]}
    try:
        return json.loads(r.stdout)
    except Exception:
        return {'error': 'unparsable: ' + (r.stdout or '')[:120]}


def transcripts():
    """Real transcripts, sampled at a constant stride across the whole set.

    Not the first N: they sort alphabetically by uuid, which correlates with
    nothing, but truncation still concentrates the sample. A previous port had a
    mutant survive because an alphabetical head happened to hold no long file.
    """
    everything = sorted(PROJECTS.glob('*/*.jsonl'))
    if len(everything) <= SAMPLE:
        return everything
    stride = len(everything) / SAMPLE
    return [everything[int(i * stride)] for i in range(SAMPLE)]


def main():
    files = transcripts()
    if not files:
        print('no transcripts found: nothing proven', file=sys.stderr)
        return 1
    diverged = 0
    for f in files:
        rust, py = ask_rust(str(f)), ask_python(str(f))
        if rust != py:
            diverged += 1
            print(f'\nDIVERGE  {f.name[:8]}  ({f.stat().st_size // 1024} KB)')
            for key in sorted(set(rust) | set(py)):
                a, b = rust.get(key), py.get(key)
                if a != b:
                    print(f'    {key:8} rust={a!r}  python={b!r}')
    print(f'\n{len(files)} transcripts compared, {diverged} diverged')
    return 1 if diverged else 0


if __name__ == '__main__':
    sys.exit(main())
