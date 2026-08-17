#!/usr/bin/env python3
"""Confronta il gancio che nomina la competenza già esistente col suo porto in Rust.

L'ORACOLO È IL PYTHON. Questo gancio non concede e non blocca niente: quello che
può divergere è **quando parla**, **cosa dice** e **cosa lascia scritto**. Si
confrontano quindi, byte per byte:

    l'uscita        deve essere 0 sempre — un UserPromptSubmit che esce diverso
                    da zero blocca il turno, ed è il difetto che l'originale ha
                    già avuto tre volte (ambiente storto, stdout non UTF-8, pipa
                    rotta)
    stdout          la riga aggiunta al contesto, parola per parola
    stderr          deve restare vuoto in entrambi
    ogni file       tutto l'albero sotto la HOME finta: il registro dei ganci
                    (`state/ganci.jsonl`) e la cache del catalogo
                    (`state/catalogo-skill.json`), col loro esatto `json.dumps`
    il marcatore    `/tmp/.innesco-<sessione>`, con i suoi permessi: è il freno
                    «una volta per sessione», e vive fuori dalla HOME

DUE HOME FINTE, UNA PER PARTE. Il gancio **scrive** la cache del catalogo e il
registro: con una cartella sola la seconda implementazione leggerebbe il lavoro
della prima — troverebbe la cache già fresca e non scandirebbe niente — e il
confronto darebbe verde per il motivo sbagliato.

LE TRASCRIZIONI SONO CONDIVISE, e non è un risparmio di spazio. La stazza del
file di trascrizione è l'ingresso che decide l'invito a passare il lavoro: se
ogni parte se la scrivesse nella propria HOME, un byte di differenza fra le due
copie diventerebbe una divergenza del porto che non esiste. Qui il file è uno
solo, in una terza cartella, e i due leggono quello.

IL MARCATORE STA IN `/tmp`, CIOÈ FUORI DALLE DUE HOME, perché il percorso è
scritto a mano nell'originale. Quindi non basta separare le case: prima di ogni
sequenza si cancellano i marcatori delle sessioni che quella sequenza usa,
altrimenti la seconda parte troverebbe il freno già tirato dalla prima e
tacerebbe — di nuovo verde per il motivo sbagliato.

    python3 tools/compare-skill-nudge.py
"""
import json
import os
import shutil
import stat
import subprocess
import sys
from pathlib import Path

ROOT = Path.home() / '.claude'
HOOK = ROOT / 'skills/hooks/skill-nudge.py'
# Il binario si può spostare con `CLAUDE_HOOKS_BIN`, e non è un vezzo: più
# sessioni fanno girare `mutants.sh` sullo stesso `target/`, quindi un giro di
# mutanti che si fida del percorso predefinito può finire per confrontare il
# mutante di un'altra sezione e contarlo come proprio. Con questa variabile e
# `CARGO_TARGET_DIR` il giro si isola.
BINARY = Path(os.environ.get('CLAUDE_HOOKS_BIN',
                             str(ROOT / 'rust/target/release/claude-hooks')))
SCRATCH = Path(os.environ.get(
    'RELAY_SCRATCH',
    '/private/tmp/claude-501/-Users-theo-orca-general/scratchpad')) / 'skill-nudge'
TRANSCRIPTS = SCRATCH / 'trascrizioni'

# Sopra la soglia predefinita (6 MB) e sotto ogni soglia assurda.
BIG = 20_000_000


def prepara_trascrizioni():
    """I due file di trascrizione, scritti una volta e letti da entrambe le parti."""
    TRANSCRIPTS.mkdir(parents=True, exist_ok=True)
    piccola = TRANSCRIPTS / 'piccola.jsonl'
    grossa = TRANSCRIPTS / 'grossa.jsonl'
    if not piccola.exists() or piccola.stat().st_size != 10:
        piccola.write_bytes(b'x' * 10)
    if not grossa.exists() or grossa.stat().st_size != BIG:
        grossa.write_bytes(b'x' * BIG)
    return str(piccola), str(grossa)


