#!/usr/bin/env python3
"""Confronta la decisione della staffetta: `relay.evaluate()` contro il porto Rust.

L'ORACOLO È IL PYTHON. È lui che gira sotto launchd su questa macchina, quindi il
porto è giusto quando non lo si distingue — non quando piace a chi lo ha scritto.

LA DIFFERENZA DI FORMA, che è tutto il lavoro. Il Python **legge il disco mentre
decide**: opt-out, raffreddamento, marcatore di consegna, marcatore
`successore-di-*`, memo della misura. Il Rust riceve quegli stessi fatti già
raccolti dentro `SessionFacts`. Confrontarli significa quindi, per ogni caso,
**fabbricare lo stato su disco** che fa vedere al Python esattamente il fatto che
si passa al Rust. Uno stato preparato a metà darebbe divergenze che non sono del
porto ma dell'apparecchio di prova.

DUE HOME FINTE, NON UNA. Le due implementazioni scrivono lo stesso memo
(`consegna-misura-<sess>`) mentre misurano: con una cartella sola, la seconda a
girare leggerebbe la risposta della prima e sarebbe sempre d'accordo con sé
stessa. Ognuna ha la sua, preparate in modo identico. Nessuna delle due è
`~/.claude/state/`, dove vivono le sessioni vere: ci si scrive e si cancellerebbe
lavoro in corso, quindi il vero stato viene fotografato prima e dopo e il
confronto fallisce se è cambiato di un byte.

AZIONE E MOTIVO SEPARATI. Un verdetto solo nasconde quale metà diverge, e su
questa funzione il motivo porta numeri che l'azione non guarda — la percentuale
del budget, la soglia, il nome del modello. Si contano le due cose a parte, più i
fatti raccolti (modello, budget, soglia, token) che servono ad attribuire una
divergenza alla raccolta invece che alla decisione.

    compare-relay-evaluate.py            il confronto
    compare-relay-evaluate.py --oracolo  il lato Python, con la HOME finta
                                         (uso interno: lo lancia il confronto)
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

# Percorsi dedotti dalla posizione di questo file e non da `Path.home()`: il lato
# oracolo gira con la HOME finta, e lì `~` non è più la casa di nessuno.
RADICE = Path(__file__).resolve().parents[2]          # ~/.claude
BIN = RADICE / 'rust' / 'target' / 'release' / 'claude-hooks'
HOOKS = RADICE / 'skills' / 'hooks'
STATO_VERO = RADICE / 'state'

SCRATCH = Path(os.environ.get(
    'RELAY_SCRATCH',
    '/private/tmp/claude-501/-Users-theo-orca-general/'
    'cdca7b36-bd04-4645-b291-ecedad59cbb7/scratchpad')) / 'relay-equivalenza'

# Un istante fisso: il raffreddamento si giudica su una differenza, e un `now`
# che scorre fra il lato Python e il lato Rust renderebbe il confine irriproducibile.
ORA = 1_755_400_000.0
COOLDOWN_SEC = 300


# ── I transcript, fabbricati ─────────────────────────────────────────────────

def riga_uso(model: str | None, usati: int) -> str:
    """Una riga di transcript come quelle vere: `usage` sommato su tre campi."""
    usage = {'input_tokens': 10, 'cache_read_input_tokens': max(usati - 15, 0),
             'cache_creation_input_tokens': 5 if usati >= 15 else 0,
             'output_tokens': 128}
    message: dict = {'role': 'assistant', 'usage': usage}
    if model is not None:
        message['model'] = model
    return json.dumps({'type': 'assistant', 'message': message})


def scrivi_transcript(cartella: Path, nome: str, righe: list[str]) -> str:
    p = cartella / f'{nome}.jsonl'
    p.write_text('\n'.join(righe) + ('\n' if righe else ''))
    return str(p)


def transcripts(cartella: Path) -> dict[str, str]:
    """Il materiale su cui si misura il contesto, per nome del caso.

    Fabbricato e non pescato dai transcript veri perché qui servono i confini
    esatti — sotto, sopra e **sulla** soglia di tre modelli — e sul disco di
    questa macchina il caso «esattamente 450.000 token» non esiste.
    """
    cartella.mkdir(parents=True, exist_ok=True)
    utente = json.dumps({'type': 'user', 'message': {'role': 'user'}})
    t: dict[str, str] = {}

    def uno(nome: str, model: str | None, usati: int) -> None:
        t[nome] = scrivi_transcript(cartella, nome, [utente, riga_uso(model, usati)])

    # Tre modelli, e per ognuno sotto/esatto/sopra la soglia d'obbligo.
    # opus-4-8: budget 200k, obbligo 180k · opus-5: 500k/450k · ignoto: 180k/162k
    for nome, model, budget in (('opus48', 'claude-opus-4-8', 200_000),
                                ('opus5', 'claude-opus-5', 500_000),
                                ('ignoto', 'modello-che-non-esiste-9', 180_000)):
        obbligo = int(budget * 0.90)
        uno(f'{nome}-sotto', model, obbligo - 1)
        uno(f'{nome}-esatto', model, obbligo)
        uno(f'{nome}-sopra', model, obbligo + 10_000)
        # I confini dell'arrotondamento: `used/budget*100` cade esattamente su
        # una metà. `round()` di Python va al pari, `f64::round()` di Rust va
        # lontano da zero — se divergono, è qui che si vede e in nessun altro caso.
        for k in (90, 91, 92, 93, 95):
            uno(f'{nome}-mezzo-{k}', model, budget * (2 * k + 1) // 200)

    # Nessun `usage`: la misura è zero, e il motivo deve dirlo con uno zero.
    t['senza-usage'] = scrivi_transcript(cartella, 'senza-usage', [utente, utente])
    # Nessun `model`: il budget scende al default prudente.
    uno('senza-model', None, 170_000)
    # File vuoto, e percorso che non esiste: due modi diversi di non avere niente.
    t['vuoto'] = scrivi_transcript(cartella, 'vuoto', [])
    t['assente'] = str(cartella / 'questo-file-non-esiste.jsonl')
    t['percorso-vuoto'] = ''

    # L'ultimo turno è sintetico: prenderlo per buono darebbe il default a una
    # sessione su Opus 5.
    t['sintetico'] = scrivi_transcript(cartella, 'sintetico', [
        riga_uso('claude-opus-5', 470_000), riga_uso('<synthetic>', 470_000)])
    # `usage` vuoto in coda: il Python lo salta e continua a scorrere.
    t['uso-vuoto'] = scrivi_transcript(cartella, 'uso-vuoto', [
        riga_uso('claude-opus-4-8', 190_000),
        json.dumps({'type': 'assistant', 'message': {'model': 'claude-opus-4-8',
                                                     'usage': {}}})])
    # Riga rotta in coda: non deve fermare la ricerca all'indietro.
    t['riga-rotta'] = scrivi_transcript(cartella, 'riga-rotta', [
        riga_uso('claude-opus-4-8', 190_000), '{"message": rotta'])
    # Più lungo della coda letta (400 KB): il modello sta **prima** del taglio,
    # quindi entrambe devono rispondere «sconosciuto». Chi leggesse tutto il file
    # direbbe opus-5 e sarebbe l'unica divergenza del giro.
    riempimento = [json.dumps({'type': 'user', 'message': {'role': 'user',
                                                           'content': 'x' * 400}})
                   for _ in range(1200)]
    t['coda-tagliata'] = scrivi_transcript(cartella, 'coda-tagliata', [
        riga_uso('claude-opus-5', 1), *riempimento, riga_uso(None, 170_000)])
    return t


# ── I casi ───────────────────────────────────────────────────────────────────

# Gli elenchi dei pannelli vivi. `None` non è la lista vuota: è «non ho potuto
# leggere», ed è la distinzione che è già costata un registro di sessioni
# cancellato ogni minuto per 276 giri.
VIVI = {
    'contiene': ['term_x', 'term_altro'],
    'senza': ['term_altro', 'term_terzo'],
    'vuota': [],
    'illeggibile': None,
}
RAFFREDDAMENTI = ('assente', 'attivo', 'scaduto')
OPT_OUT = ('nessuno', 'sessione', 'worktree')
SUCCESSORI = ('assente', 'vivo', 'morto')
CONTESTI = ('opus48-sopra', 'opus48-esatto', 'opus48-sotto', 'assente')


def caso(idx: int, **kv) -> dict:
    """Un caso completo: i fatti, e come si scrivono su disco perché il Python li veda."""
    sess = f'{idx:08d}'
    base = {
        'id': sess,
        'session_id': sess + 'aaaabbbb',   # il Python ne tiene i primi 8
        'sess': sess,
        'handle': 'term_x',
        'worktree': 'wt-prova',
        'contesto': 'opus48-sopra',
        'vivi': 'contiene',
        'optout': 'nessuno',
        'raffreddamento': 'assente',
        'consegna': True,
        'successore': 'assente',
        'memo': None,
    }
    base.update(kv)
    return base


def casi() -> list[dict]:
    """Il prodotto incrociato, più i casi che il prodotto non contiene.

    Il prodotto copre le dimensioni che si influenzano a vicenda — un record
    incompleto deve battere l'opt-out, il raffreddamento deve battere la
    consegna — perché l'ordine dei controlli **è** il comportamento, e un elenco
    di casi isolati non lo prova: prova ogni ramo una volta e nessuna precedenza.
    """
    fuori = []
    n = 0
    for record in ('completo', 'senza-handle'):
        for vivi in VIVI:
            for optout in OPT_OUT:
                for raff in RAFFREDDAMENTI:
                    for consegna in (True, False):
                        for succ in SUCCESSORI:
                            for ctx in CONTESTI:
                                n += 1
                                c = caso(n, vivi=vivi, optout=optout,
                                         raffreddamento=raff, consegna=consegna,
                                         successore=succ, contesto=ctx)
                                if record == 'senza-handle':
                                    c['handle'] = ''
                                fuori.append(c)

    def aggiungi(**kv):
        nonlocal n
        n += 1
        fuori.append(caso(n, **kv))

    # Le altre due forme di record incompleto: il prodotto ne gira una sola.
    aggiungi(worktree='')
    aggiungi(session_id='')
    aggiungi(handle='', worktree='', session_id='')

    # L'opt-out globale sta fuori dal prodotto perché il suo file non ha il nome
    # della sessione: lasciato in giro spegnerebbe tutti i casi successivi, e va
    # creato e tolto intorno al caso suo.
    for vivi in VIVI:
        aggiungi(optout='globale', vivi=vivi)

    # I confini del raffreddamento. `esatto` è `now - t == 300`: il Python
    # confronta con `<`, quindi non frena — un `<=` al suo posto lo si vede solo qui.
    for r in ('esatto', 'quasi', 'rotto', 'futuro'):
        aggiungi(raffreddamento=r)
        aggiungi(raffreddamento=r, consegna=False)

    # Le forme del marcatore di successore che il prodotto non copre.
    for s in ('illeggibile', 'tab-assente', 'solo-handle', 'solo-handle-morto',
              'vuoto'):
        aggiungi(successore=s)
        aggiungi(successore=s, vivi='senza')

    # Tutti i contesti, incrociati con la consegna e col raffreddamento: la
    # soglia si giudica per ultima, quindi ogni guardia che le sta davanti deve
    # continuare a batterla anche quando la sessione è piena.
    for nome in sorted(TRANSCRIPTS):
        aggiungi(contesto=nome)
        aggiungi(contesto=nome, consegna=False)
        aggiungi(contesto=nome, raffreddamento='attivo')
        aggiungi(contesto=nome, successore='vivo')
        aggiungi(contesto=nome, vivi='senza')

    # Il memo della misura, che è l'unico pezzo di stato che le due
    # implementazioni si scambiano davvero. `tre-campi` non è un capriccio: il
    # Python spacchetta in due nomi e un terzo campo gli fa alzare le mani, il
    # Rust ne legge due e ignora il resto.
    for memo in ('coerente', 'sotto-soglia', 'cresciuto', 'tre-campi', 'rotto',
                 'un-campo', 'negativo'):
        aggiungi(memo=memo, contesto='opus48-sopra')
        aggiungi(memo=memo, contesto='opus48-sotto')
    return fuori


# ── Il fatto, dedotto dal caso ───────────────────────────────────────────────
# Il Rust li riceve già raccolti: qui si dichiarano **per costruzione**, non
# rileggendo il Python. Se il Python li leggesse diversamente da come lo stato è
# stato scritto, il confronto lo direbbe — che è esattamente ciò che deve fare.

# Un handle vero, non `term_succ`: il motivo ne cita i primi 13 caratteri, e con
# un handle corto il troncamento non si esercita mai — cioè un porto che
# troncasse a 12 passerebbe il confronto senza che nessun caso se ne accorga.
SUCC_VIVO = 'term_dc752551-6326-46e2-924e-d3b313a71b1c'


def vivi_del_caso(c: dict) -> list[str] | None:
    vivi = VIVI[c['vivi']]
    if vivi is None:
        return None
    vivi = list(vivi)
    # Un successore «vivo» deve comparire fra i vivi, altrimenti è morto.
    if c['successore'] in ('vivo', 'solo-handle'):
        vivi.append(SUCC_VIVO)
    return vivi


def successore_atteso(c: dict) -> str:
    """L'handle che la risoluzione del marcatore deve produrre. '' = nessuno."""
    if c['successore'] in ('vivo', 'morto', 'solo-handle', 'solo-handle-morto'):
        return SUCC_VIVO
    return ''


