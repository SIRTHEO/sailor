#!/usr/bin/env python3
"""Compare the relay's regeneration step: not its output, but what it DOES.

regenerate() returns nothing. Comparing return values would be green by
construction. What it has are effects — five orca calls in an order that is the
whole point, plus the files it writes and deletes — so those are what is
compared.

    1. wait tui-idle on the old pane      never truncate a turn
    2. write riprendi-da/<worktree>       the resume signal
    3. create the successor               BEFORE closing: the tree is never
                                          left without a session
    4. wait + send to the successor       the order to resume
    5. close the old one, clean up        only now

Swapping 3 and 5 leaves a tree uncovered every time create fails. That is the
invariant this tool exists to pin.

HOW. A fake `orca` is put first on PATH. It appends every invocation to a log
and answers from a scripted table, so both implementations meet the same
sequence of replies. Two fake HOMEs, one per implementation: both write state
while they run, and a shared one would have the second read the first's work and
agree with itself.

Nothing here touches the real ~/.claude/state, and the real orca is never
called — verified by asserting the fake's log is the only one that grew.

    compare-relay-regenerate.py          the built cases
    compare-relay-regenerate.py --field  the same comparison on the REAL state
    compare-relay-regenerate.py --all    both

THE FIELD MODE, and why the built cases are not enough. Every case above is
invented: synthetic records, a `cwd` of `/x`, a fake HOME with nothing under it.
Both sides then read an empty tree and agree by construction — the same trap the
live-rules port hit on 2026-08-17, where 356 real files and zero differences hid
three real divergences. `--field` copies the records that are in
`state/sessioni-vive/` right now, symlinks `.claude/projects` so transcripts and
handoff documents resolve to the real files, and answers `terminal list` with the
real pane list frozen once. It is the only mode in which `latest_handoff` has
anything to find.
"""
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path.home() / '.claude'
SCRATCH = Path(os.environ.get(
    'RELAY_SCRATCH',
    '/private/tmp/claude-501/-Users-theo-orca-general/scratchpad')) / 'relay-regenerate'
BIN = ROOT / 'rust/target/release/claude-hooks'
RELAY = ROOT / 'skills/hooks/relay.py'
REAL_STATE = ROOT / 'state'

FAKE_ORCA = """#!/bin/sh
# Registra ogni invocazione, una per riga, e risponde dal copione.
printf '%s\\n' "$*" >> "$ORCA_LOG"
case "$*" in
  *"terminal list"*)
    if [ -n "$ORCA_LIST_FILE" ]; then cat "$ORCA_LIST_FILE"
    else printf '%s' "$ORCA_LIST_REPLY"; fi ;;
  *"terminal create"*)
    # Il successore che parte e raccoglie il mandato: e' cio' che fa
    # `register-session.py` all'avvio, qui al momento in cui accadrebbe.
    [ -n "$ORCA_PICKUP_FILE" ] && rm -f "$ORCA_PICKUP_FILE"
    printf '%s' "$ORCA_CREATE_REPLY"; exit $ORCA_CREATE_RC ;;
  *"terminal wait"*)   exit $ORCA_WAIT_RC ;;
  *"terminal send"*)   exit $ORCA_SEND_RC ;;
esac
exit 0
"""

# I secondi dell'epoca compaiono nel raffreddamento e nel memo della misura: il
# Python li scrive con `str(time.time())`, il Rust con un `f64` formattato, e
# quella cifra in fondo non e' un comportamento.
EPOCH = re.compile(r'\b17\d{8}(\.\d+)?\b')

# Le risposte che i casi usano. La prima è quella vera del 2026-08-17.
CREATE_OK = json.dumps({'ok': True, 'result': {'terminal': {
    'handle': 'term_nuovo000', 'tabId': 'tab-nuova'}}})
CREATE_SENZA_HANDLE = json.dumps({'ok': True, 'result': {}})
CREATE_HANDLE_FINTO = json.dumps({'ok': True, 'result': {'handle': 'wt-123'}})


