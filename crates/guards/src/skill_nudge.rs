//! Il giudizio di `skill-nudge.py`: quale riga aggiungere a una richiesta appena
//! arrivata, e quando invece tacere.
//!
//! Qui non si tocca né stdin né il disco: entra il testo della richiesta, la
//! stazza della trascrizione e il freno di sessione, esce l'elenco delle righe.
//! L'unica cosa che il giudizio non sa da sé è se una competenza sia davvero
//! installata, e quella arriva come richiamo (`exists`) — così il catalogo, che
//! legge dal disco, resta dall'altra parte del confine.
//!
//! IL CASO NORMALE È IL SILENZIO. Un promemoria che compare a ogni turno si
//! smette di leggere in due giorni e diventa contesto sprecato a ogni richiesta,
//! cioè esattamente il difetto che vorrebbe curare. Gli schemi sono stretti di
//! proposito: mai una parola sola del parlato comune, e dove serve due elementi
//! nella stessa frase. Il primo tentativo, con parole singole, agganciava il
//! 27,4% dei messaggi veri.
//!
//! LE REGEX SONO RISCRITTE, NON RICOPIATE, in due punti. La crate `regex` non ha
//! né sguardo indietro né sguardo avanti, e l'originale ne usa quattro su
//! `prisma` e `mastra`. La riscrittura è dichiarata sul posto, voce per voce.
//!
//! RESIDUO DICHIARATO SU `\s`. In Python `\s` comprende anche i quattro
//! separatori di controllo `\x1c`-`\x1f`, che non hanno la proprietà Unicode
//! `White_Space` e quindi qui non ci sono. Dove quella differenza è
//! raggiungibile — il taglio degli estremi della richiesta — è curata da
//! `python_trim`; **dentro** una frase servirebbe un separatore di file in mezzo
//! a due parole di un prompt, che non è un caso che questa configurazione
//! produca. Riscrivere ogni `\s` come classe estesa costa più errori di
//! trascrizione di quanti ne eviti, e la scelta è dichiarata invece che scoperta.

use regex::Regex;
use std::sync::OnceLock;

/// Oltre questa lunghezza la richiesta è già circostanziata e il gancio tace.
///
/// Alzato da 2.000 a 12.000 l'11/08/2026: la soglia bassa nasceva da «una
/// richiesta lunga è già circostanziata», vero per il testo scritto da una
/// persona e falso per come arrivano davvero i mandati.
pub const CHAR_CUTOFF: usize = 12_000;

/// Sopra questa stazza di registro la sessione è nella fascia che consuma l'82%
/// della cache riletta (misura dell'11/08/2026 su 888 sessioni).
pub const DEFAULT_THRESHOLD_MB: u64 = 6;

/// Una voce della tavola: lo schema che la aggancia, il nome con cui si invoca,
/// e cosa copre.
pub struct Skill {
    pub pattern: &'static str,
    pub name: &'static str,
    pub when: &'static str,
}

