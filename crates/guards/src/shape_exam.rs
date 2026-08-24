//! L'esame della forma: non «cosa si è rotto» ma «che forma ha preso questa
//! casa, e in che direzione si sta muovendo». Ogni altro meccanismo qui dentro
//! è un allarme, e un allarme ha bisogno di un evento; questo non ne ha uno.
//!
//! Qui c'è solo il giudizio puro — le misure arrivano già raccolte da
//! `claude-hooks/src/shape_exam.rs`, l'unico che tocca disco, `git` e registri.
//!
//! DEVE POTER DIRE «NIENTE CHE VALGA», spesso: ogni sonda porta una soglia
//! sotto la quale tace. DICE UNA COSA SOLA: `rank` sceglie il verdetto singolo.

use std::collections::{BTreeMap, BTreeSet};

// ─── Le lingue e come si misura un file ──────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Language {
    Shell,
    Python,
    JavaScript,
    Rust,
}

impl Language {
    pub fn name(self) -> &'static str {
        match self {
            Language::Shell => "shell",
            Language::Python => "python",
            Language::JavaScript => "javascript",
            Language::Rust => "rust",
        }
    }

    /// Il linguaggio provato di questa casa, deciso il 24/08/2026: tutto in
    /// Rust tranne ciò che deve girare quando il Rust non c'è.
    pub fn is_the_proven_one(self) -> bool {
        self == Language::Rust
    }
}

pub fn language_of(path: &str) -> Option<Language> {
    let ext = path.rsplit_once('.').map(|(_, e)| e)?;
    match ext {
        "sh" | "bash" | "zsh" => Some(Language::Shell),
        "py" => Some(Language::Python),
        "js" | "mjs" | "cjs" => Some(Language::JavaScript),
        "rs" => Some(Language::Rust),
        _ => None,
    }
}

/// Una riga di commento o vuota non decide niente: contarla gonfierebbe proprio
/// i file meglio spiegati, che sono l'opposto del debito che si cerca.
fn is_code_line(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && !t.starts_with('#') && !t.starts_with("//") && !t.starts_with('*')
}

/// I costrutti che fanno di una riga una scelta. Elenco per linguaggio perché
/// `&&` in shell è un ramo e in Rust quasi sempre no, e `?` in Rust è un ramo
/// mentre altrove è un ternario che qui non si conta.
fn is_branch_line(lang: Language, line: &str) -> bool {
    let t = line.trim();
    let word = |w: &str| {
        t.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .any(|p| p == w)
    };
    match lang {
        Language::Shell => {
            ["if", "elif", "case", "while", "until", "for"]
                .iter()
                .any(|w| word(w))
                || t.contains("&&")
                || t.contains("||")
        }
        Language::Python => ["if", "elif", "while", "for", "except", "match"]
            .iter()
            .any(|w| word(w)),
        Language::JavaScript => {
            ["if", "switch", "while", "for", "catch"]
                .iter()
                .any(|w| word(w))
                || t.contains("&&")
                || t.contains("||")
        }
        Language::Rust => {
            ["if", "match", "while", "for", "else"]
                .iter()
                .any(|w| word(w))
                || t.contains('?')
        }
    }
}

/// Un caso di prova dentro il file stesso. È volutamente generoso: chi risulta
/// **senza** prove qui non ne ha davvero, e la sonda che lo usa sottostima il
/// debito invece di gonfiarlo.
pub fn carries_a_test(text: &str) -> bool {
    text.contains("#[test]") || text.contains("def test_") || text.contains("#[cfg(test)]")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileShape {
    pub path: String,
    pub lang: Language,
    pub lines: usize,
    pub branches: usize,
    pub tested: bool,
}

pub fn measure_file(path: &str, lang: Language, text: &str) -> FileShape {
    let mut lines = 0;
    let mut branches = 0;
    for line in text.lines() {
        if !is_code_line(line) {
            continue;
        }
        lines += 1;
        if is_branch_line(lang, line) {
            branches += 1;
        }
    }
    FileShape {
        path: path.to_string(),
        lang,
        lines,
        branches,
        tested: carries_a_test(text),
    }
}

// ─── Il registro dei ganci, letto riga per riga ──────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalRow {
    pub day: String,
    pub hook: String,
    pub decision: String,
    pub session: String,
}

/// Le decisioni che sono un rifiuto detto a qualcuno.
///
/// `avvisa` non c'è dentro: un avviso lascia passare, e mescolarlo farebbe
/// sembrare mordace il gancio che non morde. `ferma` nemmeno, e costa una riga
/// spiegarlo: nel successore vuol dire «non ho aperto un pannello» — 350 volte
/// «albero-affollato», in silenzio. Contarlo diceva 1.114 dinieghi ripetuti di
/// `handoff-arms-successor` e mandava a riparare una cosa che non esiste.
pub fn is_denial(decision: &str) -> bool {
    matches!(decision, "blocca" | "nega" | "rifiuta" | "deny")
}

/// Il registro scrive i ganci con i nomi con cui sono nati, non con lo slug del
/// binario: senza questa tabella otto ganci vivi risulterebbero muti. Chi
/// aggiunge un gancio che registra con un nome proprio lo aggiunge qui.
pub const JOURNAL_ALIASES: &[(&str, &str)] = &[
    ("consegna-arma-successore", "handoff-arms-successor"),
    ("consegna-obbligatoria", "handoff-required"),
    ("consegna-allo-stop", "handoff-on-stop"),
    ("session-cap", "handoff-threshold"),
    ("deriva-ambito", "scope-drift"),
    ("lingua-codice", "code-language"),
    ("innesco", "skill-nudge"),
    ("registra-sessione", "register-session"),
    ("duplicazione", "duplication"),
    ("staffetta", "relay"),
    ("ripartenza", "restart-notice"),
    ("cancella-nel-worktree", "allow-worktree-deletes"),
    ("successore", "successor-probe"),
    ("titolo-richiesta", "pr-title"),
];

pub fn canonical_hook(name: &str) -> &str {
    JOURNAL_ALIASES
        .iter()
        .find(|(alias, _)| *alias == name)
        .map(|(_, slug)| *slug)
        .unwrap_or(name)
}

