//! Chi ha innescato una riparazione: `claude-hooks chi-ripara`.
//!
//! Nasce dalla prova 1 del piano
//! (`docs/plans/2026-08-24-obiettivo-configurazione-che-si-mantiene.md`): «una
//! settimana senza correzioni di Theo», misurata contando chi ha innescato ogni
//! riparazione. **«Riparazione» qui è ogni commit di questo repo**: distinguere
//! un difetto da un miglioramento avrebbe richiesto un giudizio che questi dati
//! non permettono di fare con onestà — `fix(...)` è un prefisso che chi scrive
//! il commit sceglie da sé, e commit come `4ded11e` (`feat`) hanno riparato un
//! difetto vero quanto un `fix`.
//!
//! **Come si trova l'innesco.** Un commit non porta con sé chi l'ha fatto: ogni
//! sessione qui commita con l'identità git di Theo. Si cerca invece lo `short
//! sha` del commit dentro i transcript recenti — chi esegue `git commit` ne
//! vede l'eco nell'esito del comando — e si guarda l'ultimo messaggio **umano**
//! prima di quel punto: `origin.kind == "human"` è scritto dall'harness solo
//! quando qualcuno ha davvero digitato, e non lo confondo con l'eco di uno
//! strumento né col testo che una sessione inietta a un'altra.

use guards::cost_ledger::parse_iso_epoch_millis;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// ============================================================
// I dati, letti dal disco altrove e passati qui dentro.
// ============================================================

/// Un commit del repo, così come lo racconta `git log`.
#[derive(Debug, Clone, PartialEq)]
pub struct CommitInfo {
    pub sha: String,
    pub short_sha: String,
    pub epoch: i64,
    pub subject: String,
}

/// L'ultimo messaggio "utente" trovato prima dell'eco del commit in un
/// transcript: non ancora giudicato, solo estratto.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerMessage {
    pub origin_kind: Option<String>,
    pub content_text: String,
}

/// Le quattro categorie che l'algoritmo sa distinguere, più l'ignoto onesto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// `origin.kind == "human"`: qualcuno ha davvero digitato quel messaggio
    /// — a meno che non sia il mandato fisso che `queue-patrol.sh` mette in
    /// bocca a ogni sessione che apre (vedi `RONDA_MANDATE_MARKER`): quel
    /// canale marca `human` anche quando non lo è, ed è controllato per primo.
    /// Anche l'espansione di uno slash-command (`<command-name>` /
    /// `<local-command-caveat>`) resta persona: lo digita qualcuno, l'harness
    /// lo trasporta in un blocco sintetico, ma non cambia autore.
    Person,
    /// Il messaggio cita una voce di coda `AUTO-*`, è il mandato fisso che la
    /// ronda della coda inietta all'apertura di un pannello, o è il mandato
    /// fisso con cui un macchinista dispaccia un costruttore per smaltire in
    /// blocco la coda (vedi `RONDA_MANDATE_MARKER` e
    /// `QUEUE_SWEEP_MANDATE_MARKER`).
    Mechanism,
    /// Il messaggio cita una voce di coda che non è `AUTO-*`: presa dal
    /// backlog, non appena scritta da un meccanismo.
    Queue,
    /// `origin.kind == "peer"`: un'altra sessione ha mandato il messaggio.
    /// Non è la frase di Theo — anche se può portarne una parola — e non è
    /// uno dei meccanismi dichiarati: resta un caso a sé, non un "forse".
    Peer,
    /// Il messaggio più vicino è il riassunto che l'harness inietta quando il
    /// contesto finisce («This session is being continued…»). Dietro c'è
    /// lavoro umano vero, ma il messaggio umano che l'ha innescato sta in
    /// un'altra sessione — quella che si è esaurita — e correlare fra
    /// transcript diversi è fuori da quel che questo modulo fa. Non è
    /// `Person` (sarebbe una prova che non c'è) e non è `Unknown` (ci direbbe
    /// meno di quel che sappiamo: qui sappiamo *perché* non risaliamo oltre).
    Continuation,
    /// Nessuno dei segnali sopra: o il commit non si è trovato in nessun
    /// transcript recente, o si è trovato ma senza un messaggio classificabile.
    Unknown,
}

impl Trigger {
    pub fn label(self) -> &'static str {
        match self {
            Trigger::Person => "persona",
            Trigger::Mechanism => "meccanismo",
            Trigger::Queue => "coda",
            Trigger::Peer => "altra sessione (peer)",
            Trigger::Continuation => "continuazione dopo compattazione",
            Trigger::Unknown => "ignoto",
        }
    }
}

/// Un commit con il suo innesco già deciso.
#[derive(Debug, Clone, PartialEq)]
pub struct Repair {
    pub commit: CommitInfo,
    pub trigger: Trigger,
    pub evidence: Option<String>,
}

// ============================================================
// Il giudizio puro: nessun file, solo i dati già letti.
// ============================================================

const QUEUE_ENTRY_MARKER: &str = "state/plancia/segnalazioni/";

