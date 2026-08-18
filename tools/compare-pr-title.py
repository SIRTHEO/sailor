#!/usr/bin/env python3
"""Confronta `pr-title` in Python con la sua porta in Rust, su titoli veri.

Il corpus sono i soggetti dei commit dei tre repo Other-repo più quelli della
configurazione: qualche migliaio di frasi scritte da persone, in inglese e in
italiano, che è esattamente la materia su cui questo gancio decide. Le frasi
inventate da chi porta il codice provano ciò a cui ha pensato; queste provano
ciò che succede.

Il confronto passa dal comando intero (`gh pr create --title "<soggetto>"`) e
non dalla sola funzione: così copre anche l'estrazione del titolo, che è il
pezzo dove le virgolette fanno danni.

    compare-pr-title.py
    compare-pr-title.py --record   registra le risposte dell'oracolo

L'ORACOLO PUO' NON ESSERCI PIU'. Dal 18/08/2026 le risposte del Python si
registrano su file (`tools/oracle/pr-title.json`), cosi' il confronto sopravvive
alla cancellazione degli originali. Finche' il Python e' sul disco si interroga
lui, e il registro fa da controllo di se stesso: se i due non dicono la stessa
cosa, il registro e' invecchiato e lo si sente.
"""
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from oracle import Oracle, normalise                    # noqa: E402

ORIGINAL = Path.home() / '.claude/skills/hooks/pr-title.py'
PY = ['python3', str(ORIGINAL)]
RUST = [str(Path.home() / '.claude/rust/target/release/claude-hooks'), 'pr-title']

REPOS = [
    Path.home() / 'other-repo/work/suite',
    Path.home() / 'other-repo/work/a-service',
    Path.home() / 'other-repo/work/a-client',
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


oracle = Oracle('pr-title', ORIGINAL)
print(oracle.describe())
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
    a = oracle.answer(command, lambda c=command: decision(PY, c))
    # Normalizzato anche il porto: l'oracolo torna dal JSON come lista, e una
    # tupla non e' mai uguale a una lista. E' la trappola scritta in oracle.py.
    b = normalise(decision(RUST, command))
    if a is None:
        continue                       # il registro non lo conosce: lo dira' close()
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

problems = oracle.close()
sys.exit(1 if (divergent or problems) else 0)
