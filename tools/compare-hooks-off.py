#!/usr/bin/env python3
"""Confronta `hooks-off` in Python con la sua porta in Rust.

Tre scenari costruiti a mano, perché il gancio decide guardando **il disco**:
un repo con i ganci installati, uno cieco (cartella `.husky` versionata ma
`.husky/_/pre-commit` assente — il caso misurato il 14/08), e uno che i ganci
non li ha mai avuti e che quindi va lasciato in pace.

Il confronto guarda esito **e** stdout: questo gancio nega il permesso con un
JSON su stdout, non con l'uscita 2, quindi un confronto sul solo codice di
uscita direbbe «uguali» anche se uno dei due non negasse affatto.
"""
import json
import shutil
import subprocess
import subprocess as sp
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from oracle import Oracle                                  # noqa: E402

ORIGINAL = Path.home() / '.claude/skills/hooks/hooks-off.py'
PY = ['python3', str(ORIGINAL)]
RUST = [str(Path.home() / '.claude/rust/target/release/claude-hooks'), 'hooks-off']


def repo(root: Path, kind: str) -> Path:
    """kind: 'sano' | 'cieco' | 'senza-controlli'"""
    path = root / kind
    path.mkdir(parents=True)
    sp.run(['git', 'init', '-q', str(path)], check=True)
    sp.run(['git', '-C', str(path), 'config', 'core.hooksPath', '.husky/_'], check=True)
    if kind == 'senza-controlli':
        sp.run(['git', '-C', str(path), 'config', '--unset', 'core.hooksPath'], check=True)
        return path
    (path / '.husky/_').mkdir(parents=True)
    (path / '.husky/pre-commit').write_text('#!/bin/sh\n')   # la cartella versionata
    if kind == 'sano':
        (path / '.husky/_/pre-commit').write_text('#!/bin/sh\n')
        (path / '.husky/_/pre-push').write_text('#!/bin/sh\n')
    return path


def run(cmd, command, cwd):
    """(codice di uscita, stdout): la coppia che il confronto guarda, e l'unica
    forma che sopravvive a un giro attraverso il registro dell'oracolo."""
    payload = {'tool_name': 'Bash', 'cwd': str(cwd), 'tool_input': {'command': command}}
    p = subprocess.run(cmd, input=json.dumps(payload), capture_output=True, text=True)
    return p.returncode, p.stdout


root = Path(tempfile.mkdtemp(prefix='hooks-off-confronto-'))
# La radice temporanea cambia a ogni esecuzione e compare sia nei comandi sia
# nelle risposte: senza toglierla di mezzo il registro varrebbe per un giro solo.
oracle = Oracle('hooks-off', ORIGINAL, scrub=[root])
print(oracle.describe())
cases = []
for kind in ('sano', 'cieco', 'senza-controlli'):
    path = repo(root, kind)
    for command in (f'git -C {path} commit -m "feat: x"',
                    f'git -C {path} push origin ramo',
                    f'git -C {path} status --short'):
        cases.append((kind, command, path))

print(f"{'scenario':<18}{'comando':<22}{'py':>4}{'rust':>6}   esito")
print('-' * 62)
divergent = 0
for kind, command, path in cases:
    a = oracle.answer(command, lambda c=command, p=path: run(PY, c, p))
    b = oracle.clean(run(RUST, command, path))
    if a is None:
        continue
    # Si confrontano gli **oggetti**, non il testo: `json.dumps` di Python mette
    # uno spazio dopo i due punti e la virgola, il serializzatore Rust no. Chi
    # legge questo canale fa il parsing, quindi la spaziatura non è una
    # differenza — e trattarla come tale nasconderebbe quelle vere.
    def parsed(out):
        try:
            return json.loads(out) if out.strip() else None
        except ValueError:
            return ('ILLEGGIBILE', out)

    same = a[0] == b[0] and parsed(a[1]) == parsed(b[1])
    if not same:
        divergent += 1
    gesture = command.split()[3] if len(command.split()) > 3 else '?'
    negated = 'nega' if '"deny"' in a[1] else 'passa'
    print(f'{kind:<18}{gesture + " (" + negated + ")":<22}{a[0]:>4}{b[0]:>6}   '
          f'{"uguali" if same else "DIVERGE"}')
    if not same:
        print(f'    py:   {a[1].strip()[:150]}')
        print(f'    rust: {b[1].strip()[:150]}')

shutil.rmtree(root, ignore_errors=True)
print(f'\ndivergenze: {divergent} su {len(cases)}')
sys.exit(1 if (divergent or oracle.close()) else 0)