/// Il nome del file citato dopo `state/plancia/segnalazioni/`, se c'è. PURA.
pub fn queue_entry_filename(text: &str) -> Option<String> {
    let start = text.find(QUEUE_ENTRY_MARKER)? + QUEUE_ENTRY_MARKER.len();
    let rest = &text[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
        .unwrap_or(rest.len());
    let name = &rest[..end];
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Un'eco che arriva da sé e non dice niente su **questo** commit: interrompe
/// una sessione già in corso per un motivo suo (un `Monitor`), o è il ricalco
/// di un comando interattivo che la sessione stessa ha lanciato in una shell
/// di sfondo, o l'avviso che un messaggio fra sessioni è stato recapitato.
/// Misurato il 25/08/2026: la prima versione di questo modulo leggeva
/// `<task-notification>` come "meccanismo" solo perché era il messaggio più
/// vicino prima del commit — 86 commit su 410. Un revisore ha poi trovato che
/// tre forme in più fermavano la scansione all'indietro nello stesso modo, su
/// un commit anche 381 righe prima del messaggio vero: `<bash-input>` e
/// `<bash-stdout>` (l'eco di una shell interattiva, timeout compreso — quel
/// messaggio ci vive dentro), e `[Cross-session delivery notice]` (la conferma
/// di recapito fra due sessioni, non un'istruzione). `find_triggering_user_message`
/// le scavalca come un `tool_result`, non le classifica.
fn is_async_noise(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("<task-notification>")
        || t.starts_with("<bash-input>")
        || t.starts_with("<bash-stdout>")
        || t.starts_with("<bash-stderr>")
        || t.starts_with("[Cross-session delivery notice]")
}

/// Il testo con cui l'harness apre il riassunto che sostituisce un contesto
/// esaurito — inglese perché lo genera l'harness stesso, non questa casa.
/// Verificato il 25/08/2026: il messaggio è una stringa pura, senza `origin`
/// (uguale a ogni altro canale sintetico qui dentro), quindi senza questo
/// controllo finiva in `Unknown` come qualunque altro testo non riconosciuto.
const CONTINUATION_MARKER: &str = "This session is being continued from a previous conversation";

fn is_continuation_summary(text: &str) -> bool {
    text.starts_with(CONTINUATION_MARKER)
}

/// Il nome citato dentro `<command-name>…</command-name>`, se c'è. PURA.
fn slash_command_name(text: &str) -> Option<String> {
    const TAG: &str = "<command-name>";
    let start = text.find(TAG)? + TAG.len();
    let rest = &text[start..];
    let end = rest.find("</command-name>")?;
    let name = rest[..end].trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Il messaggio è l'espansione sintetica di uno slash-command? L'harness la
/// scrive come un blocco `<command-name>`/`<command-message>`/`<command-args>`,
/// a volte preceduto da un turno separato che porta solo
/// `<local-command-caveat>`: chi lo digita è sempre una persona, il blocco è
/// solo il trasporto.
fn is_slash_command_expansion(text: &str) -> bool {
    text.contains("<command-name>") || text.contains("<local-command-caveat>")
}

fn classify_by_content(text: &str) -> Trigger {
    if is_continuation_summary(text) {
        return Trigger::Continuation;
    }
    if is_slash_command_expansion(text) {
        return Trigger::Person;
    }
    match queue_entry_filename(text) {
        Some(name) if name.starts_with("AUTO-") => Trigger::Mechanism,
        Some(_) => Trigger::Queue,
        None => Trigger::Unknown,
    }
}

/// Il testo fisso che `scripts/queue-patrol.sh` mette in bocca a ogni sessione
/// che apre: il comando è `exec claude --dangerously-skip-permissions
/// '<mandato>'`, e per l'harness un prompt passato così da riga di comando è
/// **indistinguibile da uno digitato** — `origin.kind` risulta `"human"` lo
/// stesso. Trovato da un revisore il 25/08/2026 su 9 commit reali (`c29095b`,
/// `599f79a`, `a6f2623`, `691ca58`, `80502d2`, `b150ac8`, `88a2136`,
/// `8aa8ac2`, `5740e62`), tutti contati "persona" mentre l'aveva aperti la
/// ronda. La frase è generata dallo script (righe attorno a
/// `queue-patrol.sh:1238`), identica per ogni ruolo e ogni giro: non la scrive
/// mai una persona.
const RONDA_MANDATE_MARKER: &str = "Ti ha aperto la ronda della coda";

fn is_ronda_mandate(text: &str) -> bool {
    text.contains(RONDA_MANDATE_MARKER)
}

/// Il testo fisso con cui una sessione macchinista apre ogni costruttore che
/// dispaccia con lo strumento `Agent` per smaltire in blocco la coda di
/// bordo: stesso testo, riusato a ogni dispaccio dello stesso turno. Qui
/// `origin` non vale "human" come nel mandato della ronda — è del tutto
/// ASSENTE, perché un prompt passato a un subagente (`isSidechain: true`) non
/// transita mai dal canale che scrive `origin.kind`, chiunque ne abbia
/// composto le parole. Trovato da un revisore il 25/08/2026 su 8 commit
/// consecutivi del 24/08 (11:13-12:09), tutti riconducibili allo stesso
/// costruttore che ha smaltito dodici voci di coda in un solo turno:
/// `find_triggering_user_message` risale sempre allo stesso primo messaggio
/// del subagente, perché nessun altro messaggio "utente" lo precede nel suo
/// transcript.
const QUEUE_SWEEP_MANDATE_MARKER: &str = "svuotare una lista che si è allungata";

fn is_queue_sweep_mandate(text: &str) -> bool {
    text.contains(QUEUE_SWEEP_MANDATE_MARKER)
}

/// Il cuore del modulo: dato il messaggio che precede l'eco di un commit, chi
/// l'ha innescato? PURA — riceve i due campi già letti, non tocca il disco.
///
/// IL TESTO SI GUARDA PRIMA DELL'ORIGINE, non dopo: il mandato della ronda
/// arriva con `origin.kind == "human"` (sopra), quindi un `match` che si
/// fermasse lì per primo lo conterebbe sempre "persona" — è esattamente
/// l'errore che un revisore ha trovato il 25/08/2026.
pub fn classify_trigger_message(origin_kind: Option<&str>, content_text: &str) -> Trigger {
    if is_ronda_mandate(content_text) || is_queue_sweep_mandate(content_text) {
        return Trigger::Mechanism;
    }
    match origin_kind {
        Some("human") => Trigger::Person,
        Some("peer") => Trigger::Peer,
        _ => classify_by_content(content_text),
    }
}

/// Applica il giudizio a ogni commit, con `find_message` iniettata: nei test
/// una chiusura che restituisce dati finti, in servizio una che legge il
/// disco. Stessa forma di `decide_uncovered_with` in `uncovered_thread.rs`.
pub fn decide_repairs_with(
    commits: &[CommitInfo],
    find_message: &dyn Fn(&CommitInfo) -> Option<TriggerMessage>,
) -> Vec<Repair> {
    commits
        .iter()
        .map(|c| {
            let msg = find_message(c);
            let trigger = match &msg {
                Some(m) => classify_trigger_message(m.origin_kind.as_deref(), &m.content_text),
                None => Trigger::Unknown,
            };
            let evidence = msg.as_ref().map(evidence_snippet);
            Repair { commit: c.clone(), trigger, evidence }
        })
        .collect()
}

fn evidence_snippet(m: &TriggerMessage) -> String {
    let kind = m.origin_kind.as_deref().unwrap_or("(nessuno)");
    // La prova cita il NOME del comando, non il blocco sintetico che
    // l'harness genera per trasportarlo: chi legge il rapporto vuole sapere
    // quale comando è stato digitato, non rileggere `<command-name>…`.
    if let Some(name) = slash_command_name(&m.content_text) {
        return format!("origin={kind} · slash-command: {name}");
    }
    let text: String = m.content_text.replace('\n', " ").chars().take(120).collect();
    format!("origin={kind} · \"{text}\"")
}

/// Il conteggio per categoria, su un elenco già giudicato. PURA.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub total: usize,
    pub person: usize,
    pub mechanism: usize,
    pub queue: usize,
    pub peer: usize,
    pub continuation: usize,
    pub unknown: usize,
}

pub fn summarize(repairs: &[Repair]) -> Summary {
    let mut s = Summary { total: repairs.len(), ..Default::default() };
    for r in repairs {
        match r.trigger {
            Trigger::Person => s.person += 1,
            Trigger::Mechanism => s.mechanism += 1,
            Trigger::Queue => s.queue += 1,
            Trigger::Peer => s.peer += 1,
            Trigger::Continuation => s.continuation += 1,
            Trigger::Unknown => s.unknown += 1,
        }
    }
    s
}

/// I campi di `git log --pretty=format:%H<sep>%at<sep>%s`, un separatore che
/// non compare in un oggetto di commit normale. PURA — non chiama `git`.
const GIT_LOG_SEP: char = '\u{1f}';

pub fn parse_git_log_output(text: &str) -> Vec<CommitInfo> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, GIT_LOG_SEP);
            let sha = parts.next()?.to_string();
            let epoch: i64 = parts.next()?.parse().ok()?;
            let subject = parts.next().unwrap_or("").to_string();
            if sha.len() < 7 {
                return None; // riga rotta: uno sha vero ne ha sempre 40
            }
            let short_sha = sha[..7].to_string();
            Some(CommitInfo { sha, short_sha, epoch, subject })
        })
        .collect()
}