def build_home(root: Path, case):
    """Una HOME finta con dentro esattamente lo stato che il caso descrive."""
    shutil.rmtree(root, ignore_errors=True)
    state = root / '.claude' / 'state'
    (state / 'sessioni-vive').mkdir(parents=True)
    sess = case['sess']

    transcript = root / 'transcript.jsonl'
    riga = {'type': 'assistant', 'message': {
        'model': case['model'],
        'usage': {'input_tokens': 0, 'cache_read_input_tokens': case['used'],
                  'cache_creation_input_tokens': 0, 'output_tokens': 1}}}
    transcript.write_text(json.dumps(riga) + '\n')

    record = {
        'session_id': case['session_id'],
        'terminal_handle': case['handle'],
        'worktree_id': case['worktree'],
        'tab_id': case.get('tab_id', ''),
        'transcript_path': str(transcript) if case['transcript'] else '/manca',
        'cwd': case['cwd'],
    }
    (state / 'sessioni-vive' / f'{sess}.json').write_text(json.dumps(record))

    if case['handoff_done']:
        (state / f'consegna-fatta-{sess}').write_text('1')
    if case.get('opt_out'):
        (state / f'non-rigenerare-{case["worktree"]}').write_text('1')
    if case.get('cooldown'):
        (state / f'staffetta-cooldown-{case["worktree"]}').write_text(str(time.time()))
    if case.get('armed'):
        (state / f'successore-di-{case["session_id"]}').write_text(
            json.dumps({'handle': case['armed'], 'tabId': case.get('armed_tab', '')}))
    return root


def run_side(which, case, home, log):
    # `home` serve anche a comporre il percorso del segnale: il caso «il
    # successore raccoglie» si simula cancellandolo nell'istante giusto.
    env = {
        **os.environ,
        'HOME': str(home),
        'PATH': f'{SCRATCH}/bin:' + os.environ.get('PATH', ''),
        'ORCA_LOG': str(log),
        'ORCA_LIST_REPLY': case.get('list_reply', ''),
        'ORCA_LIST_FILE': str(case.get('list_file', '')),
        'ORCA_CREATE_REPLY': case.get('create_reply', CREATE_OK),
        'ORCA_CREATE_RC': str(case.get('create_rc', 0)),
        'ORCA_WAIT_RC': str(case.get('wait_rc', 0)),
        'ORCA_SEND_RC': str(case.get('send_rc', 0)),
        # L'attesa vale 25 secondi in produzione: qui a zero, o un confronto da
        # trentaquattro scenari durerebbe piu' di dieci minuti per parte.
        'RELAY_PICKUP_TIMEOUT_SEC': str(case.get('pickup_timeout', 0)),
        'ORCA_PICKUP_FILE': (
            str(home / '.claude' / 'state' / 'riprendi-da' /
                (case['worktree'].replace('/', '_') + '.txt'))
            if case.get('pickup') else ''),
    }
    if which == 'rust':
        cmd = [str(BIN), 'relay']
    else:
        cmd = [sys.executable, str(RELAY)]
    if case.get('secco'):
        cmd.append('--secco')
    subprocess.run(cmd, capture_output=True, text=True, timeout=120, env=env)


def snapshot(home):
    """Cosa resta sul disco: i file di stato **col loro contenuto**, e il
    registro senza le date.

    Il contenuto e' entrato il 17/08/2026, e non e' zelo: il segnale di ripresa
    che finisce in `riprendi-da/<worktree>.txt` e' un percorso, e **quale**
    percorso sia e' tutto il comportamento di quel passo. Confrontando i soli
    nomi, due implementazioni che indicano al successore due consegne diverse
    risultano identiche.
    """
    state = home / '.claude' / 'state'

    def normalizza(testo):
        # Via la radice della HOME finta: si chiama `home-python` da una parte e
        # `home-rust` dall'altra, e senza questa sostituzione **ogni** percorso
        # scritto risulta divergente. Sono due divergenze finte che coprono
        # quella vera, ed è come si legge una prova che non prova niente.
        return EPOCH.sub('<EPOCA>', testo.replace(str(home), '<HOME>'))

    files = []
    for p in sorted(state.rglob('*')):
        if p.is_file() and p.name != 'staffetta.log':
            try:
                body = p.read_text(errors='replace').strip()
            except OSError:
                body = '<illeggibile>'
            files.append(f'{p.relative_to(state)} = {normalizza(body)}')
    log = state / 'staffetta.log'
    righe = []
    if log.exists():
        for r in log.read_text(errors='replace').splitlines():
            # Via la data: cambia a ogni esecuzione e non è ciò che si confronta.
            righe.append(normalizza(r[21:] if len(r) > 21 else r))
    return files, righe