/// Competenza → quando serve. Solo quelle installate e verificate: nominare una
/// competenza che non c'è manda a cercare una cosa inesistente, che è peggio del
/// silenzio — ed è il motivo per cui ogni nome passa comunque da `exists`.
///
/// L'ORDINE È PARTE DEL COMPORTAMENTO: si prendono le prime due che agganciano e
/// si smette. Un elenco lungo è rumore e viene saltato come tutto il resto.
pub static SKILLS: &[Skill] = &[
    // Il secondo tentativo fallito, che è dove il CLAUDE.md vuole la diagnosi.
    // Serve il segno che si è già provato: «non funziona» da solo è la metà dei
    // messaggi di lavoro.
    Skill {
        pattern: r"\b((ho |abbiamo |)(già |gia |)provat[oi]( \w+){0,3} (ma|e) (non|ancora)|(continua|continuano) a (fallire|non funzionare|dare errore)|(ancora|sempre) (lo stesso|il medesimo) (errore|problema)|(non|nn) (riesco|riusciamo) a (capire|trovare) (perch|il motivo|la causa))",
        name: "mattpocock-skills:diagnosing-bugs",
        when: "un difetto che resiste al secondo tentativo",
    },
    // La prova prima del codice. `tdd` da solo è una sigla che compare nei
    // documenti; serve la richiesta di scriverla o l'ordine esplicito.
    Skill {
        pattern: r"\b((scrivi|scriviamo|aggiungi|aggiungiamo) (una |il |i |le |un |)(prova|prove|test)( \w+){0,3} (prima|per prima)|(prova|test) (prima del|prima di scrivere il) codice|(in |con |a )(tdd|prova.per.prima)\b|test.driven)",
        name: "mattpocock-skills:tdd",
        when: "una funzione o una correzione prova-per-prima",
    },
    // I conflitti di unione NON hanno più una voce qui. La competenza a monte
    // `mattpocock-skills:resolving-merge-conflicts` è stata disattivata il
    // 18/08/2026 (`disable-model-invocation: true`, «do not invoke»): dice di
    // risolvere sempre e non abortire mai, mentre `rules/base-aggiornata.md`
    // prescrive l'opposto su un CONFLICT (modify/delete). Lo schema restava
    // agganciato e poi taceva, perché `exists` non la trova nel catalogo — cioè
    // costava una lettura a ogni richiesta senza poter dire niente.
    // Le parole del dominio ambigue: è il segnale che precede il modello
    // sbagliato, e costa più di ogni difetto di codice.
    Skill {
        pattern: r"\b((cosa|che cosa) (intendiamo|intendi) (per|con)|(il termine|la parola|il nome) \S+ (è|e) ambigu|(chiamiamo|chiamarlo) (in |allo )(stesso |)(modo|nome)( diverso|)|non (è|e) chiaro (cosa|che cosa) (sia|significa|intendiamo))",
        name: "mattpocock-skills:domain-modeling",
        when: "le parole del dominio ambigue",
    },
    // Le riscritture profonde, che il CLAUDE.md indirizza già a questa.
    Skill {
        pattern: r"\b((riscriv|ristruttur|rifattorizz|refactor)\w* (l.architettura|in profondità|in profondita|tutto il modulo|il modulo intero)|(l.architettura|la struttura) (di questo|del) \w+ (non va|è sbagliata|e sbagliata))",
        name: "mattpocock-skills:improve-codebase-architecture",
        when: "una riscrittura di architettura",
    },
    // Portare un mockup dentro un prodotto che esiste già. Lo schema vuole due
    // elementi — il disegno e il verbo che lo porta o lo confronta — perché
    // «guarda il mockup» da solo compare anche quando lo si sta solo commentando.
    Skill {
        pattern: r"\b((mockup|mock.up|figma|design doc|\.dc\.html)( \w+){0,4} (in|dentro|su) (suite|prodotto|app|shell|scheda)|(porta|portare|portiamo|implementa|implementiamo|replica|replichiamo|segui|seguiamo|rispecchia|rispecchiamo) (esattamente |per bene |bene |)(il |questo |lo |)(mockup|mock.up|figma|design)|(come|uguale a|secondo) (il |al |nel )(mockup|mock.up|figma|design)|(non|nn) (rispecchia|segue|è uguale a|e uguale a|torna con) (il |al )(mockup|mock.up|figma))",
        name: "mockup-fedele",
        when: "portare un mockup dentro un prodotto che esiste già",
    },
    // Il lavoro più grande di una sessione: diventa una mappa di decisioni,
    // invece di un piano che finge di sapere.
    Skill {
        pattern: r"\b((è|e) (un lavoro |una cosa |)(più|piu) (grande|grosso|lungo) di una sessione|(progetto|lavoro) (lungo|di più giorni|di piu giorni|di settimane)|non (si chiude|lo chiudiamo) (oggi|in una sessione))",
        name: "mattpocock-skills:wayfinder",
        when: "un lavoro più grande di una sessione",
    },
    // La consegna, che è il mandato dell'11/08: mai più compattazione.
    // `handoff` e non `mattpocock-skills:handoff`: la prima scrive un file di
    // memoria del progetto più il puntatore in MEMORY.md, che la sessione dopo
    // ricarica da sola; la seconda scrive nella cartella temporanea di sistema.
    Skill {
        pattern: r"\b((passa|passiamo|consegna|consegniamo) (il |questo |il lavoro|questa )\w*( a|ad) (un.altra|altra|una nuova) sessione|(scrivi|scrivimi|prepara|preparami|fai|fammi) (la |una )consegna|(chiudi|chiudiamo) (la |questa )sessione)",
        name: "handoff",
        when: "la consegna del lavoro a chi riprende",
    },
    // La richiesta grossa e confusa NON ha più una voce qui, per la stessa
    // ragione: `mattpocock-skills:triage` è disattivata dal 18/08/2026 perché
    // scrive sul tracciatore, e scrivere su Linear è riservato a Theo — un
    // gancio lo rifiuta. Quel caso lo copre già il CLAUDE.md, che su «sistema un
    // po'…» chiede l'elenco puntato dei cambiamenti prima di toccare i file.
    // Capire come funziona qualcosa nel codice: è il gate di CLAUDE.md, e
    // l'unico modo di usare grafo e artefatti insieme.
    Skill {
        pattern: r"\b((come|dove) (funziona|viene gestit[oa]|vive|sta) (il |la |lo |l.)\w+ (nel|in) (codice|repo|progetto)|(spiegami|fammi capire) come (funziona|è fatto|e fatto) )",
        name: "socraticode:codebase-exploration",
        when: "capire come funziona una parte del codice",
    },
    // La scansione di sicurezza di un repo intero, distinta dal vaglio su una
    // modifica: quella la fa l'agente `security-reviewer`.
    Skill {
        pattern: r"\b((analisi|scansione|audit) di sicurezza (di |su |del |sul )(tutto |l.intero |)(repo|repository|progetto|codice)|(cerca|trova) vulnerabilit\w+ (in|nel|su) (tutto|questo repo))",
        name: "claude-security:claude-security",
        when: "la scansione di sicurezza di un repo intero",
    },
    // Serve la coppia «cosa» + «com'è andata»: elencare le sole cose (ci, build,
    // test) aggancia chi sta semplicemente aspettando che passino.
    // `ci` nudo è anche il pronome italiano: «sys.exit(1) quando ci sono
    // falliti» chiamava la brigata dei controlli automatici. Quindi la sigla
    // vale solo maiuscola — `(?-i:CI)`, che spegne l'insensibilità solo lì.
    Skill {
        pattern: r"\b((?-i:CI)|la ci|pipeline|build|test|controlli( automatici)?|workflow)\s+(è|e|sono|sia|non|)\s*(ross[oaie]|fallit\w*|fallisc\w+|rott[oaie]|in errore|passa|passano)\b",
        name: "claude-code-harness:ci",
        when: "i controlli automatici rossi",
    },
    // «da revisionare» è stato tolto: nel prodotto è l'etichetta di stato di un
    // candidato, e agganciava codice e prove dell'interfaccia. Omonimia pura.
    Skill {
        pattern: r"\b(code.?review|revisione del codice|rivedi (il codice|questo codice)|revisiona (il|questo) codice|(fai|chiedi|fammi) (una|la) (code.?)?review)",
        name: "/code-review",
        when: "la revisione del codice",
    },
    Skill {
        pattern: r"\b((fai|scrivi|prepara|struttura|consolida) (un|il) piano|(struttur|consolid)are un piano|pianifica(mi)? (la|il|una|un)|pianifichiamo|piano di implementazione)",
        name: "claude-code-harness:harness-plan",
        when: "la pianificazione di una funzione",
    },
    Skill {
        pattern: r"\b(esegui il piano|implementa il piano|porta avanti il piano)",
        name: "claude-code-harness:harness-work",
        when: "l'esecuzione di un piano",
    },
    // `cross-repo` da solo agganciava soprattutto i DIVIETI e le etichette di
    // rischio, cioè testi che dicono di fermarsi. Resta la richiesta esplicita.
    Skill {
        pattern: r"\b(coerenza (fra|tra) i (tre |)repo|verifica (sul|il) trio|verifica cross.?repo|allinea(re|mento) (fra|tra) i repo)",
        name: "trio-verify",
        when: "la verifica sul trio di repository",
    },
    // `Plans.md` è la FONTE del piano citata da quasi ogni mandato: agganciava
    // chi sta per implementare, non chi sta per archiviare. Serve il verbo.
    Skill {
        pattern: r"\b(log vecchi|file di stato vecchi|(archivia|pulisci|potare|pota) (i |le |il |)(log|registri|fasi|plans\.md)|plans\.md (è|e) (gonfio|lungo))",
        name: "claude-code-harness:maintenance",
        when: "la pulizia di registri e file di stato",
    },
    // RISCRITTA. L'originale è `(?<![/.\w])(…)`: uno sguardo indietro, che la
    // crate `regex` non ha. Qui il carattere precedente si CONSUMA — `(?:^|…)` —
    // e la classe è l'esatto complemento di quella negata, quindi la condizione
    // «inizio del testo, oppure il carattere prima non è `/`, `.` o parola» è la
    // stessa. Consumare un carattere cambierebbe l'estensione del riscontro, ma
    // qui serve solo sapere SE aggancia: `re.search` è usata come un booleano.
    // Serve perché `prisma` e `mastra` nudi agganciavano i PERCORSI —
    // `schema.prisma`, `src/generated/prisma/**` — in testi che con quei comandi
    // non c'entravano niente.
    Skill {
        pattern: r"(?:^|[^/.\w])(prisma (migrate|generate|studio|db push)|(lancia|esegui|fai) (una |la |)migrazione|schema del database|migrazione del database)",
        name: "prisma-cli",
        when: "i comandi Prisma",
    },
    // RISCRITTA IN TRE PUNTI, e i tre non si curano allo stesso modo.
    //  1. lo sguardo indietro iniziale: come sopra, si consuma il carattere;
    //  2. `mastra(?![/.\w])\s+` — lo sguardo avanti qui è RIDONDANTE e si toglie
    //     e basta: `\s+` pretende già uno spazio, e nessuno spazio sta in
    //     `[/.\w]`. Sostituirlo con una classe consumata sarebbe stato
    //     sbagliato, perché avrebbe mangiato l'unico spazio che `\s+` esige;
    //  3. gli altri due sguardi avanti stanno a fine schema, quindi lì il
    //     carattere si può consumare: `(?:$|[^/.\w])`.
    Skill {
        pattern: r"(?:^|[^/.\w])(mastra\s+(workflow|agent|step|tool|flusso)|(workflow|agent|step|tool|flusso)\s+mastra(?:$|[^/.\w])|\b(con|in|usando|dentro) mastra(?:$|[^/.\w]))",
        name: "mastra",
        when: "il framework Mastra",
    },
    // `su linear` agganciava i divieti («VIETATO: commenti su Linear»). Restano
    // le forme in cui Linear è l'oggetto di una lettura o di un aggiornamento.
    Skill {
        pattern: r"\b(ticket linear|scheda linear|issue linear|(guarda|leggi|apri|aggiorna|sposta|chiudi) (la |il |lo |)\w* ?su linear)\b",
        name: "orca-linear",
        when: "la lettura dei ticket Linear",
    },
    Skill {
        pattern: r"\b(finestra dell.app|app desktop|interfaccia di orca|albero di accessibilit)",
        name: "computer-use",
        when: "il pilotaggio delle finestre",
    },
    Skill {
        pattern: r"\b(coordina gli agenti|dividi (il lavoro |)fra (gli |)agenti|piu. agenti in parallelo)",
        name: "orchestration",
        when: "il coordinamento fra agenti",
    },
    // `vulnerabilit` nudo agganciava anche chi PARLA di falle già trovate e i
    // mandati automatici che sono già essi stessi una revisione di sicurezza.
    // Serve il verbo di ricerca accanto: chi cerca, non chi commenta.
    Skill {
        pattern: r"\b(analisi di sicurezza|audit di sicurezza|falla di sicurezza|(cerca|cerchiamo|trova|troviamo|caccia|cacciamo|analizza|analizziamo|verifica|verifichiamo|controlla|controlliamo|scova|scoviamo)(mi)?\s+(le |la |eventuali |possibili |)(vulnerabilit|falle|buchi)|sql injection|xss|(verifica|controlla|analizza) la sicurezza|sicurezza del codice)",
        name: "/claude-security",
        when: "l'analisi di sicurezza",
    },
];

