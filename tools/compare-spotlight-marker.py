#!/usr/bin/env python3
"""Confronta il gancio che rimette l'esclusione da Spotlight col suo porto in Rust.

L'ORACOLO È IL PYTHON. Il gancio non concede e non blocca niente: quello che può
divergere è **quando scrive**, **dove scrive** e **cosa dice**. Si confrontano
quindi quattro cose, byte per byte:

    l'uscita        0 quasi sempre — ma non sempre, vedi sotto
    stdout          deve restare vuoto in entrambi
    stderr          il messaggio col conteggio e la radice, parola per parola
    l'albero        ogni file e ogni cartella rimasti sul disco dopo la corsa,
                    coi loro percorsi relativi e la loro dimensione

L'ALBERO È LO STATO, e per questo le due implementazioni ne hanno uno per parte.
Il gancio scrive marcatori dentro l'albero su cui gira: con una cartella sola,
la seconda implementazione troverebbe i marcatori già messi dalla prima e non
toccherebbe niente. Il confronto darebbe verde per il motivo sbagliato, e
proprio sul caso che conta di più — «rimette solo quello che manca». Ogni parte
ha quindi la sua HOME finta, e l'albero ci vive dentro.

I CASI SONO SEQUENZE dove lo stato conta. «Scrive una volta sola» e «rimette solo
il mancante» non si vedono in una chiamata isolata: sono proprietà di un albero
che è già stato toccato.

IL TRACEBACK SI NORMALIZZA ALLA SUA ULTIMA RIGA, e va dichiarato perché è
l'unica normalizzazione oltre ai percorsi. Cinque ingressi malformati fanno
morire l'originale con uscita 1 e un traceback: le righe intermedie contengono il
percorso del file e i numeri di riga del sorgente Python, che nessun porto può
riprodurre e che cambiano con la versione dell'interprete. L'ultima riga —
`AttributeError: 'list' object has no attribute 'get'` — è invece la parte che
dice cosa è successo, ed è quella che il Rust stampa tale e quale. Un porto che
non morisse divergerebbe comunque sull'uscita.

    python3 tools/compare-spotlight-marker.py
"""
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path.home() / '.claude'
sys.path.insert(0, str(Path(__file__).resolve().parent))
from oracle import Oracle                                  # noqa: E402

HOOK = ROOT / 'skills/hooks/spotlight-marker.py'
BINARY = ROOT / 'rust/target/release/claude-hooks'
SCRATCH = Path(os.environ.get(
    'RELAY_SCRATCH',
    '/private/tmp/claude-501/-Users-theo-orca-general/scratchpad')) / 'spotlight-marker'

MARKER = '.metadata_never_index'
# Il segnaposto che diventa il percorso dell'albero di questa parte. Serve
# perché i due alberi stanno in cartelle diverse: è ciò che rende il confronto
# possibile senza farli condividere lo stato.
TREE = '@TREE@'


# ── l'albero di partenza ─────────────────────────────────────────────────────
def costruisci(tree: Path, ricetta):
    """Crea l'albero descritto dalla ricetta.

    `('d', rel)` una cartella · `('f', rel)` un file vuoto · `('l', rel, dest)`
    un link simbolico, con `dest` risolto contro l'albero.
    """
    tree.mkdir(parents=True, exist_ok=True)
    for voce in ricetta:
        if voce[0] == 'd':
            (tree / voce[1]).mkdir(parents=True, exist_ok=True)
        elif voce[0] == 'f':
            p = tree / voce[1]
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text('')
        elif voce[0] == 'l':
            p = tree / voce[1]
            p.parent.mkdir(parents=True, exist_ok=True)
            os.symlink(str(tree / voce[2]), str(p))
        else:                                          # pragma: no cover
            raise ValueError(voce)


