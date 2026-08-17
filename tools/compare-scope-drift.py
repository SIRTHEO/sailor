#!/usr/bin/env python3
"""Confronta il gancio che avvisa sul cambio di mestiere col suo porto in Rust.

L'ORACOLO È IL PYTHON. Qui non si concede e non si blocca niente, quindi la cosa
che può divergere non è un permesso: è **quando** il gancio parla e **cosa
lascia scritto**. Si confrontano quindi quattro cose, byte per byte:

    l'uscita        deve essere 0 sempre — un PostToolUse che esce 2 rimprovera
    stdout          l'avviso, parola per parola
    stderr          deve restare vuoto in entrambi
    il file di stato  `state/scope-drift/<sessione>.json`, col suo esatto
                    `json.dumps` — separatori compresi
    la riga di registro  in `state/ganci.jsonl`, con `ensure_ascii=False` e i
                    separatori con lo spazio — non la forma senza spazi del
                    JavaScript che scrive `journal::record`

IL REGISTRO DEL PYTHON VA CHIESTO A `hook_log`, NON AL GANCIO, e la ragione è
un difetto trovato proprio da questo confronto: `scope-drift.py` importa
`log_hook` e `log_failure` da `hook_log`, che esporta `registra` e
`registra_guasto`. La rinomina in inglese non è arrivata al chiamante, e
l'`except ImportError` che doveva essere una rete li sostituisce con due
funzioni vuote. Misurato il 17/08/2026: `ganci.jsonl` ha 5610 righe e **zero**
`scope-drift`. Quindi confrontare il file scritto dal gancio significherebbe
confrontare il nulla, e un porto che non registrasse niente passerebbe.
Qui l'oracolo del **formato** è `hook_log.registra` in persona, chiamata dallo
strumento con gli stessi argomenti del gancio: così la riga del Rust resta
verificata byte per byte, e il mutante che toglie il registro resta uccidibile.
Correggere l'originale è una riga, ma sta in `skills/hooks/`, fuori dal
perimetro di questo lavoro.

I CASI SONO SEQUENZE, NON CHIAMATE SINGOLE. Le tre promesse del gancio —
parla alla terza area, parla una volta sola, non riscrive lo stato se non è
cambiato niente — non si vedono in una chiamata sola: sono proprietà di una
sessione che prosegue. Un confronto a payload isolati le lascerebbe tutte e tre
fuori, che è esattamente il modo in cui le nove prove originali riuscivano a
essere verdi senza toccare la decisione.

DUE HOME FINTE, UNA PER PARTE. Il gancio **scrive** stato per sessione, e con
una cartella sola la seconda implementazione leggerebbe il lavoro della prima:
troverebbe le aree già accese e il flag «detto» già alzato, e tacerebbe. Il
confronto darebbe verde per il motivo sbagliato.

    python3 tools/compare-scope-drift.py
"""
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path.home() / '.claude'
HOOK = ROOT / 'skills/hooks/scope-drift.py'
BINARY = ROOT / 'rust/target/release/claude-hooks'
SCRATCH = Path(os.environ.get(
    'RELAY_SCRATCH',
    '/private/tmp/claude-501/-Users-theo-orca-general/scratchpad')) / 'scope-drift'

SUITE = '/Users/theo/gyver/work/suite'
PACK = '/Users/theo/gyver/work/packages'
ME = '/Users/theo/gyver/work/matching-engine'
WA = '/Users/theo/gyver/work/whatsapp'
CONF = '/Users/theo/.claude'


sys.path.insert(0, str(ROOT / 'scripts'))
import hook_log  # noqa: E402  — l'oracolo del formato del registro


def oracolo_registro(home: Path, sessione: str, aree):
    """Scrive la riga che il gancio Python scriverebbe se il suo import fosse sano.

    Stessi argomenti della chiamata in `scope-drift.py`, stessa funzione, stesso
    `json.dumps`. Le costanti di modulo si riportano sulla HOME finta perché
    `hook_log` le calcola all'import, sulla HOME vera: senza questo, lo strumento
    scriverebbe righe di prova nel registro di produzione — che è già successo il
    17/08/2026 con la staffetta, ed è il motivo per cui esiste `HomeIsolata`.
    """
    prima = (hook_log.FOLDER, hook_log.REGISTRO)
    hook_log.FOLDER = home / '.claude' / 'state'
    hook_log.REGISTRO = hook_log.FOLDER / 'ganci.jsonl'
    try:
        hook_log.registra('scope-drift', 'avvisa', 'terza-area',
                          session=sessione[:8], areas=aree)
    finally:
        hook_log.FOLDER, hook_log.REGISTRO = prima


