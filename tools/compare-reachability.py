#!/usr/bin/env python3
"""Equivalenza fra `reachability.py` e il porto `claude-hooks reachability`.

COSA CONFRONTA. Non i conteggi: il **verdetto per file**. Un banco che si ferma
al numero certifica un totale, non un comportamento — due lati che sbagliano due
file in direzioni opposte danno lo stesso totale e sono in disaccordo su tutto.
Qui si confrontano gli insiemi dei raggiunti, la ragione di ogni irraggiungibile
e chi lo cita, uno per uno.

PERCHE' COSTRUISCE LE HOME. Le regioni che contano non si possono osservare sulla
macchina vera: nessuna etichetta launchd inventata risulta caricata, e le schede
accese oggi sono zero. Ogni scenario costruisce un albero finto e fissa quel che
launchd risponderebbe con `REACH_LAUNCHCTL_LIST`/`_DISABLED`, che entrambi i
porti leggono.

LE REGIONI, una per scenario: la radice viva contro quella che esiste e non si
accende · il cammino transitivo · il ripiego di un gancio portato e i suoi moduli
· la catena dietro un lavoro launchd spento · lo strumento datato · il confine
dei nomi che si contengono.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

RUST = Path.home() / '.claude/rust/target/release/claude-hooks'
PY = Path('/Users/theo/gyver/work/.claude/scripts/reachability.py')

LISTING_LOADED = "PID\tStatus\tLabel\n-\t0\twork.gyver.vivo\n-\t0\twork.gyver.spento\n"
DISABLED_ONE = '"work.gyver.spento" => disabled\n'


def build(root: Path, spec: dict) -> dict:
    """Costruisce un albero finto e torna l'ambiente per i due porti."""
    ws = root / 'ws'
    hc = root / 'casa'
    agents = root / 'agents'
    for d in (ws / '.claude/automazioni', ws / '.claude/scripts',
              hc / 'scripts', hc / 'skills/hooks', agents, hc / 'rust/crates/x/src'):
        d.mkdir(parents=True, exist_ok=True)

    for name, text in spec.get('settings', {}).items():
        (hc / name).write_text(text)
    for name, text in spec.get('plist', {}).items():
        (agents / name).write_text(text)
    for name, text in spec.get('cards', {}).items():
        (ws / '.claude/automazioni' / name).write_text(text)
    for name, text in spec.get('ws_scripts', {}).items():
        (ws / '.claude/scripts' / name).write_text(text)
    for name, text in spec.get('home_scripts', {}).items():
        (hc / 'scripts' / name).write_text(text)
    for name, text in spec.get('hooks', {}).items():
        (hc / 'skills/hooks' / name).write_text(text)
    for name, text in spec.get('rust', {}).items():
        (hc / 'rust/crates/x/src' / name).write_text(text)

    env = dict(os.environ)
    env['REACH_WORKSPACE'] = str(ws)
    env['REACH_HOME_CLAUDE'] = str(hc)
    env['REACH_LAUNCH_AGENTS'] = str(agents)
    env['REACH_LAUNCHCTL_LIST'] = spec.get('launchctl', LISTING_LOADED)
    env['REACH_LAUNCHCTL_DISABLED'] = spec.get('disabled', DISABLED_ONE)
    return env


def read(cmd: list[str], env: dict) -> dict | str:
    p = subprocess.run(cmd, capture_output=True, text=True, env=env, timeout=180)
    try:
        return json.loads(p.stdout)
    except ValueError:
        return f'NON-JSON (rc={p.returncode}): {p.stdout[:120]} {p.stderr[:200]}'


def comparable(d: dict) -> dict:
    """Cio' che deve combaciare.

    Fuori resta il conteggio dei raggiunti da quale radice: nel Python il
    `why` del raggiunto dipende dall'ordine della pila e non e' osservabile nel
    rapporto. Si confronta cio' che cambia il comportamento.
    """
    if not isinstance(d, dict):
        return {'errore': d}
    return {
        'nodes': d.get('nodes'),
        'reached': d.get('reached'),
        'roots': sorted(Path(r['path']).name + ' | ' + r['why']
                        for r in d.get('roots', [])),
        'orphans': sorted(
            f"{o['file']} | {o['reason']} | {','.join(sorted(o.get('cited_by') or []))}"
            for o in d.get('orphans', [])),
    }


HOOK = '#!/usr/bin/env python3\nprint(1)\n'

