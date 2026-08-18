#!/usr/bin/env python3
"""Confronta il freno della catena: `handoff_common.chain_verdict` contro il porto Rust.

L'ORACOLO È IL PYTHON, come per ogni altro porto di questa configurazione: è lui
che gira sotto launchd quando il binario manca, quindi il porto è giusto quando
non lo si distingue.

QUI NON SERVE UNA HOME FINTA, e non è un caso: la decisione è pura da entrambi i
lati — la storia della catena entra come dato invece di essere letta dal disco.
È la stessa separazione che rende confrontabile `evaluate`, ma qui è arrivata
prima del codice invece che dopo, e si vede: nessuno stato da fabbricare, nessuna
fotografia di `state/` da rimettere a posto.

SI CONFRONTANO TRE COSE, non una. Il verdetto (go/reset/stop) è ciò che cambia il
comportamento; il motivo porta i numeri che finiscono nel registro di Theo, e una
divergenza lì è una divergenza vera — le ore arrotondate hanno già fatto litigare
`round()` di Python con `f64::round` di Rust in questa configurazione, sei casi su
1932. La coda sterile è la misura da cui dipende il verdetto di stallo: contarla a
parte dice se una divergenza nasce dalla misura o dalla decisione.

    compare-relay-chain.py           il confronto (1000 casi generati + i fissi)
    compare-relay-chain.py --casi N  quanti casi generati
"""
from __future__ import annotations

import json
import random
import subprocess
import sys
from pathlib import Path

RADICE = Path(__file__).resolve().parents[2]          # ~/.claude
BIN = RADICE / 'rust' / 'target' / 'release' / 'claude-hooks'
sys.path.insert(0, str(RADICE / 'skills' / 'hooks'))
import handoff_common as hc                            # noqa: E402

# Un istante fisso: tutto qui si giudica su differenze, e un `now` che scorre fra
# i due lati renderebbe irriproducibile ogni confine.
ORA = 1_755_400_000.0

# I casi che vengono dal registro vero, non dalla fantasia: le due catene di
# `-Users-theo-orca-general` e `-Users-theo-gyver-work-suite` del 17-18/08/2026.
CASI_FISSI = [
    # catena vuota: non frena mai
    {'links': [], 'now': ORA},
    # i quattro anelli veri di `general`, ognuno con la sua consegna
    {'links': [
        {'at': ORA - 19_000, 'writes': 52, 'handoff': '/a.md'},
        {'at': ORA - 7_000, 'writes': 31, 'handoff': '/b.md'},
        {'at': ORA - 2_500, 'writes': 12, 'handoff': '/c.md'},
        {'at': ORA - 100, 'writes': 8, 'handoff': '/d.md'},
    ], 'now': ORA},
    # la fuga: una rigenerazione al minuto
    {'links': [{'at': ORA - (12 - i) * 60, 'writes': 3, 'handoff': f'/{i}.md'}
               for i in range(12)], 'now': ORA},
    # lo stallo: stessa consegna, zero scritture
    {'links': [{'at': ORA - (4 - i) * 60, 'writes': 0, 'handoff': '/ferma.md'}
               for i in range(4)], 'now': ORA},
    # il confine esatto della scadenza per inattività
    {'links': [{'at': ORA - hc.CHAIN_IDLE_RESET_SEC, 'writes': 0, 'handoff': '/x.md'}],
     'now': ORA},
    {'links': [{'at': ORA - hc.CHAIN_IDLE_RESET_SEC + 1, 'writes': 0, 'handoff': '/x.md'}],
     'now': ORA},
    # il confine dell'età, con l'arrotondamento delle ore in mezzo
    {'links': [{'at': ORA - 86_400, 'writes': 1, 'handoff': '/a.md'},
               {'at': ORA - 100, 'writes': 1, 'handoff': '/b.md'}], 'now': ORA},
    # mezz'ora esatta: `round()` di Python arrotonda al pari, `f64::round` no
    {'links': [{'at': ORA - 88_200, 'writes': 1, 'handoff': '/a.md'},
               {'at': ORA - 100, 'writes': 1, 'handoff': '/b.md'}], 'now': ORA},
    # consegna vuota: «non lo so», non «uguale»
    {'links': [{'at': ORA - (4 - i) * 60, 'writes': 0, 'handoff': ''}
               for i in range(4)], 'now': ORA},
]