/// Una riga del registro, o `None` se non è una riga vera: le prove si marcano
/// con `"prova":true` apposta per non finire nelle misure di chi conta quanto
/// morde un gate.
pub fn parse_journal_line(line: &str) -> Option<JournalRow> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if value.get("prova").and_then(serde_json::Value::as_bool) == Some(true) {
        return None;
    }
    let field = |k: &str| {
        value
            .get(k)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let stamp = field("t");
    if stamp.len() < 10 {
        return None;
    }
    let hook = field("gancio");
    if hook.is_empty() {
        return None;
    }
    // `session` è il campo di oggi, `sessione` quello delle righe più vecchie:
    // leggerne uno solo perderebbe metà archivio proprio sulla domanda «quante
    // sessioni tocca».
    let mut session = field("session");
    if session.is_empty() {
        session = field("sessione");
    }
    Some(JournalRow {
        day: stamp[..10].to_string(),
        hook: canonical_hook(&hook).to_string(),
        decision: field("decisione"),
        session,
    })
}

/// Lo slug del gancio dentro una riga di `settings.json`, o `None` se quel
/// comando non è del binario di casa.
///
/// Si cerca il token dopo `claude-hooks`, saltando le opzioni: la riga vera è
/// spesso preceduta da un `nohup` o da un `cd`, e prendere la prima parola del
/// comando conterebbe `nohup` fra i ganci cablati.
pub fn wired_slug(command: &str) -> Option<String> {
    let mut tokens = command.split_whitespace();
    tokens.find(|t| t.trim_end_matches('"').ends_with("claude-hooks"))?;
    let slug = tokens.find(|t| !t.starts_with('-'))?;
    let slug = slug.trim_matches(|c| c == '"' || c == '\'');
    (!slug.is_empty()).then(|| slug.to_string())
}

// ─── Aritmetica del calendario, quel poco che serve ──────────────────────────

/// Dalla data civile ai giorni dall'epoca (Howard Hinnant). Serve per una sola
/// domanda — «quanti giorni fa l'ho detto?» — e un confronto fra stringhe non
/// sa rispondere a quella.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYY-MM-DD` → giorni dall'epoca, `None` se la stringa non è una data.
pub fn day_number(day: &str) -> Option<i64> {
    let b = day.as_bytes();
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y = day[..4].parse::<i64>().ok()?;
    let m = day[5..7].parse::<u32>().ok()?;
    let d = day[8..10].parse::<u32>().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

/// I giorni fra due date, negativo se la prima è dopo la seconda.
pub fn days_between(from: &str, to: &str) -> Option<i64> {
    Some(day_number(to)? - day_number(from)?)
}

// ─── Un risultato di sonda ───────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// La chiave con cui la misura entra nella serie storica: non cambia mai,
    /// o la serie si spezza e la tendenza torna a mentire.
    pub probe: &'static str,
    pub subject: String,
    pub number: i64,
    pub unit: &'static str,
    /// Sotto questa soglia la sonda tace. Il perché di ciascuna sta accanto
    /// alla costante che la definisce.
    pub floor: i64,
    pub headline: String,
    /// Cosa succede se resta com'è. Senza, un numero non suggerisce niente.
    pub consequence: String,
}

impl Finding {
    pub fn is_worth_saying(&self) -> bool {
        self.number >= self.floor
    }
}

// ─── Le soglie, con il perché ────────────────────────────────────────────────

/// Duemila righe è il punto oltre il quale «lo riscrivo quando serve» smette di
/// essere vero: sotto, una riscrittura in Rust è una sessione; sopra, diventa
/// un progetto che nessuno apre.
pub const FLOOR_DECIDING_OUTSIDE: i64 = 2_000;

/// Trecento righe è la taglia oltre la quale un file non si tiene più in testa
/// tutto insieme, e senza prove l'unico modo di sapere se funziona è usarlo.
pub const FLOOR_UNPROVEN_BULK: i64 = 300;

/// Cinque: sotto, «cablato e senza una riga» è quasi sempre un gancio nuovo che
/// non ha ancora incontrato il proprio caso.
pub const FLOOR_MUTE_GUARDS: i64 = 5;

/// Cinquanta dinieghi ripetuti in un mese: meno di due al giorno è attrito
/// normale, oltre è un messaggio che non insegna niente a chi lo legge.
pub const FLOOR_REPEATED_DENIALS: i64 = 50;

/// Venticinquemila righe aggiunte in una settimana in una sola cartella: è il
/// ritmo a cui il debito si accumula più in fretta di quanto si riesca a
/// rileggerlo.
pub const FLOOR_WEEKLY_GROWTH: i64 = 25_000;

/// Quattordici giorni senza che una sola indicazione abbia mosso il proprio
/// numero. La scelta è di Theo: due settimane.
pub const FLOOR_UNHEEDED_DAYS: i64 = 14;

// ─── Le sonde ────────────────────────────────────────────────────────────────

/// Quante righe che DECIDONO stanno fuori dal linguaggio provato, e senza un
/// caso di prova. È la domanda che nessun gancio fa: ogni script funziona,
/// nessun passo era sbagliato, e non c'è nessun evento da intercettare.
///
/// Si contano solo i file con almeno un ramo: un file senza scelte è una mano,
/// non un cervello, e riscriverlo in Rust non comprerebbe niente.
pub fn probe_deciding_outside(files: &[FileShape]) -> Finding {
    let deciding = |f: &&FileShape| f.branches > 0;
    let outside: Vec<&FileShape> = files
        .iter()
        .filter(|f| !f.lang.is_the_proven_one())
        .filter(deciding)
        .collect();
    let unproven: i64 = outside
        .iter()
        .filter(|f| !f.tested)
        .map(|f| f.lines as i64)
        .sum();
    let inside: i64 = files
        .iter()
        .filter(|f| f.lang.is_the_proven_one())
        .filter(deciding)
        .map(|f| f.lines as i64)
        .sum();
    let worst = outside
        .iter()
        .filter(|f| !f.tested)
        .max_by_key(|f| f.branches)
        .map(|f| format!("{} ({} rami, {} righe)", f.path, f.branches, f.lines))
        .unwrap_or_else(|| "-".to_string());
    // La ripartizione per linguaggio è la forma vera, e senza di essa il totale
    // non dice in che direzione si sta muovendo la casa: un numero che sale
    // tutto in shell è un guasto diverso da uno che sale tutto in Python.
    let mut per_language: BTreeMap<&str, i64> = BTreeMap::new();
    for f in outside.iter().filter(|f| !f.tested) {
        *per_language.entry(f.lang.name()).or_default() += f.lines as i64;
    }
    let mut split: Vec<(&str, i64)> = per_language.into_iter().collect();
    split.sort_by_key(|(_, n)| -*n);
    let split = split
        .iter()
        .map(|(name, n)| format!("{n} {name}"))
        .collect::<Vec<_>>()
        .join(", ");
    Finding {
        probe: "deciding-outside",
        subject: worst.clone(),
        number: unproven,
        unit: "righe",
        floor: FLOOR_DECIDING_OUTSIDE,
        headline: format!(
            "{unproven} righe che decidono stanno fuori dal linguaggio provato e senza prove \
             ({split}), contro {inside} righe in Rust"
        ),
        consequence: format!(
            "ogni correzione lì dentro si prova a mano e nessun mutante la vede; \
             il pezzo più grosso è {worst}"
        ),
    }
}

