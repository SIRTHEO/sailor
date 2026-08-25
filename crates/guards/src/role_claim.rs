//! Chi tiene un mestiere della configurazione, e per quanto ancora.
//!
//! LA COSA, PRIMA DEL MECCANISMO. Una figura appena nata dichiara il proprio
//! ruolo invece di scrivere a mano il proprio file in `state/ruoli/`: qui sta il
//! giudizio — occupato, libero, o non si sa — con la serratura atomica, il
//! marcatore di ricambio, e il terzo stato (vuoto per decisione). L'I/O — mkdir,
//! `kill -0`, `ps`, `orca terminal list`, la lettura dei file — sta in
//! `claude-hooks::role_claim`.
//!
//! PERCHÉ IN RUST, DAL 25/08/2026. Il giudizio viveva in `role-claim.sh` e
//! `role-vacancy.sh`, portati insieme perché sono un solo giudizio: il primo
//! sorgeva il secondo, e la ronda della coda legge lo stesso terzo stato. Un
//! codice che decide se una dichiarazione vale non può stare in due posti.
//!
//! IL DISCRIMINE VIVO/MORTO/NON-SO NON È IL CODICE D'USCITA. `kill -0` va letto
//! **nel testo**: solo "No such process" è una morte, "Operation not permitted"
//! resta un non-so — lo stesso discrimine di
//! `claude-hooks::register_session::liveness_from`, che questo modulo non può
//! importare (guards sta sotto claude-hooks nella gerarchia delle dipendenze, e
//! quella funzione è privata al suo modulo) ma di cui rifà la stessa forma a tre
//! stati.

use crate::stale_facts::Date;

/// Un nome di titolare: esattamente otto caratteri esadecimali minuscoli. I
/// file del registro che portano un nome diverso (il mestiere stesso, o
/// `.lock.MESTIERE`) non sono un titolare.
pub fn is_holder_name(name: &str) -> bool {
    name.len() == 8 && name.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// I primi otto caratteri di un identificativo di sessione: `printf '%.8s'`.
pub fn short_id(session_id: &str) -> String {
    session_id.chars().take(8).collect()
}

/// Il nome del mestiere in maiuscolo ASCII, come `tr '[:lower:]' '[:upper:]'`
/// — non l'unicode di `to_uppercase()`, che su alcuni caratteri produce più di
/// un carattere e la stesura shell non lo farebbe mai.
pub fn ascii_upper(s: &str) -> String {
    s.bytes().map(|b| b.to_ascii_uppercase() as char).collect()
}

/// L'identificativo di sessione dal nome del file di trascrizione:
/// `basename "$path" .jsonl`.
pub fn transcript_session_id(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    base.strip_suffix(".jsonl").unwrap_or(base).to_string()
}

/// La stessa scelta a cascata del guscio: la variabile giusta, poi quella
/// dell'ambiente cambiato una volta, poi la trascrizione. `None` è il caso che
/// esce 65 — nessuna delle tre fonti aveva niente.
pub fn resolve_session_id(
    claude_code_session_id: Option<&str>,
    claude_session_id: Option<&str>,
    transcript_path: Option<&str>,
) -> Option<String> {
    if let Some(v) = claude_code_session_id.filter(|s| !s.is_empty()) {
        return Some(v.to_string());
    }
    if let Some(v) = claude_session_id.filter(|s| !s.is_empty()) {
        return Some(v.to_string());
    }
    if let Some(p) = transcript_path.filter(|s| !s.is_empty()) {
        return Some(transcript_session_id(p));
    }
    None
}

// ── Gli argomenti ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Claim,
    WhoHolds,
    HandingOver,
    LeaveEmpty,
    FillAgain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedArgs {
    pub mode: Mode,
    pub role: String,
    /// Il numero grezzo di `--for-hours`, non ancora validato: la validazione
    /// dipende dal tetto configurato, che l'I/O legge dall'ambiente.
    pub for_hours: Option<String>,
    pub why: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgError {
    /// Nessun mestiere sulla riga di comando: usage, esce 64.
    MissingRole,
    /// Un argomento che nessuna delle forme riconosce: usage, esce 64.
    Unknown(String),
    /// `--for-hours`/`--why` senza `--leave-empty`: non significano niente
    /// senza di lui.
    VacancyFlagsWithoutLeaveEmpty,
}

/// Legge gli argomenti come li legge il guscio: la prima parola sceglie la
/// forma (o cade nel gesto che prende), la seconda è il mestiere, il resto sono
/// le due opzioni del terzo stato.
///
/// NON VALIDA `--for-hours` QUI: il tetto delle ore (`VACANCY_MAX_HOURS`) è
/// configurabile dall'ambiente, e leggerlo è un affare dell'I/O — la
/// validazione del numero sta in `validate_hours`, chiamata da chi conosce
/// quel tetto.
pub fn parse_args(argv: &[String]) -> Result<ParsedArgs, ArgError> {
    let mut it = argv.iter();
    let mut first = it.next().map(String::as_str).unwrap_or("");
    let mode = match first {
        "--who-holds" => {
            first = it.next().map(String::as_str).unwrap_or("");
            Mode::WhoHolds
        }
        "--handing-over" => {
            first = it.next().map(String::as_str).unwrap_or("");
            Mode::HandingOver
        }
        "--leave-empty" => {
            first = it.next().map(String::as_str).unwrap_or("");
            Mode::LeaveEmpty
        }
        "--fill-again" => {
            first = it.next().map(String::as_str).unwrap_or("");
            Mode::FillAgain
        }
        _ => Mode::Claim,
    };
    if first.is_empty() {
        return Err(ArgError::MissingRole);
    }
    let role = ascii_upper(first);

    let mut for_hours = None;
    let mut why = None;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--for-hours" => for_hours = it.next().cloned(),
            "--why" => why = it.next().cloned(),
            other => return Err(ArgError::Unknown(other.to_string())),
        }
    }
    if mode != Mode::LeaveEmpty && (for_hours.is_some() || why.is_some()) {
        return Err(ArgError::VacancyFlagsWithoutLeaveEmpty);
    }
    Ok(ParsedArgs { mode, role, for_hours, why })
}

