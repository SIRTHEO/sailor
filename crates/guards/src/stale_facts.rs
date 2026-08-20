//! Le affermazioni datate: una riga che dichiara di aver misurato qualcosa, e
//! la data di quando l'ha misurato, più vecchia della soglia.
//!
//! Porta la metà «misure datate» di `skills/hooks/stale-facts.py`, che il gate
//! della lingua non lascia più eseguire da dentro una sessione: nel sorgente di
//! quello script ci sono percorsi d'esempio dentro stringhe, e il divieto
//! sull'interprete li legge come una scrittura imminente. Il criterio di
//! accettazione ci arrivava lanciandolo da dentro Python, dove i ganci non
//! guardano — così contava 1 affermazione datata senza che nessuno potesse
//! vedere quale, e un rosso che nessuno può ispezionare resta rosso per sempre.
//!
//! Cosa NON porta: la caccia ai percorsi morti, che vive già in
//! `memory_anchor.rs` con una domanda più precisa (è mai esistito qui?).

use regex::Regex;
use std::sync::OnceLock;

pub const DEFAULT_MONTHS: i64 = 3;

/// Le parole che trasformano una frase in una misura: senza una di queste, una
/// data è solo una data — una scadenza, un evento — e non un fatto da riprovare.
///
/// Le radici italiane sono participi troncati e vanno chiuse sulla desinenza,
/// altrimenti `verificat` prende dentro `verificatore`, che è un mestiere: era
/// l'unica segnalazione su tutto `~/.claude` il 18/08/2026, ed era falsa.
const CLAIM_WORDS: [&str; 9] = [
    "misurat",
    "verificat",
    "provat",
    "riprovat",
    "constatat",
    "osservat",
    "measured",
    "verified",
    "checked",
];

/// Le estensioni che rendono una parola «un percorso», al solo scopo di
/// coprirla prima di cercare le date: `2026-08-17-cron-e-soglie.md` porta una
/// data che non è una misura, è il nome del documento.
const PATH_EXTENSIONS: [&str; 21] = [
    "py", "sh", "ts", "tsx", "js", "mjs", "cjs", "md", "json", "yml", "yaml",
    "toml", "sql", "prisma", "html", "css", "jsonl", "rs", "txt", "tsv", "log",
];

fn claim_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let parts: Vec<String> = CLAIM_WORDS
            .iter()
            .map(|w| {
                if w.ends_with("at") {
                    format!("{w}[oaie]\\b")
                } else {
                    format!("\\b{w}\\b")
                }
            })
            .collect();
        Regex::new(&format!("(?i){}", parts.join("|"))).unwrap()
    })
}

fn path_token() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let exts = PATH_EXTENSIONS.join("|");
        Regex::new(&format!(r"https?://\S+|[\w~@.\-/]*[\w~@\-]\.(?:{exts})\b")).unwrap()
    })
}

fn iso_date() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(\d{4})-(\d{2})-(\d{2})\b").unwrap())
}

fn slash_date() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(\d{1,2})/(\d{1,2})/(\d{4})\b").unwrap())
}

/// Una data di calendario, ridotta a ciò che serve: confrontarla e sottrarla.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Date {
    pub year: i64,
    pub month: i64,
    pub day: i64,
}