def raffreddato(c: dict) -> bool:
    return c['raffreddamento'] in ('attivo', 'quasi', 'futuro')


def fatti_rust(c: dict, transcripts: dict[str, str]) -> dict:
    return {
        'session': c['session_id'][:8],
        'handle': c['handle'],
        'worktree': c['worktree'],
        'live': vivi_del_caso(c),
        'opted_out': c['optout'] != 'nessuno',
        'in_cooldown': raffreddato(c),
        'armed': successore_atteso(c),
        'handoff_done': bool(c['consegna']),
        'transcript': transcripts[c['contesto']],
        'memo_session': c['session_id'][:8],
    }


# ── Il lato Python, con la HOME finta ────────────────────────────────────────

def valore_raffreddamento(quale: str) -> str | None:
    return {
        'assente': None,
        'attivo': str(ORA - 10),
        'scaduto': str(ORA - 1000),
        'esatto': str(ORA - COOLDOWN_SEC),      # 300 esatti: non frena
        'quasi': str(ORA - COOLDOWN_SEC + 1),   # 299: frena
        'futuro': str(ORA + 50),                # differenza negativa: frena
        'rotto': 'non-un-numero',
    }[quale]


def contenuto_successore(quale: str) -> str | None:
    return {
        'assente': None,
        # L'handle scritto nel marcatore è scaduto e la tab no: è il caso vero.
        'vivo': json.dumps({'handle': 'term_scaduto', 'tabId': 'tab-succ'}),
        'morto': json.dumps({'handle': 'term_scaduto', 'tabId': 'tab-succ'}),
        'illeggibile': "non e' json",
        'tab-assente': json.dumps({'handle': 'term_scaduto', 'tabId': 'tab-ignota'}),
        # Marcatori scritti prima del 17/08/2026: solo l'handle, niente tab.
        'solo-handle': json.dumps({'handle': SUCC_VIVO}),
        'solo-handle-morto': json.dumps({'handle': SUCC_VIVO}),
        'vuoto': json.dumps({}),
    }[quale]


