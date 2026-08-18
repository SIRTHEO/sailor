#!/usr/bin/env python3
"""Confronta il freno sui rimandi in Rust con l'originale in Python.

IL CORPUS SONO I FILE VERI. Le prove interne dei due porti sono casi costruiti a
mano; qui si prendono i sorgenti dei quattro repo e si sottopongono come se
fossero appena stati scritti, col loro percorso vero — così l'esenzione e
l'esistenza del file guardano lo stesso disco da entrambe le parti.

Serve testo vero perché le due implementazioni tagliano i commenti con due
macchine diverse: la Python è a stati, quella Rust pure, ma le stringhe con un
`//` dentro, i blocchi `/* */` annidati e gli apostrofi accentati non compaiono
in nessun caso costruito. Su qualche migliaio di file, se divergono, si vede.

Il denominatore contiene anche ciò che deve restare in silenzio: i file `.md` e
i sorgenti senza rimandi. Senza di loro un porto che negasse tutto passerebbe il
confronto — è il punto cieco che `tools/mutants.sh` trova sugli altri gemelli.

LE PRIME 32 DIVERGENZE ERANO TUTTE DELL'ORIGINALE, e sono corrette. Alla prima
passata — 120 file veri, 810 casi — il Rust aveva ragione in tutte e 32:

  28 casi (MultiEdit, in tutte e due le fasi): il Python leggeva `new_string` e
      `content` e non guardava `edits`, quindi su una scrittura multipla restava
      muto mentre le righe che vieta venivano scritte davvero;
   4 casi (Write ed Edit su due file): il Python non chiudeva il blocco `/* */`
      — la chiusura spostava il cursore ma non rimetteva lo stato a «codice» — e
      da lì in poi trattava il file intero come commento. Su
      `calendar-validation.test.ts` accusava la riga 10, che è la stringa di un
      `describe(...)`, non un commento.

Nessuna delle 26 prove interne dell'originale copriva i due difetti: passavano
tutte. Ora sono 28, e ognuna delle due nuove muore col proprio mutante.

Con i due difetti corretti la passata larga — 600 file, 3.690 casi, 264 dei
quali il freno segnala — chiude a **zero divergenze**.

Che lo zero non sia cecità è provato rompendo il Rust: togliendo `in_block =
false` dal porto — cioè rimettendogli il difetto del Python — il confronto passa
da 0 a 6 divergenze.

    python3 tools/compare-comment-refs.py [quanti]
"""
import glob
import json
import os
import shutil
import subprocess
import sys
import tempfile

HOOK = os.path.expanduser('~/.claude/skills/hooks/comment-refs.py')
BINARY = os.path.expanduser('~/.claude/rust/target/release/claude-hooks')
ROOTS = [os.path.expanduser(f'~/other-repo/work/{r}')
         for r in ('suite', 'a-service', 'a-client', 'packages')]
ROOTS.append(os.path.expanduser('~/.claude'))


def corpus(limit):
    """I file da sottoporre: le estensioni guardate e alcune che non lo sono."""
    found = []
    patterns = ('src/**/*.ts', 'src/**/*.tsx', 'src/**/*.js',
                'scripts/**/*.py', 'scripts/**/*.sh', 'skills/hooks/*.py',
                'prisma/*.prisma', 'src/**/*.sql',
                # fuori perimetro, e devono restare tali
                'docs/**/*.md', 'rules/*.md', '.claude/rules/*.md')
    for root in ROOTS:
        for pattern in patterns:
            for path in glob.glob(os.path.join(root, pattern), recursive=True):
                if 'node_modules' in path or '/dist/' in path:
                    continue
                if os.path.isfile(path) and os.path.getsize(path) < 400_000:
                    found.append(path)
    found.sort()
    # Campionato a passo costante, non troncato: in ordine alfabetico un taglio
    # in testa prova quasi solo il primo repo e quasi solo una famiglia di file.
    if len(found) <= limit:
        return found
    step = len(found) / limit
    return [found[int(i * step)] for i in range(limit)]