/// La cosa più grande senza prove, in righe. Vale per ogni linguaggio: un file
/// Rust da mille righe senza `#[test]` è lo stesso debito di uno in shell.
pub fn probe_unproven_bulk(files: &[FileShape]) -> Finding {
    let worst = files.iter().filter(|f| !f.tested).max_by_key(|f| f.lines);
    let (subject, number) = match worst {
        Some(f) => (f.path.clone(), f.lines as i64),
        None => ("-".to_string(), 0),
    };
    let total: i64 = files
        .iter()
        .filter(|f| !f.tested)
        .map(|f| f.lines as i64)
        .sum();
    Finding {
        probe: "unproven-bulk",
        subject: subject.clone(),
        number,
        unit: "righe",
        floor: FLOOR_UNPROVEN_BULK,
        headline: format!(
            "il file più grande senza un caso di prova è {subject}, {number} righe \
             ({total} righe senza prove in tutto)"
        ),
        consequence: "l'unico modo di sapere se funziona è usarlo, e se ne accorge chi lo usa"
            .to_string(),
    }
}

/// Quali controlli cablati non hanno lasciato una riga nel registro. Un freno
/// che in un mese non ha detto niente o è inutile o è muto: da fuori le due
/// cose si somigliano, e questa sonda dichiara la coppia invece di scegliere.
pub fn probe_mute_guards(wired: &BTreeSet<String>, rows: &[JournalRow]) -> Finding {
    let seen: BTreeSet<&str> = rows.iter().map(|r| r.hook.as_str()).collect();
    let mute: Vec<&str> = wired
        .iter()
        .map(String::as_str)
        .filter(|w| !seen.contains(w))
        .collect();
    let names = mute.join(", ");
    Finding {
        probe: "mute-guards",
        subject: names.clone(),
        number: mute.len() as i64,
        unit: "ganci",
        floor: FLOOR_MUTE_GUARDS,
        headline: format!(
            "{} controlli cablati non hanno scritto una riga nel registro: {}",
            mute.len(),
            if names.is_empty() { "-" } else { &names }
        ),
        consequence: "o non servono o non partono, e da fuori i due casi si somigliano".to_string(),
    }
}

/// Chi nega di più, e quanti di quei dinieghi non hanno insegnato niente.
///
/// «Quanti erano sbagliati» il registro non lo sa dire — non ha un campo per
/// quello — e questo è il surrogato più onesto: un diniego ripetuto allo stesso
/// interlocutore, lo stesso giorno, dallo stesso gancio, è un messaggio che chi
/// lo legge non ha saputo usare.
pub fn probe_repeated_denials(rows: &[JournalRow]) -> Finding {
    let mut per_hook: BTreeMap<&str, i64> = BTreeMap::new();
    let mut repeats: BTreeMap<&str, i64> = BTreeMap::new();
    let mut seen: BTreeMap<(&str, &str, &str), i64> = BTreeMap::new();
    for row in rows.iter().filter(|r| is_denial(&r.decision)) {
        *per_hook.entry(row.hook.as_str()).or_default() += 1;
        let key = (row.hook.as_str(), row.day.as_str(), row.session.as_str());
        let count = seen.entry(key).or_default();
        *count += 1;
        if *count > 1 {
            *repeats.entry(row.hook.as_str()).or_default() += 1;
        }
    }
    let loudest = per_hook
        .iter()
        .max_by_key(|(_, n)| **n)
        .map(|(h, n)| (h.to_string(), *n))
        .unwrap_or_else(|| ("-".to_string(), 0));
    let worst_repeat = repeats
        .iter()
        .max_by_key(|(_, n)| **n)
        .map(|(h, n)| (h.to_string(), *n))
        .unwrap_or_else(|| ("-".to_string(), 0));
    Finding {
        probe: "repeated-denials",
        subject: worst_repeat.0.clone(),
        number: worst_repeat.1,
        unit: "dinieghi ripetuti",
        floor: FLOOR_REPEATED_DENIALS,
        headline: format!(
            "chi nega di più è {} ({} dinieghi); {} ne ha ripetuti {} alla stessa sessione \
             nello stesso giorno",
            loudest.0, loudest.1, worst_repeat.0, worst_repeat.1
        ),
        consequence: "un diniego che si ripete non ha insegnato niente a chi lo ha letto"
            .to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Growth {
    pub folder: String,
    pub added: i64,
}

/// Cosa è cresciuto più in fretta nell'ultima settimana. La crescita rapida è
/// dove si accumula il debito: dove si scrive di più è dove nessuno rilegge.
pub fn probe_fastest_growth(growth: &[Growth]) -> Finding {
    let top = growth.iter().max_by_key(|g| g.added);
    let (subject, number) = match top {
        Some(g) => (g.folder.clone(), g.added),
        None => ("-".to_string(), 0),
    };
    let total: i64 = growth.iter().map(|g| g.added).sum();
    Finding {
        probe: "fastest-growth",
        subject: subject.clone(),
        number,
        unit: "righe aggiunte",
        floor: FLOOR_WEEKLY_GROWTH,
        headline: format!(
            "in sette giorni {subject} è cresciuta di {number} righe, su {total} in tutta la casa"
        ),
        consequence: "dove si scrive di più è dove nessuno rilegge".to_string(),
    }
}

/// Cosa questo esame ha già detto, e quando.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    pub day: String,
    pub probe: String,
    pub subject: String,
    pub number: i64,
}

/// Se qualcuno ha fatto qualcosa. Chi misura va misurato: uno strumento che in
/// due settimane non ha mosso niente va spento, non lasciato girare.
///
/// Un'indicazione «ha mosso» se il numero della sua sonda è sceso di almeno il
/// 5%: sotto quella soglia è rumore di misura, non una riparazione.
pub fn probe_unheeded(
    said: &[Said],
    today: &str,
    current: &BTreeMap<String, i64>,
) -> Option<Finding> {
    let old: Vec<&Said> = said
        .iter()
        .filter(|s| days_between(&s.day, today).is_some_and(|d| d >= FLOOR_UNHEEDED_DAYS))
        .collect();
    if old.len() < 2 {
        return None; // troppo poca storia per accusare qualcuno di non ascoltare
    }
    let moved = |s: &&Said| match current.get(&s.probe) {
        Some(now) => *now as f64 <= s.number as f64 * 0.95,
        None => false,
    };
    if old.iter().any(moved) {
        return None;
    }
    let oldest = old
        .iter()
        .filter_map(|s| days_between(&s.day, today))
        .max()
        .unwrap_or(0);
    Some(Finding {
        probe: "unheeded",
        subject: format!(
            "{} indicazioni, la più vecchia di {oldest} giorni fa",
            old.len()
        ),
        number: oldest,
        unit: "giorni",
        floor: FLOOR_UNHEEDED_DAYS,
        headline: format!(
            "nessuna delle {} indicazioni di questo esame ha mosso il proprio numero \
             da {oldest} giorni",
            old.len()
        ),
        consequence: "uno strumento che nessuno ascolta va spento, non lasciato girare".to_string(),
    })
}

// ─── La tendenza, che è il punto ─────────────────────────────────────────────

/// Una deriva non è un evento, è una direzione: «39 voci aperte» non dice
/// niente, «la coda si allunga da tre esami di fila» dice tutto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trend {
    /// Meno di tre misure: non c'è ancora una direzione da leggere.
    TooShort,
    Rising(usize),
    Flat,
    Falling(usize),
}