def pannelli(c: dict) -> list[dict]:
    """L'elenco grezzo che `relay._TERMINALS` porta quando `evaluate` decide."""
    if c['successore'] == 'assente':
        return []
    fissi = [{'tabId': 'tab-succ', 'handle': SUCC_VIVO, 'worktreeId': c['worktree']},
             {'tabId': 'tab-mia', 'handle': c['handle'] or 'term_x',
              'worktreeId': c['worktree']}]
    return fissi


def contenuto_memo(quale: str | None, transcript: str) -> str | None:
    """Il memo `<byte> <token>`, nelle forme che il Python e il Rust leggono diverso."""
    if quale is None:
        return None
    try:
        size = os.path.getsize(transcript)
    except OSError:
        size = 0
    return {
        # Stessa dimensione: entrambi devono fidarsi del valore memorizzato.
        'coerente': f'{size} 190000',
        'sotto-soglia': f'{size} 1000',
        # Cresciuto oltre MIN_GROWTH: entrambi devono rimisurare.
        'cresciuto': f'{max(size - 500_000, 0)} 1000',
        'tre-campi': f'{size} 1000 extra',
        'rotto': 'niente-numeri',
        'un-campo': f'{size}',
        'negativo': f'{size} -5',
    }[quale]


def prepara(c: dict, stato: Path) -> list[Path]:
    """Scrive lo stato che fa vedere al Python i fatti passati al Rust."""
    stato.mkdir(parents=True, exist_ok=True)
    scritti = []

    def scrivi(nome: str, testo: str) -> None:
        p = stato / nome
        p.write_text(testo)
        scritti.append(p)

    sess, wt = c['session_id'][:8], c['worktree']
    if c['optout'] == 'sessione':
        scrivi(f'non-rigenerare-{sess}', '1')
    elif c['optout'] == 'worktree':
        scrivi(f'non-rigenerare-{wt}', '1')
    elif c['optout'] == 'globale':
        scrivi('non-rigenerare', '1')
    valore = valore_raffreddamento(c['raffreddamento'])
    if valore is not None:
        scrivi(f'staffetta-cooldown-{wt}', valore)
    if c['consegna']:
        scrivi(f'consegna-fatta-{sess}', '1')
    marcatore = contenuto_successore(c['successore'])
    if marcatore is not None:
        scrivi(f'successore-di-{c["session_id"]}', marcatore)
    memo = contenuto_memo(c['memo'], TRANSCRIPTS[c['contesto']])
    if memo is not None:
        scrivi(f'consegna-misura-{sess}', memo)
    return scritti