/// La durata di un `--leave-empty`, validata contro il tetto configurato.
/// Non si accorcia in silenzio: fuori tetto è un rifiuto, non una potatura.
pub fn validate_hours(raw: &str, max_hours: u32) -> Result<u32, String> {
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "role-claim: --for-hours wants a whole number of hours, got '{raw}'"
        ));
    }
    let n: u32 = raw.parse().map_err(|_| {
        format!("role-claim: --for-hours wants a whole number of hours, got '{raw}'")
    })?;
    if n < 1 || n > max_hours {
        return Err(format!(
            "role-claim: --for-hours must be between 1 and {max_hours}; past a day the decision is taken again, not extended"
        ));
    }
    Ok(n)
}

// ── Chi ha già dichiarato questo mestiere ────────────────────────────────────

/// Un file candidato del registro dei ruoli: il nome e la sua prima riga (già
/// spogliata di `\r\n`, come fa `head -n1 | tr -d '\r\n'`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleFile {
    pub name: String,
    pub first_line: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HolderScan {
    /// Solo in `--who-holds`: il file che porta il nome proprio dichiara
    /// proprio questo mestiere.
    HeldByYou,
    Held(String),
    Free,
}

/// Chi tiene un mestiere, scandendo i file come fa il guscio: prima il filtro
/// sul nome (otto esadecimali), poi il salto di sé stessi — ma non per chi
/// chiede, perché «lo tieni tu» è proprio una delle risposte che una domanda
/// deve poter dare — poi il primo file il cui contenuto è quel mestiere.
pub fn scan_holder(files: &[RoleFile], own: &str, role: &str, read_only: bool) -> HolderScan {
    for f in files {
        if !is_holder_name(&f.name) {
            continue;
        }
        if f.name == own {
            if read_only && f.first_line == role {
                return HolderScan::HeldByYou;
            }
            continue;
        }
        if f.first_line == role {
            return HolderScan::Held(f.name.clone());
        }
    }
    HolderScan::Free
}

// ── Il processo è vivo? ──────────────────────────────────────────────────────

/// Cosa ha risposto `kill -0 PID`, letto **nel testo** e non nel codice
/// d'uscita: la sola risposta che certifica una morte è "No such process".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillProbe {
    Succeeded,
    NotFound,
    /// Fallito per un'altra ragione ("Operation not permitted", una sabbiera
    /// che nasconde i processi altrui, ...): un non-so, mai una morte.
    Denied,
}

/// Pura: la classificazione del testo di `kill -0`, separata da chi lancia il
/// comando. Il guscio confronta `*[Nn]o\ such\ process*`: solo la prima lettera
/// varia maiuscola/minuscola, il resto è testo esatto.
pub fn classify_kill_probe(exit_ok: bool, stderr: &str) -> KillProbe {
    if exit_ok {
        return KillProbe::Succeeded;
    }
    if stderr.contains("No such process") || stderr.contains("no such process") {
        KillProbe::NotFound
    } else {
        KillProbe::Denied
    }
}

/// Il comando riportato da `ps` è un `claude`? Stesso confronto del guscio:
/// `*/claude` o `claude` esatto.
pub fn is_claude_comm(comm: &str) -> bool {
    comm == "claude" || comm.ends_with("/claude")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcStatus {
    Alive,
    Gone,
    Unknown,
}

impl ProcStatus {
    /// La parola che finisce nei messaggi, la stessa che il guscio scrive nel
    /// registro (`$proc_status`).
    pub fn word(self) -> &'static str {
        match self {
            ProcStatus::Alive => "alive",
            ProcStatus::Gone => "gone",
            ProcStatus::Unknown => "unknown",
        }
    }
}

/// Il verdetto sul processo: senza `pid` resta un non-so; con `pid`, la sonda
/// decide, e "vivo" richiede ANCHE che `ps` confermi che è un `claude` — un
/// pid riciclato da un altro programma non basta.
pub fn proc_status_from(pid_present: bool, probe: Option<KillProbe>, comm: Option<&str>) -> ProcStatus {
    if !pid_present {
        return ProcStatus::Unknown;
    }
    match probe {
        None => ProcStatus::Unknown,
        Some(KillProbe::NotFound) => ProcStatus::Gone,
        Some(KillProbe::Denied) => ProcStatus::Unknown,
        Some(KillProbe::Succeeded) => match comm {
            Some(c) if is_claude_comm(c) => ProcStatus::Alive,
            _ => ProcStatus::Unknown,
        },
    }
}