CONSEGNE = ['', '/a.md', '/b.md', '/ferma.md']


def casi_generati(quanti: int) -> list:
    r = random.Random(20260818)
    fuori = []
    for _ in range(quanti):
        n = r.randint(0, 13)
        # Le distanze coprono i due confini che contano: il minuto della fuga e
        # le sei ore della scadenza, più mezze ore esatte per l'arrotondamento.
        passo = r.choice([60, 900, 1800, 3600, 5400, 21_600, 43_200])
        links = []
        for i in range(n):
            links.append({
                'session': f's{i}',
                'at': ORA - (n - i) * passo,
                'turns': r.choice([0, 3, 100, 532]),
                'writes': r.choice([0, 0, 0, 1, 5, 52]),
                'handoff': r.choice(CONSEGNE),
            })
        caso = {'links': links, 'now': ORA}
        if r.random() < 0.3:
            caso['limits'] = {
                'max_links': r.choice([2, 3, 10, 40]),
                'max_age_sec': r.choice([3600.0, 86_400.0, 172_800.0]),
                'idle_reset_sec': r.choice([600.0, 21_600.0, 86_400.0]),
                'stall_links': r.choice([1, 2, 3, 5]),
            }
        fuori.append(caso)
    return fuori


def lato_python(casi: list) -> list:
    fuori = []
    for c in casi:
        limits = c.get('limits') or {
            'max_links': hc.CHAIN_MAX_LINKS,
            'max_age_sec': hc.CHAIN_MAX_AGE_SEC,
            'idle_reset_sec': hc.CHAIN_IDLE_RESET_SEC,
            'stall_links': hc.CHAIN_STALL_LINKS,
        }
        verdetto, perche = hc.chain_verdict(c['links'], c['now'], limits)
        fuori.append({'verdict': verdetto, 'reason': perche,
                      'sterile': hc.sterile_tail(c['links'])})
    return fuori


def lato_rust(casi: list) -> list:
    testo = '\n'.join(json.dumps(c) for c in casi)
    out = subprocess.run([str(BIN), 'relay-chain'], input=testo,
                         capture_output=True, text=True)
    if out.returncode != 0:
        print(f'il binario ha risposto {out.returncode}: {out.stderr[:300]}')
        sys.exit(2)
    return [json.loads(l) for l in out.stdout.splitlines() if l.strip()]


def main() -> int:
    if not BIN.exists():
        print(f'binario assente: {BIN}\ncompila con: cargo build --release -p claude-hooks')
        return 2
    quanti = 1000
    if '--casi' in sys.argv:
        quanti = int(sys.argv[sys.argv.index('--casi') + 1])
    casi = CASI_FISSI + casi_generati(quanti)
    py = lato_python(casi)
    rs = lato_rust(casi)
    if len(py) != len(rs):
        print(f'risposte diverse di numero: python {len(py)}, rust {len(rs)}')
        return 1

    diverse = {'verdict': 0, 'reason': 0, 'sterile': 0}
    esempi = []
    for caso, a, b in zip(casi, py, rs):
        for campo in diverse:
            if a[campo] != b[campo]:
                diverse[campo] += 1
                if len(esempi) < 5:
                    esempi.append((campo, caso, a, b))
    verdetti = {}
    for a in py:
        verdetti[a['verdict']] = verdetti.get(a['verdict'], 0) + 1

    print(f'{len(casi)} casi confrontati')
    print('  verdetti dell\'oracolo: ' + ', '.join(f'{k}={v}' for k, v in sorted(verdetti.items())))
    for campo, n in diverse.items():
        print(f'  {campo}: {n} divergenze')
    for campo, caso, a, b in esempi:
        print(f'\n  --- divergenza su {campo} ---')
        print(f'  caso:   {json.dumps(caso)[:300]}')
        print(f'  python: {a[campo]}')
        print(f'  rust:   {b[campo]}')
    # Un confronto in cui l'oracolo dà sempre la stessa risposta non prova
    # niente: si pretende che i tre verdetti compaiano tutti.
    if len(verdetti) < 3:
        print('\nCASI TROPPO POVERI: mancano dei verdetti, il confronto non discrimina')
        return 1
    return 1 if any(diverse.values()) else 0


if __name__ == '__main__':
    sys.exit(main())
