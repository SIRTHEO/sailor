#!/usr/bin/env python3
"""Confronta il gancio sulle regole in Rust con l'originale in Python.

IL CORPUS SONO LE REGOLE VERE, e sono di due specie che il gancio tratta in
modo opposto: le globali sotto `~/.claude/rules/` non passano dal verificatore
Node, quelle di repo sì. Un confronto che ne guardasse una sola specie
lascerebbe scoperta metà del gancio — ed è proprio la metà con dentro il
sottoprocesso, cioè quella dove le due implementazioni divergono più facilmente.

Sottoposte come se fossero appena state scritte, col loro percorso vero: così la
`stat` sui percorsi citati, la carta dei repo e l'esistenza del file guardano lo
stesso disco da entrambe le parti.

Dentro il corpus stanno anche file **fuori perimetro** — documenti, script,
regole di altri progetti. Senza di loro un mutante che allargasse il filtro
(«qualunque .md», invece di `.claude/rules/*.md`) passerebbe il confronto senza
che nessun caso se ne accorga: il denominatore deve contenere anche ciò che deve
restare in silenzio.

I FILE VERI NON BASTANO, e lo ha detto il primo giro di mutanti: 4 su 7
sopravvissuti, tutti perché **il caso non esiste sul disco**. Oggi nessuna regola
ha `paths: ["**"]`, nessuna ha un `# paths:` commentato nel frontmatter, e
nessuna regola globale ha un glob morto — quindi un binario che smettesse di
riconoscerli passerebbe il confronto a pieni voti. Un corpus fatto solo di
com'è-adesso misura la giornata, non il gancio.

Per questo la seconda metà del corpus è **costruita**, in una home finta: i casi
che il gancio deve trattare ma che nessuno ha scritto ancora. La home finta serve
perché «regola globale» vuol dire `$HOME/.claude/rules/`, e le due
implementazioni la ricavano entrambe dall'ambiente (`Path.home()` di là,
`env::var("HOME")` di qua) — scrivere una regola finta in quella vera la
caricherebbe in ogni sessione.

    python3 tools/compare-live-rules.py [quanti]
"""
import glob
import json
import os
import subprocess
import sys
import shutil
import tempfile

HOOK = os.path.expanduser('~/.claude/skills/hooks/live-rules.py')
BINARY = os.path.expanduser('~/.claude/rust/target/release/claude-hooks')
HOME = os.path.expanduser('~')
REPOS = [os.path.expanduser(f'~/other-repo/work/{r}')
         for r in ('suite', 'a-service', 'a-client', 'packages')]


def corpus(limit):
    """I file da sottoporre: regole globali, regole di repo, e fuori perimetro."""
    found = []
    found += sorted(glob.glob(os.path.join(HOME, '.claude/rules/*.md')))
    for repo in REPOS:
        found += sorted(glob.glob(os.path.join(repo, '.claude/rules/**/*.md'),
                                  recursive=True))
    # Le copie di lavoro Orca: stessa forma, due livelli sotto `workspaces`.
    found += sorted(glob.glob(os.path.expanduser(
        '~/orca/workspaces/*/*/.claude/rules/*.md')))
    # Fuori perimetro, e devono restare tali.
    found += sorted(glob.glob(os.path.join(HOME, '.claude/docs/*.md')))[:20]
    found += sorted(glob.glob(os.path.join(HOME, '.claude/skills/*/SKILL.md')))[:20]
    found += [os.path.join(HOME, '.claude/CLAUDE.md'),
              os.path.join(HOME, '.claude/rules/non-esiste.md'),
              '/etc/hosts']

    found = [p for p in found if 'node_modules' not in p]
    # Campionato a passo costante, non troncato: l'ordine mette in testa tutte
    # le globali, e tagliare in testa vorrebbe dire provare una specie sola.
    if limit and len(found) > limit:
        step = len(found) / limit
        found = [found[int(i * step)] for i in range(limit)]
    return found