SCENARI: list[tuple[str, dict]] = [
    ('una radice e cio che si accende, non cio che esiste', {
        'settings': {'settings.json': '{"h": "bash strumento.sh"}'},
        'plist': {'vivo.plist': '<key>Label</key><string>work.gyver.vivo</string>'
                                '\nbash da-vivo.sh',
                  'spento.plist': '<key>Label</key><string>work.gyver.spento</string>'
                                  '\nbash da-spento.sh'},
        'ws_scripts': {'strumento.sh': HOOK, 'da-vivo.sh': HOOK,
                       'da-spento.sh': HOOK, 'nessuno.sh': HOOK},
    }),
    ('il cammino e transitivo su tre anelli', {
        'settings': {'settings.json': '{"h": "bash a.sh"}'},
        'ws_scripts': {'a.sh': 'bash b.sh\n', 'b.sh': 'python c.py\n',
                       'c.py': HOOK, 'orfano.py': '# b.sh nominato in un commento\n'},
    }),
    ('un ripiego portato e i suoi moduli non sono difetti', {
        'settings': {'settings.json': '{"h": "claude-hooks cd-guard"}'},
        'hooks': {'cd-guard.py': 'import comune\n', 'comune.py': HOOK,
                  'nipote.py': HOOK},
        'ws_scripts': {'x.sh': HOOK},
        'rust': {'porto.rs': '//! Porta di linear-sola-lettura.py\n'},
    }),
    ('il ripiego col nome italiano lo salva il sorgente Rust', {
        'settings': {'settings.json': '{"h": "claude-hooks linear-readonly"}'},
        'hooks': {'linear-sola-lettura.py': HOOK},
        'rust': {'linear_readonly.rs': '//! Porta di linear-sola-lettura.py\n'},
    }),
    ('la catena dietro un launchd spento e in pausa, non dimenticata', {
        'settings': {'settings.json': '{"h": "echo"}'},
        'plist': {'spento.plist': '<key>Label</key><string>work.gyver.spento</string>'
                                  '\npython motore.py'},
        'ws_scripts': {'motore.py': 'import oracolo\n', 'oracolo.py': 'import capienza\n',
                       'capienza.py': HOOK, 'solo.py': HOOK},
    }),
    ('una scheda spenta porta con se il suo corpo', {
        'settings': {'settings.json': '{"h": "echo"}'},
        'cards': {'auto.scheda.json': '{"attiva": false}',
                  'viva.scheda.json': '{"attiva": true}'},
        'ws_scripts': {'auto.sh': 'python dietro.py\n', 'dietro.py': HOOK},
    }),
    ('una scheda accesa e una radice, e il suo mandato un nodo', {
        'settings': {'settings.json': '{"h": "echo"}'},
        'cards': {'viva.scheda.json': '{"attiva": true}',
                  'viva.mandato.md': 'esegui python dal-mandato.py\n'},
        'ws_scripts': {'dal-mandato.py': HOOK},
    }),
    ('lo strumento datato non e un difetto', {
        'settings': {'settings.json': '{"h": "echo"}'},
        'ws_scripts': {'clean-orphans-2026-08-16.sh': HOOK,
                       'test-leftovers-2026-08-18.py': HOOK,
                       'cross-repo.py': HOOK},
    }),
    ('i nomi che si contengono non si tengono in vita', {
        'settings': {'settings.json': '{"h": "bash chiudi-rami.py"}'},
        'ws_scripts': {'chiudi-rami.py': HOOK, 'rami.py': HOOK, 'rami.python': HOOK},
    }),
    ('nessun plist caricato: tutti dormienti', {
        'settings': {'settings.json': '{"h": "echo"}'},
        'launchctl': "PID\tStatus\tLabel\n",
        'disabled': '',
        'plist': {'uno.plist': '<key>Label</key><string>work.gyver.uno</string>'
                               '\nbash dietro.sh'},
        'ws_scripts': {'dietro.sh': HOOK},
    }),
    ('un plist senza campo Label ripiega sul nome del file', {
        'settings': {'settings.json': '{"h": "echo"}'},
        'launchctl': "PID\tStatus\tLabel\n-\t0\tsenza-campo\n",
        'disabled': '',
        'plist': {'senza-campo.plist': '<dict></dict>\nbash dietro.sh'},
        'ws_scripts': {'dietro.sh': HOOK},
    }),
    ('disabled batte loaded', {
        'settings': {'settings.json': '{"h": "echo"}'},
        'launchctl': "PID\tStatus\tLabel\n-\t0\twork.gyver.due\n",
        'disabled': '"work.gyver.due" => disabled\n',
        'plist': {'due.plist': '<key>Label</key><string>work.gyver.due</string>'
                               '\nbash dietro.sh'},
        'ws_scripts': {'dietro.sh': HOOK},
    }),
    ('lo stato e la cache non sono nodi', {
        'settings': {'settings.json': '{"h": "echo"}'},
        'ws_scripts': {'vero.sh': HOOK},
    }),
]


def main() -> int:
    if not RUST.exists():
        print(f'binario assente: {RUST}', file=sys.stderr)
        return 2
    if not PY.exists():
        print(f'script assente: {PY}', file=sys.stderr)
        return 2

    divergenze = 0
    for nome, spec in SCENARI:
        with tempfile.TemporaryDirectory() as td:
            env = build(Path(td), spec)
            # Lo stato e la cache vanno creati dopo, per provare che nessuno dei
            # due li conta come nodi lanciabili.
            stato = Path(env['REACH_WORKSPACE']) / '.claude/state'
            stato.mkdir(parents=True, exist_ok=True)
            (stato / 'finto.sh').write_text(HOOK)
            cache = Path(env['REACH_WORKSPACE']) / '.claude/scripts/__pycache__'
            cache.mkdir(parents=True, exist_ok=True)
            (cache / 'finto.py').write_text(HOOK)

            a = comparable(read(['python3', str(PY), '--json'], env))
            b = comparable(read([str(RUST), 'reachability', '--json'], env))
            if a != b:
                divergenze += 1
                print(f'DIVERGE  {nome}')
                for k in sorted(set(a) | set(b)):
                    if a.get(k) != b.get(k):
                        print(f'   {k}:\n     py:   {a.get(k)}\n     rust: {b.get(k)}')

    print(f'\n{len(SCENARI)} scenari confrontati · {divergenze} divergenze')
    return 1 if divergenze else 0


if __name__ == '__main__':
    sys.exit(main())
