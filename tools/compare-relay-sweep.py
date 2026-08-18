#!/usr/bin/env python3
"""Confronta la spazzata dello stato orfano: `relay.orphan_tree_state` col porto.

L'ORACOLO È IL PYTHON, come per ogni altro porto di questa configurazione: è lui
che gira sotto launchd quando il binario manca, quindi il porto è giusto quando
non lo si distingue.

QUI IL DISCO SERVE, e non per comodità: la decisione dipende dall'**età** dei
file e dai nomi che stanno in `state/`, cioè da due cose che solo un filesystem
vero produce. Ogni caso fabbrica una cartella con dentro i suoi file, e i due
lati la leggono entrambi. Niente `HOME` finta, però: la radice entra come
parametro in tutte e due le implementazioni, proprio perché una prova non debba
giudicare lo stato di chi sta lavorando adesso.

SI CONFRONTA UN INSIEME, NON UN ELENCO. `glob` e `read_dir` non ordinano allo
stesso modo, e confrontare l'ordine significherebbe confrontare il capriccio del
filesystem invece della decisione: entrambi i lati ordinano prima di rispondere.

LA DISTINZIONE CHE CONTA È `None` CONTRO `[]`. Elenco illeggibile ed elenco vuoto
arrivano fin qui separati e devono restare separati: leggere il silenzio come
«nessun albero è vivo» vuol dire cancellare tutto lo stato della macchina, ed è
il difetto che `read_live_handles` ha già pagato una volta, per 276 giri.

    compare-relay-sweep.py           il confronto (casi fissi + generati)
    compare-relay-sweep.py --casi N  quanti casi generati
"""
from __future__ import annotations

import json
import os
import random
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

RADICE = Path(__file__).resolve().parents[2]          # ~/.claude
BIN = RADICE / 'rust' / 'target' / 'release' / 'claude-hooks'

# Un istante fisso: il periodo di grazia si giudica su una differenza, e un `now`
# che scorre fra i due lati renderebbe irriproducibile ogni confine.
ORA = 1_787_000_000.0

# Il driver del lato Python. `relay.py` fissa `STATE_DIR` all'import: qui la
# radice si passa per parametro, ma l'import resta in un processo a parte perché
# il confronto non dipenda dallo stato della macchina che lo lancia.
DRIVER = '''
import json, sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
import relay

for riga in sys.stdin:
    if not riga.strip():
        continue
    caso = json.loads(riga)
    vivi = caso.get('live')
    radice = Path(caso['root'])
    fuori = relay.orphan_tree_state(
        None if vivi is None else set(vivi), caso.get('now') or 0, radice)
    print(json.dumps({'stale': sorted(str(f.relative_to(radice)) for f in fuori)}))
'''

FAMIGLIE = ('catene/{k}.json', 'riprendi-da/{k}.txt',
            'staffetta-cooldown-{k}', 'catena-bloccata-{k}')

VIVA = 'repo::_Users_theo_orca_viva'
MORTA = 'repo::_Users_theo_orca_morta'


def scrivi(radice: Path, rel: str, eta_sec) -> None:
    f = radice / rel
    f.parent.mkdir(parents=True, exist_ok=True)
    f.write_text('x')
    if eta_sec is not None:
        quando = ORA - eta_sec
        os.utime(f, (quando, quando))


def casi_fissi(base: Path) -> list:
    """I casi scritti a mano, ognuno nella sua cartella."""
    casi = []
    vecchio = 7200.0        # oltre il periodo di grazia

    def caso(nome: str, prepara, live) -> None:
        radice = base / nome.replace(' ', '-')
        radice.mkdir(parents=True, exist_ok=True)
        prepara(radice)
        casi.append({'nome': nome, 'root': str(radice), 'live': live, 'now': ORA})

    def quattro_famiglie(radice: Path) -> None:
        for schema in FAMIGLIE:
            scrivi(radice, schema.format(k=VIVA), vecchio)
            scrivi(radice, schema.format(k=MORTA), vecchio)

    caso('un albero morto, quattro famiglie', quattro_famiglie, [VIVA])
    caso('elenco illeggibile: non si tocca niente', quattro_famiglie, None)
    caso('elenco vuoto: non si tocca niente', quattro_famiglie, [])
    caso('nessuno di quelli sul disco e vivo', quattro_famiglie, ['repo::_altro'])

    def fresco(radice: Path) -> None:
        scrivi(radice, f'catene/{MORTA}.json', 60.0)          # dentro la grazia
        scrivi(radice, f'catene/{VIVA}.json', vecchio)

    caso('un file appena scritto non e orfano', fresco, [VIVA])

    def al_confine(radice: Path) -> None:
        scrivi(radice, f'catene/{MORTA}.json', 3600.0)        # esattamente
        scrivi(radice, f'riprendi-da/{MORTA}.txt', 3599.0)    # un secondo prima

    caso('il confine del periodo di grazia', al_confine, [VIVA])

    def estranei(radice: Path) -> None:
        # Nomi che NON appartengono alle quattro famiglie: non si toccano mai.
        scrivi(radice, 'staffetta.log', vecchio)
        scrivi(radice, 'consegna-fatta-abcd1234', vecchio)
        scrivi(radice, 'catene/non-un-json.txt', vecchio)
        scrivi(radice, f'successore-di-{MORTA}', vecchio)
        scrivi(radice, f'catene/{MORTA}.json', vecchio)

    caso('gli estranei restano dove sono', estranei, [VIVA])

    def cartelle(radice: Path) -> None:
        # Una cartella dentro `catene/` non e' un file di stato.
        (radice / 'catene' / f'{MORTA}.json').mkdir(parents=True, exist_ok=True)
        scrivi(radice, f'riprendi-da/{MORTA}.txt', vecchio)

    caso('una cartella non e un file di stato', cartelle, [VIVA])

    caso('radice inesistente', lambda r: shutil.rmtree(r, ignore_errors=True), [VIVA])
    caso('radice vuota', lambda r: None, [VIVA])

    def chiave_vuota(radice: Path) -> None:
        # Il prefisso c'e', la chiave no: non e' lo stato di nessun albero.
        scrivi(radice, 'staffetta-cooldown-', vecchio)
        scrivi(radice, 'catene/.json', vecchio)

    caso('prefisso senza chiave', chiave_vuota, [VIVA])
    return casi