PICCOLA, GROSSA = prepara_trascrizioni()


# ── i semi: cosa c'è nella casa prima che il gancio parta ───────────────────

def seme_vuoto(home: Path):
    """Nessuna competenza installata: il catalogo esce vuoto e tutto «esiste»."""


def seme_catalogo(*presenti):
    """Una cache del catalogo già scritta e fresca, con solo questi nomi dentro.

    È il seme che esercita davvero `esiste()`: senza, in una casa vuota il
    catalogo risulta illeggibile e ogni nome passa — cioè il ramo che scarta una
    competenza disinstallata non lo proverebbe nessuno.
    """
    def applica(home: Path):
        cache = home / '.claude' / 'state' / 'catalogo-skill.json'
        cache.parent.mkdir(parents=True, exist_ok=True)
        cache.write_text(json.dumps({n: 'descrizione' for n in presenti},
                                    ensure_ascii=False, indent=1), encoding='utf8')
    return applica


def seme_cache_grezza(testo, vecchia=False):
    """Una cache scritta a mano, eventualmente scaduta.

    Serve ai casi storti: una cache che è una lista, una stringa, un numero — e
    quella stantia, che deve far ripartire la scansione da zero.
    """
    def applica(home: Path):
        cache = home / '.claude' / 'state' / 'catalogo-skill.json'
        cache.parent.mkdir(parents=True, exist_ok=True)
        cache.write_text(testo, encoding='utf8')
        if vecchia:
            vecchio = 1_000_000_000
            os.utime(cache, (vecchio, vecchio))
    return applica


def seme_albero(home: Path):
    """Un albero di competenze vero: cartelle, comandi, agenti, plugin, manifesto.

    È l'unico modo di provare la SCANSIONE invece della sola lettura della cache.
    Ogni pezzo copre una trappola dell'originale: il plugin spento, il manifesto
    che dichiara meno di quel che c'è sul disco, la cartella intera dichiarata
    (`./skills/`, nessun filtro), il comando senza campo `name`, il frontmatter
    che vieta l'invocazione, e una copia per un altro runtime.
    """
    c = home / '.claude'

    def scrivi(rel, testo):
        p = c / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(testo, encoding='utf8')

    scrivi('skills/trio-verify/SKILL.md',
           '---\nname: trio-verify\ndescription: verifica il trio\n---\ncorpo\n')
    scrivi('skills/mockup-fedele/SKILL.md',
           '---\nname: mockup-fedele\ndescription: porta un mockup\nallowed-tools: Read\n---\n')
    scrivi('skills/spenta/SKILL.md',
           '---\nname: spenta\ndescription: mai\ndisable-model-invocation: true\n---\n')
    scrivi('skills/senza-nome/SKILL.md', '---\ndescription: senza name\n---\n')
    scrivi('skills/senza-frontmatter/SKILL.md', 'niente frontmatter qui\n')
    # Un altro runtime: stessi nomi, non li legge Claude Code.
    scrivi('skills/skills-codex/altra/SKILL.md',
           '---\nname: altra\ndescription: copia\n---\n')
    # Il prefisso di mattpocock, che passa anche se il plugin non è acceso.
    scrivi('skills/mattpocock-skills/skills/famiglia/tdd/SKILL.md',
           '---\nname: tdd\ndescription: prova per prima\n---\n')
    scrivi('skills/mattpocock-skills/skills/famiglia/triage/SKILL.md',
           '---\nname: triage\ndescription: spacchetta\n---\n')
    scrivi('skills/mattpocock-skills/.claude-plugin/plugin.json',
           json.dumps({'skills': ['./skills/famiglia/tdd']}))
    # I comandi: si invocano per nome come una competenza.
    scrivi('commands/handoff.md',
           '---\ndescription: scrive la consegna e il puntatore\nargument-hint: "[x]"\n---\n')
    scrivi('commands/code-review.md', '---\ndescription: rivede il diff\n---\n')
    # Un plugin acceso e uno spento, con la stessa forma di cartella.
    scrivi('plugins/cache/mercato/harness/skills/ci/SKILL.md',
           '---\nname: ci\ndescription: pipeline rossa\n---\n')
    scrivi('plugins/cache/mercato/harness/.claude-plugin/plugin.json',
           json.dumps({'skills': ['./skills/']}))
    scrivi('plugins/cache/mercato/spento/skills/x/SKILL.md',
           '---\nname: x\ndescription: mai raggiungibile\n---\n')
    scrivi('plugins/cache/mercato/harness/agents/revisore.md',
           '---\nname: revisore\ndescription: giudica\n---\n')
    scrivi('agents/locale.md', '---\nname: locale\ndescription: agente locale\n---\n')
    scrivi('settings.json', json.dumps({'enabledPlugins': {'harness@mercato': True,
                                                           'spento@mercato': False}}))


