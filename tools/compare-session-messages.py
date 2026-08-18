#!/usr/bin/env python3
"""Confronta il gancio dei messaggi fra sessioni col suo porto in Rust.

L'ORIGINALE E' L'ORACOLO, e questo gancio **concede**: e' l'unico del parco che
puo' trasformare una domanda a Theo in un permesso dato da solo. Un porto che
concedesse un caso in piu' non stamperebbe una riga diversa, aprirebbe un canale
che doveva restare chiuso. Per questo si confronta anche il registro, non solo
la decisione.

ORCA E' FINTO, E DEVE ESSERLO. Le due implementazioni chiedono a Orca l'elenco
delle copie di lavoro: sul vivo quell'elenco cambia mentre il confronto gira —
altre sessioni aprono e smontano copie — e una divergenza direbbe soltanto che
il mondo si e' mosso in mezzo. Il finto risponde sempre la stessa cosa, e sa
anche tacere (uscita 1) per il caso «Orca non risponde», che e' l'unico in cui
la prudenza si vede.

IL PERCORSO DI ORCA E' SCRITTO IN CHIARO NELL'ORIGINALE, quindi non si sposta da
fuori: qui se ne fa una **copia** in una cartella temporanea con quella riga
sostituita, e l'originale sul disco non si tocca mai. Il Rust legge lo stesso
percorso da `CLAUDE_ORCA_BIN`, che esiste solo per questo.

DUE HOME FINTE, UNA PER PARTE: il gancio **scrive** il registro. Con una
cartella sola la seconda a girare troverebbe la riga della prima e il confronto
direbbe verde per il motivo sbagliato. Il campo `t` e' l'unico escluso dal
paragone, perche' cambia a ogni esecuzione.

    compare-session-messages.py
"""
from __future__ import annotations

import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]                  # ~/.claude
ORIGINAL = ROOT / 'skills' / 'hooks' / 'allow-session-messages.py'
BINARY = ROOT / 'rust' / 'target' / 'release' / 'claude-hooks'
LOG = 'state/messaggi-concessi.jsonl'

# Le copie che il finto Orca dichiara. I nomi veri del 18/08/2026, perche' e' su
# quelli che il riconoscitore del suffisso si gioca tutto.
COPIES = {
    "result": {
        "worktrees": [
            {"displayName": "tautog", "path": "/Users/theo/orca/workspaces/suite/tautog"},
            {"name": "general", "path": "/Users/theo/orca/general"},
            {"displayName": "suite-229-tabella-residui",
             "path": "/Users/theo/orca/workspaces/suite/suite-229-tabella-residui"},
            {"path": "/Users/theo/orca/workspaces/matching-engine/parole-side-table"},
            {"displayName": "  ", "name": "", "path": ""},        # tutto vuoto: si salta
        ]
    }
}

CASES = [
    # (nome, payload, orca risponde?)
    ('una copia riconosciuta per displayName',
     {"tool_name": "SendMessage", "tool_input": {"to": "tautog", "message": "ciao"}}, True),
    ('col suffisso che distingue due sessioni',
     {"tool_name": "SendMessage", "tool_input": {"to": "tautog-3a", "message": "ciao"}}, True),
    ('col ref che disambigua due omonimi',
     {"tool_name": "SendMessage", "tool_input": {"to": "tautog-3a [c23e5e]", "message": "x"}}, True),
    ('un nome lungo con suffisso',
     {"tool_name": "SendMessage", "tool_input": {"to": "suite-229-tabella-residui-de", "message": "x"}}, True),
    ('riconosciuta dal solo percorso',
     {"tool_name": "SendMessage", "tool_input": {"to": "parole-side-table", "message": "x"}}, True),
    ('riconosciuta per name',
     {"tool_name": "SendMessage", "tool_input": {"to": "general", "message": "x"}}, True),
    ('MUTANTE: un nome che comincia uguale non e\' locale',
     {"tool_name": "SendMessage", "tool_input": {"to": "tautog-in-the-cloud", "message": "x"}}, True),
    ('MUTANTE: ne\' una parola piu\' lunga costruita sopra',
     {"tool_name": "SendMessage", "tool_input": {"to": "generaledelesercito", "message": "x"}}, True),
    ('MUTANTE: il suffisso e\' corto e alfanumerico',
     {"tool_name": "SendMessage", "tool_input": {"to": "tautog-123456789", "message": "x"}}, True),
    ('MUTANTE: e minuscolo',
     {"tool_name": "SendMessage", "tool_input": {"to": "tautog-3A", "message": "x"}}, True),
    ('destinatario interno al processo',
     {"tool_name": "SendMessage", "tool_input": {"to": "main", "message": "x"}}, True),
    ('interno anche se Orca tace',
     {"tool_name": "SendMessage", "tool_input": {"to": "main", "message": "x"}}, False),
    ('Orca non risponde: non si concede al buio',
     {"tool_name": "SendMessage", "tool_input": {"to": "tautog", "message": "x"}}, False),
    ('destinatario sconosciuto',
     {"tool_name": "SendMessage", "tool_input": {"to": "qualcun-altro", "message": "x"}}, True),
    ('destinatario assente',
     {"tool_name": "SendMessage", "tool_input": {"message": "x"}}, True),
    ('destinatario vuoto',
     {"tool_name": "SendMessage", "tool_input": {"to": "", "message": "x"}}, True),
    ('un altro strumento non riguarda questo gancio',
     {"tool_name": "Bash", "tool_input": {"to": "tautog", "command": "ls"}}, True),
    ('senza tool_input',
     {"tool_name": "SendMessage"}, True),
    ('tool_input non e\' un dizionario',
     {"tool_name": "SendMessage", "tool_input": "tautog"}, True),
    ('messaggio assente: la misura e\' zero',
     {"tool_name": "SendMessage", "tool_input": {"to": "tautog"}}, True),
    ('messaggio con accenti: il registro non li deve sfuggire',
     {"tool_name": "SendMessage", "tool_input": {"to": "tautog", "message": "perché è così"}}, True),
    ('messaggio lungo',
     {"tool_name": "SendMessage", "tool_input": {"to": "tautog", "message": "x" * 4000}}, True),
    ('destinatario non testuale',
     {"tool_name": "SendMessage", "tool_input": {"to": 3, "message": "x"}}, True),
    ('destinatario falso secondo python: zero',
     {"tool_name": "SendMessage", "tool_input": {"to": 0, "message": "x"}}, True),
    ('destinatario falso secondo python: lista vuota',
     {"tool_name": "SendMessage", "tool_input": {"to": [], "message": "x"}}, True),
    ('destinatario vero ma non testo: lista piena',
     {"tool_name": "SendMessage", "tool_input": {"to": ["tautog"], "message": "x"}}, True),
    ('destinatario vero ma non testo: dizionario',
     {"tool_name": "SendMessage", "tool_input": {"to": {"a": 1}, "message": "x"}}, True),
    ('payload che non e\' un dizionario', [1, 2, 3], True),
]