def oracolo() -> int:
    """Il Python, interrogato caso per caso con la HOME finta.

    Lo stato si scrive **qui dentro** e non dal chiamante: così la cartella che
    si prepara è per costruzione quella che `relay.py` legge, perché è lo stesso
    processo a calcolarla da `Path.home()`. Prepararla da fuori vorrebbe dire
    fidarsi di aver dedotto il percorso giusto, ed è il genere di supposizione
    che rende verde una prova che non ha provato niente.
    """
    sys.path.insert(0, str(HOOKS))
    import relay                      # noqa: E402
    import handoff_common             # noqa: E402

    stato = relay.STATE_DIR
    print(json.dumps({'home': str(Path.home()), 'stato_relay': str(stato),
                      'stato_comune': str(handoff_common.STATE_DIR)}))
    sys.stdout.flush()

    for riga in sys.stdin:
        if not riga.strip():
            continue
        c = json.loads(riga)
        scritti = prepara(c, stato)
        record = {'session_id': c['session_id'], 'terminal_handle': c['handle'],
                  'worktree_id': c['worktree'],
                  'transcript_path': TRANSCRIPTS[c['contesto']], 'cwd': '/x'}
        vivi = vivi_del_caso(c)
        relay._TERMINALS.clear()
        relay._TERMINALS.extend(pannelli(c))
        azione, motivo = relay.evaluate(record, None if vivi is None else set(vivi),
                                        ORA)
        # I fatti raccolti servono solo ad attribuire una divergenza: senza,
        # «motivo diverso» non dice se ha sbagliato la misura o la frase.
        transcript = TRANSCRIPTS[c['contesto']]
        s = handoff_common.thresholds(transcript)
        usati = handoff_common.context_used(transcript, c['session_id'][:8])
        print(json.dumps({'action': azione, 'reason': motivo, 'model': s['model'],
                          'budget': s['budget'], 'require': s['require'],
                          'used': usati}))
        sys.stdout.flush()
        for p in scritti + [stato / f'consegna-misura-{c["session_id"][:8]}']:
            try:
                p.unlink()
            except OSError:
                pass
    return 0


