#!/usr/bin/env python3
"""Confronta il freno della catena: `handoff_common.chain_verdict` contro il porto Rust.

L'ORACOLO È IL PYTHON, come per ogni altro porto di questa configurazione: è lui
che gira sotto launchd quando il binario manca, quindi il porto è giusto quando
non lo si distingue.

LA DECISIONE NON VUOLE UNA HOME FINTA, e non è un caso: è pura da entrambi i lati
— la storia della catena entra come dato invece di essere letta dal disco. È la
stessa separazione che rende confrontabile `evaluate`, ma qui è arrivata prima
del codice invece che dopo, e si vede.

LA LETTURA SÌ, e sta nella seconda metà. `read_chain` non è più un caricatore:
scarta la storia quando la cartella dell'albero è nata dopo il primo anello,
perché l'id di una copia è deterministico e rifarla con lo stesso nome le
riconsegna la catena della lavorazione morta. Quel segno è la data di nascita di
una cartella vera, quindi lì le cartelle si creano davvero, sotto una `HOME`
usa-e-getta.

SI CONFRONTANO TRE COSE, non una. Il verdetto (go/reset/stop) è ciò che cambia il
comportamento; il motivo porta i numeri che finiscono nel registro di Theo, e una
divergenza lì è una divergenza vera — le ore arrotondate hanno già fatto litigare
`round()` di Python con `f64::round` di Rust in questa configurazione, sei casi su
1932. La coda sterile è la misura da cui dipende il verdetto di stallo: contarla a
parte dice se una divergenza nasce dalla misura o dalla decisione.

    compare-relay-chain.py           il confronto (1000 casi generati + i fissi,
                                     poi i casi su disco della guardia)
    compare-relay-chain.py --casi N  quanti casi generati
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
sys.path.insert(0, str(RADICE / 'skills' / 'hooks'))
import handoff_common as hc                            # noqa: E402

# Un istante fisso: tutto qui si giudica su differenze, e un `now` che scorre fra
# i due lati renderebbe irriproducibile ogni confine.
ORA = 1_755_400_000.0

# I casi che vengono dal registro vero, non dalla fantasia: le due catene di
# `-Users-theo-orca-general` e `-Users-theo-other-repo-work-suite` del 17-18/08/2026.
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
                # I conteggi non sono numeri per l'oracolo: sono verità. Le
                # forme strane pesano quanto quelle normali, perché è lì che i
                # due lati si erano già separati due volte — `5.0` e `-5` non
                # stanno in un intero senza segno, `"0"` e `"  "` sono stringhe
                # piene, quindi vere.
                'turns': r.choice([0, 3, 100, 532, 3.0, -3, '7', '', True]),
                'writes': r.choice([0, 0, 0, 1, 5, 52, 5.0, -5, '0', '  ', False]),
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


# ─── La seconda metà: la guardia sull'albero ricreato ─────────────────────────
#
# Un `at` si sceglie sempre rispetto alla nascita **misurata** della cartella,
# mai rispetto all'orologio: fra il `mkdir` e la riga che scrive gli anelli
# passano millisecondi, e un confine calcolato su `time.time()` cadrebbe dalla
# parte sbagliata a seconda di quanto è occupata la macchina.

REPO_FINTO = 'f304fcc9-d036-47d2-a1fa-ba5661c0e9f9'
# Il margine della guardia, preso dal modulo invece che ricopiato: i casi al
# confine devono muoversi con la soglia, non restare dove stavano. L'import non
# scrive niente — `relay.py` esegue solo se lanciato come programma.
import relay as _relay                                 # noqa: E402
REBORN_MARGIN_SEC = _relay.REBORN_MARGIN_SEC

# Il driver del lato Python: `relay.py` fissa `STATE_DIR` all'import, quindi la
# `HOME` finta va messa nell'ambiente del processo, non nel nostro.
#
# ARRIVA FINO AL VERDETTO, e non e' un di piu'. Confrontare i soli anelli letti
# certifica una normalizzazione, non un comportamento: `writes` scritto `"0"`
# tornava identico da entrambe le parti e portava a decisioni opposte, perche'
# per Python quella stringa e' vera e per il porto valeva zero. Il campo si
# confronta com'e', la decisione si chiede a `chain_verdict` sugli anelli
# **grezzi** — che e' esattamente cio' che fa la staffetta.
DRIVER = '''
import json, sys
sys.path.insert(0, sys.argv[1])
import relay
import handoff_common as hc


def num(v):
    """`at` come lo legge chi decide: `chain_verdict` fa `float(at)`."""
    if isinstance(v, bool):
        return float(v)
    try:
        return float(v)
    except (TypeError, ValueError):
        return 0


def conta(v):
    """`writes` e `turns` NON sono numeri per l'oracolo: sono verita'.

    `_is_sterile` scrive `not link.get('writes')`, quindi la stringa `"0"` conta
    come lavoro fatto — e' una stringa non vuota. Il numero serve solo quando c'e'
    un numero. La regola sta qui perche' il Python non normalizza affatto: usa il
    valore grezzo dentro la condizione, e il porto deve arrivare allo stesso
    esito partendo da un intero.
    """
    if isinstance(v, bool):
        return int(v)
    if isinstance(v, (int, float)):
        # `5.0` e `-5` non stanno in un intero senza segno, ma sono VERI: valgono
        # «ha prodotto», e solo lo zero vale «non ha prodotto».
        if v == 0:
            return 0
        return v if isinstance(v, int) and v > 0 else 1
    if isinstance(v, str):
        try:
            n = int(v.strip())
        except ValueError:
            n = 0
        # `"0"`, `"-3"`, `"  "` non sono numeri utili ma sono stringhe piene, e
        # una stringa piena e' vera: si guarda `v`, non la versione spogliata.
        return n if n > 0 else (1 if v else 0)
    return 1 if v else 0


LIMITI = {'max_links': hc.CHAIN_MAX_LINKS, 'max_age_sec': hc.CHAIN_MAX_AGE_SEC,
          'idle_reset_sec': hc.CHAIN_IDLE_RESET_SEC,
          'stall_links': hc.CHAIN_STALL_LINKS}

for riga in sys.stdin:
    if not riga.strip():
        continue
    caso = json.loads(riga)
    anelli = relay.read_chain(caso.get('worktree') or '')
    fuori = {'links': [{
        'session': l.get('session') if isinstance(l.get('session'), str) else '',
        'at': num(l.get('at')),
        'turns': conta(l.get('turns')),
        'writes': conta(l.get('writes')),
        'handoff': l.get('handoff') if isinstance(l.get('handoff'), str) else '',
    } for l in anelli]}
    # `chain_verdict` fa `float(at)` senza rete: su un `at` che non e' un numero
    # solleva, e l'oracolo che esplode e' un fatto da riportare, non da nascondere
    # sotto un verdetto inventato.
    try:
        v, perche = hc.chain_verdict(anelli, caso.get('now') or 0, LIMITI)
        fuori.update({'verdict': v, 'reason': perche,
                      'sterile': hc.sterile_tail(anelli)})
    except Exception as e:
        fuori.update({'verdict': 'ECCEZIONE', 'reason': type(e).__name__,
                      'sterile': -1})
    print(json.dumps(fuori))
'''


def anello(at: float, i: int = 0) -> dict:
    return {'session': f's{i}', 'at': at, 'turns': 10, 'writes': 5,
            'handoff': f'/consegna{i}.md'}


def casi_disco(home: Path) -> list:
    """Fabbrica gli alberi e le catene, e restituisce i casi da porre ai due lati.

    Ogni caso è `(nome, worktree_id)`: la storia sta già sul disco della `HOME`
    finta, come la troverebbe la staffetta.
    """
    catene = home / '.claude' / 'state' / 'catene'
    catene.mkdir(parents=True, exist_ok=True)
    alberi = home / 'alberi'
    alberi.mkdir(parents=True, exist_ok=True)
    casi = []

    def albero(nome: str) -> tuple:
        """Una cartella vera, e la sua data di nascita letta dal filesystem."""
        d = alberi / nome
        d.mkdir(parents=True, exist_ok=True)
        return str(d), os.stat(d).st_birthtime

    def caso(nome: str, wt: str, contenuto, now: float = ORA,
             oracolo_esplode: bool = False) -> None:
        """`now` entra nel caso perché il verdetto si giudica su una differenza:
        preso dall'orologio, ogni corsa darebbe un'età diversa."""
        f = catene / f'{hc.state_key(wt)}.json'
        if contenuto is None:
            f.unlink(missing_ok=True)
        elif isinstance(contenuto, str):
            f.write_text(contenuto)
        else:
            f.write_text(json.dumps({'links': contenuto}, separators=(',', ':')))
        casi.append({'nome': nome, 'worktree': wt, 'now': now,
                     'oracolo_esplode': oracolo_esplode})

    d, nato = albero('albero-fermo')
    wt = f'{REPO_FINTO}::{d}'
    # La copia c'era già quando la catena è partita: la storia è sua.
    caso('albero piu vecchio della catena', wt,
         [anello(nato + 60 * (i + 1), i) for i in range(3)])

    d, nato = albero('albero-rifatto')
    caso('albero nato dopo il primo anello', f'{REPO_FINTO}::{d}',
         [anello(nato - 600 + 60 * i, i) for i in range(3)])

    d, nato = albero('albero-rifatto-lungo')
    caso('dodici anelli, tutti prima della nascita', f'{REPO_FINTO}::{d}',
         [anello(nato - 3600 + 60 * i, i) for i in range(12)])

    d, nato = albero('confine-esatto')
    # Nato NELLO stesso istante: la catena resta.
    caso('nascita uguale al primo anello', f'{REPO_FINTO}::{d}', [anello(nato)])

    # I due lati del margine di due minuti, che esiste perché una correzione
    # d'orologio all'indietro non tagli una catena viva.
    d, nato = albero('margine-esatto')
    caso('nascita al margine esatto', f'{REPO_FINTO}::{d}',
         [anello(nato - REBORN_MARGIN_SEC)])

    d, nato = albero('margine-superato')
    caso('nascita un secondo oltre il margine', f'{REPO_FINTO}::{d}',
         [anello(nato - REBORN_MARGIN_SEC - 1)])

    d, nato = albero('confine-un-soffio')
    caso('nascita un millisecondo dopo: dentro il margine', f'{REPO_FINTO}::{d}',
         [anello(nato - 0.001)])

    # I due numeri per esteso, che tengono il margine **da entrambi i lati**: i
    # casi qui sopra si muovono con la costante, quindi resterebbero muti sia se
    # andasse a zero sia se qualcuno la gonfiasse a dieci minuti «per prudenza» —
    # cioè proprio quando la guardia smette di prendere gli alberi rifatti.
    d, nato = albero('un-minuto-prima')
    caso('un minuto di scarto: nessun taglio', f'{REPO_FINTO}::{d}',
         [anello(nato - 60)], now=nato + 3600)

    d, nato = albero('cinque-minuti-prima')
    caso('cinque minuti di scarto: taglio', f'{REPO_FINTO}::{d}',
         [anello(nato - 300)], now=nato + 3600)

    # IL CASO CHE HA FATTO NASCERE QUESTA META' DEL CONFRONTO. `writes` scritto
    # `"0"`: per Python quella stringa è **vera**, quindi l'anello non è sterile;
    # un porto che la leggesse come zero conterebbe tre anelli sterili di fila e
    # fermerebbe una catena viva. I due lati devono dare lo stesso verdetto.
    d, nato = albero('writes-stringa-zero')
    caso('writes come stringa "0": non e sterile', f'{REPO_FINTO}::{d}',
         [{'session': f's{i}', 'at': nato + 60 * (i + 1), 'turns': 3,
           'writes': '0', 'handoff': '/ferma.md'} for i in range(4)],
         now=nato + 3600)

    # Le tre forme che `as_u64()` non regge, e che valgono «ha prodotto»: un
    # float, un negativo, una stringa di soli spazi.
    for nome, valore in (('writes-float', 5.0), ('writes-negativo', -5),
                         ('writes-spazi', '  ')):
        d, nato = albero(nome)
        caso(f'writes come {nome.split("-")[1]}: non e sterile', f'{REPO_FINTO}::{d}',
             [{'session': f's{i}', 'at': nato + 60 * (i + 1), 'turns': 3,
               'writes': valore, 'handoff': '/ferma.md'} for i in range(4)],
             now=nato + 3600)

    d, nato = albero('writes-zero-intero')
    # Il gemello con lo zero vero: qui lo stallo deve scattare da entrambe le
    # parti, altrimenti il caso sopra proverebbe solo che nessuno dei due conta.
    caso('writes zero intero: stallo', f'{REPO_FINTO}::{d}',
         [{'session': f's{i}', 'at': nato + 60 * (i + 1), 'turns': 3,
           'writes': 0, 'handoff': '/ferma.md'} for i in range(4)],
         now=nato + 3600)

    d, nato = albero('dodici-anelli-vissuto')
    # Dodici anelli su un albero che c'era già: il tetto morde, e il confronto
    # arriva a vedere uno `stop` che nasce dal disco invece che da un caso finto.
    caso('dodici anelli su albero vissuto: tetto', f'{REPO_FINTO}::{d}',
         [anello(nato + 60 * (i + 1), i) for i in range(12)], now=nato + 3600)

    d, nato = albero('primo-anello-vecchio-ultimo-nuovo')
    # Il verdetto guarda l'ultimo anello, la guardia il primo: qui divergono.
    caso('primo anello prima della nascita, ultimo dopo', f'{REPO_FINTO}::{d}',
         [anello(nato - 300, 0), anello(nato + 300, 1)])

    caso('cartella mai creata', f'{REPO_FINTO}::{alberi}/mai-esistito',
         [anello(1_700_000_000.0)])
    caso('id senza percorso', 'solo-un-nome', [anello(1_700_000_000.0)])

    d, nato = albero('at-a-zero')
    caso('primo anello con at a zero', f'{REPO_FINTO}::{d}',
         [{'session': 's0', 'at': 0, 'turns': 1, 'writes': 1, 'handoff': '/a.md'}])

    d, nato = albero('at-assente')
    caso('primo anello senza at', f'{REPO_FINTO}::{d}',
         [{'session': 's0', 'turns': 1, 'writes': 1, 'handoff': '/a.md'}])

    d, nato = albero('at-testo')
    # `chain_verdict` fa `float(at)` senza rete: qui l'oracolo **solleva**, e il
    # porto risponde con un verdetto. Si confrontano i soli anelli letti, e la
    # differenza si dichiara invece di nasconderla sotto un numero.
    caso('at che non e un numero', f'{REPO_FINTO}::{d}',
         [{'session': 's0', 'at': 'ieri', 'turns': 1, 'writes': 1, 'handoff': '/a.md'}],
         oracolo_esplode=True)

    # I TRE CASI CHE MANCAVANO, e non erano un dettaglio di completezza: con
    # `as_f64()` soltanto, il porto leggeva zero da una stringa numerica, non
    # vedeva mai l'albero rinato e teneva la catena morta — il difetto opposto a
    # quello che la guardia chiude, proprio nel campo su cui decide.
    d, nato = albero('at-stringa-rinato')
    caso('at come stringa numerica, albero rinato', f'{REPO_FINTO}::{d}',
         [{'session': 's0', 'at': str(nato - 600), 'turns': 1, 'writes': 1,
           'handoff': '/a.md'},
          {'session': 's1', 'at': str(nato - 540), 'turns': 1, 'writes': 1,
           'handoff': '/b.md'}])

    d, nato = albero('at-stringa-fermo')
    caso('at come stringa numerica, albero vissuto', f'{REPO_FINTO}::{d}',
         [{'session': 's0', 'at': f' {nato + 60} ', 'turns': '3', 'writes': '5',
           'handoff': '/a.md'}])

    d, nato = albero('at-booleano')
    # `float(True)` vale 1.0 anche in Python: un istante del 1970, quindi la
    # cartella e' sempre nata dopo.
    caso('at booleano', f'{REPO_FINTO}::{d}',
         [{'session': 's0', 'at': True, 'turns': 1, 'writes': 1, 'handoff': '/a.md'}])

    d, nato = albero('at-negativo')
    caso('at negativo', f'{REPO_FINTO}::{d}',
         [{'session': 's0', 'at': -5, 'turns': 1, 'writes': 1, 'handoff': '/a.md'}])

    d, nato = albero('catena-vuota')
    caso('elenco di anelli vuoto', f'{REPO_FINTO}::{d}', [])

    d, nato = albero('senza-file')
    caso('nessuna catena sul disco', f'{REPO_FINTO}::{d}', None)

    d, nato = albero('json-rotto')
    caso('file illeggibile', f'{REPO_FINTO}::{d}', '{non e json')

    d, nato = albero('links-non-lista')
    caso('links non e una lista', f'{REPO_FINTO}::{d}', '{"links":{"a":1}}')

    d, nato = albero('links-misti')
    # Un elemento che non è un oggetto: il Python lo scarta, e il porto deve
    # scartarlo pure — convertirlo darebbe un anello a `at` zero, cioè una storia
    # più lunga di quella vera.
    caso('elenco con elementi non oggetto', f'{REPO_FINTO}::{d}',
         json.dumps({'links': ['x', anello(nato + 60), 7]}))

    return casi


def lato_python_disco(home: Path, casi: list) -> list:
    driver = home / 'driver.py'
    driver.write_text(DRIVER)
    testo = '\n'.join(json.dumps({'worktree': c['worktree'], 'now': c['now']})
                      for c in casi)
    out = subprocess.run([sys.executable, str(driver), str(RADICE / 'skills' / 'hooks')],
                         input=testo, capture_output=True, text=True,
                         env={**os.environ, 'HOME': str(home)})
    if out.returncode != 0:
        print(f'il driver python ha risposto {out.returncode}: {out.stderr[:400]}')
        sys.exit(2)
    return [json.loads(l) for l in out.stdout.splitlines() if l.strip()]


def lato_rust_disco(home: Path, casi: list) -> list:
    testo = '\n'.join(json.dumps({'worktree': c['worktree'], 'now': c['now']})
                      for c in casi)
    out = subprocess.run([str(BIN), 'relay-read-chain'], input=testo,
                         capture_output=True, text=True,
                         env={**os.environ, 'HOME': str(home)})
    if out.returncode != 0:
        print(f'il binario ha risposto {out.returncode}: {out.stderr[:300]}')
        sys.exit(2)
    return [json.loads(l) for l in out.stdout.splitlines() if l.strip()]


def confronto_disco() -> tuple:
    """Ritorna (casi, divergenze, righe da stampare)."""
    home = Path(tempfile.mkdtemp(prefix='confronto-catene-'))
    try:
        casi = casi_disco(home)
        py = lato_python_disco(home, casi)
        rs = lato_rust_disco(home, casi)
        if len(py) != len(rs):
            return len(casi), 1, [f'  risposte diverse di numero: python {len(py)}, rust {len(rs)}']
        righe, diverse = [], 0
        tagliate, esplosi = 0, 0
        verdetti = {}
        for caso, a, b in zip(casi, py, rs):
            if not a['links']:
                tagliate += 1
            if caso['oracolo_esplode']:
                # Confronto ristretto agli anelli: il verdetto, di là, non esiste.
                esplosi += 1
                a = {'links': a['links']}
                b = {'links': b['links']}
            else:
                verdetti[a['verdict']] = verdetti.get(a['verdict'], 0) + 1
            if a != b:
                diverse += 1
                if len(righe) < 5:
                    righe.append(f"  --- divergenza su «{caso['nome']}» ---\n"
                                 f"    python: {json.dumps(a)[:220]}\n"
                                 f"    rust:   {json.dumps(b)[:220]}")
        # Un confronto in cui la guardia non taglia mai proverebbe solo che i due
        # lati leggono lo stesso file; e uno in cui il verdetto è sempre lo stesso
        # non dice niente su ciò che succede dopo la lettura.
        if tagliate == 0:
            righe.append('  CASI TROPPO POVERI: la guardia non ha tagliato nessuna catena')
            diverse += 1
        if len(verdetti) < 2:
            righe.append('  CASI TROPPO POVERI: dal disco esce sempre lo stesso verdetto')
            diverse += 1
        righe.insert(0, f'  catene azzerate dalla guardia: {tagliate} su {len(casi)}'
                        f' · verdetti: ' + ', '.join(f'{k}={v}' for k, v in sorted(verdetti.items()))
                        + (f' · oracolo che solleva: {esplosi}' if esplosi else ''))
        return len(casi), diverse, righe
    finally:
        shutil.rmtree(home, ignore_errors=True)


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

    print(f'{len(casi)} casi confrontati sulla decisione')
    print('  verdetti dell\'oracolo: ' + ', '.join(f'{k}={v}' for k, v in sorted(verdetti.items())))
    for campo, n in diverse.items():
        print(f'  {campo}: {n} divergenze')
    for campo, caso, a, b in esempi:
        print(f'\n  --- divergenza su {campo} ---')
        print(f'  caso:   {json.dumps(caso)[:300]}')
        print(f'  python: {a[campo]}')
        print(f'  rust:   {b[campo]}')
    n_disco, div_disco, righe = confronto_disco()
    print(f'\n{n_disco} casi confrontati sulla lettura da disco')
    for r in righe:
        print(r)
    print(f'  divergenze: {div_disco}')

    # Un confronto in cui l'oracolo dà sempre la stessa risposta non prova
    # niente: si pretende che i tre verdetti compaiano tutti.
    if len(verdetti) < 3:
        print('\nCASI TROPPO POVERI: mancano dei verdetti, il confronto non discrimina')
        return 1
    return 1 if any(diverse.values()) or div_disco else 0


if __name__ == '__main__':
    sys.exit(main())