def aree_correnti(home: Path, sessione: str):
    """Le aree che il gancio ha appena scritto nel proprio stato."""
    testo = stato(home, sessione)
    try:
        return json.loads(testo).get('aree', [])
    except Exception:
        return []


def cartella(home: Path):
    """Se la cartella dello stato è stata creata.

    Sembra un dettaglio e non lo è: è l'unica traccia osservabile del filtro che
    fa uscire subito quando nel payload non compare nessuna area. Quel filtro è
    ciò che rende gratuito il caso normale — la stragrande maggioranza delle
    chiamate a strumento non nomina nessun repo — e senza questo campo un porto
    che lo togliesse si comporterebbe in modo identico su tutto il resto, cioè
    passerebbe il confronto toccando il disco a ogni chiamata.
    """
    return (home / '.claude' / 'state' / 'scope-drift').is_dir()


def stato(home: Path, sessione: str):
    """Il file di stato COME TESTO: la forma è ciò che deve coincidere.

    Rianalizzarlo in un oggetto nasconderebbe proprio le due differenze che
    separano `json.dumps` da `serde_json`: i separatori e l'ordine delle chiavi.
    """
    p = home / '.claude' / 'state' / 'scope-drift' / f'{sessione}.json'
    try:
        return p.read_text(errors='replace')
    except OSError:
        return None


def registro(home: Path):
    """Le righe del registro dei ganci, senza l'istante e come testo grezzo."""
    p = home / '.claude' / 'state' / 'ganci.jsonl'
    try:
        righe = [r for r in p.read_text(errors='replace').splitlines() if r.strip()]
    except OSError:
        return []
    # L'istante differisce per costruzione. Si taglia il valore ma **non** la
    # chiave: se una delle due implementazioni smettesse di scriverlo, o lo
    # scrivesse senza lo spazio dopo i due punti, il confronto deve vederlo.
    fuori = []
    for r in righe:
        testa, _, coda = r.partition(', "gancio"')
        fuori.append(('<t>' if testa.startswith('{"t": ') else testa)
                     + ', "gancio"' + coda)
    return fuori


def esegui(comando, home: Path, passi, sessione, env_extra=None, oracolo=False):
    """Fa girare l'intera sequenza su una HOME appena azzerata.

    `oracolo` vale solo per il lato Python: quando il gancio ha parlato, la riga
    di registro la scrive lo strumento al posto suo — vedi l'intestazione.
    """
    shutil.rmtree(home, ignore_errors=True)
    (home / '.claude' / 'state').mkdir(parents=True)
    env = dict(os.environ, HOME=str(home))
    env.pop('SCOPE_DRIFT', None)
    env.update(env_extra or {})
    passaggi = []
    # I casi con un payload intero portano il proprio identificativo di
    # sessione, e lo stato va cercato SOTTO QUELLO. Prima si cercava sempre
    # `sessione`, quindi per quei casi il file confrontato era un `None` da
    # entrambe le parti: sembravano verdi senza aver confrontato niente.
    attuale = sessione
    for passo in passi:
        # Un passo è o un payload intero (per i casi malformati) o il solo
        # `tool_input`, che è la forma normale.
        if isinstance(passo, str):
            grezzo = passo
        elif '__payload__' in passo:
            grezzo = json.dumps(passo['__payload__'])
            attuale = str(passo['__payload__'].get('session_id') or '')
        else:
            grezzo = json.dumps({'tool_name': 'Bash', 'session_id': sessione,
                                 'cwd': str(home), 'tool_input': passo})
        p = subprocess.run(comando, input=grezzo, capture_output=True,
                           text=True, timeout=60, cwd=str(home), env=env)
        passaggi.append((p.returncode,
                         p.stdout.replace(str(home), '<HOME>'),
                         p.stderr.replace(str(home), '<HOME>')))
        if oracolo and p.stdout.strip():
            oracolo_registro(home, attuale, aree_correnti(home, attuale))
    return passaggi, stato(home, attuale), registro(home), cartella(home)