LIST_VIVO = json.dumps({'result': {'terminals': [
    {'handle': 'term_vecchio0', 'tabId': 'tab-1', 'worktreeId': 'wt-1',
     'title': '✳ Un lavoro'},
    {'handle': 'term_altro00', 'tabId': 'tab-2', 'worktreeId': 'wt-2',
     'title': '✳ Altro'}]}})
LIST_MORTO = json.dumps({'result': {'terminals': [
    {'handle': 'term_altro00', 'tabId': 'tab-2', 'worktreeId': 'wt-2',
     'title': '✳ Altro'}]}})


def cases():
    base = dict(sess='provareg', session_id='provareg-0000-0000',
                handle='term_vecchio0', worktree='wt-1', tab_id='tab-1',
                transcript=True, cwd='/x', model='claude-opus-4-8',
                used=190_000, handoff_done=True, list_reply=LIST_VIVO)
    return [
        ('rigenera, tutto liscio', {**base}),
        ('rigenera a secco', {**base, 'secco': True}),
        ('create senza handle', {**base, 'create_reply': CREATE_SENZA_HANDLE}),
        ('create con un handle finto', {**base, 'create_reply': CREATE_HANDLE_FINTO}),
        ('create fallito', {**base, 'create_rc': 1}),
        ('la vecchia non e idle', {**base, 'wait_rc': 1}),
        ('pannello morto: si pulisce', {**base, 'list_reply': LIST_MORTO}),
        ('elenco illeggibile', {**base, 'list_reply': 'non e json'}),
        ('elenco vuoto', {**base, 'list_reply': json.dumps({'result': {'terminals': []}})}),
        ('senza consegna', {**base, 'handoff_done': False}),
        ('opt-out sul worktree', {**base, 'opt_out': True}),
        ('raffreddamento attivo', {**base, 'cooldown': True}),
        ('sotto soglia', {**base, 'used': 50_000}),
        ('transcript assente', {**base, 'transcript': False}),
        ('successore gia armato e vivo', {**base, 'armed': 'term_altro00',
                                          'armed_tab': 'tab-2'}),
        ('successore armato ma morto', {**base, 'armed': 'term_sparito',
                                        'armed_tab': 'tab-9'}),
        ('handle scaduto, tab viva', {**base, 'handle': 'term_scaduto'}),
        ('opus 5 sotto la sua soglia', {**base, 'model': 'claude-opus-5',
                                        'used': 300_000}),
        ('opus 5 sopra la sua soglia', {**base, 'model': 'claude-opus-5',
                                        'used': 480_000}),
        ('modello sconosciuto', {**base, 'model': 'gpt-9', 'used': 175_000}),
    ]


# ── Modalita' campo: lo stato vero, non uno costruito ────────────────────────

# I soli file di stato che la staffetta legge o scrive. Copiare tutti gli 8 MB
# annegherebbe il confronto in registri che non tocca mai.
STATE_GLOBS = ('consegna-*', 'successore-*', 'staffetta-cooldown-*',
               'non-rigenerare*', 'staffetta-off')


def frozen_pane_list():
    """L'elenco vero dei pannelli, letto una volta e congelato.

    Congelato perche' le due implementazioni devono giudicare lo stesso istante:
    due letture a un secondo di distanza possono non concordare, e la differenza
    verrebbe attribuita al porto.
    """
    path = SCRATCH / 'terminal-list.json'
    if not path.exists():
        out = subprocess.run(['orca', 'terminal', 'list', '--json'],
                             capture_output=True, text=True, timeout=30)
        if out.returncode != 0 or not out.stdout.strip():
            print("l'elenco vero dei pannelli non si legge", file=sys.stderr)
            sys.exit(1)
        path.write_text(out.stdout.strip())
    return path