# ── esecuzione e raccolta ───────────────────────────────────────────────────

def albero(home: Path):
    """Ogni file sotto la casa, come testo, con l'istante del registro tolto.

    Si guarda il TESTO e non l'oggetto rianalizzato: la forma è ciò che l'altra
    implementazione deve produrre uguale, e un confronto fra oggetti non vedrebbe
    né i separatori né l'ordine delle chiavi.
    """
    fuori = {}
    for p in sorted(home.rglob('*')):
        if not p.is_file():
            continue
        rel = str(p.relative_to(home))
        # `scripts/` è un collegamento a quello vero (vedi `esegui`): non è roba
        # che il gancio ha scritto, e dentro ci sono i `.pyc` dell'interprete.
        if rel.startswith('.claude/scripts'):
            continue
        try:
            testo = p.read_text(errors='replace')
        except OSError:
            testo = '<illeggibile>'
        if rel.endswith('ganci.jsonl'):
            righe = []
            for r in testo.splitlines():
                testa, _, coda = r.partition(', "gancio"')
                # Si taglia il VALORE dell'istante, non la chiave: se una delle
                # due smettesse di scriverlo, o lo scrivesse senza lo spazio dopo
                # i due punti, il confronto deve vederlo.
                righe.append(('<t>' if testa.startswith('{"t": ') else testa)
                             + ', "gancio"' + coda)
            testo = '\n'.join(righe)
        fuori[rel] = testo.replace(str(home), '<HOME>')
    return fuori


def marcatori(sessioni):
    """Il freno: per ogni sessione, se il file c'è e con che permessi."""
    fuori = {}
    for s in sessioni:
        p = Path('/tmp') / f'.innesco-{s}'
        if p.exists():
            fuori[s] = (p.read_text(errors='replace'),
                        stat.S_IMODE(p.stat().st_mode))
        else:
            fuori[s] = None
    return fuori


def ripulisci_marcatori(sessioni):
    for s in sessioni:
        p = Path('/tmp') / f'.innesco-{s}'
        if p.exists():
            p.unlink()


def sanifica(sid):
    """La stessa ripulitura dell'originale, per sapere quale file cercare."""
    import re
    return re.sub(r'[^A-Za-z0-9_-]', '', str(sid))[:36]