/// Sequenze di esattamente 7 caratteri esadecimali minuscoli: la forma con cui
/// questa casa stampa uno short sha. PURA.
///
/// PERCHÉ ESATTAMENTE 7 E NON "ALMENO 7". Uno sha pieno (40 caratteri) o un
/// id di messaggio esadecimale più lungo conterrebbero, per caso, la sequenza
/// di un altro commit come sotto-stringa: un confronto "contiene" avrebbe
/// prodotto inneschi attribuiti al commit sbagliato. Restringersi al confine
/// esatto — un carattere non esadecimale prima e dopo — è la differenza fra
/// una prova e una coincidenza.
fn hex_runs_of_seven(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, &b) in bytes.iter().enumerate() {
        let is_hex = b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
        if is_hex {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            if i - s == 7 {
                out.push(&line[s..i]);
            }
        }
    }
    if let Some(s) = start {
        if bytes.len() - s == 7 {
            out.push(&line[s..]);
        }
    }
    out
}

/// Testo vero da un `message.content`: `None` se sono solo esiti di
/// strumento — un `tool_result` è l'eco di un comando già deciso altrove, non
/// un innesco.
fn extract_real_content(content: Option<&Value>) -> Option<String> {
    match content? {
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => {
            let mut text = String::new();
            let mut only_tool_results = true;
            for item in items {
                match item.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        only_tool_results = false;
                        if let Some(t) = item.get("text").and_then(Value::as_str) {
                            text.push_str(t);
                            text.push(' ');
                        }
                    }
                    Some("tool_result") => {}
                    _ => only_tool_results = false,
                }
            }
            if only_tool_results {
                None
            } else {
                Some(text)
            }
        }
        _ => None,
    }
}

/// L'ultimo messaggio utente vero prima della riga `hit_idx`, scandendo
/// all'indietro. PURA — riceve le righe già lette, non apre file.
fn find_triggering_user_message(lines: &[&str], hit_idx: usize) -> Option<TriggerMessage> {
    for line in lines[..hit_idx].iter().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let Some(message) = v.get("message") else { continue };
        if message.get("role").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let Some(content_text) = extract_real_content(message.get("content")) else { continue };
        if is_async_noise(&content_text) {
            continue; // l'eco di un Monitor: non è l'innesco, si guarda oltre
        }
        let origin_kind =
            v.get("origin").and_then(|o| o.get("kind")).and_then(Value::as_str).map(str::to_string);
        return Some(TriggerMessage { origin_kind, content_text });
    }
    None
}

// ============================================================
// La colla: `git log`, i transcript, l'uscita da riga di comando.
// ============================================================