def synthetic(tmp):
    """I casi che il codice vero non esercita mai, scritti apposta.

    Metà sono rimandi che vanno negati, metà sono i falsi allarmi misurati il
    17/08/2026 che devono restare muti. Senza la seconda metà un porto che
    negasse ogni commento passerebbe il confronto.
    """
    cases = [
        # i quattro rimandi che si negano
        ('src/adr.ts', '// ADR 0008 #61: il downgrade e eliminato\nconst a = 1;\n'),
        ('src/adr-dash.ts', '// vedi ADR-025 per il perche\nconst b = 2;\n'),
        ('src/plans.ts', '// il resto sta in Plans.md, fase 3\nconst c = 3;\n'),
        ('scripts/claude-dir.py', '# come da .claude/docs/analisi.md\nx = 1\n'),
        # i tre falsi allarmi del 17/08, che devono restare muti
        ('src/issue-number.ts', '// Phase 3 #14 della specifica\nconst d = 4;\n'),
        ('src/hex.ts', '// il colore di sfondo e #3d5ccc, non cambiarlo\nconst e = 5;\n'),
        ('src/readme-url.ts',
         '// tabella dei codici: README su https://esempio.it/pkg\nconst f = 6;\n'),
        ('src/code-path.ts',
         '// rispecchia il contratto di src/api/schema.ts nell altro repo\nconst g = 7;\n'),
        # una stringa non e un commento, ed e il caso in cui le due macchine a
        # stati possono divergere davvero
        ('src/string.ts', 'const h = "vedi ADR 0008 nel messaggio";\n'),
        ('src/string-slash.ts',
         'const i = "http://x/y // ADR 0008";\nconst j = 1;\n'),
        # commento a blocchi, l altro ramo
        ('src/block.ts', '/* ADR 0008 #61: rimosso */\nconst k = 8;\n'),
        # il cancelletto non e un commento in TypeScript
        ('src/hash-in-ts.ts', 'const l = { "#": "Plans.md" };\n'),
        # un file che dichiara di essere il banco di prova e esente
        ('src/banco.ts',
         '// comment-refs: banco di prova\n// ADR 0008 #61 citato apposta\n'),
        # fuori perimetro per estensione e per cartella
        ('docs/nota.md', 'vedi ADR 0008 e Plans.md\n'),
        ('src/generated/client.ts', '// ADR 0008 #61 generato\nconst m = 9;\n'),
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


EVENT = {'pre': 'PreToolUse', 'post': 'PostToolUse'}


def compare(tool, tool_input, phase, with_arg=True):
    """Le due decisioni sullo stesso ingresso.

    `with_arg=False` toglie l'argomento e lascia solo `hook_event_name`: e' la
    forma in cui arriva un gancio registrato senza ricopiare la fase in
    `settings.json`, ed e' il modo in cui un porto puo' divergere in silenzio
    dall'altro senza che nessun file di prova se ne accorga.
    """
    payload = json.dumps({'tool_name': tool, 'session_id': 'confronto',
                          'cwd': '/tmp', 'hook_event_name': EVENT[phase],
                          'tool_input': tool_input})
    args = [phase] if with_arg else []
    return (run([sys.executable, HOOK] + args, payload),
            run([BINARY, 'comment-refs'] + args, payload))


def main():
    limit = int(sys.argv[1]) if len(sys.argv) > 1 else 1200
    if not os.path.exists(BINARY):
        sys.exit(f'manca il binario: {BINARY}\n  cargo build --release')

    files = corpus(limit)
    if not files:
        sys.exit('nessun file nel corpus: i quattro repo non sono dove mi aspetto')

    tmp = tempfile.mkdtemp(prefix='confronto-rimandi-')
    built = synthetic(tmp)
    divergent = []
    flagged = 0
    for path in files + [p for p, _ in built]:
        try:
            with open(path, encoding='utf-8', errors='replace') as f:
                content = f.read()
        except OSError:
            continue
        # Write consegna tutto il contenuto, Edit solo il pezzo nuovo, MultiEdit
        # una lista: leggono campi diversi dell ingresso, e il terzo e quello che
        # i due porti trattano in modo diverso.
        forms = (
            ('Write', {'file_path': path, 'content': content}),
            ('Edit', {'file_path': path, 'old_string': '', 'new_string': content}),
            ('MultiEdit', {'file_path': path,
                           'edits': [{'old_string': '', 'new_string': content}]}),
        )
        for tool, tool_input in forms:
            # Le due fasi non sono lo stesso freno: in `pre` la negazione viaggia
            # dentro lo stdout con uscita 0, in `post` e un blocco con uscita 2.
            # Un porto che sbagliasse solo la seconda passerebbe meta confronto.
            for phase in ('pre', 'post'):
                for with_arg in (True, False):
                    python, rust = compare(tool, tool_input, phase, with_arg)
                    if with_arg and (python['stdout'] or python['uscita'] == 2):
                        flagged += 1
                    if python != rust:
                        label = phase if with_arg else f'{phase} (dall evento)'
                        divergent.append((path, tool, label, python, rust))

    shutil.rmtree(tmp, ignore_errors=True)

    total = (len(files) + len(built)) * 12
    print(f'{len(files)} file veri + {len(built)} costruiti, {total} casi confrontati')
    print(f'segnalati dal Python: {flagged}')
    print(f'divergenze: {len(divergent)}')
    # Il riepilogo per forma prima degli esempi: dieci righe di dettaglio non
    # dicono se le divergenze sono una classe sola o quattro, e la differenza
    # cambia la diagnosi.
    per_form = {}
    for _, tool, phase, _, _ in divergent:
        per_form[(tool, phase)] = per_form.get((tool, phase), 0) + 1
    for (tool, phase), n in sorted(per_form.items(), key=lambda kv: -kv[1]):
        print(f'  {tool} {phase}: {n}')
    for path, tool, phase, python, rust in divergent[:10]:
        print(f'\n── {tool} {phase} {path}')
        for key in ('uscita', 'stdout', 'stderr'):
            if python[key] != rust[key]:
                print(f'   python: {str(python[key])[:300]}')
                print(f'   rust:   {str(rust[key])[:300]}')
    if len(divergent) > 10:
        print(f'\n… e altre {len(divergent) - 10}')
    return 1 if divergent else 0


if __name__ == '__main__':
    sys.exit(main())