def esegui(comando, home: Path, passi, seme=None, env_extra=None, sessioni=()):
    """Fa girare l'intera sequenza su una HOME appena azzerata.

    I passi sono o un payload intero (dizionario) o una stringa grezza, per i
    casi in cui stdin non è nemmeno JSON.
    """
    shutil.rmtree(home, ignore_errors=True)
    (home / '.claude' / 'state').mkdir(parents=True)
    # SENZA QUESTO COLLEGAMENTO IL CONFRONTO È FINTO. L'originale fa
    # `sys.path.insert(0, Path.home()/'.claude'/'scripts')` e importa da lì il
    # catalogo e il registro; in una HOME finta quella cartella non c'è, i due
    # `except` di riserva si attivano e il Python gira **senza catalogo e senza
    # registro** — cioè con `skill_exists` che dice sempre sì e `log_hook` che
    # non scrive niente. Il primo giro ha dato 62 divergenze proprio così, tutte
    # su file che il Python non scriveva perché non aveva importato niente.
    (home / '.claude' / 'scripts').symlink_to(ROOT / 'scripts')
    (seme or seme_vuoto)(home)
    ripulisci_marcatori(sessioni)
    env = dict(os.environ, HOME=str(home))
    env.pop('INNESCO_SOGLIA_MB', None)
    env.update(env_extra or {})
    passaggi = []
    for passo in passi:
        grezzo = passo if isinstance(passo, str) else json.dumps(passo)
        p = subprocess.run(comando, input=grezzo, capture_output=True, text=True,
                           timeout=120, cwd=str(home), env=env)
        passaggi.append((p.returncode,
                         p.stdout.replace(str(home), '<HOME>'),
                         p.stderr.replace(str(home), '<HOME>')))
    return passaggi, albero(home), marcatori(sessioni)


def payload(prompt, transcript=None, sid=None):
    d = {'prompt': prompt}
    if transcript is not None:
        d['transcript_path'] = transcript
    if sid is not None:
        d['session_id'] = sid
    return d


# ── i casi ──────────────────────────────────────────────────────────────────