impl Trend {
    pub fn describe(&self) -> String {
        match self {
            Trend::TooShort => "tendenza: non ancora leggibile (meno di tre esami)".to_string(),
            Trend::Rising(n) => format!("tendenza: in salita da {n} esami di fila"),
            Trend::Flat => "tendenza: ferma".to_string(),
            Trend::Falling(n) => format!("tendenza: in calo da {n} esami di fila"),
        }
    }

    /// Quanto pesa una direzione sul verdetto. Una misura che sale vale il
    /// doppio di una ferma: è la sola cosa che questo esame sa dire e nessun
    /// allarme sa dire.
    pub fn weight(&self) -> f64 {
        match self {
            Trend::Rising(_) => 2.0,
            Trend::Flat | Trend::TooShort => 1.0,
            Trend::Falling(_) => 0.5,
        }
    }
}

/// La storia va dalla più vecchia alla più recente, ultima inclusa.
pub fn trend(history: &[i64]) -> Trend {
    if history.len() < 3 {
        return Trend::TooShort;
    }
    let mut rising = 1;
    for pair in history.windows(2).rev() {
        if pair[1] > pair[0] {
            rising += 1;
        } else {
            break;
        }
    }
    let mut falling = 1;
    for pair in history.windows(2).rev() {
        if pair[1] < pair[0] {
            falling += 1;
        } else {
            break;
        }
    }
    match (rising, falling) {
        (r, _) if r >= 3 => Trend::Rising(r),
        (_, f) if f >= 3 => Trend::Falling(f),
        _ => Trend::Flat,
    }
}

// ─── Il verdetto: una cosa sola, o niente ────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Nessuna misura supera la propria soglia. Deve poter uscire spesso e
    /// senza imbarazzo: un esaminatore che trova sempre qualcosa insegna a
    /// ignorarlo.
    Nothing { closest: Option<Finding> },
    One {
        finding: Finding,
        trend: Trend,
        runner_up: Option<Finding>,
    },
}

pub fn score(finding: &Finding, trend: &Trend) -> f64 {
    if !finding.is_worth_saying() {
        return 0.0;
    }
    finding.number as f64 / finding.floor.max(1) as f64 * trend.weight()
}

/// Sceglie la cosa sola che vale di più adesso.
///
/// `unheeded` non passa dal punteggio: se scatta, vince. Sapere che nessuno
/// ascolta questo esame conta più di qualunque cosa l'esame possa dire.
pub fn rank(findings: Vec<Finding>, history: &BTreeMap<String, Vec<i64>>) -> Verdict {
    let trend_of = |f: &Finding| {
        history
            .get(f.probe)
            .map(|h| trend(h))
            .unwrap_or(Trend::TooShort)
    };
    if let Some(unheeded) = findings.iter().find(|f| f.probe == "unheeded") {
        if unheeded.is_worth_saying() {
            let direction = trend_of(unheeded);
            return Verdict::One {
                finding: unheeded.clone(),
                trend: direction,
                runner_up: None,
            };
        }
    }
    let mut ranked: Vec<(f64, Finding, Trend)> = findings
        .into_iter()
        .map(|f| {
            let direction = trend_of(&f);
            (score(&f, &direction), f, direction)
        })
        .collect();
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    match ranked.first() {
        Some((best, _, _)) if *best <= 0.0 => Verdict::Nothing {
            // Chi era più vicino alla soglia: serve a capire se il silenzio è
            // largo o appeso a un filo.
            closest: ranked
                .into_iter()
                .map(|(_, f, _)| f)
                // Una sonda a zero non è «vicina a parlare»: su una casa nuova
                // sono tutte a zero, e nominarne una a caso è già rumore.
                .filter(|f| f.number > 0)
                .max_by(|a, b| {
                    let ra = a.number as f64 / a.floor.max(1) as f64;
                    let rb = b.number as f64 / b.floor.max(1) as f64;
                    ra.partial_cmp(&rb).unwrap_or(std::cmp::Ordering::Equal)
                }),
        },
        Some(_) => {
            let mut rest = ranked.into_iter();
            let (_, finding, direction) = rest.next().expect("appena controllato");
            let runner_up = rest.next().filter(|(s, _, _)| *s > 0.0).map(|(_, f, _)| f);
            Verdict::One {
                finding,
                trend: direction,
                runner_up,
            }
        }
        None => Verdict::Nothing { closest: None },
    }
}

