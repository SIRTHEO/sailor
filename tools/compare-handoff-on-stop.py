#!/usr/bin/env python3
"""Confronta il giudizio dello Stop col suo porto in Rust.

L'ORACOLO È IL PYTHON. Il porto è giusto quando non lo si distingue: stesso
codice d'uscita e stesso testo su stderr, **byte a byte**. Quel testo non è
cosmesi — è l'unica cosa che la sessione legge quando il gancio le impedisce di
fermarsi, e un apostrofo o una virgola delle migliaia che cambiano lo rendono un
messaggio diverso.

COSA QUESTO CONFRONTO COPRE, E COSA NO. Copre il gancio intero: l'estrazione
dalla trascrizione (modello, soglie, token in contesto, ripartenze), il giudizio,
il codice d'uscita, il testo su stderr e **la riga di registro**. Resta fuori il
solo ramo in cui si apre una scheda: là in fondo nasce una sessione Claude vera
trenta secondi dopo, e uno strumento che genera sessioni non lo rilancia nessuno.

DUE VARIABILI TOLTE A ENTRAMBI, e non è pulizia: `ORCA_TAB_ID` e
`ORCA_TERMINAL_HANDLE` sono ciò da cui il congedo capisce qual è la propria
scheda. Lasciarle vorrebbe dire far girare, dentro uno strumento di confronto, il
codice che chiude il terminale da cui lo si è lanciato. Senza, `mine` resta vuoto
e il congedo non può scattare — il suo ramo è provato dalla batteria, dove non
costa niente.

DUE HOME FINTE, UNA PER PARTE, come negli altri confronti: il Python **scrive**
mentre decide — il contatore delle forzature, il memo della misura, quello delle
ripartenze — e con una cartella sola leggerebbe il proprio lavoro. Nessuna delle
due è `~/.claude/state`.

`CLAUDE_NATO_DA_CONSEGNA=1` IN OGNI CASO, e non è un dettaglio: sul ramo della
consegna valida l'originale chiama `arm`, che **apre una scheda vera**. Quella
variabile è il primo freno di `arm` e lo ferma prima di ogni effetto. Senza,
questo confronto aprirebbe una sessione per ogni caso che passa di lì.

    python3 tools/compare-handoff-on-stop.py
"""
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path.home() / '.claude'
HOOK = ROOT / 'skills/hooks/handoff-on-stop.py'
BINARY = ROOT / 'rust/target/release/claude-hooks'
SCRATCH = Path(os.environ.get(
    'RELAY_SCRATCH',
    '/private/tmp/claude-501/-Users-theo-orca-general/scratchpad')) / 'handoff-on-stop'

STOP_CAP = 3
RESTART_CAP = 5


def transcript_con(used, model='claude-opus-5', restarts=0):
    """Una trascrizione costruita che misura esattamente `used` token.

    Compatta, senza spazi dopo i due punti: è la forma con cui l'harness scrive
    le trascrizioni vere, e i filtri grezzi delle due implementazioni cercano le
    chiavi alla lettera. Un caso costruito con `json.dumps` di default conta zero
    chiamate — difetto già pagato una volta in questa migrazione.
    """
    def compatto(d):
        return json.dumps(d, separators=(',', ':'))

    righe = []
    for _ in range(restarts):
        righe.append(compatto({'type': 'user', 'message': {
            'content': 'This session is being continued from a previous conversation'}}))
    if used:
        righe.append(compatto({'type': 'assistant', 'message': {
            'model': model,
            'usage': {'input_tokens': 0, 'cache_read_input_tokens': used,
                      'cache_creation_input_tokens': 0, 'output_tokens': 1}}}))
    else:
        righe.append(compatto({'type': 'user', 'message': {'content': 'ciao'}}))
    return '\n'.join(righe) + '\n'


