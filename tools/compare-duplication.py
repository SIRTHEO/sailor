#!/usr/bin/env python3
"""Confronta il rilevatore di copie in Rust con l'originale in Python.

IL CORPUS SONO I FILE VERI, e qui piu' che altrove: questo gancio non giudica
una stringa ma un file **contro tutti i suoi fratelli**, quindi l'unico banco
onesto e' un albero di codice vero, con le sue famiglie e i suoi punti di riuso.
Un caso costruito ha due file e nessuna somiglianza accidentale.

LO STATO E' FINTO E AZZERATO FRA LE DUE ESECUZIONI. `HOME` punta a una cartella
temporanea: la linea di base congelata e il registro degli avvisi vivono la',
non sul disco vero. Il registro va anche **cancellato fra Python e Rust**, o il
primo che gira segna il file e il secondo tace — una divergenza che sarebbe
tutta dello strumento di misura.

    python3 tools/compare-duplication.py [quanti]
"""
import glob
import json
import os
import shutil
import subprocess
import sys
import tempfile

HOOK = os.path.expanduser('~/.claude/skills/hooks/duplication.py')
BINARY = os.path.expanduser('~/.claude/rust/target/release/claude-hooks')
ROOTS = [os.path.expanduser(f'~/gyver/work/{r}')
         for r in ('suite', 'matching-engine', 'whatsapp', 'packages')]


def corpus(limit):
    """File di codice veri, campionati a passo costante su tutti i repo."""
    found = []
    for root in ROOTS:
        for pattern in ('src/**/*.ts', 'src/**/*.tsx', 'src/**/*.py',
                        'src/**/*.mjs', 'src/**/*.js'):
            for path in glob.glob(os.path.join(root, pattern), recursive=True):
                if any(x in path for x in ('node_modules', '/dist/', '/build/')):
                    continue
                if os.path.isfile(path) and os.path.getsize(path) < 200_000:
                    found.append(path)
    found.sort()
    if len(found) <= limit:
        return found
    step = len(found) / limit
    return [found[int(i * step)] for i in range(limit)]


def run_one(path, tool, phase, home):
    """Esegue Python e Rust sullo stesso file, ognuno con lo stato azzerato."""
    payload = json.dumps({'tool_name': tool, 'session_id': 'confronto',
                          'cwd': os.path.dirname(path),
                          'tool_input': {'file_path': path, 'content': ''}})
    state = os.path.join(home, '.claude', 'state', 'duplicazione')
    # Si azzera **solo** il registro degli avvisi, non la cartella: li' dentro
    # vive anche la linea di base congelata, e cancellarla faceva parlare
    # entrambe le implementazioni sul caso che doveva provare il silenzio. E' la
    # quarta volta in questa migrazione che lo strumento di misura distrugge la
    # condizione che vuole misurare.
    announced = os.path.join(state, 'annunciati.txt')
    out = []
    for argv in ([sys.executable, HOOK, phase], [BINARY, 'duplication', phase]):
        os.makedirs(state, exist_ok=True)
        if os.path.exists(announced):
            os.remove(announced)
        env = dict(os.environ, HOME=home)
        r = subprocess.run(argv, input=payload, capture_output=True,
                           text=True, env=env)
        out.append({'uscita': r.returncode, 'stderr': r.stderr.strip(),
                    'stdout': r.stdout.strip()})
    return out


def frozen_case(home):
    """Il caso della linea di base: due gemelli, congelati, che devono tacere.

    E' l'invariante che tiene acceso il gancio — «non accusa del debito altrui» —
    e nel corpus vero non si esercita mai, perche' nella HOME finta non c'e'
    niente di congelato. Senza questo caso, un rilevatore che ignorasse del tutto
    la linea di base passerebbe il confronto. Copre anche `pair_signature`: se le
    impronte delle due implementazioni divergessero, il congelato non varrebbe
    per una delle due.

    Ritorna [(percorso, atteso_prima, atteso_dopo)] gia' pronti da confrontare.
    """
    tree = tempfile.mkdtemp(prefix='congelato-')
    os.makedirs(os.path.join(tree, '.git'), exist_ok=True)
    src = os.path.join(tree, 'src')
    os.makedirs(src, exist_ok=True)
    body = '\n'.join(f'  const valoreNumero{i} = calcolaQualcosa({i}, true)'
                     for i in range(12))
    for name in ('alfa-columns.ts', 'beta-columns.ts'):
        with open(os.path.join(src, name), 'w') as f:
            f.write(f'function {name[:4]}() {{\n{body}\n}}\n')
    # Congela col Python, che e' l'oracolo anche per il formato del file.
    subprocess.run([sys.executable, HOOK, '--congela', src],
                   capture_output=True, text=True,
                   env=dict(os.environ, HOME=home))
    return os.path.join(src, 'beta-columns.ts')


def main():
    limit = int(sys.argv[1]) if len(sys.argv) > 1 else 250
    if not os.path.exists(BINARY):
        sys.exit(f'manca il binario: {BINARY}\n  cargo build --release')

    files = corpus(limit)
    if not files:
        sys.exit('nessun file nel corpus: i quattro repo non sono dove mi aspetto')

    home = tempfile.mkdtemp(prefix='confronto-duplicazione-')

    # Prima il caso del congelato: due gemelli che, dopo `--congela`, devono
    # tacere in **entrambe** le implementazioni. Se una delle due parla, o non
    # legge la linea di base o calcola impronte diverse.
    divergent, flagged = [], 0
    frozen = frozen_case(home)
    python, rust = run_one(frozen, 'Edit', 'post', home)
    if python['uscita'] != 0:
        print('ATTENZIONE: il congelato non ha zittito il Python, il caso non prova niente')
    if python != rust:
        divergent.append((frozen, 'post-congelato', python, rust))

    for path in files:
        # `post` su Edit e' il caso caldo: il file esiste, si misura cosa e'
        # uscito. `pre` su Write vale solo per un file che non esiste ancora,
        # quindi si prova su un nome nuovo nella stessa cartella.
        for tool, phase, target in (
            ('Edit', 'post', path),
            ('Write', 'pre', os.path.join(os.path.dirname(path),
                                          'mai-scritto-davvero-columns.ts')),
        ):
            python, rust = run_one(target, tool, phase, home)
            if python['uscita'] == 2:
                flagged += 1
            if python != rust:
                divergent.append((target, phase, python, rust))

    shutil.rmtree(home, ignore_errors=True)

    print(f'{len(files)} file veri, {len(files) * 2} casi confrontati')
    print(f'segnalati dal Python: {flagged}')
    print(f'divergenze: {len(divergent)}')
    for path, phase, python, rust in divergent[:8]:
        print(f'\n── {phase} {path}')
        for key in ('uscita', 'stderr', 'stdout'):
            if python[key] != rust[key]:
                print(f'   python: {str(python[key])[:500]}')
                print(f'   rust:   {str(rust[key])[:500]}')
    if len(divergent) > 8:
        print(f'\n… e altre {len(divergent) - 8}')
    return 1 if divergent else 0


if __name__ == '__main__':
    sys.exit(main())
