//! Il governo della memoria: chi frena, in quale ordine, e cosa non si tocca
//! mai. Chi legge `vm_stat`, i file di swap e il disco sta in
//! `claude-hooks/src/memory_governor.rs`, per lo stesso motivo di
//! `guards::ronda_trigger`: le soglie e la gerarchia si provano senza macchina
//! sotto, altrimenti l'unico modo di collaudare un freno è saturare davvero i
//! 18 GB — che è il guasto che questo modulo esiste per evitare.
//!
//! LA FRASE CHE COMANDA TUTTO, da Theo il 24/08/2026: **la macchina non è
//! dedicata a noi**. Ci lavora una persona, col browser e la posta aperti. Un
//! governo che libera memoria chiudendo le sue applicazioni ha fallito anche
//! se il numero migliora. Da qui la gerarchia in `judge`: prima il nostro
//! lavoro, poi il nostro lavoro che dorme, e mai il suo.

use crate::shell::split_words;

/// Quanta memoria c'è, per quel poco che questa macchina lascia leggere.
///
/// Ogni campo è `Option` di proposito: `None` vuol dire «non sono riuscito a
/// misurarlo», che non è zero e non è calma. Misurato il 24/08/2026 dentro il
/// sandbox di Claude Code: `vm_stat` risponde, `sysctl vm.swapusage` e
/// `kern.memorystatus_vm_pressure_level` rispondono *Operation not permitted*,
/// `ps` e `pgrep` pure. Un governo che tratti quel diniego come «tutto bene» è
/// il controllo che mente, ed è peggio di nessun controllo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reading {
    /// Pagine libere, in MB. **Non è la memoria disponibile**, e su macOS non
    /// ci somiglia: resta vicino a zero anche a macchina scarica, perché il
    /// kernel tiene la RAM piena di cache. Si conserva perché nel messaggio dice
    /// qualcosa a chi legge, ma non vota più — vedi `available_mb`.
    pub free_mb: Option<u64>,
    /// La memoria che il kernel può restituire subito, in MB: le pagine libere
    /// più inattive, speculative e purgeable. **È questa che vota.**
    pub available_mb: Option<u64>,
    /// Quanto tiene il compressore, in MB.
    pub compressor_mb: Option<u64>,
    /// La RAM fisica, in MB.
    pub total_mb: Option<u64>,
    /// Lo spazio di swap che il kernel ha già allocato su disco, in MB —
    /// contato sui file in `/System/Volumes/VM`, che il sandbox lascia
    /// elencare. macOS allarga quel file solo quando è pieno, quindi la sua
    /// dimensione è il segnale meno ambiguo che questa macchina offra.
    pub swap_allocated_mb: Option<u64>,
    /// Le fonti che non si sono lasciate leggere, per nome. Viaggiano nel
    /// verdetto: chi legge deve sapere su cosa il governo era cieco.
    pub unreadable: Vec<String>,
}

/// Le soglie, fuori dal codice che decide. Stanno in una struttura e non in
/// costanti perché una soglia che non si può abbassare non si può nemmeno
/// collaudare: per vedere il freno scattare servirebbe saturare la macchina
/// davvero. Stesso motivo di `GM_ETA_MIN` in `guardiano-macchina.sh`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thresholds {
    pub available_tight_mb: u64,
    pub available_critical_mb: u64,
    pub free_tight_mb: u64,
    pub free_critical_mb: u64,
    /// Quota del compressore sulla RAM, in percento.
    pub compressor_tight_pct: u64,
    pub compressor_critical_pct: u64,
    pub swap_tight_mb: u64,
    pub swap_critical_mb: u64,
}