def prepara(root: Path, caso):
    """Costruisce la HOME finta e ritorna il percorso della trascrizione."""
    shutil.rmtree(root, ignore_errors=True)
    state = root / '.claude' / 'state'
    state.mkdir(parents=True)
    # Senza questo collegamento il Python non trova `hook_log`, ripiega su una
    # funzione muta e **sembra** d'accordo con un porto muto: due implementazioni
    # silenziose vanno d'accordo per costruzione.
    try:
        (root / '.claude' / 'scripts').symlink_to(ROOT / 'scripts')
    except OSError:
        pass
    sess = caso['session'][:8]

    if caso.get('transcript_reale'):
        percorso = caso['transcript_reale']
    else:
        percorso = str(root / 'transcript.jsonl')
        Path(percorso).write_text(transcript_con(
            caso['used'], caso.get('model', 'claude-opus-5'),
            caso.get('restarts', 0)))

    if caso.get('consegna_fatta'):
        (state / f'consegna-fatta-{sess}').write_text('1')
        # Senza il riferimento, `handoff_stale` lo fissa ad adesso e la consegna
        # vale un giro. Col riferimento più basso delle ripartenze correnti, la
        # consegna è stantia: è il caso «consegnata, poi ripartita ancora».
        if 'consegna_a_ripartenze' in caso:
            (state / f'consegna-fatta-ripartenze-{sess}').write_text(
                str(caso['consegna_a_ripartenze']))
    if caso.get('blocchi'):
        (state / f'consegna-stop-{sess}').write_text(str(caso['blocchi']))
    return percorso


def ambiente(home, caso):
    env = dict(os.environ)
    env['HOME'] = str(home)
    # Il freno di generazione ferma `arm` prima di ogni effetto: senza, il ramo
    # della consegna valida aprirebbe una scheda per ogni caso che passa di lì.
    env['CLAUDE_NATO_DA_CONSEGNA'] = '1'
    env['RIPARTENZA_TETTO_COMPATT'] = str(caso.get('restart_cap', RESTART_CAP))
    for chiave in ('ORCA_TAB_ID', 'ORCA_TERMINAL_HANDLE'):
        env.pop(chiave, None)
    return env


def registro(home):
    """L'ultima riga del registro, senza l'istante e senza la propria HOME.

    Fa parte della risposta: un presidio che decide bene e non registra è
    indistinguibile da uno rotto, ed è esattamente il difetto trovato oggi nel
    porto del gancio gemello — dove il confronto guardava solo `stdout`.
    """
    p = home / '.claude' / 'state' / 'ganci.jsonl'
    if not p.exists():
        return None
    righe = [r for r in p.read_text(errors='replace').splitlines() if r.strip()]
    if not righe:
        return None
    try:
        o = json.loads(righe[-1])
    except Exception:
        return {'<riga illeggibile>': righe[-1]}
    o.pop('t', None)
    return {k: (v.replace(str(home), '<HOME>') if isinstance(v, str) else v)
            for k, v in o.items()}


def esegui(comando, home, caso):
    """Esegue il gancio, eventualmente più volte sulla **stessa** HOME.

    `ripeti` esiste per i fatti che vivono fra un turno e l'altro: il contatore
    delle forzature si legge scrivendo, e con una HOME ricostruita a ogni caso
    quel valore non viene mai riletto. Senza questi giri, un porto che non
    incrementa mai il contatore — cioè che non si arrende mai, e incastra la
    sessione — passa il confronto: misurato per mutazione il 17/08/2026.
    """
    percorso = prepara(home, caso)
    payload = {'session_id': caso['session'], 'transcript_path': percorso,
               'stop_hook_active': caso.get('stop_hook_active', False)}
    for _ in range(caso.get('ripeti', 1)):
        p = subprocess.run(comando, input=json.dumps(payload),
                           capture_output=True, text=True, timeout=120,
                           cwd=str(home), env=ambiente(home, caso))
    return p.returncode, p.stderr, registro(home)


def esegui_python(caso):
    return esegui([sys.executable, str(HOOK)], SCRATCH / 'home-python', caso)


def esegui_rust(caso):
    return esegui([str(BINARY), 'handoff-on-stop'], SCRATCH / 'home-rust', caso)