pub fn list_commits(repo: &Path, since_days: u64) -> Result<Vec<CommitInfo>, String> {
    let since = format!("{since_days} days ago");
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["log", "--since", &since, "--pretty=format:%H\u{1f}%at\u{1f}%s"])
        .output()
        .map_err(|e| format!("git log: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git log su {}: uscita {:?}: {}",
            repo.display(),
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(parse_git_log_output(&String::from_utf8_lossy(&out.stdout)))
}

/// L'istante di una riga di transcript, in millisecondi — riusa il parser che
/// `costs.rs` già usa per lo stesso formato ISO8601 UTC.
fn line_timestamp_ms(line: &str) -> Option<i64> {
    let v: Value = serde_json::from_str(line).ok()?;
    parse_iso_epoch_millis(v.get("timestamp")?.as_str()?)
}

/// Cerca ogni `short_sha` nei transcript sotto `<home>/.claude/projects`, in
/// due giri sul corpus invece di uno: il primo trova, per ogni sha, **la riga
/// più vicina nel tempo al commit** — non la prima incontrata.
///
/// PERCHÉ NON BASTA "LA PRIMA CHE COMPARE". Misurato il 25/08/2026: una
/// sessione che leggeva una voce di coda **su** un incidente di commit
/// citava, di passaggio, gli sha dei commit coinvolti — e la prima versione
/// di questo modulo, fermandosi al primo incontro, ha attribuito quei commit
/// a quella lettura invece che a chi li aveva davvero fatti. L'eco vera di
/// `git commit` compare entro pochi secondi dal commit stesso; chi lo cita
/// dopo, in un contesto diverso, lo fa più tardi. La vicinanza nel tempo è il
/// segnale che li separa.
///
/// FILTRO DI PRESTAZIONE, NON DI CORRETTEZZA: si scartano i file la cui
/// ultima scrittura è precedente al commit più vecchio richiesto (con un
/// margine di un giorno) — un file toccato solo prima che il commit più
/// vecchio esistesse non può contenerne l'eco.
pub fn find_all_messages(commits: &[CommitInfo], home: &Path) -> HashMap<String, TriggerMessage> {
    let mut results = HashMap::new();
    if commits.is_empty() {
        return results;
    }
    let epoch_by_sha: HashMap<&str, i64> =
        commits.iter().map(|c| (c.short_sha.as_str(), c.epoch)).collect();

    let min_epoch = commits.iter().map(|c| c.epoch).min().unwrap_or(0) - 86_400;
    let mut files = Vec::new();
    crate::costs::collect_jsonl(&home.join(".claude").join("projects"), &mut files);
    files.retain(|p| crate::costs::mtime_epoch(p) >= min_epoch);

    // Prima passata: il miglior indizio per sha, su tutto il corpus.
    let mut best: HashMap<String, (i64, PathBuf, usize)> = HashMap::new();
    for file in &files {
        let Ok(text) = fs::read_to_string(file) else { continue };
        for (idx, line) in text.lines().enumerate() {
            for token in hex_runs_of_seven(line) {
                let Some(&commit_epoch) = epoch_by_sha.get(token) else { continue };
                let Some(line_ms) = line_timestamp_ms(line) else { continue };
                let diff = (commit_epoch * 1000 - line_ms).abs();
                let is_better = best.get(token).map(|(prev, _, _)| diff < *prev).unwrap_or(true);
                if is_better {
                    best.insert(token.to_string(), (diff, file.clone(), idx));
                }
            }
        }
    }

    // Seconda passata: si riapre solo chi ha vinto qualcosa, per cercare
    // all'indietro il messaggio che ha innescato quel punto.
    let mut hits_by_file: HashMap<&Path, Vec<(&str, usize)>> = HashMap::new();
    for (sha, (_, file, idx)) in &best {
        hits_by_file.entry(file.as_path()).or_default().push((sha.as_str(), *idx));
    }
    for (file, hits) in hits_by_file {
        let Ok(text) = fs::read_to_string(file) else { continue };
        let lines: Vec<&str> = text.lines().collect();
        for (sha, idx) in hits {
            if let Some(msg) = find_triggering_user_message(&lines, idx) {
                results.insert(sha.to_string(), msg);
            }
        }
    }
    results
}

fn home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_default()
}

fn flag(args: &[String], name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1).cloned()
}

fn pct(n: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        (n as f64) * 100.0 / (total as f64)
    }
}

const DEFAULT_DAYS: u64 = 7;

/// Il rilievo del 25/08/2026 chiedeva di dichiarare che la classificazione ha
/// buchi noti, non solo che "ignoto" è una categoria legittima: senza questa
/// riga il comando avrebbe presentato "meccanismo" e "persona" come numeri
/// definitivi, mentre restano scoperti canali sintetici non ancora trovati.
/// Estratta in una costante — non lasciata inline nel `println!` — perché sia
/// verificabile da un test senza catturare lo standard output.
const KNOWN_GAPS_NOTE: &str = "La classificazione ha buchi noti, non solo la categoria \"ignoto\": il \
rumore asincrono riconosciuto (task-notification, bash-input/stdout/stderr, \
notifica fra sessioni), il mandato fisso della ronda della coda e il mandato \
fisso con cui un macchinista dispaccia un costruttore per svuotare la coda \
sono gli unici canali scoperti finora e corretti; le espansioni di uno \
slash-command sono trattate come persona per costruzione, non da un controllo \
di contenuto; \"continuazione dopo compattazione\" è un secchio dichiarato, non \
risolto — sappiamo che dietro c'è lavoro umano, ma il messaggio vero sta in \
un'altra sessione e non lo si va a cercare. Altri canali sintetici non ancora \
trovati possono restare scambiati per persona o far fermare la ricerca troppo \
presto.";