def build_home_field(root: Path, case):
    """Una HOME usa-e-getta con dentro lo stato di produzione, copiato."""
    shutil.rmtree(root, ignore_errors=True)
    state = root / '.claude' / 'state'
    (state / 'sessioni-vive').mkdir(parents=True)
    for f in sorted((REAL_STATE / 'sessioni-vive').glob('*.json')):
        shutil.copy2(f, state / 'sessioni-vive' / f.name)
    for pattern in STATE_GLOBS:
        for f in sorted(REAL_STATE.glob(pattern)):
            if f.is_file():
                shutil.copy2(f, state / f.name)
    # Transcript e documenti di consegna si leggono attraverso HOME: senza
    # questo collegamento la misura e' zero da entrambe le parti e ogni scenario
    # va d'accordo per il motivo sbagliato. In sola lettura: nessuna delle due
    # implementazioni scrive sotto `projects/`.
    (root / '.claude' / 'projects').symlink_to(ROOT / 'projects')

    for rec in case.get('records', []):
        (state / 'sessioni-vive' / f'{rec["session_id"][:8]}.json').write_text(
            json.dumps(rec))
    for name, body in case.get('markers', {}).items():
        (state / name).write_text(body)
    # La data del marcatore di consegna e' un ingresso, non un dettaglio: la
    # guardia «ha ricevuto lavoro dopo la consegna» confronta quel mtime coi
    # timestamp dei messaggi. Senza poterla scegliere, il marcatore nasce adesso
    # e nessun messaggio gli e' mai successivo: il caso non discriminerebbe.
    for name, quando in case.get('marker_times', {}).items():
        os.utime(state / name, (quando, quando))
    return root