def casi():
    veri = sorted((ROOT / 'projects').glob('*/*.jsonl'), key=os.path.getsize)
    base = {'session': 'provasto-0000-0000'}
    elenco = [
        ('sotto entrambe le soglie', {**base, 'used': 100_000}),
        ('appena sotto l obbligo', {**base, 'used': 449_999}),
        ('esattamente sull obbligo', {**base, 'used': 450_000}),
        ('sopra l obbligo', {**base, 'used': 480_000}),
        ('molto sopra l obbligo', {**base, 'used': 900_000}),
        ('un altro modello, budget diverso',
         {**base, 'used': 190_000, 'model': 'claude-opus-4-8'}),
        ('sotto l obbligo con quel modello',
         {**base, 'used': 150_000, 'model': 'claude-opus-4-8'}),
        ('modello sconosciuto, budget di ripiego',
         {**base, 'used': 170_000, 'model': 'modello-mai-visto'}),
        ('sopra l obbligo, ma consegna valida',
         {**base, 'used': 480_000, 'consegna_fatta': True}),
        ('consegna fatta e sessione ripartita dopo',
         {**base, 'used': 480_000, 'consegna_fatta': True, 'restarts': 3,
          'consegna_a_ripartenze': 1}),
        ('contesto basso, ripartenze sul tetto',
         {**base, 'used': 100_000, 'restarts': RESTART_CAP}),
        ('contesto basso, ripartenze appena sotto',
         {**base, 'used': 100_000, 'restarts': RESTART_CAP - 1}),
        ('contesto basso, molto oltre il tetto',
         {**base, 'used': 100_000, 'restarts': RESTART_CAP + 4}),
        ('tutti e due i motivi insieme',
         {**base, 'used': 480_000, 'restarts': RESTART_CAP + 2}),
        ('tetto delle ripartenze alzato dall ambiente',
         {**base, 'used': 100_000, 'restarts': 6, 'restart_cap': 9}),
        ('tetto alzato e superato',
         {**base, 'used': 100_000, 'restarts': 9, 'restart_cap': 9}),
        ('dentro una catena indotta',
         {**base, 'used': 480_000, 'stop_hook_active': True}),
        ('penultima forzatura', {**base, 'used': 480_000, 'blocchi': STOP_CAP - 1}),
        ('forzatura di troppo', {**base, 'used': 480_000, 'blocchi': STOP_CAP}),
        ('molto oltre il tetto delle forzature',
         {**base, 'used': 480_000, 'blocchi': 99}),
        # I giri consecutivi: è l'unico modo di vedere il contatore, che si legge
        # scrivendo e vale solo fra un turno e il successivo.
        ('due forzature di seguito', {**base, 'used': 480_000, 'ripeti': 2}),
        ('quattro di seguito: alla fine si arrende',
         {**base, 'used': 480_000, 'ripeti': STOP_CAP + 1}),
        ('nessuna misura nella trascrizione', {**base, 'used': 0}),
        ('nessuna misura ma ripartenze oltre il tetto',
         {**base, 'used': 0, 'restarts': RESTART_CAP}),
    ]
    # Le trascrizioni vere servono al realismo della misura e del modello: i casi
    # costruiti hanno una riga sola, le vere ne hanno migliaia e una coda da
    # scorrere all'indietro.
    for etichetta, indice in (('piccola', 0), ('mediana', len(veri) // 2),
                              ('grande', -1)):
        if veri:
            elenco.append((f'trascrizione vera, {etichetta}',
                           {**base, 'transcript_reale': str(veri[indice])}))
    return elenco


def main():
    if not BINARY.exists():
        print(f'manca {BINARY}: cargo build --release')
        return 1
    SCRATCH.mkdir(parents=True, exist_ok=True)
    divergenze = 0
    for nome, caso in casi():
        try:
            attesa = esegui_python(caso)
            ottenuta = esegui_rust(caso)
        except Exception as exc:                       # pragma: no cover
            print(f'  ERRORE      {nome}: {exc}')
            divergenze += 1
            continue
        if attesa == ottenuta:
            print(f'  uguale      {nome}  (uscita {attesa[0]})')
            continue
        divergenze += 1
        print(f'  DIVERGE     {nome}')
        if attesa[0] != ottenuta[0]:
            print(f'      uscita   python={attesa[0]}  rust={ottenuta[0]}')
        if attesa[1] != ottenuta[1]:
            print(f'      python: {attesa[1]!r}')
            print(f'      rust:   {ottenuta[1]!r}')
        if attesa[2] != ottenuta[2]:
            print(f'      registro python: {attesa[2]!r}')
            print(f'      registro rust:   {ottenuta[2]!r}')
    print()
    if divergenze:
        print(f'{divergenze} divergenze su {len(casi())} casi')
    else:
        print(f'{len(casi())} casi, nessuna divergenza')
    return 1 if divergenze else 0


if __name__ == '__main__':
    sys.exit(main())