def casi_generati(base: Path, quanti: int) -> list:
    r = random.Random(20260818)
    chiavi = [VIVA, MORTA, 'repo::_Users_theo_orca_terza', 'wt-prova', 'repo::']
    casi = []
    for i in range(quanti):
        radice = base / f'generato-{i}'
        radice.mkdir(parents=True, exist_ok=True)
        for _ in range(r.randint(0, 6)):
            schema = r.choice(FAMIGLIE)
            # Le età cadono attorno al confine dell'ora, che è l'unico che conta.
            eta = r.choice([0.0, 59.0, 3599.0, 3600.0, 3601.0, 86_400.0])
            scrivi(radice, schema.format(k=r.choice(chiavi)), eta)
        vivi = r.choice([None, [], [VIVA], [VIVA, MORTA], ['repo::_altro'], chiavi])
        casi.append({'nome': f'generato {i}', 'root': str(radice),
                     'live': vivi, 'now': ORA})
    return casi


def domanda(casi: list) -> str:
    return '\n'.join(json.dumps({k: c[k] for k in ('root', 'live', 'now')})
                     for c in casi)


def lato_python(casi: list, base: Path) -> list:
    driver = base / 'driver.py'
    driver.write_text(DRIVER)
    out = subprocess.run([sys.executable, str(driver), str(RADICE / 'skills' / 'hooks')],
                         input=domanda(casi), capture_output=True, text=True)
    if out.returncode != 0:
        print(f'il driver python ha risposto {out.returncode}: {out.stderr[:400]}')
        sys.exit(2)
    return [json.loads(l) for l in out.stdout.splitlines() if l.strip()]


def lato_rust(casi: list) -> list:
    out = subprocess.run([str(BIN), 'relay-sweep'], input=domanda(casi),
                         capture_output=True, text=True)
    if out.returncode != 0:
        print(f'il binario ha risposto {out.returncode}: {out.stderr[:300]}')
        sys.exit(2)
    return [json.loads(l) for l in out.stdout.splitlines() if l.strip()]


def main() -> int:
    if not BIN.exists():
        print(f'binario assente: {BIN}\ncompila con: cargo build --release -p claude-hooks')
        return 2
    quanti = 200
    if '--casi' in sys.argv:
        quanti = int(sys.argv[sys.argv.index('--casi') + 1])
    base = Path(tempfile.mkdtemp(prefix='confronto-spazzata-'))
    try:
        casi = casi_fissi(base) + casi_generati(base, quanti)
        py = lato_python(casi, base)
        rs = lato_rust(casi)
        if len(py) != len(rs):
            print(f'risposte diverse di numero: python {len(py)}, rust {len(rs)}')
            return 1
        diverse, esempi, buttati, casi_con_orfani = 0, [], 0, 0
        for caso, a, b in zip(casi, py, rs):
            buttati += len(a['stale'])
            casi_con_orfani += 1 if a['stale'] else 0
            if a != b:
                diverse += 1
                if len(esempi) < 5:
                    esempi.append((caso['nome'], a, b))
        print(f'{len(casi)} casi confrontati · {casi_con_orfani} con almeno un '
              f'orfano · {buttati} file dichiarati orfani dall\'oracolo')
        for nome, a, b in esempi:
            print(f'\n  --- divergenza su «{nome}» ---')
            print(f'    python: {json.dumps(a)[:220]}')
            print(f'    rust:   {json.dumps(b)[:220]}')
        print(f'  divergenze: {diverse}')
        # Un confronto in cui nessuno butta niente proverebbe solo che i due lati
        # sanno tacere insieme.
        if buttati == 0:
            print('\nCASI TROPPO POVERI: l\'oracolo non ha dichiarato orfano nessun file')
            return 1
        return 1 if diverse else 0
    finally:
        shutil.rmtree(base, ignore_errors=True)


if __name__ == '__main__':
    sys.exit(main())