def casi():
    """Ogni caso: nome, sequenza di passi, e la sessione a cui appartengono."""
    return [
        # ── le tre promesse, ognuna isolata ────────────────────────────────
        ('una sola area: non parla', [{'file_path': f'{SUITE}/a.ts'}]),
        ('due aree: restano normali',
         [{'file_path': f'{SUITE}/a.ts'}, {'file_path': f'{PACK}/b.ts'}]),
        ('alla terza area parla',
         [{'file_path': f'{SUITE}/a.ts'}, {'file_path': f'{PACK}/b.ts'},
          {'file_path': f'{CONF}/rules/x.md'}]),
        ('detto una volta, non lo ripete',
         [{'file_path': f'{SUITE}/a.ts'}, {'file_path': f'{PACK}/b.ts'},
          {'file_path': f'{CONF}/rules/x.md'}, {'file_path': f'{WA}/c.ts'},
          {'file_path': f'{ME}/d.ts'}]),
        ('tre aree in un colpo solo bastano',
         [{'command': f'diff {SUITE}/a {PACK}/b {CONF}/x'}]),
        ('due aree in un colpo solo non bastano',
         [{'command': f'diff {SUITE}/a {PACK}/b'}]),
        ('quattro aree in un colpo solo: il conteggio è quello vero',
         [{'command': f'diff {SUITE}/a {PACK}/b {CONF}/x {WA}/y'}]),
        ('la stessa area ripetuta non riscrive niente',
         [{'file_path': f'{SUITE}/a.ts'}, {'file_path': f'{SUITE}/b.ts'},
          {'file_path': f'{SUITE}/c.ts'}]),
        # Il ristagno DOPO la soglia: già tre aree e già detto, poi solo tocchi
        # di aree note. Se il ramo «niente di nuovo» cadesse, qui l'avviso
        # tornerebbe a ogni chiamata.
        ('a tre aree, i tocchi successivi restano muti',
         [{'command': f'diff {SUITE}/a {PACK}/b {CONF}/x'},
          {'file_path': f'{SUITE}/z.ts'}, {'file_path': f'{PACK}/z.ts'},
          {'file_path': f'{CONF}/z.md'}]),

        # ── i riconoscitori, uno per uno ───────────────────────────────────
        ('suite', [{'file_path': f'{SUITE}/src/x.ts'}]),
        ('matching-engine', [{'command': f'pnpm --dir {ME} run test'}]),
        ('whatsapp', [{'file_path': f'{WA}/src/a.ts'}]),
        ('packages', [{'file_path': f'{PACK}/geo/x.ts'}]),
        ('dev-stack', [{'command': '/Users/theo/gyver/work/dev-stack/up.sh'}]),
        ('configurazione', [{'file_path': f'{CONF}/rules/x.md'}]),
        ('personale via /personal/', [{'file_path': '/Users/theo/personal/sailor/x.ts'}]),
        ('personale via orca/general', [{'command': 'ls /Users/theo/orca/general'}]),

        # ── i confini che si leggono male ──────────────────────────────────
        ('i trascritti non sono configurazione',
         [{'file_path': f'{CONF}/projects/x/y.jsonl'}]),
        ('la cronologia non è configurazione', [{'file_path': f'{CONF}/history/x'}]),
        # `projectsX` comincia come `projects`: lo sguardo in avanti guarda il
        # prefisso, non il segmento, e quindi ESCLUDE anche questo. È il caso su
        # cui avevo scritto la prova al contrario.
        ('una cartella che comincia come projects resta esclusa',
         [{'file_path': f'{CONF}/projectsX/y'}]),
        ('il trascritto e la configurazione insieme: vince la configurazione',
         [{'command': f'diff {CONF}/projects/a.jsonl {CONF}/settings.json'}]),
        ('senza la barra finale non è configurazione', [{'file_path': CONF}]),
        # `\b` guarda i caratteri di parola, non i segmenti: `-` non è parola.
        ('un trattino dopo il nome del repo conta come suite',
         [{'file_path': '/Users/theo/gyver/work/suite-vecchia/x.ts'}]),
        ('una lettera dopo il nome del repo no',
         [{'file_path': '/Users/theo/gyver/work/suiteXX/x.ts'}]),
        ('un worktree Orca conta come il suo repo',
         [{'command': 'git -C /Users/theo/orca/workspaces/whatsapp/x status'}]),
        # QUIRK: `packages` non ha la variante `workspaces/`, quindi una sua
        # copia di lavoro non accende niente. È così anche in `measure-drift.py`.
        ('un worktree di packages non conta',
         [{'command': 'ls /Users/theo/orca/workspaces/packages/wt'}]),

        # ── i campi ────────────────────────────────────────────────────────
        ('il campo path', [{'path': f'{SUITE}/x'}]),
        ('il campo notebook_path', [{'notebook_path': f'{SUITE}/x.ipynb'}]),
        ('due campi diversi fanno due aree',
         [{'command': f'ls {SUITE}', 'file_path': f'{PACK}/x.ts'}]),
        ('un campo che non è una stringa non si legge',
         [{'command': 42, 'file_path': None}]),
        ('un campo ignoto non si guarda', [{'contenuto': f'{SUITE}/x'}]),
        ('tool_input vuoto', [{}]),
        ('niente di riconoscibile', [{'command': 'ls /tmp'}]),

        # ── perimetro e valvola ────────────────────────────────────────────
        # Il matcher registrato è `*`: l'originale NON guarda `tool_name`, e un
        # porto che lo filtrasse tacerebbe su tutto ciò che non è Bash.
        ('uno strumento che non è Bash conta lo stesso',
         [{'__payload__': {'tool_name': 'Read', 'session_id': 'S-tool',
                           'tool_input': {'command': f'diff {SUITE}/a {PACK}/b {CONF}/x'}}}]),
        ('senza tool_name conta lo stesso',
         [{'__payload__': {'session_id': 'S-notool',
                           'tool_input': {'command': f'diff {SUITE}/a {PACK}/b {CONF}/x'}}}]),

        # ── ingressi malformati: nessuno deve far uscire diverso da 0 ──────
        ('stdin vuoto', ['']),
        ('stdin non JSON', ['non sono json']),
        ('JSON che non è un oggetto', ['[1, 2, 3]']),
        ('senza session_id',
         [{'__payload__': {'tool_input': {'command': f'diff {SUITE}/a {PACK}/b {CONF}/x'}}}]),
        ('session_id vuoto',
         [{'__payload__': {'session_id': '',
                           'tool_input': {'command': f'diff {SUITE}/a {PACK}/b {CONF}/x'}}}]),
        # `str(ev.get("session_id") or "")` accetta un numero: `as_str()` no.
        ('session_id numerico',
         [{'__payload__': {'session_id': 4242,
                           'tool_input': {'command': f'diff {SUITE}/a {PACK}/b {CONF}/x'}}}]),
        # Un identificativo vero è un UUID di 36 caratteri, e nel registro se ne
        # scrivono otto. Senza questo caso il troncamento non lo prova nessuno:
        # tutte le sessioni finte qui sopra sono già corte, e il mutante che lo
        # toglie sopravviveva — misurato al primo giro di mutanti.
        ('session_id lungo: nel registro si tronca a otto',
         [{'__payload__': {'session_id': 'abcdef01-2345-6789-abcd-ef0123456789',
                           'tool_input': {'command': f'diff {SUITE}/a {PACK}/b {CONF}/x'}}}]),
        ('session_id zero è falso come la stringa vuota',
         [{'__payload__': {'session_id': 0,
                           'tool_input': {'command': f'diff {SUITE}/a {PACK}/b {CONF}/x'}}}]),
        ('senza tool_input',
         [{'__payload__': {'session_id': 'S-noargs'}}]),
        ('tool_input non è un oggetto',
         [{'__payload__': {'session_id': 'S-str', 'tool_input': 'ciao'}}]),
        ('tool_input è una lista',
         [{'__payload__': {'session_id': 'S-list', 'tool_input': [1, 2]}}]),
    ]


