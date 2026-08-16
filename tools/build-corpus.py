#!/usr/bin/env python3
"""Costruisce il corpus di equivalenza dai comandi realmente eseguiti.

I tredici casi scritti a mano nel gancio provano ciò a cui l'autore aveva
pensato. Questo prova ciò che succede davvero: prende i comandi `Bash` dai
registri della macchina, li fa giudicare dallo script Python che stiamo
sostituendo, e congela l'esito. Il test Rust poi deve dare lo stesso verdetto
su ognuno.

L'esito atteso lo produce il Python, non io: è lui l'oracolo, e resta tale
finché non lo cancelliamo.

    build-corpus.py > crates/guards/tests/corpus.jsonl
"""
import importlib.util
import json
import os
import pathlib
import sys

HOOKS = pathlib.Path.home() / '.claude' / 'skills' / 'hooks'
spec = importlib.util.spec_from_file_location('cd_guard', HOOKS / 'cd-guard.py')
cd_guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cd_guard)

sources = [
    pathlib.Path.home() / '.claude' / 'homunculus' / 'observations.jsonl',
    pathlib.Path.home() / '.claude' / 'history.jsonl',
]

seen = set()
out = []
for src in sources:
    if not src.exists():
        continue
    with open(src, errors='ignore') as f:
        for line in f:
            try:
                d = json.loads(line)
            except Exception:
                continue
            command = None
            # observations.jsonl: l'ingresso dello strumento è una stringa JSON
            if d.get('tool') == 'Bash' and d.get('event') == 'tool_start':
                try:
                    command = json.loads(d.get('input') or '{}').get('command')
                except Exception:
                    command = None
            # history.jsonl: il comando digitato, che passa dagli stessi ganci
            elif isinstance(d.get('display'), str):
                command = d['display']
            if not command or command in seen:
                continue
            seen.add(command)
            severity, _ = cd_guard.judge(command)
            out.append({'command': command, 'expected': severity or 'pass'})

# I casi interessanti sono rari: senza questo, un corpus di diecimila comandi
# innocui darebbe un verde che non prova niente. Si tengono tutti i non-pass e
# un campione dei pass, e il conto si dichiara.
non_pass = [c for c in out if c['expected'] != 'pass']
passes = [c for c in out if c['expected'] == 'pass']
sample = non_pass + passes[:3000]

for case in sample:
    print(json.dumps(case, ensure_ascii=False))

print(
    f'corpus: {len(sample)} casi — {len(non_pass)} con decisione, '
    f'{len(sample) - len(non_pass)} innocui (su {len(out)} comandi unici)',
    file=sys.stderr,
)