def fotografia(tree: Path):
    """Ogni cartella, file e link rimasti, coi percorsi relativi e la dimensione.

    Non solo i marcatori: un porto che scrivesse il file giusto nel posto
    sbagliato, o un file di nome diverso, passerebbe un confronto che guarda
    soltanto `.metadata_never_index`.
    """
    fuori = []
    for base, dirs, files in os.walk(tree, followlinks=False):
        dirs.sort()
        rel = os.path.relpath(base, tree)
        for d in dirs:
            fuori.append(('d', os.path.join(rel, d)))
        for f in sorted(files):
            p = Path(base) / f
            tipo = 'l' if p.is_symlink() else 'f'
            misura = 0 if tipo == 'l' else p.stat().st_size
            fuori.append((tipo, os.path.join(rel, f), misura))
    return sorted(fuori)


# ── la corsa ─────────────────────────────────────────────────────────────────
def sostituisci(v, tree: str):
    """Rimpiazza il segnaposto dell'albero ovunque compaia nel payload."""
    if isinstance(v, str):
        return v.replace(TREE, tree)
    if isinstance(v, dict):
        return {k: sostituisci(x, tree) for k, x in v.items()}
    if isinstance(v, list):
        return [sostituisci(x, tree) for x in v]
    return v


def normalizza(testo: str, home: Path, tree: Path):
    """Toglie i percorsi della parte e riduce il traceback alla sua ultima riga."""
    for vero in (str(tree), os.path.realpath(tree), str(home), os.path.realpath(home)):
        testo = testo.replace(vero, '<T>')
    if testo.startswith('Traceback (most recent call last):'):
        righe = [r for r in testo.splitlines() if r.strip()]
        testo = righe[-1] + '\n' if righe else testo
    return testo


def esegui(comando, home: Path, ricetta, passi, env_extra=None):
    """Una parte intera: albero da zero, tutti i passi in fila, poi la fotografia."""
    shutil.rmtree(home, ignore_errors=True)
    tree = home / 'albero'
    costruisci(tree, ricetta)
    env = dict(os.environ, HOME=str(home))
    env.pop('MARCATORE_SPOTLIGHT', None)
    env.update(env_extra or {})
    passaggi = []
    for passo in passi:
        # Un passo può essere una manomissione dell'albero invece di una
        # chiamata al gancio: è il modo di provare «rimette solo il mancante»
        # senza costruire un albero nuovo, cioè sullo stato che il gancio si è
        # scritto da solo al passo prima.
        if isinstance(passo, tuple) and passo[0] == 'rimuovi':
            (tree / passo[1]).unlink()
            continue
        grezzo = passo if isinstance(passo, str) else json.dumps(sostituisci(passo, str(tree)))
        p = subprocess.run(comando, input=grezzo, capture_output=True, text=True,
                           timeout=60, cwd=str(tree), env=env)
        passaggi.append((p.returncode,
                         normalizza(p.stdout, home, tree),
                         normalizza(p.stderr, home, tree)))
    return passaggi, fotografia(tree)


def bash(command, cwd=TREE):
    """Il payload normale: PostToolUse su Bash, con la radice sull'albero."""
    payload = {'tool_name': 'Bash', 'tool_input': {'command': command}}
    if cwd is not None:
        payload['cwd'] = cwd
    return payload


# ── i casi ───────────────────────────────────────────────────────────────────
INSTALL = 'pnpm install'
NM = [('d', 'app/node_modules'), ('d', 'target')]