// ── Il pannello è raggiungibile? ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reachable {
    Yes,
    No,
    /// La sessione non ha né tab né handle registrati (headless, schedulata):
    /// questa fonte non ha materia su cui pronunciarsi.
    Unknown,
}

/// Il tab batte l'handle quando ci sono entrambi, come nel guscio (`elif`).
pub fn compute_reachable(
    tab: Option<&str>,
    handle: Option<&str>,
    live_tabs: &[String],
    live_handles: &[String],
) -> Reachable {
    if let Some(t) = tab {
        return if live_tabs.iter().any(|x| x == t) { Reachable::Yes } else { Reachable::No };
    }
    if let Some(h) = handle {
        return if live_handles.iter().any(|x| x == h) { Reachable::Yes } else { Reachable::No };
    }
    Reachable::Unknown
}

// ── Il verdetto ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Il processo del titolare gira ancora.
    AlreadyHeldLive,
    /// La rete sotto ogni sostituzione: una trascrizione toccata da meno di
    /// `active_s` è viva, qualunque cosa dicano il processo e i pannelli.
    AlreadyHeldByTranscript,
    /// Il posto cambia mano — o, in sola lettura, si può prendere.
    Replaceable { why: String },
    /// Non si è potuto sapere: non si tocca niente.
    Unknown,
}

/// Il cuore del giudizio (righe 513-551 dello script): tre domande, in questo
/// ordine, e nessuna fonte da sola decide una morte.
pub fn evaluate_holder(
    proc_status: ProcStatus,
    talk_age_s: Option<u64>,
    reachable: Reachable,
    active_s: u64,
    expiry_s: u64,
) -> Verdict {
    if proc_status == ProcStatus::Alive {
        return Verdict::AlreadyHeldLive;
    }
    if talk_age_s.is_some_and(|age| age < active_s) {
        return Verdict::AlreadyHeldByTranscript;
    }
    if proc_status == ProcStatus::Gone && reachable == Reachable::No {
        return Verdict::Replaceable { why: "process gone, no live pane".to_string() };
    }
    if let Some(age) = talk_age_s {
        if age >= expiry_s {
            return Verdict::Replaceable {
                why: format!(
                    "nothing written in its transcript for {age}s, process {}",
                    proc_status.word()
                ),
            };
        }
    }
    Verdict::Unknown
}

// ── Il marcatore «in-ricambio» ───────────────────────────────────────────────