impl Default for Thresholds {
    /// I valori vengono dalle due misure del 24/08/2026. Nei due rapporti
    /// Jetsam della notte la memoria libera era 0,31 e 0,17 GiB e il
    /// compressore 6,5 e 8,3 GiB su 18 di RAM (36% e 46%); la mattina dopo, con
    /// la macchina ancora in ginocchio, 18 MB liberi e 9,4 GB di swap su 10,2
    /// allocati. Le soglie «tight» stanno sotto quei numeri, così il freno
    /// arriva prima del pannello e non insieme.
    fn default() -> Self {
        Self {
            // La memoria disponibile, che è il segnale che ha sostituito le
            // pagine libere il 28/08/2026. Un ordine di grandezza sopra, perché
            // misura un'altra cosa: su questa macchina «88 MB liberi» e «4.701
            // MB disponibili» erano lo stesso istante.
            available_tight_mb: 1536,
            available_critical_mb: 512,
            free_tight_mb: 300,
            free_critical_mb: 100,
            compressor_tight_pct: 33,
            compressor_critical_pct: 42,
            swap_tight_mb: 8192,
            swap_critical_mb: 10240,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pressure {
    /// Nessun segnale leggibile. Non è calma: è cecità, e si dichiara.
    Unknown,
    Calm,
    Tight,
    Critical,
}

/// La pressione, più il segnale che l'ha decisa. Il nome del segnale viaggia
/// col verdetto perché un freno che dice solo «pressione alta» non si può né
/// contestare né tarare: il 20/08 tre soglie diverse hanno convissuto nello
/// stesso registro e il primo conteggio ne è uscito sbagliato di 11 punti.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PressureVerdict {
    pub level: Pressure,
    /// I segnali che hanno votato per il livello scelto, in italiano.
    pub because: Vec<String>,
    /// Le fonti cieche, ricopiate da `Reading` perché chi riceve il verdetto
    /// non ha in mano la lettura.
    pub blind: Vec<String>,
}

/// Da una lettura al livello di pressione: vince il segnale peggiore.
///
/// Si prende il massimo e non una media perché i tre segnali non misurano la
/// stessa cosa in scale confrontabili — memoria libera, quota del compressore
/// e swap allocato — e mediarli fa sparire l'unico che sta gridando.
pub fn classify(reading: &Reading, t: &Thresholds) -> PressureVerdict {
    let mut level = Pressure::Unknown;
    let mut because = Vec::new();
    let vote = |l: Pressure, why: String, level: &mut Pressure, because: &mut Vec<String>| {
        if l > *level {
            *level = l;
            because.clear();
        }
        if l == *level {
            because.push(why);
        }
    };

    // IL SEGNALE È LA MEMORIA DISPONIBILE, NON QUELLA LIBERA. Fino al
    // 28/08/2026 votavano le pagine libere, e su macOS quel numero sta vicino a
    // zero per costruzione: il kernel riempie la RAM di cache e la restituisce a
    // chi la chiede. Alle 21:00 di quel giorno la macchina aveva 88 MB liberi e
    // 4,7 GB disponibili, il kernel dichiarava «pressione normale», e il freno
    // negava ogni compilazione da tre ore. Le pagine libere restano nel
    // messaggio, perché a chi legge dicono qualcosa; non decidono più.
    if let Some(available) = reading.available_mb {
        let l = if available < t.available_critical_mb {
            Pressure::Critical
        } else if available < t.available_tight_mb {
            Pressure::Tight
        } else {
            Pressure::Calm
        };
        let libere = match reading.free_mb {
            Some(free) => format!(", di cui {free} libere"),
            None => String::new(),
        };
        vote(
            l,
            format!("memoria disponibile {available} MB{libere}"),
            &mut level,
            &mut because,
        );
    } else if let Some(free) = reading.free_mb {
        // Senza il conto della disponibile si torna al segnale vecchio: è meno
        // buono, ma è meglio della cecità — e il messaggio dice quale dei due
        // ha parlato, perché chi legge un diniego deve poterlo pesare.
        let l = if free < t.free_critical_mb {
            Pressure::Critical
        } else if free < t.free_tight_mb {
            Pressure::Tight
        } else {
            Pressure::Calm
        };
        vote(
            l,
            format!("memoria libera {free} MB (disponibile non misurata)"),
            &mut level,
            &mut because,
        );
    }

    // La quota, non i megabyte: 5 GB di compressore su 18 di RAM e su 64
    // raccontano due macchine diverse, e la soglia scritta in megabyte
    // scadrebbe al primo cambio di macchina.
    if let (Some(comp), Some(total)) = (reading.compressor_mb, reading.total_mb) {
        if total > 0 {
            let pct = comp * 100 / total;
            let l = if pct >= t.compressor_critical_pct {
                Pressure::Critical
            } else if pct >= t.compressor_tight_pct {
                Pressure::Tight
            } else {
                Pressure::Calm
            };
            vote(
                l,
                format!("compressore al {pct}% della RAM ({comp} MB)"),
                &mut level,
                &mut because,
            );
        }
    }

    // LO SWAP ALLOCATO NON TORNA INDIETRO, quindi da solo non può negare.
    // È la traccia del picco peggiore da quando la macchina è accesa, non lo
    // stato di adesso: il 28/08/2026 è sceso da 23,5 a 12,3 GB chiudendo
    // applicazioni, e sarebbe rimasto sopra la soglia critica per il resto della
    // giornata mentre il kernel dichiarava «pressione normale». Un segnale che
    // sale e non scende, se vota `Critical`, spegne il lavoro fino al riavvio.
    //
    // Resta un segnale, e al massimo dei tre concorre: la macchina che ha già
    // scritto tanto su disco è una macchina da trattare con riguardo. Ma il
    // veto lo danno i due che sanno tornare indietro — la memoria disponibile e
    // il compressore.
    if let Some(swap) = reading.swap_allocated_mb {
        let l = if swap >= t.swap_tight_mb {
            Pressure::Tight
        } else {
            Pressure::Calm
        };
        vote(
            l,
            format!("swap allocato {swap} MB (traccia del picco, non dello stato)"),
            &mut level,
            &mut because,
        );
    }

    PressureVerdict {
        level,
        because,
        blind: reading.unreadable.clone(),
    }
}

/// Ogni quanto tornare a misurare, dato l'ultimo livello visto.
///
/// È la risposta a «non deve girare sempre»: il governo è appeso a un innesco
/// che esiste già — un `PreToolUse` su Bash — e in calma lascia scadere un
/// minuto prima di rileggere `vm_stat`, così cento comandi di fila costano una
/// misura sola. Quando la pressione sale la finestra si stringe, perché è lì
/// che un dato di un minuto fa descrive una macchina che non c'è più.
pub fn recheck_after_secs(level: Pressure) -> u64 {
    match level {
        Pressure::Calm => 60,
        Pressure::Tight => 15,
        // Cieco si rimisura spesso: la cecità può essere passeggera, e restare
        // a lungo su «non lo so» è il modo di non accorgersi di niente.
        Pressure::Critical | Pressure::Unknown => 5,
    }
}

/// Serve rileggere? `None` vuol dire che non c'è nessuna misura in memoria.
pub fn should_remeasure(last: Option<(Pressure, u64)>) -> bool {
    match last {
        None => true,
        Some((level, age_secs)) => age_secs >= recheck_after_secs(level),
    }
}

/// Ogni quanto si può ripetere a Theo che la macchina è carica. Senza questo
/// tetto un avviso su ogni comando di shell smette di essere letto in un
/// pomeriggio — già successo con `domande-aperte.md`, dove due giri hanno
/// portato gli avvisi da 2 a 6.
pub const REPORT_COOLDOWN_SECS: u64 = 1800;

pub fn should_report(last_report_age_secs: Option<u64>) -> bool {
    last_report_age_secs.is_none_or(|age| age >= REPORT_COOLDOWN_SECS)
}

/// Che cosa sta per essere eseguito, dal punto di vista della memoria.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Workload {
    /// Nostro e pesante: il primo e quasi sempre l'unico da frenare.
    OurHeavyWork {
        kind: &'static str,
        /// Il parallelismo dichiarato, se c'è.
        jobs: Option<u32>,
    },
    /// Chiude qualcosa cercandolo per nome. Non si fa mai: il 24/08/2026
    /// `pkill -f "cargo mutants"` ha spento due volte il banco di un'altra
    /// sessione, che stava misurando.
    KillsByName { pattern: String },
    /// Tocca un'applicazione di Theo. Non si fa mai, a nessuna pressione.
    TouchesTheirApps { app: String },
    /// Spegne un servizio Docker. Si segnala, non si spegne: sono i suoi
    /// servizi di sviluppo, e `socraticode-qdrant` regge l'indice semantico.
    StopsDockerService { verb: String },
    Other,
}

/// I comandi che chiedono memoria a raffica. L'elenco viene da `BUILD_RE` in
/// `registro-swap.sh`, che il 20/08/2026 ha misurato la contemporaneità dei
/// build dietro ogni dente di sega dello swap.
const HEAVY_CARGO_SUBS: &[&str] = &["build", "check", "clippy", "test", "bench", "doc"];
const HEAVY_BUNDLER_SUBS: &[&str] = &["build", "typecheck"];
const HEAVY_BUNDLERS: &[&str] = &[
    "vite", "esbuild", "rollup", "tsup", "webpack", "next", "turbo", "parcel",
];
const HEAVY_BARE: &[&str] = &["tsc", "rustc", "swc", "tsgo"];

/// Le applicazioni di Theo. Non è un elenco esaustivo e non deve esserlo: chi
/// non è qui dentro passa, perché il costo di frenare un comando innocuo è
/// molto più alto del megabyte che si recupererebbe.
const THEIR_APPS: &[&str] = &[
    "arc",
    "orca",
    "safari",
    "google chrome",
    "chrome",
    "firefox",
    "mail",
    "spark",
    "spotify",
    "music",
    "slack",
    "notion",
    "linear",
    "granola",
    "messages",
    "whatsapp",
    "telegram",
    "figma",
    "zoom",
    "docker desktop",
    "finder",
];

fn base_name(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

/// Il parallelismo dichiarato: `-j 4`, `-j4`, `--jobs 4`, `--jobs=4`.
fn parse_jobs(words: &[String]) -> Option<u32> {
    let mut it = words.iter();
    while let Some(w) = it.next() {
        if let Some(v) = w.strip_prefix("--jobs=") {
            return v.parse().ok();
        }
        if w == "--jobs" || w == "-j" {
            return it.next()?.parse().ok();
        }
        if let Some(v) = w.strip_prefix("-j") {
            if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) {
                return v.parse().ok();
            }
        }
    }
    None
}

/// Il primo argomento che non è un'opzione, dopo il programma.
fn first_subcommand(words: &[String]) -> Option<&str> {
    words
        .iter()
        .skip(1)
        .find(|w| !w.starts_with('-'))
        .map(String::as_str)
}

/// Riconosce un comando. Una virgoletta aperta fa rinunciare `split_words`, e
/// allora si risponde `Other`: su un comando che non si è capito non si frena.
pub fn classify_command(command: &str) -> Workload {
    let Some(words) = split_words(command) else {
        return Workload::Other;
    };
    if words.is_empty() {
        return Workload::Other;
    }
    let program = base_name(&words[0]).to_ascii_lowercase();
    let sub = first_subcommand(&words).map(str::to_ascii_lowercase);
    let jobs = parse_jobs(&words);

    // Chiudere per nome: si guarda il verbo, non chi è la vittima. `pkill -f`
    // e `killall` colpiscono tutto ciò che combacia, compreso il lavoro di
    // un'altra sessione che chi scrive non può vedere.
    if program == "pkill" || program == "killall" {
        let pattern = words
            .iter()
            .skip(1)
            .find(|w| !w.starts_with('-'))
            .cloned()
            .unwrap_or_default();
        // Un'applicazione di Theo nominata qui è il caso peggiore dei due, e
        // merita il messaggio suo.
        if let Some(app) = matched_app(&pattern) {
            return Workload::TouchesTheirApps { app };
        }
        return Workload::KillsByName { pattern };
    }

    // `osascript -e 'quit app "Arc"'` è l'altra porta per chiudere un'app.
    if program == "osascript" || program == "open" {
        if let Some(app) = matched_app(command) {
            return Workload::TouchesTheirApps { app };
        }
    }

    if program == "docker" || program == "podman" || program == "colima" {
        let verb = sub.clone().unwrap_or_default();
        let stops = matches!(verb.as_str(), "stop" | "kill" | "rm" | "down" | "prune")
            || (verb == "compose"
                && words.iter().any(|w| w == "down" || w == "stop" || w == "rm"));
        if stops {
            return Workload::StopsDockerService { verb };
        }
        return Workload::Other;
    }

    if program == "cargo" {
        if sub.as_deref() == Some("mutants") {
            return Workload::OurHeavyWork {
                kind: "banco dei mutanti",
                jobs,
            };
        }
        if sub.as_deref().is_some_and(|s| HEAVY_CARGO_SUBS.contains(&s)) {
            return Workload::OurHeavyWork {
                kind: "compilazione Rust",
                jobs,
            };
        }
        return Workload::Other;
    }

    if HEAVY_BARE.contains(&program.as_str()) {
        return Workload::OurHeavyWork {
            kind: "compilazione",
            jobs,
        };
    }

    // Il sottocomando è obbligatorio: `vite build` chiede gigabyte a raffica,
    // `vite dev` è un server acceso da ore che non è lui il dente di sega.
    if HEAVY_BUNDLERS.contains(&program.as_str())
        && sub
            .as_deref()
            .is_some_and(|s| HEAVY_BUNDLER_SUBS.contains(&s))
    {
        return Workload::OurHeavyWork {
            kind: "compilazione del pacchetto",
            jobs,
        };
    }

    // `make -j 8` senza tetto è la stessa moltiplicazione di `cargo mutants -j 4`.
    if program == "make" && jobs.is_some_and(|j| j > 1) {
        return Workload::OurHeavyWork {
            kind: "make in parallelo",
            jobs,
        };
    }

    Workload::Other
}

/// L'applicazione di Theo nominata nel testo, se c'è. Il confronto è su parole
/// intere e non su sottostringhe: `arc` dentro `search` non è il browser, e un
/// riscontro così avrebbe rifiutato comandi innocui.
fn matched_app(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    // Le voci di due parole si cercano tali e quali, le altre come parola sola.
    THEIR_APPS
        .iter()
        .find(|app| {
            if app.contains(' ') {
                lower.contains(*app)
            } else {
                lower
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .any(|w| w == **app)
            }
        })
        .map(|app| (*app).to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Passa in silenzio.
    Pass,
    /// Passa, ma con una riga a chi scrive.
    Notice(String),
    /// Non passa, con il motivo e cosa fare invece.
    Refuse(String),
}

/// La gerarchia, nell'ordine dettato da Theo il 24/08/2026.
///
/// 1. Il nostro lavoro si frena per primo, sempre.
/// 2. Poi ciò che è nostro e dorme (lo raccoglie il guardiano periodico).
/// 3. Le applicazioni di Theo non si toccano mai: si dice che pesano.
/// 4. I container Docker si segnalano, non si spengono.
///
/// I punti 3 e 4 valgono a qualunque pressione, anche a macchina in ginocchio:
/// sono divieti, non tarature. Il punto 1 è l'unico che dipende dalla misura.
pub fn judge(command: &str, pressure: &PressureVerdict) -> Verdict {
    match classify_command(command) {
        Workload::TouchesTheirApps { app } => Verdict::Refuse(format!(
            "governo della memoria: «{app}» è un'applicazione di Theo e non si chiude da qui, \
             a nessuna pressione. Questa macchina non è dedicata a noi: ci lavora una persona. \
             Se sta pesando davvero, dillo — la decisione è sua."
        )),
        Workload::StopsDockerService { verb } => Verdict::Refuse(format!(
            "governo della memoria: `docker {verb}` spegne un servizio di sviluppo di Theo. \
             Si segnala, non si spegne — `socraticode-qdrant` regge l'indice semantico che \
             ha appena scelto per la memoria. Scrivi che pesa e lascia decidere lui."
        )),
        Workload::KillsByName { pattern } => Verdict::Refuse(format!(
            "governo della memoria: chiudere per nome colpisce anche il lavoro di un'altra \
             sessione. Il 24/08/2026 `pkill -f \"cargo mutants\"` ha spento due volte il banco \
             di un builder che stava misurando. Si ferma un pid conosciuto: `kill <pid>`, col \
             pid letto da chi ha avviato il lavoro (per il banco: `-o <cartella>/mutants.out`). \
             Riscontro cercato: {pattern:?}."
        )),
        Workload::OurHeavyWork { kind, jobs } => heavy_work_verdict(kind, jobs, pressure),
        Workload::Other => Verdict::Pass,
    }
}

/// Il solo ramo che guarda la misura. Frena il nostro lavoro, e nient'altro.
fn heavy_work_verdict(kind: &str, jobs: Option<u32>, pressure: &PressureVerdict) -> Verdict {
    let signals = pressure.because.join(", ");
    match pressure.level {
        // Cieco non è calmo. Non si blocca — un governo che ferma il lavoro
        // perché non sa misurare è la sua stessa avaria moltiplicata — ma lo
        // si dice forte, ogni volta, invece di tacere.
        Pressure::Unknown => Verdict::Notice(format!(
            "governo della memoria: NON HO POTUTO MISURARE la pressione ({}). \
             Sto per lasciar partire {kind} alla cieca: guarda tu `vm_stat` prima di insistere.",
            if pressure.blind.is_empty() {
                "nessuna fonte leggibile".to_string()
            } else {
                pressure.blind.join(", ")
            }
        )),
        Pressure::Critical => Verdict::Refuse(format!(
            "governo della memoria: NON ADESSO. {signals}. \
             {kind} è lavoro nostro, e il nostro si ferma per primo: la notte del 24/08/2026 \
             `cargo mutants -j 4` ha esaurito lo swap e il kernel ha sospeso Arc e Orca a Theo. \
             Aspetta che la pressione scenda, poi riparti a `-j 1`."
        )),
        Pressure::Tight => match jobs {
            Some(1) => Verdict::Notice(format!(
                "governo della memoria: macchina carica ({signals}). {kind} a `-j 1` passa, \
                 ma non lanciarne un secondo in parallelo."
            )),
            _ => Verdict::Refuse(format!(
                "governo della memoria: macchina carica ({signals}). \
                 {kind} qui va a `-j 1` esplicito, non al parallelismo di default: misurato il \
                 24/08/2026 sullo stesso lotto, `-j 4` fa 2.443 MB di picco e `-j 1` ne fa 956. \
                 Rilancia lo stesso comando con `-j 1`."
            )),
        },
        Pressure::Calm => match jobs {
            // Il banco resta un caso a parte anche a macchina scarica: `-j 4`
            // moltiplica per quattro un difetto che da solo già bastava.
            Some(j) if j > 1 && kind == "banco dei mutanti" => Verdict::Notice(format!(
                "governo della memoria: la macchina ora è tranquilla, ma il banco a `-j {j}` è \
                 ciò che ha messo in ginocchio il Mac la notte del 24/08/2026. `-j 1` è il \
                 valore di casa; se ti serve di più, resta a guardare l'RSS."
            )),
            _ => Verdict::Pass,
        },
    }
}

/// Un processo nostro, con quel che serve a decidere se dorme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OurProcess {
    pub pid: u32,
    pub name: String,
    pub rss_mb: u64,
    /// Quota di un core, in percento e per intero: 100 vuol dire un core pieno.
    pub cpu_pct: u64,
    pub age_secs: u64,
    /// C'è ancora, risalendo la catena dei genitori, una sessione `claude`
    /// viva che l'ha avviato? È la domanda giusta, e **non** è «il genitore è
    /// `launchd`»: misurato il 24/08/2026, il server MCP che bruciava un core
    /// da undici ore e trentanove aveva `ppid` diverso da 1 e un `claude` vivo
    /// due gradini più su. Un rilevatore fermo a `ppid == 1` l'avrebbe
    /// mancato; uno che l'avesse chiuso avrebbe rotto una sessione viva.
    pub owner_alive: bool,
}

/// Cosa farne. Nessuna variante chiude niente: la più forte propone un `kill`
/// su un pid che ha già in mano, e il gesto resta di chi legge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reclaim {
    /// Nessuna sessione lo rivendica più: si può chiudere, ed è il livello 2
    /// della gerarchia — nostro, e dorme.
    OrphanedByUs(String),
    /// Ha un padrone vivo ma consuma da ore. Si dice e basta: chiudere il
    /// server di chi sta lavorando è molto peggio dei megabyte che libera.
    LoudButOwned(String),
}

