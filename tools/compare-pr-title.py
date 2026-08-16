#!/usr/bin/env python3
"""Confronta `pr-title` in Python con la sua porta in Rust, su titoli veri.

Il corpus sono i soggetti dei commit dei tre repo Gyver più quelli della
configurazione: qualche migliaio di frasi scritte da persone, in inglese e in
italiano, che è esattamente la materia su cui questo gancio decide. Le frasi
inventate da chi porta il codice provano ciò a cui ha pensato; queste provano
ciò che succede.

Il confronto passa dal comando intero (`gh pr create --title "<soggetto>"`) e
non dalla sola funzione: così copre anche l'estrazione del titolo, che è il
pezzo dove le virgolette fanno danni.

    compare-pr-title.py
"""
import json
import subprocess
import sys
from pathlib import Path

PY = ['python3', str(Path.home() / '.claude/skills/hooks/pr-title.py')]
RUST = [str(Path.home() / '.claude/rust/target/release/claude-hooks'), 'pr-title']

REPOS = [
    Path.home() / 'gyver/work/suite',
    Path.home() / 'gyver/work/matching-engine',
    Path.home() / 'gyver/work/whatsapp',
    Path.home() / '.claude',
]


def subjects(limit=800):
    seen = []
    for repo in REPOS:
        if not (repo / '.git').exists():
            continue
        out = subprocess.run(
            ['git', '-C', str(repo), 'log', f'-{limit}', '--format=%s'],
            capture_output=True, text=True,
        )
        seen += [s for s in out.stdout.splitlines() if s.strip()]
    return list(dict.fromkeys(seen))


def decision(cmd, command):
    payload = {'tool_name': 'Bash', 'tool_input': {'command': command}}
    out = subprocess.run(cmd, input=json.dumps(payload), capture_output=True, text=True)
    if not out.stdout.strip():
        return out.returncode, None
    try:
        parsed = json.loads(out.stdout)
        reason = parsed['hookSpecificOutput']['permissionDecisionReason']
    except (ValueError, KeyError):
        return out.returncode, ('ILLEGGIBILE', out.stdout)
    return out.returncode, reason


corpus = subjects()
print(f'corpus: {len(corpus)} soggetti di commit veri')

divergent = []
refused = 0
for subject in corpus:
    # le virgolette doppie dentro il titolo romperebbero il comando: si citano
    # con gli apici singoli, come si farebbe scrivendolo a mano
    if "'" in subject:
        command = 'gh pr create --title ' + json.dumps(subject)
    else:
        command = f"gh pr create --title '{subject}'"
    a = decision(PY, command)
    b = decision(RUST, command)
    if a[1] is not None:
        refused += 1
    if a != b:
        divergent.append((subject, a, b))

print(f'rifiutati dal Python: {refused}   passati: {len(corpus) - refused}')
print(f'divergenze: {len(divergent)}')
for subject, a, b in divergent[:10]:
    print(f'\n  soggetto: {subject[:90]!r}')
    print(f'    python: {str(a[1])[:150]}')
    print(f'    rust:   {str(b[1])[:150]}')

sys.exit(1 if divergent else 0)