/// Quello che legge chi lavora: poche righe, un numero, e cosa succede se resta
/// com'è. Il resto sta nel rapporto lungo.
pub fn render_verdict(day: &str, verdict: &Verdict) -> String {
    match verdict {
        Verdict::Nothing { closest } => {
            let tail = match closest {
                Some(f) => format!(
                    "  La più vicina a parlare: {} — {} {} su una soglia di {}.\n",
                    f.probe, f.number, f.unit, f.floor
                ),
                None => String::new(),
            };
            format!("Esame della forma — {day}\n  Niente che valga.\n{tail}")
        }
        Verdict::One {
            finding,
            trend: direction,
            runner_up,
        } => {
            let tail = match runner_up {
                Some(f) => format!("\n  Poi verrebbe: {} ({} {}).\n", f.probe, f.number, f.unit),
                None => String::new(),
            };
            format!(
                "Esame della forma — {day}\n\n  {}\n  {}\n  Se resta com'è: {}\n{tail}",
                finding.headline,
                direction.describe(),
                finding.consequence
            )
        }
    }
}

/// Il rapporto lungo: tutte le sonde, per chi vuole guardarlo.
pub fn render_report(findings: &[Finding], history: &BTreeMap<String, Vec<i64>>) -> String {
    let mut out = String::from("\n  Tutte le misure\n");
    for f in findings {
        let direction = history
            .get(f.probe)
            .map(|h| trend(h))
            .unwrap_or(Trend::TooShort);
        let label = if f.is_worth_saying() {
            "sopra soglia"
        } else {
            "sotto soglia"
        };
        out.push_str(&format!(
            "  · {:<18} {:>8} {:<18} soglia {:<7} {label:<12} {}\n",
            f.probe,
            f.number,
            f.unit,
            f.floor,
            direction.describe()
        ));
        out.push_str(&format!("      {}\n", f.headline));
    }
    out
}

/// Va registrato che l'esame ha detto questa cosa?
///
/// No se l'ha già detta oggi: `probe_unheeded` conta le indicazioni, e due
/// copie della stessa basterebbero da sole a far scattare l'accusa «nessuno
/// ascolta» — un contatore che si auto-alimenta è il difetto peggiore che
/// possa avere chi si misura da solo.
pub fn should_record_said(last: Option<&Said>, day: &str, finding: &Finding) -> bool {
    match last {
        Some(prev) => !(prev.day == day && prev.probe == finding.probe),
        None => true,
    }
}

/// Va scritta una voce in coda per questo verdetto?
///
/// Il guasto che abbiamo oggi è che ogni meccanismo scrive una voce e la lista
/// si allunga: qui una voce si scrive solo se non ce n'è già una aperta E se
/// la cosa da dire è diversa o è peggiorata di almeno un decimo. Ridirla
/// uguale è esattamente ciò che insegna a ignorare la coda.
pub fn should_file_entry(verdict: &Verdict, entry_is_open: bool, last_said: Option<&Said>) -> bool {
    let Verdict::One { finding, .. } = verdict else {
        return false;
    };
    if entry_is_open {
        return false;
    }
    match last_said {
        None => true,
        Some(prev) if prev.probe != finding.probe => true,
        Some(prev) => finding.number as f64 >= prev.number as f64 * 1.10,
    }
}

// ─── L'innesco: una soglia, non un orologio ──────────────────────────────────

/// Quante righe devono essersi mosse da un esame all'altro perché valga la pena
/// rifarlo. Duemila righe è, al ritmo misurato di questa casa (164.160 righe in
/// sette giorni, 24/08/2026), poche ore di lavoro: sotto, la forma non ha
/// ancora avuto il tempo di cambiare.
pub const MOVEMENT_THRESHOLD: i64 = 2_000;

