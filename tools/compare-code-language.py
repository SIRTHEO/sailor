#!/usr/bin/env python3
"""Confronta il gancio sulla lingua in Rust con l'originale in Python.

IL CORPUS SONO I FILE VERI. Le prove interne dell'originale sono una dozzina di
casi costruiti; qui si prendono i file di test e di gate dei quattro repo e si
sottopongono come se fossero appena stati scritti, col loro percorso vero — così
`is_exempt` e l'esistenza del file guardano lo stesso disco da entrambe le
parti. È l'unico modo per far emergere le differenze che nascono su testo vero:
apostrofi, virgolette annidate, interpolazioni, commenti a blocchi, accenti.

Le regex dell'originale chiudono la stringa con una backreference, che il motore
di Rust non ha e che qui è diventata tre alternative esplicite. Su un corpus
costruito la differenza non si vedrebbe mai; su qualche migliaio di file veri,
se c'è, si vede.

    python3 tools/compare-code-language.py [quanti]
"""
import glob
import json
import os
import shutil
import subprocess
import sys
import tempfile

HOOK = os.path.expanduser('~/.claude/skills/hooks/code-language.py')
BINARY = os.path.expanduser('~/.claude/rust/target/release/claude-hooks')
ROOTS = [os.path.expanduser(f'~/other-repo/work/{r}')
         for r in ('suite', 'a-service', 'a-client', 'packages')]
ROOTS.append(os.path.expanduser('~/.claude'))


def corpus(limit):
    """I file da sottoporre: dentro e **fuori** dal perimetro.

    I sorgenti normali servono quanto i test: la regola li lascia in italiano di
    proposito, e senza di loro un controllo che allargasse il perimetro a ogni
    file passerebbe il confronto senza che nessun caso se ne accorga — trovato
    da `tools/mutants.sh`, non a ragionamento. Il denominatore di un confronto
    deve contenere anche ciò che deve restare in silenzio.
    """
    found = []
    patterns = ('**/*.test.ts', '**/*.test.tsx', '**/*.spec.ts', '**/*.test.js',
                'scripts/**/*.sh', 'scripts/**/*.py', 'scripts/**/*.mjs',
                'skills/hooks/*.py', '.github/workflows/*.yml',
                # fuori perimetro, e devono restare tali
                'src/lib/**/*.ts', 'src/components/**/*.tsx', 'rules/*.md')
    for root in ROOTS:
        for pattern in patterns:
            for path in glob.glob(os.path.join(root, pattern), recursive=True):
                if 'node_modules' in path or '/dist/' in path:
                    continue
                if os.path.isfile(path) and os.path.getsize(path) < 400_000:
                    found.append(path)
    found.sort()
    # Campionato a passo costante, non troncato: l'ordine è alfabetico, quindi
    # tagliare in testa vuol dire provare quasi solo il primo repo e quasi solo
    # una famiglia di file. Con il taglio in testa un mutante che allargava la
    # soglia di lunghezza sopravviveva, e sembrava un punto cieco della batteria
    # mentre era il campione a essere sbilanciato.
    if len(found) <= limit:
        return found
    step = len(found) / limit
    return [found[int(i * step)] for i in range(limit)]


def synthetic(tmp):
    """I casi che il codice vero non esercita mai, scritti apposta.

    Il corpus dei file veri copre l'uso reale; queste coprono le invarianti che
    l'uso reale non tocca — e senza di loro un controllo che smettesse di
    togliere i commenti passerebbe il confronto senza che nessun caso se ne
    accorga. Trovato da `tools/mutants.sh`, non a ragionamento.

    I file vengono scritti davvero: `is_exempt` e l'esistenza del percorso li
    guardano da entrambe le parti, e su un percorso finto le due implementazioni
    guarderebbero cose diverse.
    """
    cases = [
        # un commento che cita una chiamata non è la chiamata
        ('scripts/citazione.sh',
         '#!/bin/bash\n# qui si usa echo "il file non esiste ancora" per dirlo\ntrue\n'),
        ('scripts/citazione.py',
         '# print("questa riga resta in italiano e non si tocca")\nx = 1\n'),
        # una dichiarazione italiana dentro un commento non è una dichiarazione
        ('scripts/dichiarata.py',
         '# def raccogli_le_schede():\n#     pass\nvalue = 1\n'),
        ('a-commento.test.ts',
         "// const chiudiScheda = () => {}\nit('works fine', () => {})\n"),
        # commento a blocchi, che è l'altro ramo della regex
        ('b-blocco.test.ts',
         "/* it('rifiuta le date future', () => {}) */\nit('works fine', () => {})\n"),
        # e i casi veri, che devono continuare a essere segnalati
        ('c-vero.test.ts',
         "it('rifiuta le date future quando manca il campo', () => {})\n"),
        ('scripts/vero.sh',
         '#!/bin/bash\necho "il file non esiste ancora, controlla il percorso"\n'),
    ]
    out = []
    for name, content in cases:
        path = os.path.join(tmp, name)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, 'w', encoding='utf-8') as f:
            f.write(content)
        out.append((path, content))
    return out


def run(argv, payload):
    r = subprocess.run(argv, input=payload, capture_output=True, text=True)
    return {'uscita': r.returncode, 'stdout': r.stdout.strip(),
            'stderr': r.stderr.strip()}


def compare(tool, tool_input):
    payload = json.dumps({'tool_name': tool, 'session_id': 'confronto',
                          'cwd': '/tmp', 'tool_input': tool_input})
    return (run([sys.executable, HOOK, 'pre'], payload),
            run([BINARY, 'code-language', 'pre'], payload))


def main():
    limit = int(sys.argv[1]) if len(sys.argv) > 1 else 1200
    if not os.path.exists(BINARY):
        sys.exit(f'manca il binario: {BINARY}\n  cargo build --release')

    files = corpus(limit)
    if not files:
        sys.exit('nessun file nel corpus: i quattro repo non sono dove mi aspetto')

    tmp = tempfile.mkdtemp(prefix='confronto-lingua-')
    built = synthetic(tmp)
    divergent = []
    flagged = 0
    for path in files + [p for p, _ in built]:
        try:
            with open(path, encoding='utf-8', errors='replace') as f:
                content = f.read()
        except OSError:
            continue
        # Write consegna tutto il contenuto; Edit solo il pezzo nuovo. Si provano
        # entrambe le forme, perché leggono campi diversi dell'ingresso.
        for tool, tool_input in (
            ('Write', {'file_path': path, 'content': content}),
            ('Edit', {'file_path': path, 'old_string': '', 'new_string': content}),
        ):
            python, rust = compare(tool, tool_input)
            if python['stdout'] or python['uscita'] == 2:
                flagged += 1
            if python != rust:
                divergent.append((path, tool, python, rust))

    shutil.rmtree(tmp, ignore_errors=True)

    print(f'{len(files)} file veri + {len(built)} costruiti, '
          f'{(len(files) + len(built)) * 2} casi confrontati')
    print(f'segnalati dal Python: {flagged}')
    print(f'divergenze: {len(divergent)}')
    for path, tool, python, rust in divergent[:10]:
        print(f'\n── {tool} {path}')
        for key in ('uscita', 'stdout', 'stderr'):
            if python[key] != rust[key]:
                print(f'   python: {str(python[key])[:300]}')
                print(f'   rust:   {str(rust[key])[:300]}')
    if len(divergent) > 10:
        print(f'\n… e altre {len(divergent) - 10}')
    return 1 if divergent else 0


if __name__ == '__main__':
    sys.exit(main())