def field_cases(pane_list):
    d = json.loads(pane_list.read_text())
    items = d.get('result', d)
    if isinstance(items, dict):
        items = items.get('terminals') or []
    first = items[0] if items else {'handle': 'term_x', 'tabId': 'tab-x'}
    second = items[1] if len(items) > 1 else first

    # Una sessione che era davvero piena e davvero rigenerata stamattina:
    # `e1fa3600` misura **488076** token — il 98% del budget opus-5, sopra
    # l'obbligo di 450000 — e la staffetta l'ha sostituita alle 11:12:09. Il suo
    # transcript e' ancora sul disco, quindi la misura di questi casi e' vera.
    #
    # UN TRANSCRIPT SI SCEGLIE MISURANDOLO. La prima stesura usava quello di
    # `cdca7b36`, che il registro dava a 530857 token: oggi lo stesso file ne
    # misura **72106**, e ogni caso «sessione piena» rispondeva «sotto soglia»
    # da entrambe le parti. Nove scenari verdi, nessuno dei quali arrivava a
    # toccare il codice che dovevano confrontare.
    full = str(ROOT / 'projects/-Users-theo-orca-workspaces-suite-tautog/'
                      'e1fa3600-c381-411d-ae14-7bad97fed6bb.jsonl')
    piena = {
        'session_id': 'e1fa3600-c381-411d-ae14-7bad97fed6bb',
        'terminal_handle': first.get('handle', ''),
        'tab_id': first.get('tabId', ''),
        'worktree_id': 'f304fcc9-d036-47d2-a1fa-ba5661c0e9f9::/home/someone/orca/general',
        'transcript_path': full,
        'cwd': '/home/someone/orca/general',
    }
    # LA STESSA SESSIONE, IN UN PROGETTO CHE NON E' QUELLO DELLA CONSEGNA PIU'
    # RECENTE. Il segnale di ripresa e' un percorso, e i due lo scelgono in modo
    # diverso: `relay.py` chiama `latest_handoff()` **senza** cwd — il piu'
    # recente ovunque — mentre il porto passa quello del record. Finche' il cwd
    # e' il progetto che ha consegnato per ultimo i due coincidono, ed e' il
    # motivo per cui nessun confronto l'aveva visto. `3720a9a4` e' una sessione
    # vera su `other-repo/work/suite`, con la consegna gia' scritta e 369416 token:
    # le mancano ottantamila token per diventare questo caso in produzione.
    altro_progetto = dict(
        piena,
        worktree_id='9591c8dd-9b12-4342-bcbb-c1d6ffff9ff3::/home/someone/other-repo/work/suite',
        cwd='/home/someone/other-repo/work/suite',
    )
    # LA STESSA SESSIONE DENTRO UN ALBERO DI LAVORO, che e' il caso normale: 5
    # rigenerazioni su 11 nel registro del 18/08/2026 partono da qui. La cartella
    # di memoria di `orca/workspaces/suite/tautog` non esiste — la sua sta sotto
    # il repo che lo ospita — quindi il segnale di ripresa dipende dal ramo
    # aggiunto quel giorno, e senza questo caso la parita' su quel ramo non e'
    # confrontata da nessuna parte. Il percorso e' vero perche' `canonical_root`
    # legge il `.git` sul disco, non dentro la HOME finta.
    albero_di_lavoro = dict(
        piena,
        worktree_id='9591c8dd-9b12-4342-bcbb-c1d6ffff9ff3::/home/someone/orca/workspaces/suite/tautog',
        cwd='/home/someone/orca/workspaces/suite/tautog',
    )
    # UN PROGETTO CHE NON HA CONSEGNE PROPRIE. Serve perche' e' l'unico caso che
    # raggiunge il ramo «non trovo niente»: fino al 18/08/2026 li' si ripiegava
    # sul piu' recente ovunque, cioe' sul lavoro di un altro. Senza questo
    # scenario il mutante che rimette il ripiego sopravvive — misurato, era
    # l'unico dei cinque che la batteria non vedeva.
    senza_consegne = dict(
        piena,
        worktree_id='9591c8dd-9b12-4342-bcbb-c1d6ffff9ff3::/home/someone/orca/workspaces/a-client/senza-memoria',
        cwd='/home/someone/orca/workspaces/a-client/senza-memoria',
    )
    # Lo stesso caso con un identificativo di copia senza barre. Ogni worktree
    # vero porta `::/percorso/assoluto`, e un file di raffreddamento chiamato
    # cosi' non si puo' creare: questo e' il controllo che lo dice ad alta voce.
    piena_piatta = dict(piena, worktree_id='wt-campo')
    morto = {
        'session_id': 'deadbeef-0000-0000-0000-000000000000',
        'terminal_handle': 'term_scomparso',
        'tab_id': 'tab-scomparsa',
        'worktree_id': 'wt-campo-morto',
        'transcript_path': full,
        'cwd': '/home/someone/orca/general',
    }
    base = {'list_file': pane_list, 'field': True}
    consegnata = {'consegna-fatta-e1fa3600': '1'}
    return [
        ('CAMPO: lo stato vero, intatto', {**base}),
        ('CAMPO: piu un record col pannello sparito', {**base, 'records': [morto]}),
        ('CAMPO: piu una sessione piena e consegnata',
         {**base, 'records': [piena], 'markers': consegnata}),
        ('CAMPO: la stessa, in un altro progetto',
         {**base, 'records': [altro_progetto], 'markers': consegnata}),
        ('CAMPO: la stessa, dentro un albero di lavoro',
         {**base, 'records': [albero_di_lavoro], 'markers': consegnata}),
        ('CAMPO: la stessa, in un progetto senza consegne',
         {**base, 'records': [senza_consegne], 'markers': consegnata}),
        ('CAMPO: la stessa, con una copia senza barre',
         {**base, 'records': [piena_piatta], 'markers': consegnata}),
        ('CAMPO: piena, ma il successore e gia vivo',
         {**base, 'records': [piena], 'markers': {
             **consegnata,
             'successore-di-e1fa3600-c381-411d-ae14-7bad97fed6bb': json.dumps(
                 {'handle': 'term_stantio', 'tabId': second.get('tabId', '')})}}),
        ('CAMPO: piena, ma il create fallisce',
         {**base, 'records': [piena], 'markers': consegnata, 'create_rc': 1}),
        ('CAMPO: piena, ma la vecchia sta lavorando',
         {**base, 'records': [piena], 'markers': consegnata, 'wait_rc': 1}),
        # LA GUARDIA SUL LAVORO ARRIVATO DOPO. Il transcript di `e1fa3600` ha
        # due messaggi non-strumento alle 09:09:56Z: con la consegna datata
        # prima, la sessione risulta al lavoro e non si tocca; con la consegna
        # datata dopo, torna rigenerabile. Servono tutti e due — un caso solo
        # non distingue «la guardia funziona» da «la guardia dice sempre no».
        ('CAMPO: consegna datata prima del lavoro arrivato dopo',
         {**base, 'records': [piena], 'markers': consegnata,
          'marker_times': {'consegna-fatta-e1fa3600': 1_786_957_200.0}}),   # 09:00:00Z
        ('CAMPO: consegna datata dopo, torna rigenerabile',
         {**base, 'records': [piena], 'markers': consegnata,
          'marker_times': {'consegna-fatta-e1fa3600': 1_786_959_000.0}}),   # 09:30:00Z
        # L'ordine a voce che non arriva: e' il caso reale su **ogni**
        # rigenerazione mai fatta, e finche' gli esiti venivano scartati non
        # lasciava traccia da nessuna delle due parti.
        ('CAMPO: piena, e il successore raccoglie il mandato',
         {**base, 'records': [piena], 'markers': consegnata,
          'worktree': piena['worktree_id'], 'pickup': True, 'pickup_timeout': 5}),
        ('CAMPO: piena, ma l ordine a voce non arriva',
         {**base, 'records': [piena], 'markers': consegnata, 'send_rc': 1}),
        ('CAMPO: piena, a secco',
         {**base, 'records': [piena], 'markers': consegnata, 'secco': True}),
        ('CAMPO: pannello sparito, a secco',
         {**base, 'records': [morto], 'secco': True}),
    ]