def casi():
    """Ogni caso: nome, ricetta dell'albero, sequenza di passi, ambiente."""
    fuori = [
        # ── il mestiere ────────────────────────────────────────────────────
        ('mette il marcatore dove manca', NM, [bash(INSTALL)]),
        ('due passate: la seconda non tocca niente', NM, [bash(INSTALL), bash(INSTALL)]),
        ('un marcatore già presente non si conta',
         NM + [('f', f'target/{MARKER}')], [bash(INSTALL)]),
        ('tutti già presenti: non parla',
         NM + [('f', f'target/{MARKER}'), ('f', f'app/node_modules/{MARKER}')],
         [bash(INSTALL)]),
        ('nessun albero di dipendenze: non parla', [('d', 'src')], [bash(INSTALL)]),
        ('albero vuoto', [], [bash(INSTALL)]),
        # Il caso che presidia il `-prune`: senza, la annidata prende un
        # marcatore dentro un albero che ha già il suo.
        ('la node_modules annidata resta fuori',
         [('d', 'app/node_modules/pkg/node_modules')], [bash(INSTALL)]),
        ('al quinto livello non si guarda',
         [('d', 'b/c/d/node_modules'), ('d', 'b/c/d/e/node_modules')], [bash(INSTALL)]),
        ('al quarto livello sì', [('d', 'b/c/d/node_modules')], [bash(INSTALL)]),
        ('più cartelle insieme: il conteggio è quello vero',
         [('d', 'a/node_modules'), ('d', 'b/node_modules'), ('d', 'target')],
         [bash(INSTALL)]),
        # Un marcatore che è una cartella: `open(..., 'a')` fallisce e il
        # percorso non entra fra i toccati, ma l'altro sì.
        ('un marcatore che è una cartella non si conta',
         NM + [('d', f'target/{MARKER}')], [bash(INSTALL)]),
        ('una cartella che si chiama come il marcatore altrove non conta',
         [('d', 'src'), ('d', f'src/{MARKER}')], [bash(INSTALL)]),

        # ── i link simbolici ───────────────────────────────────────────────
        # `-type d` guarda il file, non il bersaglio: un link non combacia.
        ('un link chiamato node_modules non combacia',
         [('d', 's/real'), ('l', 's/node_modules', 's/real')], [bash(INSTALL)]),
        # `find` senza `-L` non attraversa nemmeno la radice, se è un link.
        ('una radice che è un link non si attraversa',
         NM + [('l', 'scorciatoia', '')],
         [bash(INSTALL, cwd=f'{TREE}/scorciatoia')]),

        # ── la forma della radice ──────────────────────────────────────────
        ('la barra finale si conserva nel messaggio', NM,
         [bash(INSTALL, cwd=f'{TREE}/')]),
        ('la radice è essa stessa una node_modules',
         [('d', 'node_modules/pkg/node_modules')],
         [bash(INSTALL, cwd=f'{TREE}/node_modules')]),
        ('la radice è una node_modules, con la barra',
         [('d', 'node_modules/pkg')],
         [bash(INSTALL, cwd=f'{TREE}/node_modules/')]),
        ('la radice non esiste', NM, [bash(INSTALL, cwd=f'{TREE}/nope')]),
        ('la radice è un file', NM + [('f', 'file')], [bash(INSTALL, cwd=f'{TREE}/file')]),
        ('una radice relativa', NM, [bash(INSTALL, cwd='.')]),
        ('senza cwd si usa quella del processo', NM, [bash(INSTALL, cwd=None)]),
        ('cwd vuota ripiega sulla cwd del processo', NM, [bash(INSTALL, cwd='')]),
        # DIFETTO: `find` riceve una lista di percorsi ma l'originale la
        # rilegge come testo e la rispezza con `splitlines()`. Una cartella
        # intermedia con un a capo produce una riga finta — e se quella riga
        # esiste come cartella, ci finisce dentro un marcatore.
        ('un a capo nel nome di una cartella spezza la riga',
         [('d', 'we\nird/node_modules'), ('d', 'we'), ('d', 'ok/node_modules')],
         [bash(INSTALL)]),
        # I separatori su cui Python spezza e `str::lines()` di Rust no. Col
        # solo `\n` il mutante che scambia i due sopravviveva: `lines()` spezza
        # anche lui sul ritorno a capo, quindi il caso qui sopra non lo vedeva.
        # Qui la riga finta è `<T>/we`, che esiste come cartella: chi non
        # spezza mette il marcatore dentro la `node_modules` vera, chi spezza lo
        # mette in `<T>/we`. Due alberi diversi, divergenza visibile.
        ('un ritorno a capo solitario spezza come in Python',
         [('d', 'we\rird/node_modules'), ('d', 'we')], [bash(INSTALL)]),
        ('il separatore di riga Unicode spezza come in Python',
         [('d', 'we\u2028ird/node_modules'), ('d', 'we')], [bash(INSTALL)]),
        ('la tabulazione verticale spezza come in Python',
         [('d', 'we\vird/node_modules'), ('d', 'we')], [bash(INSTALL)]),

        # ── i riconoscitori, uno per uno ───────────────────────────────────
    ]
    for c in ('pnpm install', 'pnpm add react', 'pnpm update', 'npm install',
              'npm ci', 'npm update', 'yarn install', 'yarn add react',
              'bun install', 'bun add react', 'cargo build --release',
              'cargo update'):
        fuori.append((f'riconosce «{c}»', NM, [bash(c)]))
    for c in ('pnpm run build', 'npm test', 'git status', 'ls node_modules',
              'pnpm exec tsc', 'npm run install-check', ''):
        fuori.append((f'ignora «{c}»', NM, [bash(c)]))
    fuori += [
        ('il comando si normalizza sugli spazi', NM, [bash('pnpm\n\n   install')]),
        # `str.split()` di Python spezza anche sui separatori di controllo, che
        # `char::is_whitespace` di Rust non considera spazio.
        ('gli spazi di Python comprendono i separatori di controllo', NM,
         [bash('pnpm\x1cinstall')]),
        ('un a capo dentro la parola non la ricompone', NM, [bash('pnpmi\nnstall')]),
        ('il confronto è per sottostringa', NM, [bash('echo pnpm install')]),

        # ── perimetro ──────────────────────────────────────────────────────
        ('uno strumento che non è Bash non fa niente', NM,
         [{'tool_name': 'Read', 'cwd': TREE, 'tool_input': {'command': INSTALL}}]),
        ('senza tool_name non fa niente', NM,
         [{'cwd': TREE, 'tool_input': {'command': INSTALL}}]),
        ('tool_name non testuale non fa niente', NM,
         [{'tool_name': 42, 'cwd': TREE, 'tool_input': {'command': INSTALL}}]),
        ('senza tool_input', NM, [{'tool_name': 'Bash', 'cwd': TREE}]),
        ('senza command', NM, [{'tool_name': 'Bash', 'cwd': TREE, 'tool_input': {}}]),

        # ── ingressi malformati ────────────────────────────────────────────
        ('stdin vuoto', NM, ['']),
        ('stdin non JSON', NM, ['non sono json']),
        # I falsi passano dal `or {}` e non fanno esplodere niente.
        ('tool_input zero', NM, [{'tool_name': 'Bash', 'cwd': TREE, 'tool_input': 0}]),
        ('tool_input stringa vuota', NM,
         [{'tool_name': 'Bash', 'cwd': TREE, 'tool_input': ''}]),
        ('tool_input lista vuota', NM,
         [{'tool_name': 'Bash', 'cwd': TREE, 'tool_input': []}]),
        ('tool_input null', NM,
         [{'tool_name': 'Bash', 'cwd': TREE, 'tool_input': None}]),
        # DIFETTO: `payload.get` sta fuori dal `try`. Uscita 1 e traceback.
        ('un payload che non è un oggetto', NM, ['[1, 2, 3]']),
        ('un payload che è una stringa', NM, ['"ciao"']),
        ('un payload che è un numero', NM, ['42']),
        ('un payload che è null', NM, ['null']),
        ('un payload che è true', NM, ['true']),
        ('un payload che è un decimale', NM, ['4.2']),
        # DIFETTO: `tool_input` vero ma non dizionario, stessa uscita.
        ('tool_input è una stringa', NM,
         [{'tool_name': 'Bash', 'cwd': TREE, 'tool_input': 'ciao'}]),
        ('tool_input è una lista', NM,
         [{'tool_name': 'Bash', 'cwd': TREE, 'tool_input': [1, 2]}]),
        # DIFETTO: `command.split()` su un valore che non è testo.
        ('command numerico', NM,
         [{'tool_name': 'Bash', 'cwd': TREE, 'tool_input': {'command': 42}}]),
        ('command null', NM,
         [{'tool_name': 'Bash', 'cwd': TREE, 'tool_input': {'command': None}}]),
        ('command lista', NM,
         [{'tool_name': 'Bash', 'cwd': TREE, 'tool_input': {'command': ['pnpm install']}}]),
        # DIFETTO: `os.path.isdir` su un intero lo legge come descrittore di
        # file e risponde no; su un decimale o una lista alza un TypeError.
        ('cwd numerica', NM,
         [{'tool_name': 'Bash', 'cwd': 999, 'tool_input': {'command': INSTALL}}]),
        ('cwd decimale', NM,
         [{'tool_name': 'Bash', 'cwd': 4.2, 'tool_input': {'command': INSTALL}}]),
        ('cwd lista', NM,
         [{'tool_name': 'Bash', 'cwd': ['x'], 'tool_input': {'command': INSTALL}}]),
        ('cwd dizionario', NM,
         [{'tool_name': 'Bash', 'cwd': {'a': 1}, 'tool_input': {'command': INSTALL}}]),

        # ── sequenze ───────────────────────────────────────────────────────
        ('installa, si perde un marcatore, reinstalla: ne rimette uno solo', NM,
         [bash(INSTALL), ('rimuovi', f'target/{MARKER}'), bash(INSTALL)]),
        ('si perdono tutti e due: ne rimette due', NM,
         [bash(INSTALL), ('rimuovi', f'target/{MARKER}'),
          ('rimuovi', f'app/node_modules/{MARKER}'), bash(INSTALL)]),
        ('una installazione dopo un comando qualunque', NM,
         [bash('git status'), bash(INSTALL), bash('npm test'), bash(INSTALL)]),
    ]
    return fuori