def casi():
    """Ogni caso: nome, sequenza, seme, ambiente, sessioni da ripulire."""
    lungo = 'i test sono rossi ' + 'x' * 12_000
    al_taglio = 'i test sono rossi ' + 'x' * (12_000 - len('i test sono rossi '))
    return [
        # ── i tre rami di `lines()`, uno per uno ───────────────────────────
        ('una competenza combacia', [payload('i test sono rossi')], None, None, ()),
        ('nessuna competenza combacia',
         [payload('aggiorna la riga 12 di questo file')], None, None, ()),
        ('due competenze al massimo',
         [payload('fai un piano, poi una code review, e verifica la coerenza '
                  'fra i tre repo')], None, None, ()),
        ('la conferma breve resta muta', [payload('procedi')], None, None, ()),
        ('la conferma breve sopra soglia invita a passare',
         [payload('procedi', GROSSA, 'ackgrossa')], None, None, ('ackgrossa',)),
        ('okay procedi: due parole di conferma',
         [payload('okay procedi', GROSSA, 'ackdue')], None, None, ('ackdue',)),
        ('la richiesta vaga e grande', [payload("sistema un po' le cose")],
         None, None, ()),
        ('vaga con lavoro vero', [payload('sistemiamo e puliamo allora')],
         None, None, ()),
        ('vaga senza lavoro non è vaghezza',
         [payload('ma dovevi aprirmelo qui non so dove l hai aperto')], None, None, ()),
        ('vaga e sopra soglia: due righe insieme',
         [payload("sistema un po' le cose", GROSSA, 'vagagrossa')], None, None,
         ('vagagrossa',)),
        ('tre righe in una volta',
         [payload("sistemiamo un po' tutto, e fai un piano", GROSSA, 'trerighe')],
         None, None, ('trerighe',)),

        # ── il taglio in lunghezza ────────────────────────────────────────
        ('oltre il taglio in lunghezza tace', [payload(lungo)], None, None, ()),
        ('esattamente al taglio parla ancora', [payload(al_taglio)], None, None, ()),
        # `len()` di Python conta i CARATTERI: con i byte, un testo accentato
        # cadrebbe dalla parte sbagliata del taglio.
        ('il taglio si misura in caratteri, non in byte',
         [payload('i test sono rossi ' + 'è' * (12_000 - 18))], None, None, ()),

        # ── il freno «una volta per sessione» ─────────────────────────────
        ('la prima volta invita, la seconda tace',
         [payload('implementa la funzione', GROSSA, 'freno'),
          payload('implementa la funzione', GROSSA, 'freno')], None, None, ('freno',)),
        ('senza session_id il freno non si arma',
         [payload('implementa la funzione', GROSSA),
          payload('implementa la funzione', GROSSA)], None, None, ()),
        ('un session_id con una barra scrive comunque il marcatore',
         [payload('implementa la funzione', GROSSA, 'a/b')], None, None,
         (sanifica('a/b'),)),
        ('un session_id che è una fuga',
         [payload('implementa la funzione', GROSSA, '../fuga')], None, None,
         (sanifica('../fuga'),)),
        ('un session_id lunghissimo si tronca a 36',
         [payload('implementa la funzione', GROSSA, 'x' * 200)], None, None,
         (sanifica('x' * 200),)),
        ('un session_id che è una lista',
         [payload('implementa la funzione', GROSSA, ['a'])], None, None,
         (sanifica(['a']),)),
        ('un session_id numerico',
         [payload('implementa la funzione', GROSSA, 4242)], None, None,
         (sanifica(4242),)),
        ('un session_id fatto di soli segni non arma niente',
         [payload('implementa la funzione', GROSSA, '///')], None, None, ()),
        # Il marcatore si scrive SOLO per l'invito a passare: una competenza da
        # sola non deve armare il freno, o l'invito non uscirebbe mai.
        ('una competenza sola non arma il freno',
         [payload('i test sono rossi', PICCOLA, 'solocomp')], None, None,
         ('solocomp',)),

        # ── la soglia da ambiente ─────────────────────────────────────────
        ('soglia valida sotto la stazza: parla',
         [payload('implementa la funzione', GROSSA, 'sogliaok')], None,
         {'INNESCO_SOGLIA_MB': '20'}, ('sogliaok',)),
        ('soglia valida sopra la stazza: tace',
         [payload('implementa la funzione', GROSSA, 'sogliaalta')], None,
         {'INNESCO_SOGLIA_MB': '25'}, ('sogliaalta',)),
        ('soglia vuota non blocca il turno',
         [payload('implementa la funzione', GROSSA, 'sogliavuota')], None,
         {'INNESCO_SOGLIA_MB': ''}, ('sogliavuota',)),
        ('soglia zero non fa parlare a ogni messaggio',
         [payload('aggiorna la riga 12', PICCOLA, 'sogliazero')], None,
         {'INNESCO_SOGLIA_MB': '0'}, ('sogliazero',)),
        ('soglia negativa vale come zero',
         [payload('implementa la funzione', PICCOLA, 'sioglianeg')], None,
         {'INNESCO_SOGLIA_MB': '-3'}, ('sioglianeg',)),
        ('soglia non numerica non blocca il turno',
         [payload('implementa la funzione', GROSSA, 'sogliaabc')], None,
         {'INNESCO_SOGLIA_MB': 'abc'}, ('sogliaabc',)),
        ('soglia con decimali cade sul valore predefinito',
         [payload('implementa la funzione', GROSSA, 'sogliadec')], None,
         {'INNESCO_SOGLIA_MB': '7.5'}, ('sogliadec',)),
        ('soglia con spazi attorno',
         [payload('implementa la funzione', GROSSA, 'sogliaspazi')], None,
         {'INNESCO_SOGLIA_MB': ' 21 '}, ('sogliaspazi',)),
        ('soglia col trattino basso, che Python accetta',
         [payload('implementa la funzione', GROSSA, 'sogliauno')], None,
         {'INNESCO_SOGLIA_MB': '2_1'}, ('sogliauno',)),
        ('soglia enorme',
         [payload('implementa la funzione', GROSSA, 'sogliahuge')], None,
         {'INNESCO_SOGLIA_MB': '99999999999999999999999'}, ('sogliahuge',)),

        # ── il catalogo ───────────────────────────────────────────────────
        # Il caso che smaschera `esiste()`: la competenza aggancia ma non è
        # installata, quindi la riga non esce. Senza un catalogo seminato, in una
        # casa vuota ogni nome passa e questo ramo non lo prova nessuno.
        ('una competenza agganciata ma disinstallata resta muta',
         [payload('i test sono rossi')], seme_catalogo('altra-cosa'), None, ()),
        ('una competenza agganciata e installata parla',
         [payload('i test sono rossi')], seme_catalogo('claude-code-harness:ci'),
         None, ()),
        # `/code-review` non passa dal catalogo: serve la CLI e passa comunque.
        # IL DIFETTO DELL'ORIGINALE, RIPRODOTTO NEL PORTO. Il tetto di due si
        # applica PRIMA del filtro sull'installato: se le prime due competenze
        # agganciate non ci sono, la terza — che c'è — non viene mai nominata.
        # Non è teorico: il 17/08/2026 `mattpocock-skills:resolving-merge-conflicts`
        # e `mattpocock-skills:triage` risultano entrambe disinstallate, e questa
        # frase le aggancia in prima e seconda posizione. Provato sull'originale
        # col catalogo vero: esce solo la riga «chiarire», e `harness-plan` tace.
        ('due voci mute silenziano la terza, che invece esiste',
         [payload("risolvi i conflitti di merge, poi sistemiamo un po' tutto, "
                  "e fai un piano")],
         seme_catalogo('claude-code-harness:harness-plan'), None, ()),
        ('un nome con la barra passa senza catalogo',
         [payload('fammi una code review del diff')], seme_catalogo('niente'),
         None, ()),
        ('il catalogo si costruisce solo se qualcuno lo interroga',
         [payload('aggiorna la riga 12 di questo file')], None, None, ()),
        ('la cache stantia fa ripartire la scansione',
         [payload('i test sono rossi')],
         seme_cache_grezza('{"claude-code-harness:ci": "d"}', vecchia=True), None, ()),
        ('la cache fresca si legge e basta',
         [payload('i test sono rossi')],
         seme_cache_grezza('{"claude-code-harness:ci": "d"}'), None, ()),
        ('una cache illeggibile fa ripartire la scansione',
         [payload('i test sono rossi')], seme_cache_grezza('non sono json'), None, ()),
        ('una cache che è una lista',
         [payload('i test sono rossi')],
         seme_cache_grezza('["claude-code-harness:ci"]'), None, ()),
        ('una cache che è una lista senza il nome',
         [payload('i test sono rossi')], seme_cache_grezza('["altro"]'), None, ()),
        ('una cache che è una stringa',
         [payload('i test sono rossi')],
         seme_cache_grezza('"claude-code-harness:ci e altro"'), None, ()),
        # `name in 5` è un TypeError: l'originale esce muto, senza registrare.
        ('una cache che è un numero spegne tutto',
         [payload('i test sono rossi', GROSSA, 'cachenum')],
         seme_cache_grezza('5'), None, ('cachenum',)),
        ('una cache vuota vale come catalogo illeggibile',
         [payload('i test sono rossi')], seme_cache_grezza('{}'), None, ()),

        # ── la scansione vera ─────────────────────────────────────────────
        ('la scansione di un albero vero',
         [payload('i test sono rossi')], seme_albero, None, ()),
        ('la scansione e una competenza dichiarata dal manifesto',
         [payload('scrivi la prova prima del codice')], seme_albero, None, ()),
        # `triage` sta sul disco ma il manifesto non la dichiara: muta.
        ('una competenza sul disco ma non dichiarata resta muta',
         [payload("sistema un po' tutto")], seme_albero, None, ()),
        ('un comando indicizzato per nome di file',
         [payload('fammi la consegna della sessione')], seme_albero, None, ()),

        # ── i richiami: frasi che DEVONO agganciare ───────────────────────
        ('la CI è rotta', [payload('la CI è rotta')], None, None, ()),
        ('la pipeline non passa', [payload('la pipeline non passa')], None, None, ()),
        ('prepara il piano', [payload('prepara il piano per la funzione nuova')],
         None, None, ()),
        ('pianifichiamo', [payload('pianifichiamo il lavoro di domani')], None, None, ()),
        ('chiudi questa sessione', [payload('chiudi questa sessione')], None, None, ()),
        ('sql injection', [payload('è vulnerabile a SQL injection')], None, None, ()),
        ('cerca le vulnerabilità',
         [payload('cerca le vulnerabilità in questo modulo')], None, None, ()),
        ('audit di sicurezza', [payload('fammi un audit di sicurezza del repo')],
         None, None, ()),
        ('il secondo tentativo fallito',
         [payload('ho già provato ma non funziona')], None, None, ()),
        ('i conflitti di unione',
         [payload('risolvi i conflitti di merge')], None, None, ()),
        ('il mockup', [payload('porta esattamente il mockup dentro suite')],
         None, None, ()),
        ('più grande di una sessione',
         [payload('è un lavoro più grande di una sessione')], None, None, ()),
        ('come funziona nel codice',
         [payload('come funziona la staffetta nel codice')], None, None, ()),
        ('la finestra dell app', [payload("leggi la finestra dell'app")], None, None, ()),

        # ── le controprove che il restringimento esiste per reggere ───────
        ('la CI verde non è una CI rossa',
         [payload('archivialo, e mergia appena la CI è verde')], None, None, ()),
        ('ci pronome', [payload('sys.exit(1) quando ci sono falliti')], None, None, ()),
        ('rossi nella tavolozza',
         [payload('ci sono rossi e verdi nella tavolozza')], None, None, ()),
        ('un percorso mastra non è il framework',
         [payload('- src/mastra/workflows/steps/file/merge-items.ts')], None, None, ()),
        ('mastra nominata di passaggio',
         [payload('matching-engine usa Mastra e Prisma')], None, None, ()),
        ('uno step mastra invece sì',
         [payload('fammi uno step mastra per la normalizzazione')], None, None, ()),
        ('un percorso prisma non è un comando',
         [payload('src/generated/prisma/** è codice generato, escludilo')], None, None, ()),
        ('prisma migrate invece sì',
         [payload('lancia prisma migrate sul database di prova')], None, None, ()),
        # Il primo carattere del testo: lo sguardo indietro riscritto deve
        # accettare anche l'inizio della stringa.
        ('prisma migrate a inizio richiesta',
         [payload('prisma migrate sul database di prova')], None, None, ()),
        # I DUE CASI CHE PROVANO LA RISCRITTURA DEGLI SGUARDI. Senza di loro,
        # togliere lo sguardo indietro di `prisma` e quello avanti di `mastra`
        # non cambia niente su nessun altro caso: il primo giro di mutanti li ha
        # visti sopravvivere entrambi, ed è così che si è scoperto il buco.
        ('un percorso che finisce in prisma non è un comando',
         [payload('vedi src/db/prisma migrate.ts')], None, None, ()),
        ('mastra seguita da un punto è un file, non il framework',
         [payload('lavora con mastra.config.ts')], None, None, ()),
        ('mastra seguita da una barra è un percorso',
         [payload('cerca dentro mastra/src')], None, None, ()),
        ('Plans.md come fonte non è pulizia',
         [payload('Implementa la lavorazione 8.8 di Plans.md')], None, None, ()),
        ('archivia le fasi invece sì',
         [payload('archivia le fasi chiuse di Plans.md')], None, None, ()),
        ('il divieto su Linear non è una lettura',
         [payload('VIETATO: commenti su Linear e su GitHub')], None, None, ()),
        ('guarda la scheda su linear invece sì',
         [payload('guarda la scheda su linear prima di partire')], None, None, ()),
        ('l etichetta da revisionare non è una revisione',
         [payload('const NO_STATUS_LABEL = "Da revisionare"')], None, None, ()),
        ('le falle già trovate non sono una richiesta',
         [payload('non ho trovato vulnerabilità sfruttabili')], None, None, ()),
        ('cross-repo nel divieto',
         [payload('MAI unire: cambi di auth, migration, cross-repo')], None, None, ()),

        # ── la domanda, che non è lavoro nuovo ────────────────────────────
        ('una domanda sopra soglia non innesca il passaggio',
         [payload('quanto pesa la cartella?', GROSSA, 'domanda')], None, None,
         ('domanda',)),
        ('una domanda con un verbo di compito',
         [payload('hai testato e verificato in ui?', GROSSA, 'domandadue')], None,
         None, ('domandadue',)),

        # ── il taglio degli estremi ───────────────────────────────────────
        ('solo spazi resta muto', [payload('   \n \t ')], None, None, ()),
        # `str.strip()` di Python toglie anche i separatori di controllo, che
        # `trim()` di Rust si lascia dietro.
        ('solo separatori di controllo resta muto',
         [payload('\x1c\x1d\x1e\x1f')], None, None, ()),
        ('separatori attorno a una conferma',
         [payload('\x1c procedi \x1f', GROSSA, 'sepack')], None, None, ('sepack',)),

        # ── ingressi storti: nessuno deve far uscire diverso da zero ──────
        ('stdin vuoto', [''], None, None, ()),
        ('stdin non JSON', ['non sono json'], None, None, ()),
        ('oggetto vuoto', ['{}'], None, None, ()),
        ('prompt nullo', ['{"prompt": null}'], None, None, ()),
        ('JSON che è una lista', ['[]'], None, None, ()),
        ('JSON che è una stringa', ['"stringa"'], None, None, ()),
        ('prompt numerico', ['{"prompt": 123}'], None, None, ()),
        ('prompt zero, che in Python è la stringa vuota', ['{"prompt": 0}'],
         None, None, ()),
        ('prompt lista vuota', ['{"prompt": []}'], None, None, ()),
        ('transcript_path che è una lista',
         ['{"prompt": "i test sono rossi", "transcript_path": []}'], None, None, ()),
        ('transcript_path inesistente',
         ['{"prompt": "i test sono rossi", "transcript_path": "/non/esiste/mai"}'],
         None, None, ()),
        ('transcript_path che è una cartella',
         [payload('i test sono rossi', '/tmp')], None, None, ()),
        ('session_id che è una lista con un numero',
         [payload('implementa la funzione', GROSSA, [1])], None, None,
         (sanifica([1]),)),
    ]