def main():
    SCRATCH.mkdir(parents=True, exist_ok=True)
    binroot = SCRATCH / 'bin'
    binroot.mkdir(exist_ok=True)
    fake = binroot / 'orca'
    fake.write_text(FAKE_ORCA)
    fake.chmod(0o755)

    # Prova che il finto orca sia davvero quello che risponde, non quello vero.
    probe = subprocess.run(['orca', 'prova'], capture_output=True, text=True,
                           env={**os.environ, 'PATH': f'{binroot}:' + os.environ['PATH'],
                                'ORCA_LOG': str(SCRATCH / 'probe.log')})
    if not (SCRATCH / 'probe.log').exists():
        print('IL FINTO ORCA NON RISPONDE: il confronto non proverebbe niente',
              file=sys.stderr)
        return 1
    (SCRATCH / 'probe.log').unlink()

    solo_campo = '--field' in sys.argv
    tutto = '--all' in sys.argv
    scenari = []
    if not solo_campo:
        scenari += cases()
    if solo_campo or tutto:
        scenari += field_cases(frozen_pane_list())

    # Lo stato vero si fotografa col tempo di modifica, non col solo nome: un
    # file riscritto con lo stesso contenuto e' comunque una scrittura andata
    # dove non doveva.
    prima_stato = {str(p): p.stat().st_mtime
                   for p in REAL_STATE.rglob('*') if p.is_file()}
    diverged = 0
    for nome, case in scenari:
        risultati = {}
        costruisci = build_home_field if case.get('field') else build_home
        for which in ('python', 'rust'):
            home = costruisci(SCRATCH / f'home-{which}', case)
            log = SCRATCH / f'orca-{which}.log'
            log.unlink(missing_ok=True)
            run_side(which, case, home, log)
            chiamate = log.read_text().splitlines() if log.exists() else []
            risultati[which] = (chiamate, *snapshot(home))

        if risultati['python'] != risultati['rust']:
            diverged += 1
            print(f'\nDIVERGE [{nome}]')
            for i, etichetta in enumerate(('chiamate a orca', 'file di stato',
                                           'righe di registro')):
                a, b = risultati['rust'][i], risultati['python'][i]
                if a == b:
                    continue
                print(f'  {etichetta}:')
                # Elenco per differenza: sullo stato vero i file sono decine, e
                # due liste stampate intere non si confrontano a occhio.
                for riga in sorted(set(b) - set(a)):
                    print(f'    solo python: {riga}')
                for riga in sorted(set(a) - set(b)):
                    print(f'    solo rust  : {riga}')
                if sorted(a) == sorted(b) and a != b:
                    print(f'    stesso insieme, ordine diverso:\n'
                          f'      rust  ={a}\n      python={b}')
        else:
            print(f'  uguali: {nome}  ({len(risultati["rust"][0])} chiamate, '
                  f'{len(risultati["rust"][2])} righe di registro)')

    dopo_stato = {str(p): p.stat().st_mtime
                  for p in REAL_STATE.rglob('*') if p.is_file()}
    if prima_stato != dopo_stato:
        cambiati = {k for k in set(prima_stato) | set(dopo_stato)
                    if prima_stato.get(k) != dopo_stato.get(k)}
        print(f'ATTENZIONE: ~/.claude/state è cambiato durante il confronto: '
              f'{sorted(cambiati)}', file=sys.stderr)
        diverged += 1

    print(f'\n{len(scenari)} scenari confrontati, {diverged} divergenti')
    return 1 if diverged else 0


if __name__ == '__main__':
    sys.exit(main())