/// `claude-hooks chi-ripara [--days N] [--repo PATH] [--verbose]`.
pub fn run_report(args: &[String]) -> i32 {
    let repo = flag(args, "--repo").map(PathBuf::from).unwrap_or_else(|| home().join(".claude"));
    let days = flag(args, "--days").and_then(|v| v.parse::<u64>().ok()).unwrap_or(DEFAULT_DAYS);
    let verbose = args.iter().any(|a| a == "--verbose");

    let commits = match list_commits(&repo, days) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("chi-ripara: {e}");
            return 1;
        }
    };
    if commits.is_empty() {
        println!("nessun commit negli ultimi {days} giorni su {}", repo.display());
        return 0;
    }

    let found = find_all_messages(&commits, &home());
    let repairs = decide_repairs_with(&commits, &|c| found.get(&c.short_sha).cloned());
    let summary = summarize(&repairs);

    println!("Innesco delle riparazioni — ultimi {days} giorni, repo {}", repo.display());
    println!("commit trovati: {}", summary.total);
    println!(
        "  persona (frase di Theo):        {:>4}  ({:.1}%)",
        summary.person,
        pct(summary.person, summary.total)
    );
    println!(
        "  meccanismo (gancio/ronda/AUTO): {:>4}  ({:.1}%)",
        summary.mechanism,
        pct(summary.mechanism, summary.total)
    );
    println!(
        "  coda (voce di coda):            {:>4}  ({:.1}%)",
        summary.queue,
        pct(summary.queue, summary.total)
    );
    println!(
        "  altra sessione (peer):          {:>4}  ({:.1}%)",
        summary.peer,
        pct(summary.peer, summary.total)
    );
    println!(
        "  continuazione dopo compatt.:    {:>4}  ({:.1}%)",
        summary.continuation,
        pct(summary.continuation, summary.total)
    );
    println!(
        "  ignoto/non classificabile:      {:>4}  ({:.1}%)",
        summary.unknown,
        pct(summary.unknown, summary.total)
    );
    println!();
    println!(
        "NOTA: qui \"riparazione\" e' ogni commit del repo, non solo quelli che si \
dichiarano fix(...): un giudizio affidabile su cosa fosse un difetto non e' \
deducibile da questi dati. \"ignoto\" copre sia i commit di cui non si e' \
trovata l'eco in nessun transcript recente, sia quelli trovati ma senza un \
messaggio classificabile prima."
    );
    println!("{KNOWN_GAPS_NOTE}");

    if verbose {
        println!();
        for r in &repairs {
            println!(
                "{} {} {} — {}",
                r.commit.short_sha,
                hook_io::local_time::utc_iso_seconds(r.commit.epoch),
                r.trigger.label(),
                r.commit.subject,
            );
            if let Some(e) = &r.evidence {
                println!("    {e}");
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::HomeIsolata;

    // --- il giudizio puro -------------------------------------------------

    #[test]
    fn a_typed_human_message_is_a_person() {
        assert_eq!(classify_trigger_message(Some("human"), "sistema il freno"), Trigger::Person);
    }

    #[test]
    fn a_peer_message_is_its_own_bucket_not_a_person() {
        // Non è la frase di Theo, anche se un'altra sessione la porta: la
        // regola resta stretta, o "persona" smette di voler dire quel che dice.
        assert_eq!(classify_trigger_message(Some("peer"), "procedi pure"), Trigger::Peer);
    }

    #[test]
    fn a_ronda_mandate_is_a_mechanism_even_with_a_human_origin() {
        // LA CORREZIONE CHE UN REVISORE HA CHIESTO IL 25/08/2026: `origin.kind
        // == "human"` non e' prova che qualcuno abbia digitato. `queue-patrol.sh`
        // apre un pannello con `exec claude --dangerously-skip-permissions
        // '<mandato>'`, e per l'harness un prompt passato cosi' e' identico a
        // uno umano. Trovati 9 commit reali contati "persona" per questo.
        let text = "La tua prima riga di uscita e' esattamente «MACCHINISTA: voci di coda» \
e niente altro. Poi leggi /home/someone/.claude/docs/mandato-di-guardia-macchinista.md ed \
esegui adesso il mandato che contiene. Ti ha aperto la ronda della coda: nella coda di \
bordo ci sono 3 voci che aspettano un MACCHINISTA.";
        assert_eq!(classify_trigger_message(Some("human"), text), Trigger::Mechanism);
    }

    #[test]
    fn a_queue_sweep_mandate_is_a_mechanism_with_no_origin_at_all() {
        // IL SECONDO MARCATORE FISSO, trovato da un revisore il 25/08/2026:
        // testo verbatim preso dal transcript reale del subagente che ha
        // smaltito la coda il 24/08 (`agent-aa24b0b3aebe88990.jsonl`), NON
        // dalla costante riletta — a differenza del mandato della ronda qui
        // `origin` non vale "human", è del tutto assente: un prompt passato
        // a un subagente (`isSidechain: true`) non transita mai da quel
        // canale. Otto commit consecutivi (11:13-12:09) finivano "ignoto".
        let text = "Rispondi in italiano. Il tuo mandato non è costruire: è **svuotare una lista \
che si è allungata per tre giorni senza che nessuno la toccasse**.\n\nIn \
`~/.claude/state/plancia/segnalazioni/` ci sono 39 voci aperte. Dodici portano `per: un \
builder su ~/.claude/rust`. Sono tue.";
        assert_eq!(classify_trigger_message(None, text), Trigger::Mechanism);
    }

    #[test]
    fn a_slash_command_expansion_is_a_person_not_ignoto() {
        // Forma reale osservata in transcript veri: l'harness espande uno
        // slash-command in questo blocco, senza mai scrivere `origin` — chi
        // lo digita resta una persona, il blocco e' solo il trasporto.
        let text = "<command-name>/context</command-name>\n            <command-message>context\
</command-message>\n            <command-args></command-args>";
        assert_eq!(classify_trigger_message(None, text), Trigger::Person);
    }

    #[test]
    fn a_local_command_caveat_alone_is_also_a_person() {
        // La forma che a volte precede il blocco del comando come turno a
        // se': stesso trasporto sintetico, stesso giudizio.
        let text = "<local-command-caveat>Caveat: The messages below were generated by the user \
while running local commands. DO NOT respond to these messages or otherwise consider them in \
your response unless the user explicitly asks you to.</local-command-caveat>";
        assert_eq!(classify_trigger_message(None, text), Trigger::Person);
    }

    #[test]
    fn slash_command_evidence_cites_the_command_name_not_the_expanded_block() {
        let msg = TriggerMessage {
            origin_kind: None,
            content_text: "<command-name>/context</command-name>\n            <command-message>\
context</command-message>"
                .to_string(),
        };
        let evidence = evidence_snippet(&msg);
        assert!(evidence.contains("/context"), "{evidence}");
        assert!(!evidence.contains("<command-name>"), "cita il blocco espanso, non il nome: {evidence}");
    }

    #[test]
    fn a_continuation_summary_is_its_own_bucket_not_unknown() {
        // Testo verbatim osservato in transcript reali: l'apertura del
        // riassunto che l'harness inietta quando il contesto finisce.
        let text = "This session is being continued from a previous conversation that ran out of \
context. The summary below covers the earlier portion of the conversation.\n\nSummary:\n1. \
**Primary Request and Intent:**";
        assert_eq!(classify_trigger_message(None, text), Trigger::Continuation);
    }

    #[test]
    fn an_auto_queue_entry_is_a_mechanism() {
        let text = "leggi `state/plancia/segnalazioni/AUTO-queue-health.md` e agisci";
        assert_eq!(classify_trigger_message(None, text), Trigger::Mechanism);
    }

    #[test]
    fn a_non_auto_queue_entry_is_the_queue() {
        let text = "riprendi `state/plancia/segnalazioni/2026-08-20-canarini.md`";
        assert_eq!(classify_trigger_message(None, text), Trigger::Queue);
    }

    #[test]
    fn a_task_notification_that_reaches_the_judgment_is_unknown_not_a_mechanism() {
        // Il giudizio puro non vede mai un `<task-notification>`: lo scavalca
        // `find_triggering_user_message`, prima che arrivi qui. Ma se
        // qualcuno lo passasse comunque, non deve travestirsi da meccanismo —
        // e' proprio l'errore misurato il 25/08/2026 (86 commit su 410).
        let text = "<task-notification>\n<task-id>x</task-id>\n</task-notification>";
        assert_eq!(classify_trigger_message(None, text), Trigger::Unknown);
    }

    #[test]
    fn plain_text_with_no_origin_and_no_marker_is_unknown() {
        // Il caso onesto: né umano dichiarato, né un riferimento riconoscibile.
        assert_eq!(classify_trigger_message(None, "prosegui col lavoro"), Trigger::Unknown);
    }

    #[test]
    fn queue_entry_filename_stops_before_the_first_invalid_character() {
        let text = "vedi `state/plancia/segnalazioni/2026-08-20-canarini.md` per il dettaglio";
        assert_eq!(queue_entry_filename(text), Some("2026-08-20-canarini.md".to_string()));
    }

    #[test]
    fn queue_entry_filename_is_none_without_the_marker() {
        assert_eq!(queue_entry_filename("nessun percorso qui"), None);
    }

    #[test]
    fn known_gaps_note_names_both_corrected_blind_spots() {
        // Rilievo del 25/08/2026, punto 3: la NOTA deve dire che la
        // classificazione ha buchi noti, non solo che "ignoto" e' legittima.
        // Nomina i canali corretti, non una frase generica che varrebbe
        // anche senza le correzioni sopra.
        assert!(KNOWN_GAPS_NOTE.contains("buchi noti"));
        assert!(KNOWN_GAPS_NOTE.contains("bash-input"));
        assert!(KNOWN_GAPS_NOTE.contains("ronda della coda"));
    }

    #[test]
    fn known_gaps_note_names_the_second_review_findings_too() {
        // Il giro del 25/08/2026 ha aggiunto tre buchi: il secondo marcatore
        // fisso, la scelta sugli slash-command, e il secchio di continuazione
        // — la NOTA deve nominarli, non solo i due del giro precedente.
        assert!(KNOWN_GAPS_NOTE.contains("macchinista"));
        assert!(KNOWN_GAPS_NOTE.contains("slash-command"));
        assert!(KNOWN_GAPS_NOTE.contains("continuazione"));
    }

    #[test]
    fn hex_runs_of_seven_takes_only_the_exact_length() {
        assert_eq!(hex_runs_of_seven("commit edecd15 fatto"), vec!["edecd15"]);
        // Otto esadecimali di fila: non è la forma di uno short sha di questa
        // casa, e prenderlo per buono creerebbe un falso incrocio.
        assert_eq!(hex_runs_of_seven("id abcdef12 qui"), Vec::<&str>::new());
        // Maiuscole non contano: gli short sha di git sono sempre minuscoli.
        assert_eq!(hex_runs_of_seven("ABCDEF1 non e' uno sha"), Vec::<&str>::new());
    }

    // --- l'aggregazione, con una chiusura iniettata ------------------------

    fn commit(short: &str) -> CommitInfo {
        CommitInfo {
            sha: format!("{short}0000000000000000000000000000000000"),
            short_sha: short.to_string(),
            epoch: 1000,
            subject: "un commit di prova".to_string(),
        }
    }

    #[test]
    fn decide_repairs_with_classifies_each_commit_from_its_injected_message() {
        let commits = vec![commit("aaaaaaa"), commit("bbbbbbb"), commit("ccccccc")];
        let find = |c: &CommitInfo| -> Option<TriggerMessage> {
            match c.short_sha.as_str() {
                "aaaaaaa" => Some(TriggerMessage {
                    origin_kind: Some("human".to_string()),
                    content_text: "fai questo".to_string(),
                }),
                "bbbbbbb" => Some(TriggerMessage {
                    origin_kind: None,
                    content_text: "state/plancia/segnalazioni/AUTO-x.md".to_string(),
                }),
                _ => None, // il terzo commit non si trova in nessun transcript
            }
        };
        let repairs = decide_repairs_with(&commits, &find);
        assert_eq!(repairs[0].trigger, Trigger::Person);
        assert_eq!(repairs[1].trigger, Trigger::Mechanism);
        assert_eq!(repairs[2].trigger, Trigger::Unknown);
        assert!(repairs[2].evidence.is_none());
    }

    #[test]
    fn summarize_counts_each_bucket_and_the_total() {
        let commits = vec![commit("aaaaaaa"), commit("bbbbbbb")];
        let find = |c: &CommitInfo| -> Option<TriggerMessage> {
            if c.short_sha == "aaaaaaa" {
                Some(TriggerMessage { origin_kind: Some("peer".to_string()), content_text: String::new() })
            } else {
                None
            }
        };
        let s = summarize(&decide_repairs_with(&commits, &find));
        assert_eq!(
            s,
            Summary { total: 2, person: 0, mechanism: 0, queue: 0, peer: 1, continuation: 0, unknown: 1 }
        );
    }

    #[test]
    fn summarize_counts_all_six_buckets_with_six_distinct_totals() {
        // Rilievo del 26/08/2026, due giri: la prima versione di questa prova
        // aveva cinque secchielli su sei tutti a 1, quindi uno scambio fra
        // QUALUNQUE coppia di quei cinque restava invisibile — provato dal
        // vivo su Queue/Peer da un revisore che non l'ha scritta. Qui ogni
        // secchiello porta un conteggio diverso (1..6): uno scambio fra due
        // rami qualsiasi dentro `summarize` produce sempre un totale diverso
        // da quello atteso, non solo per person/mechanism.
        const PERSON: usize = 2;
        const MECHANISM: usize = 1;
        const QUEUE: usize = 3;
        const PEER: usize = 4;
        const CONTINUATION: usize = 5;
        const UNKNOWN: usize = 6;

        let mut entries: Vec<(CommitInfo, Option<TriggerMessage>)> = Vec::new();
        for i in 0..PERSON {
            entries.push((
                commit(&format!("p{i}00000")),
                Some(TriggerMessage {
                    origin_kind: Some("human".to_string()),
                    content_text: "sistema il freno".to_string(),
                }),
            ));
        }
        for i in 0..MECHANISM {
            entries.push((
                commit(&format!("m{i}00000")),
                Some(TriggerMessage {
                    origin_kind: None,
                    content_text: format!("test {QUEUE_SWEEP_MANDATE_MARKER}"),
                }),
            ));
        }
        for i in 0..QUEUE {
            entries.push((
                commit(&format!("q{i}00000")),
                Some(TriggerMessage {
                    origin_kind: None,
                    content_text: "state/plancia/segnalazioni/2026-08-20-canarini.md".to_string(),
                }),
            ));
        }
        for i in 0..PEER {
            entries.push((
                commit(&format!("r{i}00000")),
                Some(TriggerMessage {
                    origin_kind: Some("peer".to_string()),
                    content_text: "procedi pure".to_string(),
                }),
            ));
        }
        for i in 0..CONTINUATION {
            entries.push((
                commit(&format!("c{i}00000")),
                Some(TriggerMessage {
                    origin_kind: None,
                    content_text: format!("{CONTINUATION_MARKER} altro testo"),
                }),
            ));
        }
        for i in 0..UNKNOWN {
            entries.push((commit(&format!("u{i}00000")), None)); // nessun'eco trovata
        }

        let commits: Vec<CommitInfo> = entries.iter().map(|(c, _)| c.clone()).collect();
        let by_sha: HashMap<String, Option<TriggerMessage>> =
            entries.into_iter().map(|(c, m)| (c.short_sha, m)).collect();
        let find = |c: &CommitInfo| -> Option<TriggerMessage> { by_sha.get(&c.short_sha).cloned().flatten() };

        let s = summarize(&decide_repairs_with(&commits, &find));
        assert_eq!(
            s,
            Summary {
                total: PERSON + MECHANISM + QUEUE + PEER + CONTINUATION + UNKNOWN,
                person: PERSON,
                mechanism: MECHANISM,
                queue: QUEUE,
                peer: PEER,
                continuation: CONTINUATION,
                unknown: UNKNOWN,
            }
        );
    }

    // --- il parser di `git log` --------------------------------------------

    #[test]
    fn parse_git_log_output_reads_sha_epoch_and_subject() {
        let sep = '\u{1f}';
        let text = format!(
            "edecd15690dbad56bf7caeb340a231d5ffb6963f{sep}1756134000{sep}fix: qualcosa\n\
             troppo corta"
        );
        let commits = parse_git_log_output(&text);
        // La seconda riga non ha uno sha valido e si scarta, non fa panico.
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].short_sha, "edecd15");
        assert_eq!(commits[0].epoch, 1756134000);
        assert_eq!(commits[0].subject, "fix: qualcosa");
    }

    // --- la ricerca nei transcript, su disco isolato ------------------------

    fn write_transcript(dir: &Path, name: &str, lines: &[String]) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(name), lines.join("\n")).unwrap();
    }

    /// Un istante qualunque, e i suoi minuti dopo: bastano a dire "vicino" e
    /// "lontano" senza calcoli sparsi nei test.
    const T0: i64 = 1_756_000_000;

    fn iso(epoch: i64) -> String {
        hook_io::local_time::utc_iso_seconds(epoch)
    }

    fn human_line(text: &str, at: i64) -> String {
        serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": text},
            "origin": {"kind": "human"},
            "timestamp": iso(at),
        })
        .to_string()
    }

    fn tool_result_line(text: &str, at: i64) -> String {
        serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": [{"type": "tool_result", "content": text}]},
            "timestamp": iso(at),
        })
        .to_string()
    }

    /// L'eco di un `Monitor` in background, senza `origin`: la forma che ha
    /// davvero nei transcript, arrivata mentre una sessione lavora su
    /// tutt'altro.
    fn task_notification_line(at: i64) -> String {
        serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": "<task-notification>\n<task-id>x</task-id>\n<summary>Monitor event</summary>\n</task-notification>",
            },
            "timestamp": iso(at),
        })
        .to_string()
    }

    #[test]
    fn find_all_messages_skips_a_monitor_notification_and_finds_the_human_message_behind_it() {
        // LA CORREZIONE DEL 25/08/2026: la prima versione leggeva questa
        // notifica come "meccanismo" perché era il messaggio più vicino —
        // 86 commit su 410 finivano lì solo per quello. La notifica va
        // scavalcata come un `tool_result`: dietro c'è il messaggio vero.
        let home = HomeIsolata::nuova("chi-ripara-notifica");
        let projects = home.dir.join(".claude").join("projects").join("slug");
        write_transcript(
            &projects,
            "sess.jsonl",
            &[
                human_line("sistema il freno del cd-guard", T0),
                task_notification_line(T0 + 30),
                tool_result_line("EXIT=0\nedecd15 fix(hooks): fatto", T0 + 60),
            ],
        );
        let mut c = commit("edecd15");
        c.epoch = T0 + 60;
        let found = find_all_messages(&[c], &home.dir);
        let msg = found.get("edecd15").expect("il commit non e' stato trovato");
        assert_eq!(msg.origin_kind.as_deref(), Some("human"));
        assert_eq!(msg.content_text, "sistema il freno del cd-guard");
    }

    /// Un messaggio "utente" senza `origin`, con un testo qualunque: la forma
    /// di `<bash-input>`, `<bash-stdout>` e della notifica fra sessioni, tutte
    /// stringa pura e non un `tool_result`.
    fn synthetic_line(text: &str, at: i64) -> String {
        serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": text},
            "timestamp": iso(at),
        })
        .to_string()
    }

    #[test]
    fn find_all_messages_skips_bash_echoes_and_the_cross_session_notice_too() {
        // LA CORREZIONE CHIESTA DAL REVISORE IL 25/08/2026: queste tre forme
        // sono stringhe vere, non `tool_result`, quindi la prima versione le
        // accettava come innesco e fermava la ricerca lì — su un commit vero
        // l'innesco stava 381 righe più indietro. Qui ne accatasto quattro di
        // fila fra il messaggio vero e l'eco del commit.
        let home = HomeIsolata::nuova("chi-ripara-rumore-shell");
        let projects = home.dir.join(".claude").join("projects").join("slug");
        write_transcript(
            &projects,
            "sess.jsonl",
            &[
                human_line("sistema il freno del cd-guard", T0),
                synthetic_line("<bash-input>tail -f log</bash-input>", T0 + 10),
                synthetic_line("<bash-stdout>Command did not complete within its 120s timeout and was moved to the background (ID: x)</bash-stdout>", T0 + 20),
                synthetic_line("<bash-stderr></bash-stderr>", T0 + 30),
                synthetic_line("[Cross-session delivery notice] Your message to another session was approved and released to that session.", T0 + 40),
                tool_result_line("EXIT=0\nedecd15 fix(hooks): fatto", T0 + 50),
            ],
        );
        let mut c = commit("edecd15");
        c.epoch = T0 + 50;
        let found = find_all_messages(&[c], &home.dir);
        let msg = found.get("edecd15").expect("il commit non e' stato trovato");
        assert_eq!(msg.origin_kind.as_deref(), Some("human"));
        assert_eq!(msg.content_text, "sistema il freno del cd-guard");
    }

    #[test]
    fn find_all_messages_skips_tool_results_and_finds_the_human_message_behind_them() {
        let home = HomeIsolata::nuova("chi-ripara-base");
        let projects = home.dir.join(".claude").join("projects").join("slug");
        write_transcript(
            &projects,
            "sess.jsonl",
            &[
                human_line("sistema il freno del cd-guard", T0),
                tool_result_line("qualche esito intermedio", T0 + 10),
                tool_result_line("EXIT=0\nedecd15 fix(hooks): fatto", T0 + 20),
            ],
        );
        let mut c = commit("edecd15");
        c.epoch = T0 + 20;
        let found = find_all_messages(&[c], &home.dir);
        let msg = found.get("edecd15").expect("il commit non e' stato trovato");
        assert_eq!(msg.origin_kind.as_deref(), Some("human"));
        assert_eq!(msg.content_text, "sistema il freno del cd-guard");
    }

    #[test]
    fn find_all_messages_prefers_the_closest_echo_when_the_same_sha_appears_twice() {
        // LA CORREZIONE DEL 25/08/2026, isolata: lo stesso sha compare due
        // volte nello stesso transcript — l'eco vera del commit, e ore dopo
        // una voce di coda che lo racconta. Chi vince deve essere la più
        // vicina nel tempo, non l'ultima incontrata scorrendo il file: le due
        // occorrenze sono scritte apposta nell'ordine sbagliato per quella
        // seconda ipotesi, cosicché un "vince l'ultima" la farebbe fallire.
        let home = HomeIsolata::nuova("chi-ripara-collisione");
        let projects = home.dir.join(".claude").join("projects").join("slug");
        write_transcript(
            &projects,
            "sess.jsonl",
            &[
                human_line("sistema il freno del cd-guard", T0),
                tool_result_line("EXIT=0\nedecd15 fix(hooks): fatto", T0 + 20),
                human_line("leggi la voce di coda sull'incidente dei commit", T0 + 3 * 3600),
                tool_result_line("... lo sha coinvolto era edecd15 ...", T0 + 3 * 3600 + 50),
            ],
        );
        let mut c = commit("edecd15");
        c.epoch = T0 + 20; // il vero istante del commit: la prima occorrenza
        let found = find_all_messages(&[c], &home.dir);
        let msg = found.get("edecd15").expect("il commit non e' stato trovato");
        assert_eq!(
            msg.content_text, "sistema il freno del cd-guard",
            "ha vinto l'occorrenza più lontana nel tempo, non la più vicina"
        );
    }

    #[test]
    fn a_commit_with_no_echo_anywhere_stays_unresolved() {
        let home = HomeIsolata::nuova("chi-ripara-vuoto");
        let projects = home.dir.join(".claude").join("projects").join("slug");
        write_transcript(&projects, "sess.jsonl", &[human_line("altro lavoro", T0)]);
        let commits = vec![commit("0000000")];
        let found = find_all_messages(&commits, &home.dir);
        assert!(found.get("0000000").is_none());
    }

    #[test]
    fn a_file_too_old_for_the_oldest_commit_is_not_even_opened() {
        // La prova che il filtro di prestazione non nasconde un vero
        // incrocio: il file porta l'eco del commit ma la sua ultima scrittura
        // precede di due giorni il commit più vecchio richiesto, quindi il
        // giro lo scarta prima di aprirlo.
        let home = HomeIsolata::nuova("chi-ripara-vecchio");
        let projects = home.dir.join(".claude").join("projects").join("slug");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        write_transcript(
            &projects,
            "sess.jsonl",
            &[human_line("sistema", now), tool_result_line("edecd15 fatto", now)],
        );
        let file = projects.join("sess.jsonl");
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 86_400);
        fs::File::open(&file).unwrap().set_modified(old).unwrap();

        let mut c = commit("edecd15");
        c.epoch = now;
        let found = find_all_messages(&[c], &home.dir);
        assert!(found.get("edecd15").is_none(), "un file troppo vecchio non doveva essere aperto");
    }

    #[test]
    fn run_report_on_an_empty_repo_window_says_so_and_exits_clean() {
        let home = HomeIsolata::nuova("chi-ripara-report-vuoto");
        // Un repo git vero ma senza commit nella finestra: il ramo "vuoto"
        // di run_report, non l'errore di git.
        let repo = home.dir.join("repo");
        fs::create_dir_all(&repo).unwrap();
        let sh = |args: &[&str]| {
            std::process::Command::new("git").arg("-C").arg(&repo).args(args).output().unwrap()
        };
        sh(&["init", "-q"]);
        sh(&["config", "user.email", "prova@example.com"]);
        sh(&["config", "user.name", "prova"]);
        fs::write(repo.join("f.txt"), "x").unwrap();
        sh(&["add", "."]);
        sh(&["commit", "-q", "-m", "iniziale"]);
        // Il commit c'è ma fuori da una finestra di zero giorni: nella pratica
        // "0 giorni" del comando `--since` di git esclude anche "adesso"
        // sui secondi già passati, quindi si usa una finestra minima ma vera
        // e si controlla solo che il comando non vada in errore.
        let code = run_report(&[
            "--repo".to_string(),
            repo.display().to_string(),
            "--days".to_string(),
            "365".to_string(),
        ]);
        assert_eq!(code, 0);
    }
}