def fake_orca(path: Path, answers: bool) -> Path:
    """Un `orca` che risponde sempre uguale, o che fallisce."""
    script = path / 'orca'
    if answers:
        body = f"#!/bin/sh\ncat <<'JSON'\n{json.dumps(COPIES)}\nJSON\n"
    else:
        body = "#!/bin/sh\necho 'orca: non disponibile' >&2\nexit 1\n"
    script.write_text(body)
    script.chmod(script.stat().st_mode | stat.S_IEXEC)
    return script


def python_copy(dest: Path, orca: Path) -> Path:
    """Copia dell'originale con il percorso di orca sostituito. L'originale sul
    disco non si tocca: e' l'unica riga che cambia, e si controlla che cambi."""
    text = ORIGINAL.read_text()
    marker = 'ORCA = "/usr/local/bin/orca"'
    if marker not in text:
        print(f'la riga `{marker}` non c\'e\' piu\' in {ORIGINAL.name}: aggiorna questo confronto')
        sys.exit(2)
    copy = dest / 'allow-session-messages.py'
    copy.write_text(text.replace(marker, f'ORCA = "{orca}"'))
    return copy


def log_lines(home: Path) -> list:
    f = home / LOG
    if not f.exists():
        return []
    out = []
    for line in f.read_text().splitlines():
        try:
            d = json.loads(line)
        except Exception:
            out.append({'illeggibile': line})
            continue
        d.pop('t', None)       # cambia a ogni esecuzione
        out.append(d)
    return out


def run_case(command: list, home: Path, orca: Path, payload) -> tuple:
    env = dict(os.environ)
    env['HOME'] = str(home)
    env['CLAUDE_ORCA_BIN'] = str(orca)
    (home / '.claude' / 'state').mkdir(parents=True, exist_ok=True)
    p = subprocess.run(command, input=json.dumps(payload), capture_output=True,
                       text=True, env=env, timeout=60)
    return p.returncode, p.stdout, p.stderr


def main() -> int:
    if not BINARY.exists():
        print(f'binario assente: {BINARY}')
        return 2
    scratch = Path(tempfile.mkdtemp(prefix='confronto-messaggi-'))
    try:
        diverging = []
        granted = 0
        for name, payload, answers in CASES:
            orca_dir = scratch / 'orca'
            orca_dir.mkdir(exist_ok=True)
            orca = fake_orca(orca_dir, answers)

            results = {}
            for side in ('python', 'rust'):
                home = scratch / f'home-{side}'
                shutil.rmtree(home, ignore_errors=True)
                (home / '.claude' / 'state').mkdir(parents=True)
                if side == 'python':
                    copy = python_copy(scratch, orca)
                    cmd = [sys.executable, str(copy)]
                    # L'originale scrive il registro in `~/.claude/state/...`
                    # tramite `os.path.expanduser`, che segue HOME.
                else:
                    cmd = [str(BINARY), 'allow-session-messages']
                code, out, err = run_case(cmd, home, orca, payload)
                results[side] = (code, out, log_lines(home / '.claude'))

            if results['python'][1].strip():
                granted += 1
            if results['python'] != results['rust']:
                diverging.append((name, results['python'], results['rust']))

        print(f'{len(CASES)} casi confrontati (uscita, stdout, registro)')
        print(f'  divergenze: {len(diverging)}')
        for name, a, b in diverging[:6]:
            print(f'\n  --- {name}')
            print(f'  python: {str(a)[:220]}')
            print(f'  rust:   {str(b)[:220]}')
        print(f'  casi in cui l\'oracolo concede: {granted}')
        # Un confronto in cui nessun caso concede non prova niente: due ganci
        # che tacciono sempre sono indistinguibili, e un porto rotto passerebbe.
        if granted == 0:
            print('\nCASI TROPPO POVERI: nessuna concessione, il confronto non discrimina')
            return 1
        return 1 if diverging else 0
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


if __name__ == '__main__':
    sys.exit(main())
