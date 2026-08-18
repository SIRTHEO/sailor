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
    radice = Path(caso['root'])
    if caso.get('orca_out') is not None:
        # Si parte dalla risposta GREZZA di Orca e si passa da
        # `live_worktree_keys`: era l'unico pezzo della catena fuori dal
        # confronto, ed e' quello che decide chi e' vivo.
        rc, out = int(caso.get('orca_rc') or 0), caso['orca_out']
        relay.orca = lambda *a, **k: (rc, out)
        try:
            vivi = relay.live_worktree_keys()
        except Exception as e:
            print(json.dumps({'stale': [], 'keys': [], 'known': False,
                              'eccezione': type(e).__name__}))
            continue
    else:
        vivi = caso.get('live')
        vivi = None if vivi is None else set(vivi)
    fuori = relay.orphan_tree_state(vivi, caso.get('now') or 0, radice)
    print(json.dumps({
        'stale': sorted(str(f.relative_to(radice)) for f in fuori),
        'keys': sorted(vivi or []),
        'known': vivi is not None,
    }))
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

    def piu_corti_del_prefisso(radice: Path) -> None:
        # LA CLASSE CHE HA FATTO PANICARE IL BINARIO SUL VIVO, il 18/08 alle
        # 15:30: `byte index 19 is out of bounds of 'census-needles.txt'` — un
        # nome piu' corto della somma di prefisso e suffisso, tagliato per
        # indice. Un giro di staffetta perso, non un file saltato. I nomi qui
        # sotto sono quelli veri di `state/`, non inventati: il banco partiva
        # da nomi lunghi e questa regione non la toccava nessuno.
        for nome in ('census-needles.txt', 'censimento-ganci.txt',
                     'catalogo-skill.json', 'a', 'x.json'):
            scrivi(radice, nome, vecchio)
        # E i nomi che combaciano col prefisso ma non lasciano nessuna chiave:
        # il taglio riesce e restituisce il vuoto, che non e' l'id di nessuno.
        for nome in ('staffetta-cooldown-', 'catena-bloccata-'):
            scrivi(radice, nome, vecchio)
        scrivi(radice, 'catene/.json', vecchio)
        scrivi(radice, 'riprendi-da/.txt', vecchio)
        scrivi(radice, f'catene/{MORTA}.json', vecchio)

    caso('nomi piu corti del prefisso', piu_corti_del_prefisso, [VIVA])

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

    # ─── Da qui in giu' si parte dalla risposta GREZZA di Orca ────────────────
    #
    # `live_worktree_keys` era l'unico anello fuori dal confronto, e il primo
    # caso grezzo ha trovato subito una divergenza: su un array nudo il porto
    # accettava gli elementi come copie di lavoro, il Python sollevava.
    def grezzo(nome: str, rc: int, out: str) -> None:
        radice = base / ('grezzo-' + nome.replace(' ', '-'))
        radice.mkdir(parents=True, exist_ok=True)
        for schema in FAMIGLIE:
            scrivi(radice, schema.format(k=VIVA), vecchio)
            scrivi(radice, schema.format(k=MORTA), vecchio)
        casi.append({'nome': f'grezzo: {nome}', 'root': str(radice),
                     'live': None, 'now': ORA, 'orca_rc': rc, 'orca_out': out})

    id_viva = f'repo::/home/someone/orca/viva'
    id_morta = f'repo::/home/someone/orca/morta'
    grezzo('la forma vera di oggi', 0,
           json.dumps({'id': 'x', 'result': {'worktrees': [
               {'id': id_viva, 'path': '/home/someone/orca/viva'}]}}))
    grezzo('result con items', 0, json.dumps({'result': {'items': [{'id': id_viva}]}}))
    grezzo('result e una lista', 0, json.dumps({'result': [{'id': id_viva}]}))
    grezzo('oggetto senza result', 0, json.dumps({'worktrees': [{'id': id_viva}]}))
    grezzo('array nudo', 0, json.dumps([{'id': id_viva}]))
    grezzo('array vuoto', 0, '[]')
    grezzo('oggetto vuoto', 0, '{}')
    grezzo('result vuoto', 0, json.dumps({'result': {'worktrees': []}}))
    grezzo('id vuoti', 0, json.dumps({'result': {'worktrees': [{'id': ''}, {}]}}))
    grezzo('id non stringa', 0, json.dumps({'result': {'worktrees': [{'id': 7}]}}))
    grezzo('due copie, una morta', 0,
           json.dumps({'result': {'worktrees': [{'id': id_viva}, {'id': id_morta}]}}))
    grezzo('numero al posto della risposta', 0, '42')
    grezzo('stringa JSON', 0, '"niente"')
    grezzo('null', 0, 'null')
    grezzo('testo non JSON', 0, 'boh')
    grezzo('uscita vuota', 0, '')
    grezzo('orca ha risposto male', 1, json.dumps({'result': {'worktrees': [{'id': id_viva}]}}))
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
    campi = ('root', 'live', 'now', 'orca_rc', 'orca_out')
    return '\n'.join(json.dumps({k: c[k] for k in campi if k in c})
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
        grezzi = sum(1 for c in casi if 'orca_out' in c)
        ignoti = sum(1 for a in py if not a.get('known'))
        print(f'{len(casi)} casi confrontati ({grezzi} dalla risposta grezza di '
              f'Orca) · {casi_con_orfani} con almeno un orfano · {buttati} file '
              f'dichiarati orfani dall\'oracolo · {ignoti} volte «non lo so»')
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