# ── Il confronto ─────────────────────────────────────────────────────────────

def fotografia(cartella: Path) -> list[tuple]:
    """Nome, dimensione e modifica di ogni file: il vero stato non deve muoversi."""
    if not cartella.exists():
        return []
    fuori = []
    for p in sorted(cartella.rglob('*')):
        try:
            st = p.stat()
            fuori.append((str(p), st.st_size, st.st_mtime_ns))
        except OSError:
            pass
    return fuori


def prepara_home(radice: Path, lista: list[dict]) -> Path:
    """Una HOME finta con lo stato che il **Rust** legge da sé: solo il memo.

    L'opt-out, il raffreddamento e i marcatori il Rust non li legge — li riceve
    come fatti — ma il memo sì, perché la misura del contesto passa ancora dal
    disco. Preparato identico a quello del Python, altrimenti il confronto
    misurerebbe due stati diversi e chiamerebbe divergenza la differenza.
    """
    stato = radice / '.claude' / 'state'
    if radice.exists():
        shutil.rmtree(radice)
    stato.mkdir(parents=True)
    for c in lista:
        memo = contenuto_memo(c['memo'], TRANSCRIPTS[c['contesto']])
        if memo is not None:
            (stato / f'consegna-misura-{c["session_id"][:8]}').write_text(memo)
    return stato


