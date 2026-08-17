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
SOURCES = [os.path.expanduser(f'~/.claude/rust/crates/{p}') for p in (
    'guards/src/live_rules.rs', 'claude-hooks/src/live_rules.rs')]
REPOS = [os.path.expanduser(f'~/gyver/work/{r}')
         for r in ('suite', 'matching-engine', 'whatsapp', 'packages')]


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
    # Un file **vuoto**: zero byte è un file letto benissimo, e senza frontmatter
    # la regola si carica in ogni sessione. Confonderlo con «non ho potuto
    # leggere» lo fa passare in silenzio — è il difetto che una revisione
    # indipendente ha trovato nella prima stesura, e che qui nessun caso vedeva.
    ('rules/empty.md', ''),
    ('rules/only-newline.md', '\n'),
]

# I casi che non si scrivono come testo: byte non validi e collegamenti. Stanno
# a parte perché `build_fake_home` deve trattarli in modo diverso.
BUILT_BINARY = [
    # Non è UTF-8. L'originale non cattura `UnicodeDecodeError`, quindi esce zero
    # **lasciando una riga nel registro dei guasti**: un porting che lo inghiotte
    # in silenzio perde l'unica cosa che distingue un gancio rotto da uno contento.
    ('rules/not-utf8.md', b'---\npaths: ["src/**"]\n---\n\nvedi \xff\xfe qui\n'),
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
    for rel, blob in BUILT_BINARY:
        p = os.path.join(home, '.claude', rel)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, 'wb') as f:
            f.write(blob)
        paths.append(p)
    return home, paths


def build_linked_home(base):
    """Una seconda home finta in cui **`rules/` è un collegamento**.

    È la direzione che il primo porting sbagliava: non «il percorso passa per un
    link e si scioglie sulla stessa cartella», ma «la cartella di destinazione è
    essa stessa un link altrove». Lì un confronto letterale risponde «globale» e
    lo scioglimento risponde di no, e i due ganci prendono strade diverse.
    """
    home = os.path.join(base, 'linked')
    altrove = os.path.join(base, 'altrove')
    os.makedirs(os.path.join(home, '.claude'), exist_ok=True)
    os.makedirs(altrove, exist_ok=True)
    os.symlink(altrove, os.path.join(home, '.claude', 'rules'))
    paths = []
    for rel, text in (('scoped.md', '---\npaths: ["src/**"]\n---\n\n# x\n'),
                      ('dead-ref.md',
                       '---\npaths: ["src/**"]\n---\n\nvedi `~/.claude/via.md`\n'),
                      ('universal.md', '---\npaths: ["**"]\n---\n\n# x\n')):
        p = os.path.join(altrove, rel)
        with open(p, 'w') as f:
            f.write(text)
        # Sottoposto col percorso che **passa dal collegamento**, non con quello
        # sciolto: è così che arriva da un worktree.
        paths.append(os.path.join(home, '.claude', 'rules', rel))
    return home, paths


def build_worktree_view(base, home):
    """La forma vera delle copie di lavoro: `.claude` è un link alla home.

    Il percorso sottoposto è `<albero>/.claude/rules/x.md`, che **alla lettera**
    non è dentro la home e **sciolto** sì. È il solo caso in cui «confronta i
    percorsi» e «sciogli e poi confronta» danno risposte opposte, e senza di lui
    un binario che smettesse di sciogliere passerebbe il confronto — misurato:
    il mutante sopravviveva.
    """
    albero = os.path.join(base, 'albero')
    os.makedirs(albero, exist_ok=True)
    link = os.path.join(albero, '.claude')
    if not os.path.exists(link):
        os.symlink(os.path.join(home, '.claude'), link)
    rel = 'rules/via-link.md'
    target = os.path.join(home, '.claude', rel)
    with open(target, 'w') as f:
        f.write('---\npaths: ["src/**"]\n---\n\nvedi `~/.claude/sparito.md`\n')
    return [os.path.join(albero, '.claude', rel)]


def payload(path):
    return json.dumps({
        'session_id': 'compare',
        'tool_name': 'Edit',
        'tool_input': {'file_path': path},
    })


def failures_log(home):
    """Il registro dei guasti dentro la home data, o '' se non c'è."""
    p = os.path.join(home or HOME, '.claude/state/regole-vive-guasti.log')
    try:
        with open(p) as f:
            return f.read()
    except OSError:
        return ''


def run(cmd, data, home=None):
    """Esegue e restituisce (uscita, stdout, stderr, righe scritte nel registro).

    IL REGISTRO FA PARTE DELLA RISPOSTA. Un gancio che muore su un file illeggibile
    esce zero e non stampa niente: guardando solo uscita e stderr, «tace perché va
    tutto bene» e «tace perché è morto» sono lo stesso caso — che è precisamente
    ciò che l'originale dichiara di voler distinguere. Senza questo, il caso
    non-UTF-8 passa il confronto pur essendo trattato in modo opposto.
    """
    env = dict(os.environ)
    if home:
        env['HOME'] = home
    before = failures_log(home)
    p = subprocess.run(cmd, input=data, capture_output=True, text=True,
                       timeout=120, env=env)
    after = failures_log(home)
    # Solo la coda nuova, e senza la data: cambia a ogni esecuzione.
    grown = after[len(before):] if after.startswith(before) else after
    logged = '\n'.join(l for l in grown.splitlines() if not l.startswith('--- 2'))
    return p.returncode, p.stdout.strip(), p.stderr.strip(), logged.strip()


def main():
    limit = int(sys.argv[1]) if len(sys.argv) > 1 else 0
    files = [(p, None) for p in corpus(limit)]
    if not os.path.exists(BINARY):
        print('binario assente: cargo build --release')
        return 1

    # «Esiste» non è «è quello che hai appena scritto». Senza questo controllo un
    # confronto lanciato dopo una modifica non ricompilata dà «0 differenze» sul
    # codice di ieri — ed è la stessa forma dell'incidente del 17/08/2026, quando
    # lo strumento dei mutanti lasciò in produzione un binario mutato.
    stale = [s for s in SOURCES
             if os.path.exists(s) and os.path.getmtime(s) > os.path.getmtime(BINARY)]
    if stale:
        print('il binario e\' piu\' vecchio dei sorgenti: ricompila prima di leggere '
              'questo confronto\n  ' + '\n  '.join(stale))
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
    linked_home, linked = build_linked_home(tmp)
    files += [(p, linked_home) for p in linked]
    built += linked
    via_link = build_worktree_view(tmp, fake_home)
    files += [(p, fake_home) for p in via_link]
    built += via_link

    diffs, checked, spoke = [], 0, 0
    for path, home in files:
        data = payload(path)
        py = run(['python3', HOOK], data, home)
        rs = run([BINARY, 'live-rules'], data, home)
        checked += 1
        if py[0] != 0:
            spoke += 1
        # Si confronta la decisione, il testo E il registro: il messaggio
        # finisce nel contesto del modello, quindi una parola diversa è una
        # differenza vera; e il registro è l'unico posto in cui si vede la
        # differenza fra un gancio che tace contento e uno che è morto.
        if py[0] != rs[0] or py[2] != rs[2] or bool(py[3]) != bool(rs[3]):
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