def stato_corrotto():
    """I casi che partono da un file di stato già scritto, e storto.

    Ognuno è (nome, contenuto del file, passo). Servono perché lo stato è
    l'unico ingresso che il gancio si scrive da solo: se le due implementazioni
    lo rileggono in modo diverso, la divergenza compare **al secondo giro** di
    una sessione vera, che è il posto peggiore per scoprirla.
    """
    return [
        ('stato illeggibile', 'non sono json'),
        ('stato vuoto', '{}'),
        ('stato senza aree', '{"detto": false}'),
        ('stato con aree vuote', '{"aree": [], "detto": false}'),
        ('stato già a due aree', '{"aree": ["suite", "packages"], "detto": false}'),
        ('stato già detto', '{"aree": ["suite", "packages"], "detto": true}'),
        # IL CASO CHE SMASCHERA il ramo «niente di nuovo»: tre aree già accese,
        # avviso non ancora dato, e un tocco che non aggiunge niente. Con il ramo
        # si tace; senza, si parlerebbe — e si parlerebbe di nuovo a ogni tocco
        # successivo. Senza questo caso quel ramo non lo prova nessuno.
        ('stato a tre aree non ancora detto',
         '{"aree": ["suite", "packages", "configurazione"], "detto": false}'),
        # `set("suite")` di Python sono i caratteri, non la parola.
        ('aree come stringa', '{"aree": "suite", "detto": false}'),
        # `bool(1)` è vero: l'avviso risulta già dato.
        ('detto come numero', '{"aree": ["suite", "packages"], "detto": 1}'),
        ('detto come stringa vuota', '{"aree": ["suite", "packages"], "detto": ""}'),
        ('aree con un elemento non testuale', '{"aree": ["suite", null], "detto": false}'),
        ('stato che è una lista', '[1, 2, 3]'),
    ]