def chiedi_al_python(lista: list[dict], home: Path) -> tuple[list[dict], dict]:
    ambiente = dict(os.environ, HOME=str(home))
    ingresso = '\n'.join(json.dumps(c) for c in lista)
    r = subprocess.run([sys.executable, str(Path(__file__).resolve()), '--oracolo'],
                       input=ingresso, capture_output=True, text=True,
                       env=ambiente, timeout=900)
    righe = [l for l in r.stdout.splitlines() if l.strip()]
    if not righe:
        print('il lato Python non ha risposto:', (r.stderr or '')[-800:],
              file=sys.stderr)
        return [], {}
    intestazione = json.loads(righe[0])
    return [json.loads(l) for l in righe[1:]], intestazione


def chiedi_al_rust(lista: list[dict], home: Path) -> list[dict]:
    ambiente = dict(os.environ, HOME=str(home))
    ingresso = '\n'.join(json.dumps(fatti_rust(c, TRANSCRIPTS)) for c in lista)
    r = subprocess.run([str(BIN), 'relay-evaluate'], input=ingresso,
                       capture_output=True, text=True, env=ambiente, timeout=900)
    righe = [l for l in r.stdout.splitlines() if l.strip()]
    if not righe:
        print('il lato Rust non ha risposto:', (r.stderr or '')[-800:],
              file=sys.stderr)
    return [json.loads(l) for l in righe]


def descrivi(c: dict) -> str:
    return (f"record={'incompleto' if not (c['handle'] and c['worktree'] and c['session_id']) else 'completo'}"
            f" handle={c['handle']!r} worktree={c['worktree']!r}"
            f" vivi={c['vivi']} opt-out={c['optout']}"
            f" raffreddamento={c['raffreddamento']} consegna={c['consegna']}"
            f" successore={c['successore']} contesto={c['contesto']}"
            f" memo={c['memo']}")