/// Una richiesta vaga e grande: qui si chiarisce prima di partire a fare.
const VAGUE_PATTERN: &str = r"(?i)\b(non so|boh|vedi tu|come (ti sembra|preferisci)|sistema un po|fai come|un po.? di ordine|migliora (un po|il)|ottimizza tutto|sistemiamo|miglioriamo|vediamo un po)\b";

/// Conferme che non aprono lavoro nuovo: restano mute.
///
/// Il vocabolario a parola singola non copriva come Theo conferma davvero —
/// «okay» (che `ok\b` non aggancia, perché fra «k» e «a» non c'è confine di
/// parola), «okay procedi», «va bene procedi». Quindi: una o due parole, tutte
/// dalla stessa lista.
const ACKS: &str = r"ok(ay)?|va bene|perfetto|proced[io]|procediamo|vai|avanti|continua|s[ìi]|no|grazie|esatto|giusto|fatto|d.accordo|dai";

/// Verbi che aprono lavoro vero: servono a distinguere un compito da una
/// domanda. Le forme in -iamo ci sono perché Theo scrive quasi sempre alla prima
/// plurale, e senza di esse il filtro scartava proprio i suoi incarichi veri.
const TASK_PATTERN: &str = r"(?i)\b(implement|scriv|cre|corregg|sistem|rifa|aggiung|tolg|toglia|rimuov|analizz|verific|misur|pulis|pulia|aggiorn|spost|rinomin|consolid|ottimizz|miglior)\w*\b";