def esegui_corrotto(comando, home: Path, contenuto, sessione, oracolo=False):
    shutil.rmtree(home, ignore_errors=True)
    d = home / '.claude' / 'state' / 'scope-drift'
    d.mkdir(parents=True)
    (d / f'{sessione}.json').write_text(contenuto)
    env = dict(os.environ, HOME=str(home))
    env.pop('SCOPE_DRIFT', None)
    grezzo = json.dumps({'tool_name': 'Bash', 'session_id': sessione,
                         'tool_input': {'command': f'ls {CONF}/rules'}})
    p = subprocess.run(comando, input=grezzo, capture_output=True, text=True,
                       timeout=60, cwd=str(home), env=env)
    if oracolo and p.stdout.strip():
        oracolo_registro(home, sessione, aree_correnti(home, sessione))
    return ([(p.returncode, p.stdout.replace(str(home), '<HOME>'),
              p.stderr.replace(str(home), '<HOME>'))],
            stato(home, sessione), registro(home), cartella(home))


def main():
    if not BINARY.exists():
        print(f'manca {BINARY}: cargo build --release')
        return 1
    SCRATCH.mkdir(parents=True, exist_ok=True)
    py, rs = SCRATCH / 'home-python', SCRATCH / 'home-rust'
    cmd_py = [sys.executable, str(HOOK)]
    cmd_rs = [str(BINARY), 'scope-drift']

    # `oracolo` si accende solo sul lato Python: è il rattoppo che rimette la
    # riga di registro che il gancio non riesce più a scrivere.
    prove = []
    for i, (nome, passi) in enumerate(casi()):
        sessione = f'S{i:03d}'
        prove.append((nome, lambda c, h, o, p=passi, s=sessione: esegui(c, h, p, s, oracolo=o)))
    for i, (nome, contenuto) in enumerate(stato_corrotto()):
        sessione = f'C{i:03d}'
        prove.append((f'stato: {nome}',
                      lambda c, h, o, t=contenuto, s=sessione:
                      esegui_corrotto(c, h, t, s, oracolo=o)))
    # La valvola: spenta, il gancio non deve fare NIENTE — nemmeno scrivere lo
    # stato. Senza questo caso, un porto che leggesse la variabile dopo aver
    # scritto passerebbe il confronto.
    prove.append(('valvola SCOPE_DRIFT=off',
                  lambda c, h, o: esegui(c, h, [{'command': f'diff {SUITE}/a {PACK}/b {CONF}/x'}],
                                         'S-off', {'SCOPE_DRIFT': 'off'}, oracolo=o)))
    prove.append(('valvola con un valore diverso da off: resta accesa',
                  lambda c, h, o: esegui(c, h, [{'command': f'diff {SUITE}/a {PACK}/b {CONF}/x'}],
                                         'S-on', {'SCOPE_DRIFT': 'acceso'}, oracolo=o)))

    divergenze = 0
    for nome, corri in prove:
        try:
            attesa = corri(cmd_py, py, True)
            ottenuta = corri(cmd_rs, rs, False)
        except Exception as exc:                       # pragma: no cover
            print(f'  ERRORE      {nome}: {exc}')
            divergenze += 1
            continue
        if attesa == ottenuta:
            parla = 'PARLA' if any(s.strip() for _, s, _ in attesa[0]) else 'tace'
            print(f'  uguale      {nome}  ({parla})')
            continue
        divergenze += 1
        print(f'  DIVERGE     {nome}')
        if attesa[0] != ottenuta[0]:
            for n, (a, o) in enumerate(zip(attesa[0], ottenuta[0])):
                if a != o:
                    print(f'      passo {n} python={a!r}')
                    print(f'      passo {n} rust=  {o!r}')
        if attesa[1] != ottenuta[1]:
            print(f'      stato python: {attesa[1]!r}')
            print(f'      stato rust:   {ottenuta[1]!r}')
        if attesa[2] != ottenuta[2]:
            print(f'      registro python: {attesa[2]!r}')
            print(f'      registro rust:   {ottenuta[2]!r}')
        if attesa[3] != ottenuta[3]:
            print(f'      cartella dello stato  python={attesa[3]}  rust={ottenuta[3]}')
    print()
    if divergenze:
        print(f'{divergenze} divergenze su {len(prove)} casi')
    else:
        print(f'{len(prove)} casi, nessuna divergenza')
    return 1 if divergenze else 0


if __name__ == '__main__':
    sys.exit(main())
