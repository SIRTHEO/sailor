#!/usr/bin/env python3
"""Confronta il gate SocratiCode in Node con la sua porta in Rust.

Perché serve uno strumento a parte e non il solito corpus: questo gancio ha
**stato** — un contatore per sessione, una traccia di ricerca, un marcatore per
file — e la decisione dipende da un repo indicizzato. Confrontarlo contro lo
stato vero della macchina darebbe esiti diversi a ogni giro e, peggio, ne
consumerebbe i contatori.

Qui ogni caso gira in una `HOME` e una `TMPDIR` finte, ricreate da zero per
ciascuna implementazione: i due partono sempre dalla stessa condizione, e la
macchina non se ne accorge.

    compare-socraticode-gate.py
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

NODE = ['node', str(Path.home() / '.claude/scripts/socraticode-gate-v2.js')]
RUST = [str(Path.home() / '.claude/rust/target/release/claude-hooks'), 'socraticode-gate']


def scenario(root: Path, indexed: bool) -> Path:
    """Un finto repo, indicizzato o no, con la casa e i suoi marcatori."""
    repo = root / 'repo'
    (repo / 'src').mkdir(parents=True)
    if indexed:
        (repo / '.socraticodeignore').write_text('')
    (root / 'home/.claude/state').mkdir(parents=True)
    (root / 'tmp').mkdir()
    # Il gancio Node carica il registro da `$HOME/.claude/scripts/hook-log.js`:
    # senza, il `require` fallisce in silenzio e il gancio **non registra
    # nulla**. Il primo giro di questo confronto lo leggeva come «il Rust scrive
    # e il Node no», che era falso — il Node non poteva.
    scripts = root / 'home/.claude/scripts'
    scripts.mkdir(parents=True)
    shutil.copy(Path.home() / '.claude/scripts/hook-log.js', scripts)
    return repo


def journal(root: Path) -> list:
    """Le righe scritte nel registro, senza l'istante.

    Guardare solo l'esito non basta, e questo controllo nasce da una divergenza
    vera: il Rust scriveva `"conteggio":"16"` come stringa dove il JavaScript
    scriveva `16` come numero. Exit code uguale, messaggio uguale, e chi somma
    quel campo si rompe.
    """
    path = root / 'home/.claude/state/ganci.jsonl'
    if not path.exists():
        return []
    lines = []
    for line in path.read_text().splitlines():
        try:
            record = json.loads(line)
        except ValueError:
            lines.append(('ILLEGGIBILE', line))
            continue
        record.pop('t', None)   # l'istante differisce sempre, e non è il punto
        # il tipo del valore fa parte del confronto: "16" e 16 non sono uguali
        lines.append(sorted((k, type(v).__name__, v) for k, v in record.items()))
    return lines


def run(cmd, payload, root: Path):
    env = dict(os.environ, HOME=str(root / 'home'), TMPDIR=str(root / 'tmp'))
    return subprocess.run(
        cmd, input=json.dumps(payload), capture_output=True, text=True, env=env
    )


CASES = [
    ('grep ricorsivo in repo indicizzato', True,
     {'tool_name': 'Bash', 'tool_input': {'command': 'grep -r foo {repo}/src'}}),
    ('grep ricorsivo fuori dall\'indice', False,
     {'tool_name': 'Bash', 'tool_input': {'command': 'grep -r foo {repo}/src'}}),
    ('grep dopo una pipe', True,
     {'tool_name': 'Bash', 'tool_input': {'command': 'ls | grep -r foo'}}),
    ('grep non ricorsivo', True,
     {'tool_name': 'Bash', 'tool_input': {'command': 'grep foo {repo}/src/a.ts'}}),
    ('rg in posizione di comando', True,
     {'tool_name': 'Bash', 'tool_input': {'command': 'rg foo {repo}/src'}}),
    ('Grep con path esplicito', True,
     {'tool_name': 'Grep', 'tool_input': {'path': '{repo}/src', 'pattern': 'foo'}}),
    ('Edit che rimuove un export', True,
     {'tool_name': 'Edit', 'tool_input': {
         'file_path': '{repo}/src/a.ts',
         'old_string': 'export const foo = 1;\nexport const bar = 2;',
         'new_string': 'export const foo = 1;'}}),
    ('Edit che cambia solo il corpo', True,
     {'tool_name': 'Edit', 'tool_input': {
         'file_path': '{repo}/src/a.ts',
         'old_string': 'export function foo() { return 1 }',
         'new_string': 'export function foo() { return 2 }'}}),
    ('Edit su file non TypeScript', True,
     {'tool_name': 'Edit', 'tool_input': {
         'file_path': '{repo}/src/a.md',
         'old_string': 'export const foo = 1;', 'new_string': ''}}),
    ('Write di un file di codice nuovo', True,
     {'tool_name': 'Write', 'tool_input': {'file_path': '{repo}/src/nuovo.ts'}}),
    ('Write di un documento', True,
     {'tool_name': 'Write', 'tool_input': {'file_path': '{repo}/src/nota.md'}}),
    ('Write in scratchpad', True,
     {'tool_name': 'Write', 'tool_input': {'file_path': '/private/tmp/x/scratchpad/a.ts'}}),
    ('Write fuori dall\'indice', False,
     {'tool_name': 'Write', 'tool_input': {'file_path': '{repo}/src/nuovo.ts'}}),
    ('strumento che non lo riguarda', True,
     {'tool_name': 'Read', 'tool_input': {'file_path': '{repo}/src/a.ts'}}),
]

print(f"{'caso':<40}{'node':>6}{'rust':>6}   esito")
print('-' * 70)
divergent = 0
for name, indexed, template in CASES:
    outcomes = []
    # **La stessa radice per entrambi**, ricreata da zero fra un'esecuzione e
    # l'altra. Due cartelle temporanee diverse darebbero due percorsi diversi
    # dentro il messaggio, e il confronto segnalerebbe una divergenza che è solo
    # sua — è successo al primo giro.
    root = Path(tempfile.gettempdir()) / 'gate-confronto'
    for cmd in (NODE, RUST):
        shutil.rmtree(root, ignore_errors=True)
        repo = scenario(root, indexed)
        payload = json.loads(json.dumps(template).replace('{repo}', str(repo)))
        payload['session_id'] = 'prova'
        payload['cwd'] = str(repo)
        result = run(cmd, payload, root)
        outcomes.append((result.returncode, result.stderr.strip(), journal(root)))
    shutil.rmtree(root, ignore_errors=True)

    (node_code, node_err, node_log), (rust_code, rust_err, rust_log) = outcomes
    same_text = node_err == rust_err
    same_log = node_log == rust_log
    ok = node_code == rust_code and same_text and same_log
    if not ok:
        divergent += 1
    if ok:
        note = 'uguali'
    elif node_code != rust_code:
        note = 'esito diverso'
    elif not same_text:
        note = 'testo diverso'
    else:
        note = 'REGISTRO diverso'
        print(f'    node: {node_log}')
        print(f'    rust: {rust_log}')
    print(f'{name:<40}{node_code:>6}{rust_code:>6}   {note}')
    if not ok:
        print(f'    node: {node_err[:120]!r}')
        print(f'    rust: {rust_err[:120]!r}')

print()

# ── La prova che riguarda lo stato: a quale giro blocca? ─────────────────────
#
# È la parte che i casi singoli non toccano. Il contatore parte a metà quota,
# quindi la prima ricerca di una sessione passa sempre: se le due
# implementazioni contassero in modo diverso, sopra si vedrebbero comunque due
# zeri uguali. Qui si guarda **dove** cade il blocco, che è l'unico numero in
# cui il comportamento con stato può divergere.
print('sequenza di ricerche nella stessa sessione: al quale giro blocca?')
blocking_turn = {}
for label, cmd in (('node', NODE), ('rust', RUST)):
    root = Path(tempfile.gettempdir()) / 'gate-confronto-sequenza'
    shutil.rmtree(root, ignore_errors=True)
    repo = scenario(root, indexed=True)
    payload = {
        'tool_name': 'Bash',
        'session_id': 'sequenza',
        'cwd': str(repo),
        'tool_input': {'command': f'grep -r foo {repo}/src'},
    }
    turn = None
    for i in range(1, 41):
        if run(cmd, payload, root).returncode == 2:
            turn = i
            break
    blocking_turn[label] = turn
    shutil.rmtree(root, ignore_errors=True)
    print(f'  {label}: blocca al giro {turn}')

if blocking_turn['node'] != blocking_turn['rust']:
    divergent += 1
    print('  ← DIVERGE: i due contano in modo diverso')
elif blocking_turn['node'] is None:
    divergent += 1
    print('  ← nessuno dei due ha mai bloccato in 40 giri: la prova non prova niente')

print()
print(f'divergenze totali: {divergent}')
sys.exit(1 if divergent else 0)