/// Da quanto un processo nostro va lasciato in pace prima di chiamarlo
/// abbandonato. Mezz'ora: sotto, è più probabile che sia lavoro appena
/// avviato di cui non abbiamo ancora visto il padrone.
pub const ORPHAN_MIN_AGE_SECS: u64 = 1800;
/// E le due soglie per «rumoroso ma di qualcuno»: un core mezzo occupato, per
/// più di due ore. Il caso del 24/08 stava a un core pieno da 11h39.
pub const LOUD_MIN_CPU_PCT: u64 = 50;
pub const LOUD_MIN_AGE_SECS: u64 = 7200;

/// Il livello 2 della gerarchia: ciò che è nostro e dorme.
pub fn sleeping_work(procs: &[OurProcess]) -> Vec<Reclaim> {
    let mut out = Vec::new();
    for p in procs {
        let hours = p.age_secs / 3600;
        let minutes = (p.age_secs % 3600) / 60;
        if !p.owner_alive && p.age_secs >= ORPHAN_MIN_AGE_SECS {
            out.push(Reclaim::OrphanedByUs(format!(
                "{} (pid {}) — {} MB, vivo da {hours}h{minutes:02}, nessuna sessione lo \
                 rivendica. Si chiude con `kill {}`, mai con `pkill -f`.",
                p.name, p.pid, p.rss_mb, p.pid
            )));
        } else if p.owner_alive && p.cpu_pct >= LOUD_MIN_CPU_PCT && p.age_secs >= LOUD_MIN_AGE_SECS
        {
            out.push(Reclaim::LoudButOwned(format!(
                "{} (pid {}) — {} MB e {}% di un core da {hours}h{minutes:02}, ma la sessione \
                 che l'ha avviato è viva: non si tocca, si dice.",
                p.name, p.pid, p.rss_mb, p.cpu_pct
            )));
        }
    }
    out
}