def main():
    if not BINARY.exists():
        print(f'manca {BINARY}: cargo build --release')
        return 1
    SCRATCH.mkdir(parents=True, exist_ok=True)
    py, rs = SCRATCH / 'home-python', SCRATCH / 'home-rust'
    cmd_py = [sys.executable, str(HOOK)]
    cmd_rs = [str(BINARY), 'skill-nudge']

    divergenze = 0
    prove = casi()
    for nome, passi, seme, ambiente, sessioni in prove:
        try:
            attesa = esegui(cmd_py, py, passi, seme, ambiente, sessioni)
            ottenuta = esegui(cmd_rs, rs, passi, seme, ambiente, sessioni)
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
            for k in sorted(set(attesa[1]) | set(ottenuta[1])):
                if attesa[1].get(k) != ottenuta[1].get(k):
                    print(f'      file {k}')
                    print(f'         python: {attesa[1].get(k)!r}')
                    print(f'         rust:   {ottenuta[1].get(k)!r}')
        if attesa[2] != ottenuta[2]:
            print(f'      marcatori python: {attesa[2]!r}')
            print(f'      marcatori rust:   {ottenuta[2]!r}')
    print()
    if divergenze:
        print(f'{divergenze} divergenze su {len(prove)} casi')
    else:
        print(f'{len(prove)} casi, nessuna divergenza')
    return 1 if divergenze else 0


if __name__ == '__main__':
    sys.exit(main())