def main(attese: int | None) -> int:
    if not BIN.exists():
        print(f'binario assente ({BIN}): cargo build --release', file=sys.stderr)
        return 1

    globals()['TRANSCRIPTS'] = transcripts(SCRATCH / 'transcripts')
    lista = casi()

    prima = fotografia(STATO_VERO)
    home_py = prepara_home(SCRATCH / 'home-python', lista)
    home_rs = prepara_home(SCRATCH / 'home-rust', lista)

    py, intestazione = chiedi_al_python(lista, home_py.parents[1])
    rs = chiedi_al_rust(lista, home_rs.parents[1])
    dopo = fotografia(STATO_VERO)

    # L'isolamento si dimostra, non si dichiara: dove ha guardato il Python, e
    # che il vero stato non si è mosso di un byte.
    print('isolamento della HOME')
    for chiave in ('home', 'stato_relay', 'stato_comune'):
        valore = intestazione.get(chiave, '?')
        ok = str(valore).startswith(str(SCRATCH))
        print(f'  {"ok " if ok else "NO "} {chiave}: {valore}')
    if prima == dopo:
        print(f'  ok  {STATO_VERO}: {len(prima)} file, invariati')
    else:
        cambiati = set(map(str, prima)) ^ set(map(str, dopo))
        print(f'  NO  {STATO_VERO} È CAMBIATO: {sorted(cambiati)[:5]}')
    scritti_rust = len(list((home_rs).glob('*')))
    print(f'  {"ok " if scritti_rust else "NO "} il Rust ha scritto in '
          f'{home_rs}: {scritti_rust} file')

    if len(py) != len(lista) or len(rs) != len(lista):
        print(f'\nrisposte mancanti: {len(lista)} casi, python {len(py)}, '
              f'rust {len(rs)} — niente da confrontare', file=sys.stderr)
        return 1

    diverse_azione = diverso_motivo = diversi_fatti = divergenti = 0
    for c, a, b in zip(lista, py, rs):
        d_az = a['action'] != b['action']
        d_mo = a['reason'] != b['reason']
        d_fa = any(a[k] != b[k] for k in ('model', 'budget', 'require', 'used'))
        diverse_azione += d_az
        diverso_motivo += d_mo
        diversi_fatti += d_fa
        if d_az or d_mo:
            divergenti += 1
            print(f'\nDIVERGE caso {c["id"]}')
            print(f'    {descrivi(c)}')
            if d_az:
                print(f'    azione  python={a["action"]!r}  rust={b["action"]!r}')
            if d_mo:
                print(f'    motivo  python={a["reason"]!r}\n            '
                      f'rust  ={b["reason"]!r}')
            if d_fa:
                for k in ('model', 'budget', 'require', 'used'):
                    if a[k] != b[k]:
                        print(f'    {k:8} python={a[k]!r}  rust={b[k]!r}')

    # Quante volte ogni ramo ha davvero deciso. Senza questo conteggio «zero
    # divergenze» non distingue due implementazioni d'accordo da un corpus che
    # non arriva mai al ramo: un ramo a zero è un punto cieco, non un successo.
    print('\nrami toccati (secondo l\'oracolo)')
    rami: dict[tuple, int] = {}
    for a in py:
        rami[(a['action'], a['reason'].split('(')[0].strip())] = \
            rami.get((a['action'], a['reason'].split('(')[0].strip()), 0) + 1
    for (azione, ramo), quanti in sorted(rami.items(), key=lambda x: -x[1]):
        print(f'  {quanti:5}  {azione:8} {ramo}')

    print(f'\n{len(lista)} casi confrontati · divergenti: {divergenti} · '
          f'azione diversa: {diverse_azione} · motivo diverso: {diverso_motivo} · '
          f'fatti raccolti diversi: {diversi_fatti}')

    # `--attese N` esiste per i mutanti, e per una ragione precisa: il porto ha
    # divergenze **vere** già oggi, quindi un giudizio «esce 0 oppure no»
    # dichiarerebbe ucciso ogni mutante senza che nessuno l'abbia visto. Il
    # numero atteso è quello misurato sul sorgente non mutato, nello stesso giro
    # e con lo stesso comando. Non è una linea di base congelata su file: la si
    # rimisura ogni volta, e va aggiornata a mano quando una divergenza viene
    # chiusa — mai per far tornare i conti.
    if attese is not None:
        esito = 'come la copia non mutata' if divergenti == attese else 'DIVERSO'
        print(f'attese {attese} divergenze, trovate {divergenti}: {esito}')
        return 0 if divergenti == attese else 1
    return 1 if divergenti else 0


TRANSCRIPTS: dict[str, str] = {}

if __name__ == '__main__':
    if '--oracolo' in sys.argv:
        TRANSCRIPTS = transcripts(SCRATCH / 'transcripts')
        sys.exit(oracolo())
    quante = None
    if '--attese' in sys.argv:
        quante = int(sys.argv[sys.argv.index('--attese') + 1])
    sys.exit(main(quante))
