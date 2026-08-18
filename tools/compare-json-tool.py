#!/usr/bin/env python3
"""Confronta `claude-hooks json` con i `python3 -c` che sostituisce.

L'ORACOLO E' IL PYTHON, come per ogni altro porto di questa configurazione. Qui
pero' l'originale non e' uno script: sono **tre righe dentro tre ganci scritti in
shell**, ricopiate qui sotto alla lettera. Se una di quelle righe cambia, questo
confronto va aggiornato a mano — non c'e' un file da importare, ed e' il motivo
per cui i percorsi stanno scritti accanto a ciascuna.

    handoff-precompact.sh:27      print(d.get(sys.argv[1], ""))          -> json get
    scratchpad-at-session-end.sh  print(json.load(...).get("session_id","")) -> json get
    handoff-precompact.sh:126     json.dumps({"systemMessage": ...})     -> json message
    inject-instincts.sh:35        json.dumps({"hookSpecificOutput"...})  -> json context

SI CONFRONTA L'USCITA BYTE A BYTE, l'a capo compreso: questi valori finiscono in
un `$(...)` dentro uno script di shell, dove uno spazio in piu' diventa parte di
un nome di file.

I CASI STORTI CONTANO PIU' DEI DIRITTI. I quattro usi veri passano sempre un
oggetto con campi stringa, e su quelli qualunque implementazione va bene. Le
divergenze, se ci sono, stanno dove il gancio riceve un payload troncato, una
lista al posto di un oggetto, o un campo che non e' una stringa — cioe' proprio
dove l'originale aveva `2>/dev/null || true` a coprire tutto.

    compare-json-tool.py            il confronto
    compare-json-tool.py --debug    usa il binario di prova invece di quello vivo
"""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]           # ~/.claude
RELEASE = ROOT / 'rust' / 'target' / 'release' / 'claude-hooks'
DEBUG = ROOT / 'rust' / 'target' / 'debug' / 'claude-hooks'

# Le tre righe originali, ricopiate dai tre script.
GET = '''
import sys, json
try:
    d = json.load(sys.stdin)
except Exception:
    d = {}
print(d.get(sys.argv[1], ""))
'''

MESSAGE = '''
import json, sys
print(json.dumps({"systemMessage": sys.stdin.read()}))
'''

CONTEXT = '''
import json, sys
print(json.dumps({"hookSpecificOutput": {"hookEventName": sys.argv[1],
                                         "additionalContext": sys.stdin.read()}}))
'''

PAYLOADS = [
    '{"session_id":"abc123","cwd":"/home/someone/orca/general","trigger":"auto"}',
    '{"session_id":"","cwd":"/tmp"}',
    '{"transcript_path":"/a/b.jsonl","session_id":"d0ac4ff0-fe60-4930-a3d5-0db9ea873606"}',
    '{}',
    '',
    'not json at all',
    '{"session_id":',                       # troncato a meta'
    '[1,2,3]',                              # una lista, non un oggetto
    '"just a string"',
    'null',
    '{"session_id":3}',                     # numero
    '{"session_id":true}',                  # booleano
    '{"session_id":null}',                  # nullo esplicito
    '{"session_id":["a","b"]}',             # lista
    '{"session_id":{"a":1}}',               # dizionario
    '{"cwd":"/percorso con spazi/e-trattini"}',
    '{"cwd":"/percorso/con\\"virgolette"}',
    '{"trigger":"auto","session_id":"x","extra":{"nested":{"deep":1}}}',
    '{"session_id":"con\\naccapo"}',
    '{"session_id":"accenti: \u00e0 \u00e8 \u00ec \u00f2 \u00f9"}',
    '{"session_id":"fuori dal piano base: \U0001F680"}',
]

KEYS = ['session_id', 'cwd', 'trigger', 'transcript_path', 'missing']

TEXTS = [
    '',
    'una riga',
    'due\nrighe',
    'con "virgolette" e \\barre',
    'accenti: \u00e0 \u00e8 \u00ec \u00f2 \u00f9',
    'fuori dal piano base: \U0001F680 e un tab\the un ritorno\r',
    'x' * 5000,
]


def oracle(code: str, arg: str, stdin: str) -> str:
    r = subprocess.run([sys.executable, '-c', code, arg], input=stdin,
                       capture_output=True, text=True)
    return r.stdout


def port(binary: Path, args: list[str], stdin: str) -> str:
    r = subprocess.run([str(binary), 'json', *args], input=stdin,
                       capture_output=True, text=True)
    return r.stdout


def main() -> int:
    binary = DEBUG if '--debug' in sys.argv else RELEASE
    if not binary.exists():
        print(f'binario assente: {binary}')
        return 2
    # Un binario che non conosce ancora il sottocomando direbbe «gancio
    # sconosciuto» su ogni caso, e il confronto conterebbe cento divergenze
    # identiche invece di dire la cosa vera.
    if 'sconosciuto' in port(binary, ['get', 'x'], '{}'):
        print(f'{binary.name} non conosce ancora `json`: ricompila, oppure usa --debug')
        return 2

    cases = 0
    diverging = []

    for payload in PAYLOADS:
        for key in KEYS:
            a = oracle(GET, key, payload)
            b = port(binary, ['get', key], payload)
            cases += 1
            if a != b:
                diverging.append(('get', repr(payload)[:60], key, repr(a), repr(b)))

    for text in TEXTS:
        a = oracle(MESSAGE, '', text)
        b = port(binary, ['message'], text)
        cases += 1
        if a != b:
            diverging.append(('message', repr(text)[:60], '', repr(a)[:80], repr(b)[:80]))

        for event in ('SessionStart', 'PreCompact'):
            a = oracle(CONTEXT, event, text)
            b = port(binary, ['context', event], text)
            cases += 1
            if a != b:
                diverging.append(('context', repr(text)[:60], event, repr(a)[:80], repr(b)[:80]))

    print(f'{cases} casi confrontati contro le tre righe originali')
    print(f'  divergenze: {len(diverging)}')
    for mode, payload, arg, a, b in diverging[:8]:
        print(f'\n  --- {mode} {arg} su {payload}')
        print(f'  python: {a}')
        print(f'  rust:   {b}')
    return 1 if diverging else 0


if __name__ == '__main__':
    sys.exit(main())