/// L'esame si sveglia quando qualcosa è cambiato abbastanza, non a un'ora
/// fissa: su un giacimento fermo è lavoro sprecato. Stessa forma della prima
/// esecuzione della ronda — senza watermark si registra e non si scatta.
pub fn should_examine(lines_moved: Option<i64>) -> bool {
    match lines_moved {
        None => false, // prima esecuzione: si registra il punto e basta
        Some(n) => n >= MOVEMENT_THRESHOLD,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(path: &str, lang: Language, lines: usize, branches: usize, tested: bool) -> FileShape {
        FileShape {
            path: path.into(),
            lang,
            lines,
            branches,
            tested,
        }
    }

    // ─── La misura di un file ────────────────────────────────────────────

    #[test]
    fn it_classifies_a_path_by_extension() {
        assert_eq!(language_of("/x/a.sh"), Some(Language::Shell));
        assert_eq!(language_of("/x/a.py"), Some(Language::Python));
        assert_eq!(language_of("/x/a.mjs"), Some(Language::JavaScript));
        assert_eq!(language_of("/x/a.rs"), Some(Language::Rust));
        assert_eq!(language_of("/x/LEGGIMI"), None);
        assert_eq!(language_of("/x/a.md"), None);
    }

    #[test]
    fn comments_and_blank_lines_are_not_code() {
        let text = "# commento\n\n  \nif [ -f x ]; then\n  echo ok\nfi\n";
        let m = measure_file("/x/a.sh", Language::Shell, text);
        assert_eq!(m.lines, 3, "tre righe di codice, non sei");
        assert_eq!(m.branches, 1);
    }

    #[test]
    fn a_word_inside_another_word_is_not_a_branch() {
        // `verificatore` contiene `for`, e contarlo gonfierebbe proprio i file
        // meglio nominati.
        let m = measure_file("/x/a.sh", Language::Shell, "verificatore=1\nifconfig -a\n");
        assert_eq!(m.branches, 0, "nessuna di queste due righe decide");
    }

    #[test]
    fn shell_counts_the_and_or_chains_that_rust_does_not() {
        let sh = measure_file("/x/a.sh", Language::Shell, "test -f x && echo si\n");
        assert_eq!(sh.branches, 1);
        let rs = measure_file("/x/a.rs", Language::Rust, "let x = a && b;\n");
        assert_eq!(rs.branches, 0, "in Rust `&&` dentro un let non è un ramo");
    }

    #[test]
    fn a_file_that_carries_its_own_test_is_marked() {
        assert!(carries_a_test("#[test]\nfn x() {}"));
        assert!(carries_a_test("def test_qualcosa():\n    pass"));
        assert!(!carries_a_test("fn x() {}"));
    }

    // ─── Il registro ─────────────────────────────────────────────────────

    #[test]
    fn a_test_row_never_enters_the_measures() {
        // Le prove si marcano apposta per non falsare chi conta quanto morde
        // un gate: 233 righe su 6.832 il 19/08/2026.
        let line = r#"{"t":"2026-08-24T10:00:00+00:00","gancio":"cd-guard","decisione":"blocca","motivo":"x","prova":true}"#;
        assert_eq!(parse_journal_line(line), None);
    }

    #[test]
    fn a_journal_row_is_read_under_its_canonical_slug() {
        let line = r#"{"t":"2026-08-24T10:00:00+00:00","gancio":"lingua-codice","decisione":"nega","motivo":"x","session":"abcd1234"}"#;
        let row = parse_journal_line(line).expect("riga valida");
        assert_eq!(row.hook, "code-language");
        assert_eq!(row.day, "2026-08-24");
        assert_eq!(row.session, "abcd1234");
    }

    #[test]
    fn the_old_session_field_is_read_too() {
        let line = r#"{"t":"2026-08-11T21:28:15+00:00","gancio":"innesco","decisione":"suggerisce","motivo":"x","sessione":"prova1"}"#;
        let row = parse_journal_line(line).expect("riga valida");
        assert_eq!(row.session, "prova1");
        assert_eq!(row.hook, "skill-nudge");
    }

    #[test]
    fn a_warning_is_not_a_denial() {
        assert!(is_denial("blocca"));
        assert!(is_denial("nega"));
        assert!(!is_denial("avvisa"), "un avviso lascia passare");
        assert!(!is_denial("passa"));
    }

    #[test]
    fn deciding_not_to_act_is_not_a_denial_either() {
        // Misurato il 24/08/2026 sul registro vero: `ferma` era il 96% dei
        // «dinieghi» e nessuno di quelli era detto a qualcuno.
        assert!(!is_denial("ferma"));
        assert!(!is_denial("apre"));
        assert!(!is_denial("salta"));
    }

    // ─── Cosa è cablato ──────────────────────────────────────────────────

    #[test]
    fn it_finds_the_slug_behind_a_wrapper() {
        assert_eq!(
            wired_slug("/Users/theo/.claude/rust/target/release/claude-hooks cd-guard").as_deref(),
            Some("cd-guard")
        );
        assert_eq!(
            wired_slug("nohup /x/claude-hooks hook-census > /dev/null 2>&1 &").as_deref(),
            Some("hook-census"),
            "prendere la prima parola conterebbe `nohup` fra i ganci"
        );
        assert_eq!(
            wired_slug("cd \"$CLAUDE_PROJECT_DIR\"; /x/claude-hooks orca-cleanup --close --quiet")
                .as_deref(),
            Some("orca-cleanup")
        );
    }

    #[test]
    fn a_command_that_is_not_ours_has_no_slug() {
        assert_eq!(wired_slug("/Users/theo/.claude/guard-scope.sh"), None);
        assert_eq!(wired_slug("/x/claude-hooks"), None, "senza sottocomando");
    }

    // ─── Le sonde ────────────────────────────────────────────────────────

    #[test]
    fn deciding_outside_counts_only_files_that_actually_decide() {
        let files = vec![
            shape("/x/brain.sh", Language::Shell, 600, 170, false),
            shape("/x/hand.sh", Language::Shell, 400, 0, false), // nessun ramo
            shape("/x/engine.rs", Language::Rust, 900, 90, true),
        ];
        let f = probe_deciding_outside(&files);
        assert_eq!(f.number, 600, "le 400 righe senza rami non sono un cervello");
        assert!(f.subject.contains("brain.sh"), "{}", f.subject);
        assert!(
            f.headline.contains("900"),
            "cita anche il lato provato: {}",
            f.headline
        );
    }

    #[test]
    fn deciding_outside_says_which_language_the_debt_is_in() {
        // Il totale da solo non dice la direzione: shell e Python sono due
        // guasti diversi, e la riga deve nominarli in ordine di peso.
        let files = vec![
            shape("/x/a.sh", Language::Shell, 600, 100, false),
            shape("/x/b.py", Language::Python, 900, 100, false),
            shape("/x/c.mjs", Language::JavaScript, 100, 10, false),
        ];
        let f = probe_deciding_outside(&files);
        assert_eq!(f.number, 1600);
        assert!(
            f.headline.contains("900 python, 600 shell, 100 javascript"),
            "{}",
            f.headline
        );
    }

    #[test]
    fn deciding_outside_ignores_what_is_already_proven() {
        let files = vec![shape("/x/a.py", Language::Python, 500, 80, true)];
        let f = probe_deciding_outside(&files);
        assert_eq!(f.number, 0, "un file con le sue prove non è questo debito");
        assert!(!f.is_worth_saying());
    }

    #[test]
    fn unproven_bulk_names_the_biggest_file_without_a_case() {
        let files = vec![
            shape("/x/big.py", Language::Python, 1200, 200, false),
            shape("/x/bigger.rs", Language::Rust, 3000, 300, true),
        ];
        let f = probe_unproven_bulk(&files);
        assert_eq!(f.subject, "/x/big.py");
        assert_eq!(f.number, 1200);
    }

    #[test]
    fn mute_guards_lists_the_wired_ones_the_journal_never_saw() {
        let wired: BTreeSet<String> = ["cd-guard", "pr-title", "scope-drift"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let rows = vec![JournalRow {
            day: "2026-08-24".into(),
            hook: "cd-guard".into(),
            decision: "blocca".into(),
            session: "a".into(),
        }];
        let f = probe_mute_guards(&wired, &rows);
        assert_eq!(f.number, 2);
        assert!(f.subject.contains("pr-title") && f.subject.contains("scope-drift"));
    }

    #[test]
    fn repeated_denials_count_only_the_ones_beyond_the_first() {
        let deny = |hook: &str, day: &str, session: &str| JournalRow {
            day: day.into(),
            hook: hook.into(),
            decision: "blocca".into(),
            session: session.into(),
        };
        let rows = vec![
            deny("cd-guard", "2026-08-24", "aaa"),
            deny("cd-guard", "2026-08-24", "aaa"),
            deny("cd-guard", "2026-08-24", "aaa"),
            // Giorno diverso: il conto riparte, non è la stessa lezione.
            deny("cd-guard", "2026-08-25", "aaa"),
            // Sessione diversa: nemmeno questa è una ripetizione.
            deny("cd-guard", "2026-08-24", "bbb"),
        ];
        let f = probe_repeated_denials(&rows);
        assert_eq!(f.number, 2, "cinque dinieghi, due ripetuti");
        assert_eq!(f.subject, "cd-guard");
    }

    #[test]
    fn a_pass_row_never_becomes_a_denial() {
        let rows = vec![JournalRow {
            day: "2026-08-24".into(),
            hook: "socraticode-gate".into(),
            decision: "passa".into(),
            session: "aaa".into(),
        }];
        let f = probe_repeated_denials(&rows);
        assert_eq!(f.number, 0);
        assert!(f.headline.contains("- (0 dinieghi)"), "{}", f.headline);
    }

    #[test]
    fn fastest_growth_takes_the_top_folder() {
        let growth = vec![
            Growth {
                folder: "rust".into(),
                added: 75_664,
            },
            Growth {
                folder: "docs".into(),
                added: 28_126,
            },
        ];
        let f = probe_fastest_growth(&growth);
        assert_eq!(f.subject, "rust");
        assert_eq!(f.number, 75_664);
        assert!(
            f.headline.contains("103790"),
            "somma tutto: {}",
            f.headline
        );
    }

    // ─── Calendario ──────────────────────────────────────────────────────

    #[test]
    fn it_counts_the_days_between_two_dates() {
        assert_eq!(days_between("2026-08-10", "2026-08-24"), Some(14));
        assert_eq!(days_between("2026-02-28", "2026-03-01"), Some(1)); // 2026 non è bisestile
        assert_eq!(days_between("2024-02-28", "2024-03-01"), Some(2)); // 2024 sì
        assert_eq!(days_between("nondata", "2026-08-24"), None);
    }

    // ─── Chi misura va misurato ──────────────────────────────────────────

    #[test]
    fn unheeded_stays_quiet_when_a_number_actually_moved() {
        let said = vec![
            Said {
                day: "2026-08-01".into(),
                probe: "deciding-outside".into(),
                subject: "x".into(),
                number: 16_000,
            },
            Said {
                day: "2026-08-02".into(),
                probe: "unproven-bulk".into(),
                subject: "y".into(),
                number: 1_200,
            },
        ];
        let mut current = BTreeMap::new();
        current.insert("deciding-outside".to_string(), 9_000_i64); // sceso: qualcuno ha lavorato
        current.insert("unproven-bulk".to_string(), 1_200_i64);
        assert_eq!(probe_unheeded(&said, "2026-08-24", &current), None);
    }

    #[test]
    fn unheeded_fires_when_nothing_moved_in_two_weeks() {
        let said = vec![
            Said {
                day: "2026-08-01".into(),
                probe: "deciding-outside".into(),
                subject: "x".into(),
                number: 16_000,
            },
            Said {
                day: "2026-08-02".into(),
                probe: "unproven-bulk".into(),
                subject: "y".into(),
                number: 1_200,
            },
        ];
        let mut current = BTreeMap::new();
        current.insert("deciding-outside".to_string(), 16_300_i64); // peggiorato
        current.insert("unproven-bulk".to_string(), 1_200_i64);
        let f = probe_unheeded(&said, "2026-08-24", &current).expect("deve scattare");
        assert_eq!(f.number, 23, "la più vecchia è del primo agosto");
        assert!(f.consequence.contains("spento"));
    }

    #[test]
    fn unheeded_needs_history_before_it_accuses_anyone() {
        let said = vec![Said {
            day: "2026-08-01".into(),
            probe: "deciding-outside".into(),
            subject: "x".into(),
            number: 16_000,
        }];
        let current = BTreeMap::new();
        assert_eq!(
            probe_unheeded(&said, "2026-08-24", &current),
            None,
            "una sola indicazione non prova che nessuno ascolti"
        );
    }

    #[test]
    fn a_recent_word_does_not_count_as_unheeded() {
        let said = vec![
            Said {
                day: "2026-08-20".into(),
                probe: "a".into(),
                subject: "x".into(),
                number: 10,
            },
            Said {
                day: "2026-08-21".into(),
                probe: "b".into(),
                subject: "y".into(),
                number: 10,
            },
        ];
        assert_eq!(probe_unheeded(&said, "2026-08-24", &BTreeMap::new()), None);
    }

    // ─── Tendenza ────────────────────────────────────────────────────────

    #[test]
    fn a_trend_needs_three_measures_before_it_says_anything() {
        assert_eq!(trend(&[10, 20]), Trend::TooShort);
        assert_eq!(trend(&[]), Trend::TooShort);
    }

    #[test]
    fn three_rises_in_a_row_are_a_direction() {
        assert_eq!(trend(&[10, 20, 30]), Trend::Rising(3));
        assert_eq!(trend(&[5, 10, 20, 30]), Trend::Rising(4));
    }

    #[test]
    fn a_single_rise_after_a_fall_is_not_a_direction() {
        assert_eq!(trend(&[30, 20, 25]), Trend::Flat);
    }

    #[test]
    fn three_falls_in_a_row_are_read_too() {
        assert_eq!(trend(&[30, 20, 10]), Trend::Falling(3));
    }

    // ─── Il verdetto ─────────────────────────────────────────────────────

    fn small(probe: &'static str, number: i64, floor: i64) -> Finding {
        Finding {
            probe,
            subject: "s".into(),
            number,
            unit: "righe",
            floor,
            headline: format!("{probe}: {number}"),
            consequence: "c".into(),
        }
    }

    #[test]
    fn everything_under_its_floor_says_nothing_at_all() {
        let findings = vec![small("a", 10, 100), small("b", 50, 100)];
        match rank(findings, &BTreeMap::new()) {
            Verdict::Nothing { closest } => {
                assert_eq!(
                    closest.map(|f| f.probe),
                    Some("b"),
                    "la più vicina alla soglia"
                );
            }
            other => panic!("atteso silenzio, ottenuto {other:?}"),
        }
        let spoken = render_verdict(
            "2026-08-24",
            &rank(vec![small("a", 10, 100)], &BTreeMap::new()),
        );
        assert!(spoken.contains("Niente che valga"), "{spoken}");
    }

    #[test]
    fn a_house_where_every_measure_is_zero_says_nothing_and_names_nobody() {
        match rank(vec![small("a", 0, 100), small("b", 0, 100)], &BTreeMap::new()) {
            Verdict::Nothing { closest } => assert_eq!(closest, None),
            other => panic!("atteso silenzio pieno, ottenuto {other:?}"),
        }
    }

    #[test]
    fn the_verdict_is_one_thing_and_names_the_runner_up() {
        let findings = vec![small("a", 800, 100), small("b", 300, 100)];
        match rank(findings, &BTreeMap::new()) {
            Verdict::One {
                finding, runner_up, ..
            } => {
                assert_eq!(finding.probe, "a");
                assert_eq!(runner_up.map(|f| f.probe), Some("b"));
            }
            other => panic!("atteso un verdetto, ottenuto {other:?}"),
        }
    }

    #[test]
    fn a_rising_measure_beats_a_bigger_one_that_is_falling() {
        // È il punto dell'intero strumento: la direzione conta più della
        // fotografia. `b` è più grande, ma sta scendendo.
        let mut history = BTreeMap::new();
        history.insert("a".to_string(), vec![100, 200, 400]);
        history.insert("b".to_string(), vec![900, 700, 600]);
        match rank(vec![small("a", 400, 100), small("b", 600, 100)], &history) {
            Verdict::One {
                finding,
                trend: direction,
                ..
            } => {
                assert_eq!(finding.probe, "a");
                assert_eq!(direction, Trend::Rising(3));
            }
            other => panic!("atteso a, ottenuto {other:?}"),
        }
    }

    #[test]
    fn nobody_listening_wins_over_everything_else() {
        let mut unheeded = small("unheeded", 20, FLOOR_UNHEEDED_DAYS);
        unheeded.unit = "giorni";
        match rank(vec![small("a", 100_000, 100), unheeded], &BTreeMap::new()) {
            Verdict::One {
                finding, runner_up, ..
            } => {
                assert_eq!(finding.probe, "unheeded");
                assert_eq!(
                    runner_up, None,
                    "quando nessuno ascolta non c'è un secondo posto"
                );
            }
            other => panic!("atteso unheeded, ottenuto {other:?}"),
        }
    }

    #[test]
    fn the_spoken_verdict_carries_the_number_the_trend_and_the_consequence() {
        let mut history = BTreeMap::new();
        history.insert("a".to_string(), vec![100, 200, 400]);
        let spoken = render_verdict("2026-08-24", &rank(vec![small("a", 400, 100)], &history));
        assert!(spoken.contains("400"), "{spoken}");
        assert!(spoken.contains("in salita"), "{spoken}");
        assert!(spoken.contains("Se resta com'è"), "{spoken}");
    }

    // ─── La coda, e l'innesco ────────────────────────────────────────────

    #[test]
    fn the_same_thing_is_not_recorded_twice_in_one_day() {
        let finding = small("deciding-outside", 16_660, 2_000);
        let today = Said {
            day: "2026-08-24".into(),
            probe: "deciding-outside".into(),
            subject: "s".into(),
            number: 16_660,
        };
        assert!(!should_record_said(Some(&today), "2026-08-24", &finding));
        assert!(should_record_said(Some(&today), "2026-08-25", &finding));
        assert!(should_record_said(None, "2026-08-24", &finding));
        let other = Said {
            day: "2026-08-24".into(),
            probe: "mute-guards".into(),
            subject: "s".into(),
            number: 10,
        };
        assert!(
            should_record_said(Some(&other), "2026-08-24", &finding),
            "una sonda diversa lo stesso giorno è un'altra indicazione"
        );
    }

    #[test]
    fn no_entry_is_written_when_there_is_nothing_to_say() {
        assert!(!should_file_entry(
            &Verdict::Nothing { closest: None },
            false,
            None
        ));
    }

    #[test]
    fn no_second_entry_while_one_is_open() {
        let verdict = rank(vec![small("a", 800, 100)], &BTreeMap::new());
        assert!(!should_file_entry(&verdict, true, None));
    }

    #[test]
    fn the_same_thing_said_again_unchanged_does_not_get_a_new_entry() {
        let verdict = rank(vec![small("a", 800, 100)], &BTreeMap::new());
        let same = Said {
            day: "2026-08-20".into(),
            probe: "a".into(),
            subject: "s".into(),
            number: 800,
        };
        assert!(
            !should_file_entry(&verdict, false, Some(&same)),
            "ridirla uguale è ciò che insegna a ignorare la coda"
        );
        let worsened = Said {
            day: "2026-08-20".into(),
            probe: "a".into(),
            subject: "s".into(),
            number: 700,
        };
        assert!(
            should_file_entry(&verdict, false, Some(&worsened)),
            "800 su 700 è oltre un decimo"
        );
    }

    #[test]
    fn a_different_probe_earns_its_own_entry() {
        let verdict = rank(vec![small("b", 800, 100)], &BTreeMap::new());
        let previous = Said {
            day: "2026-08-20".into(),
            probe: "a".into(),
            subject: "s".into(),
            number: 900,
        };
        assert!(should_file_entry(&verdict, false, Some(&previous)));
    }

    #[test]
    fn a_still_deposit_is_not_examined_again() {
        assert!(!should_examine(Some(0)));
        assert!(!should_examine(Some(MOVEMENT_THRESHOLD - 1)));
        assert!(should_examine(Some(MOVEMENT_THRESHOLD)));
        assert!(
            !should_examine(None),
            "prima esecuzione: si registra il punto"
        );
    }
}