impl Date {
    /// Vero solo per una data che esiste davvero: `31/02` è una data finta, e
    /// una data finta non è una misura.
    pub fn new(year: i64, month: i64, day: i64) -> Option<Date> {
        if !(1..=12).contains(&month) || day < 1 || year < 1 {
            return None;
        }
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let last = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            _ => 28,
        };
        if day > last {
            return None;
        }
        Some(Date { year, month, day })
    }

    /// I giorni dall'epoca civile, per confrontare e sottrarre senza dipendenze
    /// esterne. È l'algoritmo `days_from_civil`, che tratta marzo come primo
    /// mese e fa cadere il giorno bisestile in fondo all'anno.
    pub fn days(&self) -> i64 {
        let y = if self.month <= 2 { self.year - 1 } else { self.year };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let mp = (self.month + 9) % 12;
        let doy = (153 * mp + 2) / 5 + self.day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn iso(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// La riga con percorsi e indirizzi sostituiti da spazi, lunghezza invariata.
///
/// La lunghezza deve restare la stessa perché le posizioni servono dopo: una
/// parola-misura si accoppia alla data che ha più vicina, e accorciare la riga
/// sposterebbe quella distanza.
fn mask_paths(line: &str) -> String {
    let mut out = line.to_string();
    for m in path_token().find_iter(line) {
        let blanks = " ".repeat(m.as_str().len());
        out.replace_range(m.range(), &blanks);
    }
    out
}

/// Le date che una parola-misura, sulla stessa riga, rivendica davvero.
///
/// Una riga può portarne più d'una, e prendere la prima scaduta era sbagliato:
/// in «la voce nasce il 2026-05-15, chiusa il 2026-07-30, e i numeri sono
/// misurati sul giro di luglio» la misura è di luglio, e maggio è la nascita
/// della voce. Ogni parola-misura si accoppia alla data che ha più vicina, che
/// è il modo in cui una frase le lega. (L'esempio porta due date apposta: con
/// una sola, questa riga di commento si autodenuncerebbe al primo passaggio —
/// successo davvero, il 20/08/2026, alla prima esecuzione del porto.)
fn dated_claims(line: &str) -> Vec<Date> {
    let masked = mask_paths(line);
    let mut dates: Vec<(usize, Date)> = Vec::new();
    for c in iso_date().captures_iter(&masked) {
        let m = c.get(0).unwrap();
        let n = |i: usize| c.get(i).unwrap().as_str().parse::<i64>().unwrap_or(0);
        if let Some(d) = Date::new(n(1), n(2), n(3)) {
            dates.push((m.start(), d));
        }
    }
    for c in slash_date().captures_iter(&masked) {
        let m = c.get(0).unwrap();
        let n = |i: usize| c.get(i).unwrap().as_str().parse::<i64>().unwrap_or(0);
        if let Some(d) = Date::new(n(3), n(2), n(1)) {
            dates.push((m.start(), d));
        }
    }
    if dates.is_empty() {
        return Vec::new();
    }
    claim_re()
        .find_iter(&masked)
        .map(|claim| {
            dates
                .iter()
                .min_by_key(|(pos, _)| (*pos as i64 - claim.start() as i64).abs())
                .map(|(_, d)| *d)
                .unwrap()
        })
        .collect()
}

pub struct Claim {
    pub line_no: usize,
    pub line: String,
    pub date: Date,
    pub days: i64,
}

/// Ogni misura più vecchia della soglia, con la riga da cui viene.
///
/// Serve una parola che dichiari la misura **e** una data sulla stessa riga:
/// una data da sola non basta, perché `createdAt` e le scadenze non affermano
/// niente su cosa vale adesso.
pub fn stale_claims(text: &str, today: Date, months: i64) -> Vec<Claim> {
    let cutoff = today.days() - months * 30;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let stale: Vec<Date> = dated_claims(line)
            .into_iter()
            .filter(|d| d.days() < cutoff)
            .collect();
        if let Some(date) = stale.into_iter().min() {
            let trimmed: String = line.trim().chars().take(110).collect();
            out.push(Claim {
                line_no: i + 1,
                line: trimmed,
                date,
                days: today.days() - date.days(),
            });
        }
    }
    out
}

fn test_file_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(^|/)(test_|prova-|prove-)|(_test|\.test|\.spec|-prova)\.").unwrap()
    })
}

/// Un file di prove cita fantasmi e date vecchie per mestiere: è il suo lavoro.
pub fn is_test_file(path: &str) -> bool {
    test_file_re().is_match(path)
}

/// Il testo senza i corpi delle funzioni di prova.
///
/// Si esclude il corpo INTERO, non le singole asserzioni: il primo tentativo
/// filtrava le righe con la chiamata di controllo e lasciava passare i dati
/// d'appoggio — una variabile che contiene una misura datata — che sono finti
/// esattamente come le asserzioni che alimentano.
pub fn strip_test_bodies(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        if is_test_start(line) {
            skipping = true;
        } else if skipping && is_top_level(line) {
            skipping = false;
        }
        out.push(if skipping { "" } else { line });
    }
    out.join("\n")
}