fn skill_regexes() -> &'static [Regex] {
    static COMPILED: OnceLock<Vec<Regex>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        SKILLS
            .iter()
            .map(|s| Regex::new(&format!("(?i){}", s.pattern)).expect("schema di competenza"))
            .collect()
    })
}

fn vague() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(VAGUE_PATTERN).expect("schema del vago"))
}

fn task() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(TASK_PATTERN).expect("schema del compito"))
}

fn ack() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // `^…$` senza multilinea: in Python `$` accetta anche un a-capo finale,
        // in Rust no — ma il testo qui è già passato da `python_trim`, quindi un
        // a-capo finale non può esserci e le due forme coincidono.
        Regex::new(&format!(r"(?i)^\s*({ACKS})([\s,]+({ACKS}))?\b[\s.!]*$"))
            .expect("schema della conferma")
    })
}

/// `str.strip()` di Python, che non è `str::trim()` di Rust.
///
/// Python toglie i caratteri per cui `isspace()` è vero, e quell'insieme
/// comprende i quattro separatori di controllo `\x1c`-`\x1f`, che non hanno la
/// proprietà Unicode `White_Space` e quindi `trim()` si lascia dietro. Su una
/// richiesta fatta di soli separatori il Python tace e un `trim()` avrebbe
/// parlato: è la differenza fra «si comportano uguale» e «quasi».
pub fn python_trim(s: &str) -> &str {
    let is_py_space = |c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c);
    s.trim_matches(is_py_space)
}