# I casi che il gancio deve trattare e che sul disco non ci sono. Ognuno porta
# scritto perché esiste: il nome è il mutante che senza di lui sopravvive.
BUILT = [
    # perimetro: le due forme di «si carica sempre»
    ('rules/universal.md', '---\npaths: ["**"]\n---\n\n# x\n'),
    ('rules/spaced.md', '---\npaths: [ "**" ]\n---\n\n# x\n'),
    ('rules/no-frontmatter.md', '# x\n\nbody\n'),
    ('rules/no-paths.md', '---\ndescription: x\n---\n\n# x\n'),
    ('rules/commented-paths.md',
     '---\n# paths: ["src/**"]\ndescription: x\n---\n\n# x\n'),
    ('rules/comment-above-paths.md',
     '---\n# perché è stretto\npaths: ["src/**"]\n---\n\n# x\n'),
    # Il caso che distingue davvero «commenti tolti» da «commenti tenuti»: uno
    # scope largo spiegato in un commento e poi ristretto sotto. Senza togliere
    # i commenti, il `["**"]` commentato viene letto come configurazione e la
    # regola viene rimproverata per uno scope che non ha.
    ('rules/commented-wide-then-narrow.md',
     '---\n# prima era paths: ["**"]\npaths: ["src/**"]\n---\n\n# x\n'),
    # riferimenti di una regola globale: glob vivi, glob morti, jolly in testa
    ('rules/dead-glob.md', '---\npaths: ["srcs/**"]\n---\n\n# x\n'),
    ('rules/live-glob.md', '---\npaths: ["src/**"]\n---\n\n# x\n'),
    ('rules/wildcard-head.md', '---\npaths: ["**/.claude/**"]\n---\n\n# x\n'),
    ('rules/mixed-globs.md',
     '---\npaths: ["src/**", "nonesiste/**", "scripts/**"]\n---\n\n# x\n'),
    # percorsi citati: uno vivo, uno sparito, una cartella, una riga, un'ancora
    ('rules/dead-path.md',
     '---\npaths: ["src/**"]\n---\n\nvedi `~/.claude/sparito.md` e `~/.claude/c-e.md`,\n'
     'la cartella `~/.claude/rules/`, la riga `~/.claude/sparito.md:12`\n'
     'e l\'ancora `~/.claude/sparito.md#sezione`\n'),
    ('rules/absolute-path.md',
     '---\npaths: ["src/**"]\n---\n\nvedi `/Users/nessuno/x.ts`\n'),
    # Una cartella citata e **inesistente**: senza la pretesa dell'estensione
    # corta diventerebbe un reperto, e ogni regola che nomina una cartella si
    # prenderebbe un rimprovero. Quella dell'altro caso esiste, quindi non
    # distingue niente — è la differenza fra un caso e un caso che prova.
    ('rules/dead-directory.md',
     '---\npaths: ["src/**"]\n---\n\nvedi la cartella `~/.claude/mai-esistita/`\n'),
    # fuori perimetro: stesso testo, posto sbagliato — devono tacere entrambi
    ('docs/universal.md', '---\npaths: ["**"]\n---\n\n# x\n'),
    ('rules/not-markdown.txt', '---\npaths: ["**"]\n---\n\n# x\n'),
]


def build_fake_home(base):
    """Scrive i casi costruiti in una home finta e restituisce (home, percorsi).

    `c-e.md` esiste e `sparito.md` no: senza un percorso vivo accanto a uno
    morto, un binario che dichiarasse morto tutto passerebbe lo stesso.
    """
    home = os.path.join(base, 'home')
    os.makedirs(os.path.join(home, '.claude', 'rules'), exist_ok=True)
    os.makedirs(os.path.join(home, '.claude', 'docs'), exist_ok=True)
    with open(os.path.join(home, '.claude', 'c-e.md'), 'w') as f:
        f.write('esisto\n')
    paths = []
    for rel, text in BUILT:
        p = os.path.join(home, '.claude', rel)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, 'w') as f:
            f.write(text)
        paths.append(p)
    return home, paths


def payload(path):
    return json.dumps({
        'session_id': 'compare',
        'tool_name': 'Edit',
        'tool_input': {'file_path': path},
    })


def run(cmd, data, home=None):
    env = dict(os.environ)
    if home:
        env['HOME'] = home
    p = subprocess.run(cmd, input=data, capture_output=True, text=True,
                       timeout=120, env=env)
    return p.returncode, p.stdout.strip(), p.stderr.strip()


def main():
    limit = int(sys.argv[1]) if len(sys.argv) > 1 else 0
    files = [(p, None) for p in corpus(limit)]
    if not os.path.exists(BINARY):
        print('binario assente: cargo build --release')
        return 1

    # `realpath` sulla radice temporanea, e non è un dettaglio: su macOS
    # `/var` è un collegamento a `/private/var`, e sotto un percorso così
    # l'originale Python **non riconosce come globale** una regola che lo è —
    # confronta `Path(path).resolve()` (sciolto) con `Path.home()` (non
    # sciolto), e i due non combaciano mai. Il Rust scioglie entrambi i lati e
    # risponde giusto. La differenza è vera ma non riguarda il porting: qui la
    # radice si prende già canonica, così il banco misura il gancio e non il
    # collegamento sotto la cartella temporanea.
    tmp = tempfile.mkdtemp(prefix='live-rules-confronto-',
                           dir=os.path.realpath(tempfile.gettempdir()))
    fake_home, built = build_fake_home(tmp)
    files += [(p, fake_home) for p in built]

    diffs, checked, spoke = [], 0, 0
    for path, home in files:
        data = payload(path)
        py = run(['python3', HOOK], data, home)
        rs = run([BINARY, 'live-rules'], data, home)
        checked += 1
        if py[0] != 0:
            spoke += 1
        # Si confronta la decisione E il testo: il messaggio finisce nel
        # contesto del modello, quindi una parola diversa è una differenza vera,
        # non cosmetica.
        if py[0] != rs[0] or py[2] != rs[2]:
            diffs.append((path, py, rs))

    shutil.rmtree(tmp, ignore_errors=True)
    print(f'{checked} casi ({len(built)} costruiti), '
          f'{spoke} su cui l\'originale parla, {len(diffs)} differenze')
    for path, py, rs in diffs[:10]:
        print(f'\n--- {path}')
        print(f'  python: exit={py[0]}\n    {py[2][:400]}')
        print(f'  rust:   exit={rs[0]}\n    {rs[2][:400]}')
    if spoke == 0:
        print('\nATTENZIONE: nessun caso in cui il gancio parla. Un confronto '
              'fatto solo di silenzi non distingue due implementazioni: '
              'passerebbe anche un binario che esce sempre zero.')
        return 1
    return 1 if diffs else 0


if __name__ == '__main__':
    sys.exit(main())
