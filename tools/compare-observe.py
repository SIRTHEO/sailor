#!/usr/bin/env python3
"""Confronta `observe.sh` con la sua porta in Rust.

Questo gancio non decide niente: l'unica cosa che produce è **la riga che
scrive**. Confrontare l'esito direbbe «uguali» anche se uno dei due non
scrivesse affatto — che è esattamente il modo in cui un raccoglitore si rompe
senza che nessuno se ne accorga per un mese.

Si confronta quindi la riga, campo per campo, tipi compresi, con l'istante
tolto perché differisce sempre.

    compare-observe.py
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

SH = ['bash', str(Path.home() / '.claude/skills/hooks/observe.sh')]
RUST = [str(Path.home() / '.claude/rust/target/release/claude-hooks'), 'observe']

CASES = [
    ('pre', {'tool_name': 'Bash', 'session_id': 's1',
             'tool_input': {'command': 'echo ciao'}}),
    ('post', {'tool_name': 'Bash', 'session_id': 's1',
              'tool_input': {'command': 'echo ciao'},
              'tool_response': {'stdout': 'ciao', 'stderr': ''}}),
    ('post', {'tool_name': 'Bash', 'session_id': 's1',
              'tool_response': {'stdout': '', 'stderr': 'boom'}}),
    ('post', {'tool_name': 'Bash', 'session_id': 's1',
              'tool_response': {'is_error': True, 'stdout': 'x'}}),
    ('post', {'tool_name': 'Read', 'session_id': 's2',
              'tool_response': 'Error: no such file'}),
    ('post', {'tool_name': 'Read', 'session_id': 's2',
              'tool_response': 'tutto bene'}),
    ('post', {'tool_name': 'Edit', 'session_id': 's3',
              'tool_response': {'interrupted': True}}),
    ('pre', {'tool_name': 'Write', 'session_id': 's3',
             'tool_input': {'file_path': '/x/y.ts', 'content': 'a' * 6000}}),
    # accenti: il troncamento va fatto sui caratteri, non sui byte
    ('pre', {'tool_name': 'Bash', 'session_id': 's4',
             'tool_input': {'command': 'è' * 6000}}),
    ('post', {'tool_name': 'Bash', 'session_id': 's5', 'tool_response': ''}),
]


def line_written(cmd, phase, payload, home):
    obs = Path(home) / '.claude/homunculus/observations.jsonl'
    if obs.exists():
        obs.unlink()
    env = dict(os.environ, HOME=str(home))
    subprocess.run(cmd + [phase], input=json.dumps(payload), capture_output=True,
                   text=True, env=env)
    if not obs.exists():
        return None
    written = obs.read_text().splitlines()
    if len(written) != 1:
        return ('RIGHE', len(written))
    record = json.loads(written[0])
    record.pop('timestamp', None)
    # il tipo fa parte del confronto: true e "true" non sono la stessa cosa
    return sorted((k, type(v).__name__, v) for k, v in record.items())


home = Path(tempfile.mkdtemp(prefix='observe-confronto-'))
(home / '.claude/homunculus').mkdir(parents=True)

print(f"{'caso':<34}{'esito'}")
print('-' * 52)
divergent = 0
for phase, payload in CASES:
    a = line_written(SH, phase, payload, home)
    b = line_written(RUST, phase, payload, home)
    same = a == b
    if not same:
        divergent += 1
    tool = payload.get('tool_name', '?')
    label = f'{phase} {tool} {"errore" if "err" in str(a) else ""}'.strip()
    print(f'{label:<34}{"uguali" if same else "DIVERGE"}')
    if not same:
        print(f'    sh:   {a}')
        print(f'    rust: {b}')

shutil.rmtree(home, ignore_errors=True)
print(f'\ndivergenze: {divergent} su {len(CASES)}')
sys.exit(1 if divergent else 0)