/// La prima riga del file è `in-ricambio`? Se sì, la seconda è chi l'ha
/// marcato. `None` per un file assente o che non porta quel marcatore.
pub fn read_handoff(text: Option<&str>) -> Option<String> {
    let text = text?;
    if nth_line_stripped(text, 1) != "in-ricambio" {
        return None;
    }
    Some(nth_line_stripped(text, 2))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffState {
    NotHandoff,
    /// Marcato da un'altra sessione, e più giovane della soglia: blocca.
    Blocking { owner: String },
    /// Scaduto, o marcato da me stesso: chi prende lo toglie.
    Stale { owner: String },
}

pub fn evaluate_handoff(owner: Option<String>, age_s: u64, own: &str, stale_s: u64) -> HandoffState {
    let Some(owner) = owner else {
        return HandoffState::NotHandoff;
    };
    if age_s < stale_s && owner != own {
        HandoffState::Blocking { owner }
    } else {
        HandoffState::Stale { owner }
    }
}

// ── Il terzo stato: vuoto per decisione ──────────────────────────────────────

pub const VACANCY_MARKER: &str = "vuoto-per-decisione";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VacancyActive {
    pub who: String,
    pub until: i64,
    pub left: i64,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VacancyRead {
    Active(VacancyActive),
    /// Nessun file, o un file che non porta il marcatore.
    None,
    Expired { until: i64, who: String, why: String },
    /// La terza riga non è un numero leggibile: vale come nessuna
    /// dichiarazione, mai come «per sempre».
    Malformed { until_raw: String, who: String, why: String },
}

/// Il porto di `vacancy_read`: `text` è il contenuto del file se già letto e
/// leggibile, `None` altrimenti (`[ -r "$file" ]` che fallisce).
pub fn read_vacancy(text: Option<&str>, now: i64) -> VacancyRead {
    let Some(text) = text else {
        return VacancyRead::None;
    };
    if nth_line_stripped(text, 1) != VACANCY_MARKER {
        return VacancyRead::None;
    }
    let who = nth_line_stripped(text, 2);
    let until_raw = nth_line_stripped(text, 3);
    let why = nth_line_stripped(text, 4);

    // UNA DICHIARAZIONE SENZA SCADENZA LEGGIBILE NON VALE: trattarla come «per
    // sempre» sarebbe il vincolo permanente che questo marcatore non deve
    // poter creare.
    if until_raw.is_empty() || !until_raw.bytes().all(|b| b.is_ascii_digit()) {
        return VacancyRead::Malformed { until_raw, who, why };
    }
    let Ok(until) = until_raw.parse::<i64>() else {
        return VacancyRead::Malformed { until_raw, who, why };
    };
    if now >= until {
        return VacancyRead::Expired { until, who, why };
    }
    VacancyRead::Active(VacancyActive { who, until, left: until - now, why })
}

/// Il muro d'orologio locale dell'alba cercata: quale giorno, a che ora. Sceglie
/// oggi o domani con lo scarto valido ADESSO, che è l'unico che dice che ora è
/// adesso.
fn dawn_wall_clock(now: i64, offset_now: i64, dawn_hour: i64) -> i64 {
    let local = now + offset_now;
    let local_hour = local.rem_euclid(86_400) / 3600;
    let day_start_local = local.div_euclid(86_400) * 86_400;
    if local_hour < dawn_hour {
        day_start_local + dawn_hour * 3600
    } else {
        day_start_local + 86_400 + dawn_hour * 3600
    }
}

/// L'alba dopo `now`, in secondi dal 1970: la scadenza di serie di
/// `--leave-empty` senza `--for-hours`. `offset_at` rende lo scarto dall'UTC a un
/// istante dato — questa funzione non tocca l'orologio, lo interroga.
///
/// PERCHÉ UNA FUNZIONE E NON UN NUMERO (rilievo del revisore, 25/08/2026). Prima
/// qui entrava un solo scarto, quello di adesso, e lo si applicava anche all'alba:
/// **la notte del cambio dell'ora legale la scadenza usciva sbagliata di un'ora**,
/// e il posto tornava riprendibile un'ora prima del previsto. Lo script shell non
/// aveva il difetto perché `date -v` ricalcola lo scarto sull'istante bersaglio.
/// Qui si fa lo stesso: si sceglie il muro d'orologio con lo scarto di adesso, poi
/// si riproietta con lo scarto dell'alba. **Una sola ripetizione basta**: fra due
/// albe consecutive c'è al massimo un salto d'orologio.
pub fn vacancy_next_dawn(now: i64, offset_at: impl Fn(i64) -> i64, dawn_hour: i64) -> i64 {
    let offset_now = offset_at(now);
    let dawn_local = dawn_wall_clock(now, offset_now, dawn_hour);
    let first = dawn_local - offset_now;
    let offset_dawn = offset_at(first);
    if offset_dawn == offset_now {
        return first;
    }
    // Stessa ora locale voluta, scarto giusto per quel momento.
    dawn_local - offset_dawn
}

/// La scadenza in parole, ora locale al minuto: `%Y-%m-%d %H:%M`, senza la
/// sigla del fuso (`%Z`) che lo script shell porta e che qui non è
/// disponibile — solo lo scarto numerico lo è (vedi la nota nel modulo
/// `claude-hooks` che chiama questa funzione). Il calendario è
/// `stale_facts::Date::from_days`, non riscritto: stesso algoritmo di
/// `hook-io::journal::civil_from_days`, ma quello è privato al suo crate.
pub fn vacancy_until_text(epoch: i64, offset_s: i64) -> String {
    let local = epoch + offset_s;
    let days = local.div_euclid(86_400);
    let rem = local.rem_euclid(86_400);
    let d = Date::from_days(days);
    format!("{} {:02}:{:02}", d.iso(), rem / 3600, (rem % 3600) / 60)
}

// ── La serratura ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockStep {
    /// `mkdir` è fallito ma la cartella non c'è: non è una serratura occupata,
    /// è un'altra ragione (permessi, disco). Restare nel ciclo girerebbe a
    /// vuoto per sempre.
    NotAnOccupiedLock,
    /// Più vecchia della soglia: di un processo morto, si rompe e si riprova.
    BreakStale { owner: String },
    TimedOut,
    Wait,
}

/// Un passo del ciclo d'attesa sulla serratura (righe 227-255): cosa dice
/// l'ultimo tentativo di `mkdir` fallito, dato lo stato di quella cartella.
pub fn classify_lock_wait(
    dir_exists: bool,
    lock_age_s: u64,
    lock_stale_s: u64,
    now: i64,
    deadline: i64,
    stale_owner: Option<&str>,
) -> LockStep {
    if !dir_exists {
        return LockStep::NotAnOccupiedLock;
    }
    if lock_age_s > lock_stale_s {
        return LockStep::BreakStale { owner: stale_owner.unwrap_or("an unnamed session").to_string() };
    }
    if now >= deadline {
        return LockStep::TimedOut;
    }
    LockStep::Wait
}

// ── Le righe per uno script (`--who-holds`) ─────────────────────────────────
//
// LA RISPOSTA È ANCHE PER UNO SCRIPT: la prima riga su stdout è una riga di
// campi, e sopra quella si decide senza leggere la prosa (commento originale,
// righe 40-50 di `role-claim.sh`).

pub fn status_free(role: &str) -> String {
    format!("status=free role={role}")
}
pub fn status_held(role: &str, holder: &str) -> String {
    format!("status=held role={role} holder={holder}")
}
pub fn status_held_by_you(role: &str, holder: &str) -> String {
    format!("status=held-by-you role={role} holder={holder}")
}
pub fn status_takeable(role: &str, holder: &str) -> String {
    format!("status=takeable role={role} holder={holder}")
}
pub fn status_vacant(role: &str, by: &str, until: i64, remaining: i64) -> String {
    format!("status=vacant-by-decision role={role} by={by} until={until} remaining={remaining}")
}
pub fn status_unknown(role: &str, holder: &str) -> String {
    format!("status=unknown role={role} holder={holder}")
}
pub fn status_handoff(role: &str, by: &str, age_s: u64) -> String {
    format!("status=handoff role={role} by={by} age={age_s}")
}

// ── Una riga, spogliata come `sed -n 'Np' | tr -d '\r\n'` ──────────────────

/// La riga N (1-indicizzata) di un testo, senza il ritorno a capo finale.
/// `str::lines()` di Rust stacca già un `\r` finale per riga, lo stesso
/// effetto di `tr -d '\r\n'` su una singola riga estratta da `sed`.
fn nth_line_stripped(text: &str, n: usize) -> String {
    text.lines().nth(n.saturating_sub(1)).unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Nomi e identificativi ────────────────────────────────────────────

    #[test]
    fn a_holder_name_is_exactly_eight_lowercase_hex_chars() {
        assert!(is_holder_name("aaaaaaaa"));
        assert!(is_holder_name("0123abcd"));
        assert!(!is_holder_name("AAAAAAAA"), "maiuscolo non è un titolare");
        assert!(!is_holder_name("MACCHINISTA"), "il mestiere non è un titolare");
        assert!(!is_holder_name("aaaaaaa"), "sette non bastano");
        assert!(!is_holder_name(".lock.X"));
    }

    #[test]
    fn short_id_takes_the_first_eight_characters() {
        assert_eq!(short_id("bbbbbbbb-0000-0000-0000-000000000000"), "bbbbbbbb");
        assert_eq!(short_id("abc"), "abc", "più corto di otto resta com'è");
    }

    #[test]
    fn ascii_upper_matches_tr() {
        assert_eq!(ascii_upper("macchinista"), "MACCHINISTA");
    }

    #[test]
    fn transcript_session_id_strips_the_jsonl_suffix() {
        assert_eq!(transcript_session_id("/x/y/bbbbbbbb-0000.jsonl"), "bbbbbbbb-0000");
    }

    #[test]
    fn resolve_session_id_falls_back_through_three_sources() {
        assert_eq!(
            resolve_session_id(Some("a"), Some("b"), Some("/c.jsonl")),
            Some("a".to_string())
        );
        assert_eq!(
            resolve_session_id(None, Some("b"), Some("/c.jsonl")),
            Some("b".to_string()),
            "il nome corto, se manca quello vero"
        );
        assert_eq!(
            resolve_session_id(None, None, Some("/x/y-0.jsonl")),
            Some("y-0".to_string()),
            "il ripiego sulla trascrizione"
        );
        assert_eq!(resolve_session_id(None, None, None), None, "nessuna fonte, exit 65");
        // Una variabile vuota vale come assente, non come un id vuoto.
        assert_eq!(
            resolve_session_id(Some(""), None, Some("/c.jsonl")),
            Some("c".to_string())
        );
    }

    // ── Gli argomenti ────────────────────────────────────────────────────

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_bare_role_claims() {
        let a = parse_args(&argv(&["macchinista"])).unwrap();
        assert_eq!(a.mode, Mode::Claim);
        assert_eq!(a.role, "MACCHINISTA");
    }

    #[test]
    fn the_four_named_forms_are_recognised() {
        assert_eq!(parse_args(&argv(&["--who-holds", "x"])).unwrap().mode, Mode::WhoHolds);
        assert_eq!(
            parse_args(&argv(&["--handing-over", "x"])).unwrap().mode,
            Mode::HandingOver
        );
        assert_eq!(parse_args(&argv(&["--leave-empty", "x"])).unwrap().mode, Mode::LeaveEmpty);
        assert_eq!(parse_args(&argv(&["--fill-again", "x"])).unwrap().mode, Mode::FillAgain);
    }

    #[test]
    fn no_role_is_the_usage_error() {
        assert_eq!(parse_args(&argv(&[])), Err(ArgError::MissingRole));
        assert_eq!(parse_args(&argv(&["--who-holds"])), Err(ArgError::MissingRole));
    }

    #[test]
    fn vacancy_flags_mean_nothing_without_leave_empty() {
        assert_eq!(
            parse_args(&argv(&["capitano", "--for-hours", "2"])),
            Err(ArgError::VacancyFlagsWithoutLeaveEmpty)
        );
        assert!(parse_args(&argv(&["--leave-empty", "capitano", "--for-hours", "2"])).is_ok());
    }

    #[test]
    fn an_hour_count_is_validated_against_the_configured_cap() {
        assert_eq!(validate_hours("2", 24), Ok(2));
        assert!(validate_hours("0", 24).is_err(), "zero non conta");
        assert!(validate_hours("72", 24).is_err(), "oltre il tetto si rifiuta, non si accorcia");
        assert!(validate_hours("abc", 24).is_err());
        assert!(validate_hours("", 24).is_err());
    }

    // ── Chi tiene il mestiere ────────────────────────────────────────────

    fn rf(name: &str, role: &str) -> RoleFile {
        RoleFile { name: name.into(), first_line: role.into() }
    }

    #[test]
    fn a_free_post_has_no_holder() {
        assert_eq!(scan_holder(&[], "bbbbbbbb", "MACCHINISTA", false), HolderScan::Free);
    }

    #[test]
    fn the_first_matching_holder_wins() {
        let files = [rf("aaaaaaaa", "MACCHINISTA"), rf("cccccccc", "MACCHINISTA")];
        assert_eq!(
            scan_holder(&files, "bbbbbbbb", "MACCHINISTA", false),
            HolderScan::Held("aaaaaaaa".into())
        );
    }

    #[test]
    fn taking_skips_its_own_file_even_if_it_matches() {
        let files = [rf("bbbbbbbb", "MACCHINISTA")];
        assert_eq!(scan_holder(&files, "bbbbbbbb", "MACCHINISTA", false), HolderScan::Free);
    }

    #[test]
    fn asking_about_a_post_it_holds_itself_says_so() {
        let files = [rf("bbbbbbbb", "MACCHINISTA")];
        assert_eq!(scan_holder(&files, "bbbbbbbb", "MACCHINISTA", true), HolderScan::HeldByYou);
    }

    // ── Il processo è vivo? ──────────────────────────────────────────────

    #[test]
    fn kill_probe_reads_the_text_not_the_exit_code() {
        assert_eq!(classify_kill_probe(true, ""), KillProbe::Succeeded);
        assert_eq!(
            classify_kill_probe(false, "kill: 123: No such process"),
            KillProbe::NotFound
        );
        assert_eq!(
            classify_kill_probe(false, "kill: 123: Operation not permitted"),
            KillProbe::Denied,
            "negato non è morto"
        );
    }

    #[test]
    fn is_claude_comm_matches_the_shell_case_pattern() {
        assert!(is_claude_comm("claude"));
        assert!(is_claude_comm("/usr/local/bin/claude"));
        assert!(!is_claude_comm("claude-helper"));
    }

    #[test]
    fn without_a_pid_the_process_stays_unknown() {
        assert_eq!(proc_status_from(false, None, None), ProcStatus::Unknown);
    }

    #[test]
    fn a_confirmed_claude_process_is_alive() {
        assert_eq!(
            proc_status_from(true, Some(KillProbe::Succeeded), Some("claude")),
            ProcStatus::Alive
        );
    }

    #[test]
    fn a_pid_that_answers_but_is_not_claude_stays_unknown() {
        assert_eq!(
            proc_status_from(true, Some(KillProbe::Succeeded), Some("some-other-thing")),
            ProcStatus::Unknown,
            "un pid riciclato non certifica una morte né una vita"
        );
    }

    #[test]
    fn a_certified_absence_is_gone() {
        assert_eq!(proc_status_from(true, Some(KillProbe::NotFound), None), ProcStatus::Gone);
    }

    #[test]
    fn a_denied_probe_is_not_a_death() {
        // IL TERZO BRACCIO CHE CONTA (braccio 14 della batteria shell): un
        // titolare che esiste ma su cui `kill -0` risponde «non ti è
        // permesso» non è un titolare morto.
        assert_eq!(proc_status_from(true, Some(KillProbe::Denied), None), ProcStatus::Unknown);
    }

    // ── Il pannello raggiungibile ────────────────────────────────────────

    #[test]
    fn reachable_prefers_the_tab_over_the_handle() {
        let tabs = vec!["tab-1".to_string()];
        let handles = vec!["h-tab-1".to_string()];
        assert_eq!(compute_reachable(Some("tab-1"), Some("h-x"), &tabs, &handles), Reachable::Yes);
        assert_eq!(compute_reachable(Some("tab-9"), Some("h-tab-1"), &tabs, &handles), Reachable::No);
    }

    #[test]
    fn reachable_falls_back_to_the_handle() {
        let handles = vec!["h-1".to_string()];
        assert_eq!(compute_reachable(None, Some("h-1"), &[], &handles), Reachable::Yes);
    }

    #[test]
    fn without_tab_or_handle_reachability_is_unknown() {
        assert_eq!(compute_reachable(None, None, &[], &[]), Reachable::Unknown);
    }

    // ── Il verdetto ──────────────────────────────────────────────────────

    const ACTIVE_S: u64 = 3600;
    const EXPIRY_S: u64 = 21600;

    #[test]
    fn a_live_process_keeps_the_post() {
        // Braccio 2: il processo del titolare gira ancora.
        assert_eq!(
            evaluate_holder(ProcStatus::Alive, Some(7200), Reachable::No, ACTIVE_S, EXPIRY_S),
            Verdict::AlreadyHeldLive
        );
    }

    #[test]
    fn process_gone_and_no_pane_replaces_the_post() {
        // Braccio 3.
        assert_eq!(
            evaluate_holder(ProcStatus::Gone, Some(7200), Reachable::No, ACTIVE_S, EXPIRY_S),
            Verdict::Replaceable { why: "process gone, no live pane".into() }
        );
    }

    #[test]
    fn a_pane_that_outlived_its_agent_does_not_hold_forever() {
        // IL BRACCIO CHE CONTA (braccio 4): pannello vivo, processo sparito,
        // trascrizione ferma da sette ore -- il subentro deve avvenire.
        assert_eq!(
            evaluate_holder(ProcStatus::Gone, Some(25_200), Reachable::Yes, ACTIVE_S, EXPIRY_S),
            Verdict::Replaceable {
                why: "nothing written in its transcript for 25200s, process gone".into()
            }
        );
    }

    #[test]
    fn a_pane_whose_transcript_is_moving_keeps_the_post() {
        // Braccio 5, il gemello contrario del quarto: stessa scena, ma la
        // trascrizione è fresca -- qui non si tocca niente.
        //
        // L'ATTESO ERA SBAGLIATO, NON IL CODICE (rilievo del revisore, 25/08/2026):
        // diceva `Unknown`, cioè «non si sa». Ma una trascrizione mossa da 300s è
        // sotto ACTIVE_S, e la rete sotto ogni sostituzione dice che quella sessione
        // è VIVA qualunque cosa dicano processo e pannelli. Il braccio 5 della
        // batteria shell (`role-claim.test.sh:144-149`) esige `exit 1`, che è
        // «già tenuto» -- `Unknown` uscirebbe 2. Il porto era fedele; il test no.
        assert_eq!(
            evaluate_holder(ProcStatus::Gone, Some(300), Reachable::Yes, ACTIVE_S, EXPIRY_S),
            Verdict::AlreadyHeldByTranscript,
            "sotto ACTIVE_S la trascrizione tiene il posto: nessun ramo più sotto sfratta"
        );
    }

    #[test]
    fn a_fresh_transcript_outranks_a_lost_pid_and_a_lost_pane() {
        // Braccio 13: la rete sotto ogni sostituzione.
        assert_eq!(
            evaluate_holder(ProcStatus::Gone, Some(60), Reachable::No, ACTIVE_S, EXPIRY_S),
            Verdict::AlreadyHeldByTranscript
        );
    }

    #[test]
    fn no_record_and_no_transcript_is_a_do_not_know() {
        // Braccio 7: niente pid, niente trascrizione -- non si sa, non si tocca.
        assert_eq!(
            evaluate_holder(ProcStatus::Unknown, None, Reachable::Yes, ACTIVE_S, EXPIRY_S),
            Verdict::Unknown
        );
    }

    #[test]
    fn a_denied_probe_with_a_stale_transcript_stays_unknown_not_dead() {
        // Braccio 14: negato non è morto, e senza pannello raggiungibile la
        // sola difesa è questa distinzione.
        assert_eq!(
            evaluate_holder(ProcStatus::Unknown, Some(7200), Reachable::No, ACTIVE_S, EXPIRY_S),
            Verdict::Unknown
        );
    }

    #[test]
    fn a_ps_that_cannot_answer_does_not_certify_a_death() {
        // Braccio 12: `ps` che esce diverso da zero su un pid vivo diventa
        // Denied/Unknown a monte -- qui si prova che il verdetto lo rispetta.
        assert_eq!(
            evaluate_holder(ProcStatus::Unknown, Some(7200), Reachable::No, ACTIVE_S, EXPIRY_S),
            Verdict::Unknown
        );
    }

    // ── Il marcatore in-ricambio ─────────────────────────────────────────

    #[test]
    fn a_handoff_marker_names_its_writer() {
        assert_eq!(read_handoff(Some("in-ricambio\nbbbbbbbb\n")), Some("bbbbbbbb".into()));
        assert_eq!(read_handoff(Some("qualcos'altro\n")), None);
        assert_eq!(read_handoff(None), None);
    }

    #[test]
    fn a_fresh_handoff_from_someone_else_blocks() {
        assert_eq!(
            evaluate_handoff(Some("cccccccc".into()), 10, "bbbbbbbb", 1800),
            HandoffState::Blocking { owner: "cccccccc".into() }
        );
    }

    #[test]
    fn a_handoff_from_ourselves_never_blocks() {
        assert_eq!(
            evaluate_handoff(Some("bbbbbbbb".into()), 10, "bbbbbbbb", 1800),
            HandoffState::Stale { owner: "bbbbbbbb".into() }
        );
    }

    #[test]
    fn a_stale_handoff_does_not_block() {
        assert_eq!(
            evaluate_handoff(Some("cccccccc".into()), 2000, "bbbbbbbb", 1800),
            HandoffState::Stale { owner: "cccccccc".into() }
        );
    }

    // ── Il terzo stato ───────────────────────────────────────────────────

    #[test]
    fn an_active_declaration_is_read() {
        let text = format!("{VACANCY_MARKER}\nbbbbbbbb\n2000000000\nragione\n");
        match read_vacancy(Some(&text), 1_000) {
            VacancyRead::Active(v) => {
                assert_eq!(v.who, "bbbbbbbb");
                assert_eq!(v.until, 2_000_000_000);
                assert_eq!(v.why, "ragione");
            }
            other => panic!("attesa Active, avuto {other:?}"),
        }
    }

    #[test]
    fn no_file_and_no_marker_are_both_none() {
        assert_eq!(read_vacancy(None, 1_000), VacancyRead::None);
        assert_eq!(read_vacancy(Some("qualcos'altro\n"), 1_000), VacancyRead::None);
    }

    #[test]
    fn a_declaration_past_its_deadline_expires() {
        // Braccio 18, provato facendo passare il tempo.
        let text = format!("{VACANCY_MARKER}\nbbbbbbbb\n500\n\n");
        assert!(matches!(read_vacancy(Some(&text), 1_000), VacancyRead::Expired { .. }));
    }

    #[test]
    fn a_deadline_that_is_not_a_number_never_counted() {
        // Braccio 19.
        let text = format!("{VACANCY_MARKER}\nbbbbbbbb\nforever\n\n");
        assert!(matches!(read_vacancy(Some(&text), 1_000), VacancyRead::Malformed { .. }));
    }

    #[test]
    fn vacancy_next_dawn_picks_today_or_tomorrow() {
        // Mezzanotte UTC del 2026-08-26, offset zero, alba alle 6.
        let midnight: i64 = 1_787_702_400;
        assert_eq!(
            vacancy_next_dawn(midnight + 3600, |_| 0, 6), // le 01:00, prima dell'alba
            midnight + 6 * 3600
        );
        assert_eq!(
            vacancy_next_dawn(midnight + 7 * 3600, |_| 0, 6), // le 07:00, dopo l'alba
            midnight + 86_400 + 6 * 3600
        );
    }

    #[test]
    fn the_dawn_after_the_clocks_change_is_not_an_hour_early() {
        // IL CASO CHE ATTRAVERSA IL CAMBIO D'ORA (rilievo del revisore, 25/08/2026).
        // Scena reale: sera del 2026-10-24 alle 23:00 CEST (+2), l'alba cercata cade
        // dopo il ritorno all'ora solare CET (+1) di quella notte.
        //
        // L'ORACOLO È LO SCRIPT SHELL: `date -v+1d -v6H -v0M -v0S '+%s'` su quella
        // sera rende 1792904400 (2026-10-25 06:00 CET). La stesura precedente, che
        // applicava all'alba lo scarto di adesso, rendeva 1792900800 -- un'ora prima,
        // cioè un posto riprendibile prima del dovuto.
        let evening: i64 = 1_792_875_600; // 2026-10-24 23:00 CEST
        let change: i64 = 1_792_899_600; // l'istante del salto (+2 -> +1)
        let tz = |t: i64| if t < change { 7200 } else { 3600 };
        assert_eq!(
            vacancy_next_dawn(evening, tz, 6),
            1_792_904_400,
            "l'alba dopo il cambio d'ora vuole lo scarto di QUEL momento, non di adesso"
        );
        // Controprova: senza salto d'orologio la risposta non cambia.
        assert_eq!(vacancy_next_dawn(evening, |_| 7200, 6), 1_792_900_800);
    }

    #[test]
    fn vacancy_until_text_reads_local_time() {
        let midnight: i64 = 1_787_702_400; // 2026-08-26T00:00:00Z
        assert_eq!(vacancy_until_text(midnight + 6 * 3600 + 300, 0), "2026-08-26 06:05");
        // stesso istante, un fuso avanti di due ore
        assert_eq!(vacancy_until_text(midnight + 6 * 3600 + 300, 7200), "2026-08-26 08:05");
    }

    // ── La serratura ─────────────────────────────────────────────────────

    #[test]
    fn a_missing_directory_is_not_an_occupied_lock() {
        assert_eq!(classify_lock_wait(false, 0, 120, 0, 100, None), LockStep::NotAnOccupiedLock);
    }

    #[test]
    fn a_stale_lock_is_broken() {
        assert_eq!(
            classify_lock_wait(true, 200, 120, 0, 100, Some("cccccccc")),
            LockStep::BreakStale { owner: "cccccccc".into() }
        );
    }

    #[test]
    fn a_fresh_lock_past_the_deadline_times_out() {
        assert_eq!(classify_lock_wait(true, 10, 120, 100, 100, None), LockStep::TimedOut);
    }

    #[test]
    fn a_fresh_lock_before_the_deadline_waits() {
        assert_eq!(classify_lock_wait(true, 10, 120, 50, 100, None), LockStep::Wait);
    }

    // ── Le righe per uno script ──────────────────────────────────────────

    #[test]
    fn the_status_lines_match_the_documented_contract() {
        // Righe 44-50 dello script: la forma che uno script legge.
        assert_eq!(status_free("X"), "status=free role=X");
        assert_eq!(status_held("X", "a"), "status=held role=X holder=a");
        assert_eq!(status_held_by_you("X", "a"), "status=held-by-you role=X holder=a");
        assert_eq!(status_takeable("X", "a"), "status=takeable role=X holder=a");
        assert_eq!(
            status_vacant("X", "a", 10, 5),
            "status=vacant-by-decision role=X by=a until=10 remaining=5"
        );
        assert_eq!(status_unknown("X", "a"), "status=unknown role=X holder=a");
        assert_eq!(status_handoff("X", "a", 3), "status=handoff role=X by=a age=3");
    }

    #[test]
    fn a_post_left_empty_by_decision_answers_differently_from_a_free_post() {
        // Braccio 17, IL QUARTO BRACCIO CHE CONTA: la prima riga distingue il
        // terzo stato da un posto libero e basta.
        let vacant = status_vacant("CAPITANO", "bbbbbbbb", 2_000_000_000, 999);
        assert!(vacant.starts_with("status=vacant-by-decision"));
        let free = status_free("NOSTROMO");
        assert!(free.starts_with("status=free"));
    }
}