fn is_test_start(line: &str) -> bool {
    // Python e Rust dichiarano una prova in due modi diversi, e questo testo
    // gira su entrambi gli alberi.
    //
    // `def test(` senza trattino basso è arrivato il 20/08/2026, e non da una
    // congettura: il gemello Python cercava `def test_` e non riconosceva la
    // funzione `test()` di `accettazione.py`, così le stringhe d'esempio delle
    // sue prove interne — che citano misure vecchie per mestiere — venivano
    // segnalate come affermazioni datate. Era l'unica segnalazione su tutto
    // `~/.claude`, cioè il rosso che teneva rosso il criterio, ed era falsa.
    for p in ["def self_test", "def prova", "def test_", "def _prova", "def test("] {
        if line.starts_with(p) {
            return true;
        }
    }
    line.trim_start() == "#[test]"
}

fn is_top_level(line: &str) -> bool {
    line.starts_with("def ") || line.starts_with("class ") || line.starts_with("@")
}

/// Le cartelle che non si attraversano, e il perché di ognuna.
///
/// Senza `plugins/` il rapporto era illeggibile: 5.255 segnalazioni misurate il
/// 12/08/2026 su `~/.claude`, di cui il 91% dentro codice di terzi che non è
/// nostro da correggere. `projects/` sono memorie e trascrizioni, che citano
/// come cronaca; `generated/` è codice generato, e Prisma ci serializza l'intero
/// schema su una riga sola, dove righe lontane diventano adiacenti.
pub const SKIP_DIRS: [&str; 22] = [
    "node_modules",
    ".git",
    "__pycache__",
    "dist",
    "build",
    "state",
    "worktrees",
    ".venv",
    "coverage",
    "plugins",
    "backups",
    "projects",
    "sessions",
    "debug",
    "skills-quarantena",
    "site-packages",
    "lib-dynload",
    "agent-sdk-venv",
    "agent-memory",
    "cache",
    "generated",
    "target",
];

const SKIP_PREFIXES: [&str; 5] = [
    "archive-",
    "archivio-",
    "skills-archivio-",
    "mandati-backup-",
    "commands-backup-",
];

/// Le raccolte installate da un marketplace stanno sotto `<autore>-skills`, e
/// non sono nostre da correggere.
const SKIP_SUFFIXES: [&str; 1] = ["-skills"];

const SCANNED: [&str; 7] = [".md", ".py", ".sh", ".ts", ".tsx", ".prisma", ".rs"];

pub fn skip_dir(name: &str) -> bool {
    SKIP_DIRS.contains(&name)
        || SKIP_PREFIXES.iter().any(|p| name.starts_with(p))
        || SKIP_SUFFIXES.iter().any(|s| name.ends_with(s))
}