/// La soglia di costo letta dall'ambiente (`INNESCO_SOGLIA_MB`).
///
/// Una variabile esportata vuota faceva morire il modulo prima ancora di
/// `main()`, con uscita 1 — cioè il turno bloccato: era l'unico punto del file
/// capace di farlo, ed è curato dal `or '6'`. Zero o negativo non è una soglia
/// più severa: è il gancio che parla a ogni messaggio con un verbo dentro.
pub fn threshold_mb(raw: Option<&str>) -> u64 {
    let value = match raw {
        None => Some(DEFAULT_THRESHOLD_MB as i128),
        // `os.environ.get(...) or '6'`: la stringa vuota è falsa e vale 6.
        Some(s) if s.is_empty() => Some(DEFAULT_THRESHOLD_MB as i128),
        Some(s) => python_int(s),
    };
    match value {
        // `except ValueError` dell'originale.
        None => DEFAULT_THRESHOLD_MB,
        Some(v) if v < 1 => DEFAULT_THRESHOLD_MB,
        Some(v) => v.min(u64::MAX as i128) as u64,
    }
}

/// `int(s)` di Python, per quel che serve a una soglia.
///
/// Accetta lo spazio attorno, il segno, e il trattino basso fra le cifre
/// (`int("1_0") == 10`). RESIDUO DICHIARATO: Python accetta anche le cifre
/// decimali non ASCII (`int("١٢") == 12`); qui no, e una soglia scritta in
/// devanagari cade sul valore predefinito invece che sul numero. Nessun profilo
/// di questa macchina esporta una variabile così, ed è dichiarato invece che
/// scoperto dopo.
fn python_int(s: &str) -> Option<i128> {
    let t = python_trim(s);
    let (negative, digits) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if digits.is_empty() || digits.starts_with('_') || digits.ends_with('_') {
        return None;
    }
    let mut value: i128 = 0;
    let mut previous_underscore = false;
    for c in digits.chars() {
        if c == '_' {
            if previous_underscore {
                return None;
            }
            previous_underscore = true;
            continue;
        }
        previous_underscore = false;
        let d = c.to_digit(10)? as i128;
        // Un numero enorme non è un errore per Python: qui si satura, perché
        // sopra ogni trascrizione immaginabile il comportamento è identico.
        value = value.saturating_mul(10).saturating_add(d);
    }
    Some(if negative { -value } else { value })
}

/// Il motivo di una riga: chiave stabile, non un pezzo della frase emessa.
///
/// Prima il freno si attivava cercando la parola «passalo» dentro il testo, e il
/// banco di prova classificava cercando «esiste già»: riscrivere un messaggio
/// avrebbe spento il freno in silenzio, senza che nessuna prova se ne accorgesse.
pub const COMPETENZA: &str = "competenza";
pub const CHIARIRE: &str = "chiarire";
pub const PASSARE: &str = "passare";