def prove():
    """I casi normali più quelli che hanno bisogno di un ambiente diverso."""
    fuori = [(nome, ricetta, passi, None) for nome, ricetta, passi in casi()]
    # La valvola: spenta, il gancio non deve scrivere NIENTE. Senza questo caso
    # un porto che la leggesse dopo aver scritto passerebbe il confronto.
    fuori.append(('valvola MARCATORE_SPOTLIGHT=off', NM, [bash(INSTALL)],
                  {'MARCATORE_SPOTLIGHT': 'off'}))
    fuori.append(('valvola con un valore diverso da off: resta accesa', NM,
                  [bash(INSTALL)], {'MARCATORE_SPOTLIGHT': 'acceso'}))
    return fuori


def main():
    if not BINARY.exists():
        print(f'manca {BINARY}: cargo build --release')
        return 1
    SCRATCH.mkdir(parents=True, exist_ok=True)
    py, rs = SCRATCH / 'home-python', SCRATCH / 'home-rust'
    cmd_py = [sys.executable, str(HOOK)]
    cmd_rs = [str(BINARY), 'spotlight-marker']
    # L'oracolo puo' non esserci piu': finche' il Python e' sul disco si
    # interroga lui e il registro fa da controllo di se stesso; quando sara'
    # cancellato, le sue risposte restano qui. `SCRATCH` va ripulita perche'
    # compare dentro le risposte e cambia a ogni esecuzione.
    oracle = Oracle('spotlight-marker', HOOK, scrub=[SCRATCH])
    print(oracle.describe())

    divergenze = 0
    elenco = prove()
    for nome, ricetta, passi, env in elenco:
        try:
            attesa = oracle.answer(nome, lambda: esegui(cmd_py, py, ricetta, passi, env))
            ottenuta = oracle.clean(esegui(cmd_rs, rs, ricetta, passi, env))
        except Exception as exc:                       # pragma: no cover
            print(f'  ERRORE      {nome}: {exc}')
            divergenze += 1
            continue
        if attesa is None:
            continue                # il registro non lo conosce: lo dira' close()
        if attesa == ottenuta:
            parla = 'PARLA' if any(s.strip() for _, _, s in attesa[0]) else 'tace'
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
            solo_py = [x for x in attesa[1] if x not in ottenuta[1]]
            solo_rs = [x for x in ottenuta[1] if x not in attesa[1]]
            print(f'      albero solo python: {solo_py!r}')
            print(f'      albero solo rust:   {solo_rs!r}')
    print()
    if divergenze:
        print(f'{divergenze} divergenze su {len(elenco)} casi')
    else:
        print(f'{len(elenco)} casi, nessuna divergenza')
    return 1 if (divergenze or oracle.close()) else 0


if __name__ == '__main__':
    sys.exit(main())