pub fn is_scanned(name: &str) -> bool {
    SCANNED.iter().any(|e| name.ends_with(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> Date {
        Date::new(2026, 8, 20).unwrap()
    }

    fn stale(text: &str) -> Vec<String> {
        stale_claims(text, today(), DEFAULT_MONTHS)
            .into_iter()
            .map(|c| c.date.iso())
            .collect()
    }

    #[test]
    fn a_measure_older_than_the_cutoff_is_reported() {
        assert_eq!(stale("misurato il 2026-01-10 su tre repo"), ["2026-01-10"]);
    }

    #[test]
    fn a_recent_measure_is_not() {
        assert!(stale("misurato il 2026-08-01 su tre repo").is_empty());
    }

    #[test]
    fn a_date_without_a_claim_word_is_only_a_date() {
        assert!(stale("scadenza fissata al 2026-01-10").is_empty());
    }

    #[test]
    fn the_truncated_participle_does_not_catch_a_trade() {
        // «un verificatore fermo al 2026-05-15» era l'unica segnalazione su
        // tutto ~/.claude il 18/08, ed era falsa.
        assert!(stale("un verificatore verticale, fermo al 2026-01-15").is_empty());
        assert_eq!(stale("verificata il 2026-01-15"), ["2026-01-15"]);
    }

    #[test]
    fn the_day_first_form_is_read_day_first() {
        assert_eq!(stale("misurato il 10/01/2026 a mano"), ["2026-01-10"]);
    }

    #[test]
    fn an_impossible_date_is_not_a_measure() {
        assert!(stale("misurato il 31/02/2026").is_empty());
    }

    #[test]
    fn a_date_inside_a_filename_is_not_the_date_of_the_measure() {
        // Il nome del documento porta una data vecchia; la misura è di ieri.
        assert!(stale("misurato il 2026-08-19, vedi 2026-01-01-cron-e-soglie.md").is_empty());
    }

    #[test]
    fn each_claim_takes_the_nearest_date_not_the_first() {
        // La voce nasce a gennaio, la misura è di agosto: nessuna scaduta.
        assert!(stale("la voce nasce il 2026-01-15 ed e' stata misurata il 2026-08-18").is_empty());
    }

    #[test]
    fn two_claims_on_one_line_report_the_oldest_stale_one() {
        let out = stale("misurato il 2026-01-10 e riprovato il 2026-02-10");
        assert_eq!(out, ["2026-01-10"]);
    }

    #[test]
    fn the_reported_line_carries_its_number_and_is_trimmed() {
        let text = "prima riga\n   misurato il 2026-01-10   \n";
        let claims = stale_claims(text, today(), DEFAULT_MONTHS);
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].line_no, 2);
        assert_eq!(claims[0].line, "misurato il 2026-01-10");
        assert_eq!(claims[0].days, 222);
    }

    #[test]
    fn the_english_words_count_too() {
        assert_eq!(stale("measured on 2026-01-10"), ["2026-01-10"]);
    }

    #[test]
    fn a_test_body_is_not_scanned() {
        let text =
            "def prova():\n    misurato il 2026-01-10\ndef altro():\n    misurato il 2026-01-11\n";
        assert_eq!(stale(&strip_test_bodies(text)), ["2026-01-11"]);
    }

    #[test]
    fn a_test_function_without_an_underscore_is_a_test_body_too() {
        // Il caso vero: `accettazione.py` dichiara `def test()`, e le stringhe
        // d'esempio delle sue prove interne citano misure vecchie per mestiere.
        let text = "def test() -> int:\n    same(\"x\", conta(\"measured 2026-01-01 (400 days ago)\"), 3)\ndef altro():\n    misurato il 2026-01-11\n";
        assert_eq!(stale(&strip_test_bodies(text)), ["2026-01-11"]);
    }

    #[test]
    fn a_rust_test_body_is_not_scanned_either() {
        let text = "#[test]\nfn x() {\n    // misurato il 2026-01-10\n}\n";
        assert!(stale(&strip_test_bodies(text)).is_empty());
    }

    #[test]
    fn test_files_are_recognised_by_their_name() {
        assert!(is_test_file("/x/tests/prova-gate.py"));
        assert!(is_test_file("/x/src/a.test.ts"));
        assert!(!is_test_file("/x/src/a.ts"));
    }

    #[test]
    fn the_skipped_directories_include_third_party_and_generated_code() {
        assert!(skip_dir("plugins"));
        assert!(skip_dir("projects"));
        assert!(skip_dir("mattpocock-skills"));
        assert!(skip_dir("archivio-2026-08"));
        assert!(!skip_dir("scripts"));
    }

    #[test]
    fn only_the_prose_and_code_extensions_are_scanned() {
        assert!(is_scanned("nota.md"));
        assert!(is_scanned("gate.rs"));
        assert!(!is_scanned("dati.json"));
    }

    #[test]
    fn the_civil_day_count_agrees_with_known_distances() {
        let a = Date::new(2026, 1, 10).unwrap();
        let b = Date::new(2026, 8, 20).unwrap();
        assert_eq!(b.days() - a.days(), 222);
        // Un anno bisestile, attraverso il 29 febbraio.
        let c = Date::new(2024, 2, 28).unwrap();
        let d = Date::new(2024, 3, 1).unwrap();
        assert_eq!(d.days() - c.days(), 2);
    }

    #[test]
    fn february_thirty_first_does_not_exist_but_february_twentynine_does_in_a_leap_year() {
        assert!(Date::new(2026, 2, 29).is_none());
        assert!(Date::new(2024, 2, 29).is_some());
        assert!(Date::new(2026, 13, 1).is_none());
    }
}