/// Le righe da aggiungere, come coppie (motivo, testo). Vuoto è il caso normale.
///
/// `exists` risponde `None` quando il catalogo è inutilizzabile in un modo che
/// in Python solleva — `name in 5` è un `TypeError`. Lì l'eccezione risale fino
/// al `try` di `main()`, che esce 0 senza stampare, senza registrare e senza
/// scrivere il marcatore: qui è lo stesso `None` restituito da questa funzione.
pub fn lines(
    prompt: &str,
    mb: u64,
    already_said: bool,
    threshold: u64,
    exists: &mut dyn FnMut(&str) -> Option<bool>,
) -> Option<Vec<(&'static str, String)>> {
    let mut out: Vec<(&'static str, String)> = Vec::new();
    let p = python_trim(prompt);
    if p.is_empty() {
        return Some(out);
    }

    let is_ack = ack().is_match(p);

    // 1. La competenza che copre questo lavoro. Al massimo due.
    if !is_ack {
        let mut matched: Vec<&Skill> = Vec::new();
        for (skill, re) in SKILLS.iter().zip(skill_regexes()) {
            if re.is_match(p) {
                matched.push(skill);
            }
            if matched.len() == 2 {
                break;
            }
        }
        for skill in matched {
            if !exists(skill.name)? {
                continue;
            }
            // Constatazione, non ordine. Fino al 12/08/2026 la riga diceva
            // «invoca `{name}` PRIMA di rispondere», e con quella formulazione
            // non si può misurare niente: se la competenza parte, non si sa se
            // perché serviva o perché era stato ordinato.
            out.push((
                COMPETENZA,
                format!(
                    "Esiste `{}`: copre {}, con la procedura già scritta e provata.",
                    skill.name, skill.when
                ),
            ));
        }
    }

    // 2. La richiesta è vaga E apre lavoro: prima si capisce, poi si costruisce.
    //    Senza il secondo pezzo, «non so» dentro una lamentela sul passato
    //    innescava un invito a indagare su niente.
    if !is_ack && vague().is_match(p) && task().is_match(p) {
        // Fino al 19/08/2026 questa riga diceva «invoca `indaga`», e `indaga` è
        // in quarantena dal 12/08: non è nel catalogo, quindi il rimando mandava
        // a una competenza che non esiste. Non passava da `exists` perché è
        // testo fisso, non una voce della tavola — 81 righe di registro fino al
        // 18/08 hanno nominato una porta murata. Qui resta la prescrizione del
        // CLAUDE.md, che non dipende da nessuna competenza installata.
        out.push((
            CHIARIRE,
            "La richiesta lascia spazio a letture diverse: restituisci l'elenco \
puntato dei cambiamenti previsti prima di toccare qualcosa."
                .to_string(),
        ));
    }

    // 3. La sessione è già cara e sta per prendersi altro lavoro. Una volta sola.
    //    Una domanda non è lavoro nuovo, e non va passata a nessuno: «hai testato
    //    e verificato in ui?» contiene un verbo di compito al participio.
    let is_question = p.trim_end().ends_with('?');
    if mb >= threshold && !already_said && !is_question && (task().is_match(p) || is_ack) {
        // `Agent` e `SendMessage` sono strumenti e ci sono sempre; `handoff` è
        // una competenza, e una competenza va chiesta al catalogo come tutte le
        // altre. Fino al 19/08/2026 era nominata a scatola chiusa, ed è
        // esattamente il modo in cui il rimando a `indaga` è sopravvissuto a sé
        // stesso per una settimana.
        let closing = if exists("handoff")? {
            " Se invece va chiusa, la consegna si scrive con `handoff`."
        } else {
            ""
        };
        out.push((
            PASSARE,
            format!(
                "Questa sessione ha già prodotto {mb} MB di registro, nella fascia che \
consuma l'82% della cache riletta. Se il lavoro che arriva è separabile, passalo: \
`Agent` per un contesto nuovo, `SendMessage` per una sessione già viva.{closing}"
            ),
        ));
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// L'oracolo del catalogo nei casi in cui non è il catalogo a essere in
    /// prova: tutto esiste, come quando `catalogo_skill` è illeggibile.
    fn all_exist(_: &str) -> Option<bool> {
        Some(true)
    }

    fn said(prompt: &str, mb: u64) -> Vec<(&'static str, String)> {
        lines(prompt, mb, false, DEFAULT_THRESHOLD_MB, &mut all_exist).unwrap()
    }

    fn reasons(prompt: &str, mb: u64) -> Vec<&'static str> {
        said(prompt, mb).into_iter().map(|(r, _)| r).collect()
    }

    /// Nessuna riga può nominare una competenza senza averla chiesta al
    /// catalogo. Il difetto vero, corretto il 19/08/2026: la riga che invita a
    /// chiarire diceva «invoca `indaga`», e `indaga` è in quarantena dal 12/08 —
    /// il rimando non passava da `exists` perché era testo fisso, e per una
    /// settimana ha mandato a una porta murata (81 righe di registro).
    #[test]
    fn no_line_names_a_skill_that_was_never_looked_up() {
        let asked = std::cell::RefCell::new(Vec::<String>::new());
        let mut spy = |name: &str| -> Option<bool> {
            asked.borrow_mut().push(name.to_string());
            Some(true)
        };
        // Vaga, apre lavoro, e la sessione è già cara: le tre righe insieme.
        let rows = lines(
            "sistema un po' tutto e poi implementa la scheda",
            DEFAULT_THRESHOLD_MB,
            false,
            DEFAULT_THRESHOLD_MB,
            &mut spy,
        )
        .unwrap();
        assert!(rows.len() >= 2, "servono almeno due righe: {rows:?}");

        // Gli strumenti dell'harness non sono competenze e non stanno in nessun
        // catalogo: sono l'elenco chiuso delle eccezioni.
        let tools = ["Agent", "SendMessage"];
        let looked_up = asked.borrow().clone();
        for (reason, text) in &rows {
            for name in quoted(text) {
                if tools.contains(&name.as_str()) {
                    continue;
                }
                assert!(
                    looked_up.contains(&name),
                    "la riga «{reason}» nomina `{name}` senza chiederlo al catalogo"
                );
            }
        }
    }

    /// I nomi fra apici rovesci di una riga.
    fn quoted(text: &str) -> Vec<String> {
        let re = Regex::new(r"`([^`]+)`").unwrap();
        re.captures_iter(text)
            .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
            .collect()
    }

    /// Una competenza che il catalogo non ha non si nomina, nemmeno nelle righe
    /// scritte a mano: senza questo, `handoff` tornerebbe a essere citata a
    /// scatola chiusa il giorno che sparisce.
    #[test]
    fn a_missing_skill_is_not_named_at_all() {
        let mut none_exists = |_: &str| Some(false);
        let rows = lines(
            "sistema un po' tutto e poi implementa la scheda",
            DEFAULT_THRESHOLD_MB,
            false,
            DEFAULT_THRESHOLD_MB,
            &mut none_exists,
        )
        .unwrap();
        for (reason, text) in &rows {
            assert!(
                !text.contains("handoff"),
                "la riga «{reason}» nomina una competenza che il catalogo non ha: {text}"
            );
        }
    }

    /// Se questo si rompe, il gancio non parte proprio: una regex storta è un
    /// panico all'avvio, e il binario porta anche gli altri ventidue ganci.
    #[test]
    fn every_pattern_compiles() {
        assert_eq!(skill_regexes().len(), SKILLS.len());
        let _ = (vague(), task(), ack());
    }

    /// Il caso normale è il silenzio: se si rompe, il gancio diventa rumore a
    /// ogni turno e smette di essere letto.
    #[test]
    fn an_ordinary_request_stays_mute() {
        assert!(said("aggiorna la riga 12 di questo file", 1).is_empty());
        assert!(said("procedi", 1).is_empty());
        assert!(said("", 1).is_empty());
        assert!(said("   \n ", 1).is_empty());
    }

    /// I quattro separatori che `trim()` non conosce e `str.strip()` sì.
    #[test]
    fn the_control_separators_count_as_blank() {
        assert_eq!(python_trim("\u{1c}\u{1f} ciao \u{1e}"), "ciao");
        assert!(said("\u{1c}\u{1d}\u{1e}\u{1f}", 1).is_empty());
    }

    /// Lo schema più sbagliato di tutti: «CI» e «test» compaiono in continuazione
    /// in frasi che non chiedono di sistemare niente.
    #[test]
    fn the_pipeline_pattern_keeps_its_counter_examples() {
        let hits = |t: &str| said(t, 1).iter().any(|(_, s)| s.contains("harness:ci"));
        for good in [
            "i controlli automatici sono rossi, guarda",
            "la CI è rossa su suite",
            "la pipeline è rossa",
            "i test sono rossi",
            "la build è fallita di nuovo",
            "il workflow è in errore",
            "i test falliscono",
            "la CI è rotta",
            "la pipeline non passa",
        ] {
            assert!(hits(good), "doveva agganciare: {good}");
        }
        for bad in [
            "archivialo, e mergia appena la CI è verde",
            "aspetto che la CI sia verde poi merge",
            "ci sono tre file da sistemare",
            "scrivi i test per questa funzione",
            "fai il build della app",
            "sys.exit(1) quando ci sono falliti",
            "ci sono rossi e verdi nella tavolozza",
        ] {
            assert!(!hits(bad), "doveva ignorare: {bad}");
        }
    }

    /// Le due voci riscritte perché la crate `regex` non ha gli sguardi: se la
    /// riscrittura fosse storta, tornerebbero ad agganciare i percorsi.
    #[test]
    fn the_rewritten_lookarounds_still_ignore_paths() {
        let hits = |t: &str, name: &str| said(t, 1).iter().any(|(_, s)| s.contains(name));
        assert!(!hits("- src/mastra/workflows/steps/file/merge-items.ts", "`mastra`"));
        assert!(!hits("matching-engine usa Mastra e Prisma", "`mastra`"));
        assert!(hits("fammi uno step mastra per la normalizzazione", "`mastra`"));
        assert!(hits("con mastra si fa prima", "`mastra`"));
        assert!(!hits("con mastra.ts si fa prima", "`mastra`"));
        assert!(!hits("src/generated/prisma/** è codice generato, escludilo", "prisma-cli"));
        assert!(!hits("rilegge prisma.job_offers direttamente", "prisma-cli"));
        assert!(hits("lancia prisma migrate sul database di prova", "prisma-cli"));
        // Il primo carattere del testo: senza `^` nell'alternativa, una richiesta
        // che comincia con lo schema resterebbe muta.
        assert!(hits("prisma migrate sul database di prova", "prisma-cli"));
    }

    /// Al massimo due, e la frase di prova deve agganciarne davvero più di due —
    /// altrimenti il tetto passerebbe anche cancellandolo.
    #[test]
    fn at_most_two_skills_at_a_time() {
        let many = "fai un piano, poi una code review, e verifica la coerenza fra i tre repo";
        let hit = skill_regexes().iter().filter(|r| r.is_match(many)).count();
        assert!(hit > 2, "la frase aggancia solo {hit} schemi");
        assert_eq!(
            said(many, 1).iter().filter(|(r, _)| *r == COMPETENZA).count(),
            2
        );
    }

    /// Una voce che nomina qualcosa di non raggiungibile smette di parlare, non
    /// dà errore. È successo due volte, e nessuna prova se n'era accorta.
    #[test]
    fn an_uninstalled_skill_goes_silent_instead_of_speaking() {
        let mut none = |_: &str| Some(false);
        let out = lines("i test sono rossi", 1, false, 6, &mut none).unwrap();
        assert!(out.is_empty());
        // Catalogo inutilizzabile: il gancio si ferma del tutto.
        let mut broken = |_: &str| None;
        assert!(lines("i test sono rossi", 1, false, 6, &mut broken).is_none());
    }

    #[test]
    fn the_vague_request_needs_real_work_next_to_it() {
        assert!(reasons("sistema un po' le cose", 1).contains(&CHIARIRE));
        assert!(reasons("sistemiamo e puliamo allora", 1).contains(&CHIARIRE));
        assert!(!reasons("correggi il typo a riga 4", 1).contains(&CHIARIRE));
        assert!(!reasons("ma dovevi aprirmelo qui non so dove l hai aperto", 1).contains(&CHIARIRE));
        assert!(!reasons("non so se ti risulta lo stesso errore", 1).contains(&CHIARIRE));
    }

    #[test]
    fn the_cost_threshold_speaks_only_above_itself() {
        assert!(!reasons("implementa la funzione", 1).contains(&PASSARE));
        assert!(reasons("implementa la funzione", 20).contains(&PASSARE));
        assert!(reasons("procedi", 20).contains(&PASSARE));
        assert!(!reasons("quanto pesa la cartella?", 20).contains(&PASSARE));
        assert!(!reasons("hai testato e verificato in ui?", 20).contains(&PASSARE));
        // Il freno: già detto una volta, non lo ripete.
        let out = lines("implementa la funzione", 20, true, 6, &mut all_exist).unwrap();
        assert!(!out.iter().any(|(r, _)| *r == PASSARE));
    }

    /// L'ambiente storto non deve spostare la soglia in un posto qualunque.
    #[test]
    fn the_threshold_from_the_environment_falls_back_on_six() {
        assert_eq!(threshold_mb(None), 6);
        assert_eq!(threshold_mb(Some("")), 6);
        assert_eq!(threshold_mb(Some("abc")), 6);
        assert_eq!(threshold_mb(Some("0")), 6);
        assert_eq!(threshold_mb(Some("-3")), 6);
        assert_eq!(threshold_mb(Some("7.5")), 6);
        assert_eq!(threshold_mb(Some("20")), 20);
        assert_eq!(threshold_mb(Some(" 7 ")), 7);
        assert_eq!(threshold_mb(Some("+7")), 7);
        assert_eq!(threshold_mb(Some("1_0")), 10);
    }
}