/// La riga per Theo quando la macchina è carica per colpa di qualcosa che non
/// possiamo toccare. Non chiude niente e non propone di chiudere: dice che
/// pesa, e chi decide è lui.
pub fn heavy_neighbours_notice(pressure: &PressureVerdict, top: &[(String, u64)]) -> Option<String> {
    if pressure.level < Pressure::Tight || top.is_empty() {
        return None;
    }
    let listed = top
        .iter()
        .map(|(name, mb)| format!("{name} {mb} MB"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "governo della memoria: la macchina è carica ({}). Non è lavoro nostro: {listed}. \
         Non tocco niente di tuo — se vuoi liberare, la scelta è tua.",
        pressure.because.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calm() -> PressureVerdict {
        PressureVerdict {
            level: Pressure::Calm,
            because: vec!["memoria libera 4000 MB".into()],
            blind: vec![],
        }
    }
    fn tight() -> PressureVerdict {
        PressureVerdict {
            level: Pressure::Tight,
            because: vec!["swap allocato 9216 MB".into()],
            blind: vec![],
        }
    }
    fn critical() -> PressureVerdict {
        PressureVerdict {
            level: Pressure::Critical,
            because: vec!["memoria libera 18 MB".into()],
            blind: vec![],
        }
    }

    #[test]
    fn the_worst_signal_decides_the_level() {
        // Memoria disponibile abbondante, ma il compressore è oltre la soglia
        // critica: mediare i tre segnali avrebbe fatto sparire quello che grida.
        let r = Reading {
            free_mb: Some(4000),
            available_mb: Some(6000),
            compressor_mb: Some(8000),
            total_mb: Some(18432),
            swap_allocated_mb: Some(2048),
            unreadable: vec![],
        };
        let v = classify(&r, &Thresholds::default());
        assert_eq!(v.level, Pressure::Critical);
        assert_eq!(
            v.because,
            vec!["compressore al 43% della RAM (8000 MB)".to_string()]
        );
    }

    #[test]
    fn the_morning_of_24_08_reads_critical() {
        // I numeri veri di quella mattina: 18 MB liberi, 5,3 GB nel
        // compressore, swap a 9,4 su 10,2 GB. Se questa lettura non fosse
        // critica, il governo non servirebbe a niente.
        //
        // La memoria disponibile non c'è, e non si inventa: quel giorno non la
        // misurava nessuno. Vale quindi il ripiego sulle pagine libere, ed è la
        // prova che il ripiego funziona — su una lettura vera, non costruita.
        let r = Reading {
            free_mb: Some(18),
            available_mb: None,
            compressor_mb: Some(5300),
            total_mb: Some(18432),
            swap_allocated_mb: Some(10240),
            unreadable: vec![],
        };
        assert_eq!(
            classify(&r, &Thresholds::default()).level,
            Pressure::Critical
        );
    }

    #[test]
    fn a_quiet_machine_reads_calm() {
        let r = Reading {
            free_mb: Some(6000),
            available_mb: Some(9000),
            compressor_mb: Some(2000),
            total_mb: Some(18432),
            swap_allocated_mb: Some(2048),
            unreadable: vec![],
        };
        let v = classify(&r, &Thresholds::default());
        assert_eq!(v.level, Pressure::Calm);
    }

    /// LA LETTURA CHE IL 28/08/2026 BLOCCAVA TUTTO, e non doveva. Ottantotto
    /// megabyte liberi su una macchina con quattro giga e mezzo disponibili e il
    /// kernel che dichiarava «pressione normale»: il freno leggeva le pagine
    /// libere, che su macOS stanno vicino a zero per costruzione, e negava ogni
    /// compilazione.
    #[test]
    fn few_free_pages_with_plenty_available_is_not_pressure() {
        let r = Reading {
            free_mb: Some(88),
            available_mb: Some(4701),
            compressor_mb: Some(5295),
            total_mb: Some(18432),
            swap_allocated_mb: Some(12288),
            unreadable: vec![],
        };
        assert_eq!(classify(&r, &Thresholds::default()).level, Pressure::Tight);
    }

    /// E il contrario, che è il caso per cui il freno esiste: poca memoria
    /// disponibile davvero, non solo poche pagine libere.
    #[test]
    fn little_available_memory_is_still_critical() {
        let r = Reading {
            free_mb: Some(88),
            available_mb: Some(200),
            compressor_mb: Some(2000),
            total_mb: Some(18432),
            ..Default::default()
        };
        assert_eq!(classify(&r, &Thresholds::default()).level, Pressure::Critical);
    }

    /// Lo swap allocato è la traccia del picco peggiore da quando la macchina è
    /// accesa, e non scende: se negasse da solo, spegnerebbe il lavoro fino al
    /// riavvio. Concorre, non decide.
    #[test]
    fn high_allocated_swap_alone_never_refuses() {
        let r = Reading {
            available_mb: Some(9000),
            compressor_mb: Some(2000),
            total_mb: Some(18432),
            swap_allocated_mb: Some(20480),
            ..Default::default()
        };
        assert_eq!(classify(&r, &Thresholds::default()).level, Pressure::Tight);
    }

    #[test]
    fn nothing_readable_is_unknown_not_calm() {
        let r = Reading {
            unreadable: vec!["vm_stat".into()],
            ..Default::default()
        };
        let v = classify(&r, &Thresholds::default());
        assert_eq!(v.level, Pressure::Unknown);
        assert_eq!(v.blind, vec!["vm_stat".to_string()]);
    }

    #[test]
    fn a_zero_sized_machine_does_not_divide_by_zero() {
        let r = Reading {
            compressor_mb: Some(500),
            total_mb: Some(0),
            ..Default::default()
        };
        assert_eq!(classify(&r, &Thresholds::default()).level, Pressure::Unknown);
    }

    #[test]
    fn the_compressor_is_judged_as_a_share_not_in_megabytes() {
        // Gli stessi 6 GB: su 18 di RAM sono il 33%, su 64 il 9%. Una soglia
        // in megabyte avrebbe dato lo stesso verdetto a due macchine diverse.
        let t = Thresholds::default();
        let small = Reading {
            compressor_mb: Some(6200),
            total_mb: Some(18432),
            ..Default::default()
        };
        let big = Reading {
            compressor_mb: Some(6200),
            total_mb: Some(65536),
            ..Default::default()
        };
        assert_eq!(classify(&small, &t).level, Pressure::Tight);
        assert_eq!(classify(&big, &t).level, Pressure::Calm);
    }

    #[test]
    fn lowering_a_threshold_moves_the_verdict() {
        // La prova che le soglie sono davvero la leva: stessa lettura, due
        // tarature, due verdetti. Senza questo, un collaudo del freno
        // pretenderebbe di saturare i 18 GB veri.
        let r = Reading {
            available_mb: Some(2000),
            ..Default::default()
        };
        let base = Thresholds::default();
        assert_eq!(classify(&r, &base).level, Pressure::Calm);
        let stricter = Thresholds {
            available_tight_mb: 3000,
            ..base
        };
        assert_eq!(classify(&r, &stricter).level, Pressure::Tight);
    }

    #[test]
    fn the_cadence_widens_when_calm_and_tightens_when_loaded() {
        assert_eq!(recheck_after_secs(Pressure::Calm), 60);
        assert_eq!(recheck_after_secs(Pressure::Tight), 15);
        assert_eq!(recheck_after_secs(Pressure::Critical), 5);
        // Cieco si rimisura come critico, non come calmo.
        assert_eq!(recheck_after_secs(Pressure::Unknown), 5);
    }

    #[test]
    fn a_fresh_calm_reading_is_not_measured_again() {
        assert!(!should_remeasure(Some((Pressure::Calm, 30))));
        assert!(should_remeasure(Some((Pressure::Calm, 60))));
        // Sotto pressione la stessa età di 30 s è già vecchia.
        assert!(should_remeasure(Some((Pressure::Tight, 30))));
        assert!(should_remeasure(None));
    }

    #[test]
    fn the_report_to_theo_has_a_cooldown() {
        assert!(should_report(None));
        assert!(!should_report(Some(60)));
        assert!(should_report(Some(REPORT_COOLDOWN_SECS)));
    }

    #[test]
    fn the_bench_and_its_parallelism_are_recognised() {
        assert_eq!(
            classify_command("cargo mutants -j 4 -o /tmp/out"),
            Workload::OurHeavyWork {
                kind: "banco dei mutanti",
                jobs: Some(4)
            }
        );
        assert_eq!(
            classify_command("cargo mutants --jobs=2"),
            Workload::OurHeavyWork {
                kind: "banco dei mutanti",
                jobs: Some(2)
            }
        );
        assert_eq!(
            classify_command("cargo mutants -j1"),
            Workload::OurHeavyWork {
                kind: "banco dei mutanti",
                jobs: Some(1)
            }
        );
        assert_eq!(
            classify_command("cargo mutants"),
            Workload::OurHeavyWork {
                kind: "banco dei mutanti",
                jobs: None
            }
        );
    }

    #[test]
    fn rust_and_bundler_builds_count_as_ours() {
        assert!(matches!(
            classify_command("cargo build --release"),
            Workload::OurHeavyWork { .. }
        ));
        assert!(matches!(
            classify_command("tsc --noEmit"),
            Workload::OurHeavyWork { .. }
        ));
        assert!(matches!(
            classify_command("vite build"),
            Workload::OurHeavyWork { .. }
        ));
        // `vite dev` è un server acceso per ore, non una raffica: stessa
        // distinzione che `registro-swap.sh` fa dal 20/08.
        assert_eq!(classify_command("vite dev"), Workload::Other);
        assert_eq!(classify_command("cargo fmt"), Workload::Other);
    }

    #[test]
    fn an_unparsable_command_is_not_braked() {
        // Virgoletta aperta: `split_words` rinuncia, e su un comando che non si
        // è capito non si frena.
        assert_eq!(
            classify_command("cargo mutants -F 'mai chiusa"),
            Workload::Other
        );
        assert_eq!(classify_command(""), Workload::Other);
    }

    #[test]
    fn killing_by_name_is_refused_at_any_pressure() {
        for p in [calm(), tight(), critical()] {
            let v = judge("pkill -f \"cargo mutants\"", &p);
            let Verdict::Refuse(m) = v else {
                panic!(
                    "un pkill per nome deve essere rifiutato, pressione {:?}",
                    p.level
                )
            };
            assert!(m.contains("pid conosciuto"), "{m}");
        }
    }

    #[test]
    fn killing_a_known_pid_is_allowed() {
        assert_eq!(judge("kill 21964", &critical()), Verdict::Pass);
    }

    #[test]
    fn theo_apps_are_never_touched_not_even_when_critical() {
        for cmd in [
            "killall Arc",
            "pkill -f Orca",
            "killall Spotify",
            "osascript -e 'quit app \"Mail\"'",
        ] {
            let Verdict::Refuse(m) = judge(cmd, &critical()) else {
                panic!("{cmd} doveva essere rifiutato")
            };
            assert!(m.contains("applicazione di Theo"), "{cmd}: {m}");
        }
    }

    #[test]
    fn an_app_name_inside_another_word_is_not_the_app() {
        // `arc` dentro `search` non è il browser: un riscontro per
        // sottostringa avrebbe rifiutato un comando innocuo.
        assert_eq!(
            classify_command("pkill -f search-indexer"),
            Workload::KillsByName {
                pattern: "search-indexer".into()
            }
        );
    }

    #[test]
    fn docker_is_reported_never_stopped() {
        for cmd in [
            "docker stop socraticode-qdrant",
            "docker kill gyver-inngest",
            "docker compose down",
            "colima stop",
        ] {
            let Verdict::Refuse(m) = judge(cmd, &critical()) else {
                panic!("{cmd} doveva essere rifiutato")
            };
            assert!(m.contains("Si segnala, non si spegne"), "{cmd}: {m}");
        }
        // Leggere Docker si può, e deve: il governo segnala guardando.
        assert_eq!(judge("docker ps", &critical()), Verdict::Pass);
        assert_eq!(judge("docker stats --no-stream", &critical()), Verdict::Pass);
    }

    #[test]
    fn our_work_is_the_first_thing_braked() {
        let Verdict::Refuse(m) = judge("cargo mutants -j 4", &critical()) else {
            panic!("a pressione critica il banco non parte")
        };
        assert!(m.contains("NON ADESSO"), "{m}");
        assert!(m.contains("si ferma per primo"), "{m}");
    }

    #[test]
    fn under_load_our_work_is_throttled_not_stopped() {
        let Verdict::Refuse(m) = judge("cargo build --release", &tight()) else {
            panic!("a macchina carica una compilazione senza tetto va frenata")
        };
        assert!(m.contains("-j 1"), "{m}");
        // Con il tetto già dichiarato passa, con una riga di avviso.
        let Verdict::Notice(m) = judge("cargo build -j 1", &tight()) else {
            panic!("a `-j 1` deve passare")
        };
        assert!(m.contains("macchina carica"), "{m}");
    }

    #[test]
    fn a_quiet_machine_lets_our_work_through() {
        assert_eq!(judge("cargo build --release", &calm()), Verdict::Pass);
        assert_eq!(judge("tsc --noEmit", &calm()), Verdict::Pass);
        // Il banco in parallelo resta un avviso anche a macchina scarica.
        assert!(matches!(
            judge("cargo mutants -j 4", &calm()),
            Verdict::Notice(_)
        ));
        assert_eq!(judge("cargo mutants -j 1", &calm()), Verdict::Pass);
    }

    #[test]
    fn blindness_is_shouted_never_swallowed() {
        let blind = PressureVerdict {
            level: Pressure::Unknown,
            because: vec![],
            blind: vec!["vm_stat".into(), "/System/Volumes/VM".into()],
        };
        let Verdict::Notice(m) = judge("cargo build", &blind) else {
            panic!("cieco non blocca, ma parla")
        };
        assert!(m.contains("NON HO POTUTO MISURARE"), "{m}");
        assert!(m.contains("vm_stat"), "{m}");
    }

    #[test]
    fn ordinary_commands_are_never_touched() {
        for cmd in ["ls -la", "git status", "rg pattern src/", "cargo fmt --check"] {
            assert_eq!(judge(cmd, &critical()), Verdict::Pass, "{cmd}");
        }
    }

    fn proc(name: &str, cpu_pct: u64, age_secs: u64, owner_alive: bool) -> OurProcess {
        OurProcess {
            pid: 61912,
            name: name.into(),
            rss_mb: 314,
            cpu_pct,
            age_secs,
            owner_alive,
        }
    }

    #[test]
    fn the_socraticode_server_of_24_08_is_named_never_killed() {
        // Il caso vero: `node socraticode`, 314 MB, un core pieno da 11h39 —
        // e un `claude` vivo due gradini sopra. È il caso che smentisce
        // «server MCP orfano»: chiuderlo avrebbe rotto una sessione viva.
        let v = sleeping_work(&[proc("node socraticode", 100, 41_983, true)]);
        let [Reclaim::LoudButOwned(m)] = &v[..] else {
            panic!("con il padrone vivo si segnala e basta, non si propone un kill: {v:?}")
        };
        assert!(m.contains("non si tocca"), "{m}");
        assert!(m.contains("11h39"), "{m}");
    }

    #[test]
    fn the_same_server_without_an_owner_becomes_reclaimable() {
        // Stesso processo, stessa età: cambia solo che nessuno lo rivendica.
        // Se il verdetto non cambiasse, `owner_alive` non servirebbe a niente.
        let v = sleeping_work(&[proc("node socraticode", 100, 41_983, false)]);
        let [Reclaim::OrphanedByUs(m)] = &v[..] else {
            panic!("senza padrone è nostro e dorme: {v:?}")
        };
        assert!(m.contains("kill 61912"), "{m}");
        assert!(m.contains("mai con `pkill -f`"), "{m}");
    }

    #[test]
    fn a_young_orphan_is_left_alone() {
        // Dieci minuti: può essere lavoro appena avviato di cui non abbiamo
        // ancora visto il padrone. Si aspetta.
        assert!(sleeping_work(&[proc("node", 0, 600, false)]).is_empty());
    }

    #[test]
    fn a_quiet_long_lived_server_with_an_owner_says_nothing() {
        // Vecchio ma silenzioso e di qualcuno: non c'è niente da dire, e un
        // guardiano che parla comunque smette di essere letto.
        assert!(sleeping_work(&[proc("node socraticode", 0, 41_983, true)]).is_empty());
        // E rumoroso ma giovane nemmeno.
        assert!(sleeping_work(&[proc("node socraticode", 100, 600, true)]).is_empty());
    }

    #[test]
    fn the_neighbours_notice_names_without_proposing_to_kill() {
        let top = vec![("Arc".to_string(), 3500u64), ("Docker".to_string(), 2100)];
        let m = heavy_neighbours_notice(&tight(), &top).expect("a macchina carica si dice");
        assert!(m.contains("Arc 3500 MB"), "{m}");
        assert!(m.contains("Non tocco niente di tuo"), "{m}");
        // A macchina tranquilla non si dice niente.
        assert_eq!(heavy_neighbours_notice(&calm(), &top), None);
        // E senza vicini da nominare, nemmeno sotto pressione.
        assert_eq!(heavy_neighbours_notice(&tight(), &[]), None);
    }
}
