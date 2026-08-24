//! Il registro dei costi: parsing del transcript, deduplica dello streaming,
//! classificazione meccanica, listino dei modelli e rapporto. Logica pura —
//! niente stdin, niente disco, niente orologio (le funzioni che ne hanno
//! bisogno lo ricevono come parametro). La colla che tocca il mondo — stdin
//! del gancio, i percorsi dei transcript, i cursori, `state/costi.jsonl` —
//! sta in `claude-hooks/src/costs.rs`. La classificazione meccanica porta la
//! definizione di `docs/2026-08-22-turni-delegabili-ad-altri-modelli.md`
//! (script `analyze2.py`).

// La data civile non si riscrive qui: [`crate::stale_facts::Date`] ha già
// l'algoritmo di Howard Hinnant nei due versi, nello stesso crate — riusato
// invece di portarne una seconda copia per il taglio `--days`.
use crate::stale_facts::Date;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

// ============================================================
// Un turno del transcript, già deduplicato dallo streaming.
// ============================================================

/// Un record `assistant` del transcript, con i soli campi che il registro usa.
#[derive(Debug, Clone, PartialEq)]
pub struct RawTurn {
    pub message_id: String,
    pub model: String,
    /// ISO8601 UTC (`2026-08-11T14:05:14.688Z`), confrontabile a livello di
    /// stringa: i campi sono a larghezza fissa e con zeri iniziali.
    pub timestamp: String,
    pub session: String,
    pub is_sidechain: bool,
    pub attribution_agent: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// Il nome del primo `tool_use` nel contenuto del messaggio, se c'è.
    pub first_tool: Option<String>,
    /// Il comando, solo se `first_tool` è `Bash`.
    pub bash_command: Option<String>,
}

/// Una riga del transcript in un `RawTurn`, o `None` se non è un turno da
/// contare: non è `assistant`, oppure non porta `message.usage`.
pub fn parse_transcript_line(line: &str) -> Option<RawTurn> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(|x| x.as_str()) != Some("assistant") {
        return None;
    }
    let message = v.get("message")?;
    let usage = message.get("usage")?.as_object()?;
    let message_id = message.get("id")?.as_str()?.to_string();
    let model = message
        .get("model")
        .and_then(|x| x.as_str())
        .unwrap_or("unknown")
        .to_string();
    let timestamp = v
        .get("timestamp")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let session = v
        .get("sessionId")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let is_sidechain = v
        .get("isSidechain")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let attribution_agent = v
        .get("attributionAgent")
        .and_then(|x| x.as_str())
        .map(String::from);
    let get_u64 = |k: &str| usage.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    let input_tokens = get_u64("input_tokens");
    let output_tokens = get_u64("output_tokens");
    let cache_read = get_u64("cache_read_input_tokens");
    let cache_write = get_u64("cache_creation_input_tokens");

    let mut first_tool = None;
    let mut bash_command = None;
    if let Some(content) = message.get("content").and_then(|x| x.as_array()) {
        if let Some(tool) = content
            .iter()
            .find(|c| c.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
        {
            let name = tool.get("name").and_then(|x| x.as_str()).map(String::from);
            if name.as_deref() == Some("Bash") {
                bash_command = tool
                    .get("input")
                    .and_then(|i| i.get("command"))
                    .and_then(|c| c.as_str())
                    .map(String::from);
            }
            first_tool = name;
        }
    }

    Some(RawTurn {
        message_id,
        model,
        timestamp,
        session,
        is_sidechain,
        attribution_agent,
        input_tokens,
        output_tokens,
        cache_read,
        cache_write,
        first_tool,
        bash_command,
    })
}

/// Un turno per `message.id`, tenendo il chunk con `output_tokens` massimo.
/// Lo streaming scrive lo stesso `message.id` due o tre volte, crescente
/// (1→1→527) col resto costante: l'ultimo è anche l'unico a portare il
/// `tool_use` completo, quindi è anche quello giusto da cui classificare. Chi
/// arriva prima vince il posto nell'ordine, che la durata di un batch legge
/// dal primo e dall'ultimo timestamp aggregato.
pub fn dedupe_by_message_id(turns: Vec<RawTurn>) -> Vec<RawTurn> {
    let mut order: Vec<String> = Vec::new();
    let mut best: std::collections::HashMap<String, RawTurn> = std::collections::HashMap::new();
    for t in turns {
        let id = t.message_id.clone();
        let keep = match best.get(&id) {
            Some(existing) => t.output_tokens > existing.output_tokens,
            None => {
                order.push(id.clone());
                true
            }
        };
        if keep {
            best.insert(id, t);
        }
    }
    order.into_iter().filter_map(|id| best.remove(&id)).collect()
}

// ============================================================
// La classificazione meccanica.
// ============================================================

const MECHANICAL_AGENT_TYPES: &[&str] = &["measurer", "Explore", "code-reviewer"];

/// Verbi di sola lettura, ancorati all'inizio del sottocomando — porta
/// `READ_VERB_RE` di `analyze2.py` **con lo stesso limite**: come `re.match`
/// in Python, un prefisso che combacia basta a far passare l'intero
/// sottocomando, anche se dopo c'è dell'altro (`git status && ls` passa per
/// intero sul prefisso `git status`). Non è un difetto da correggere qui: è
/// la definizione che il documento del 22/08 ha già misurato, e cambiarla
/// sposterebbe i numeri del rapporto rispetto a quel confronto.
fn read_verb() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        // Una riga sola apposta: una stringa raw spezzata su più righe con un
        // backslash a fine riga NON continua la riga come in una stringa
        // normale — il backslash e l'a-capo restano dentro il pattern, e la
        // regex smette di combaciare con qualunque comando vero.
        regex::Regex::new(concat!(
            r"^(git\s+(status|log|diff|show|branch|blame|rev-list|rev-parse|describe|remote(\s+-v)?|fetch)\b",
            r"|ls\b|cat\b|wc\b|find\b|grep\b|rg\b|pwd\b|which\b|head\b|tail\b|file\b|stat\b|readlink\b",
            r"|echo\b|sed\s+-n\b|gh\s+(pr\s+(view|diff|checks)|run\s+(list|view)|api\b|issue\s+view)",
            r"|env\b|sysctl\b|ollama\s+(list|ps)|du\b|df\b|whoami\b|date\b|uname\b)",
        ))
        .expect("il verbo di lettura deve compilare")
    })
}

fn var_assignment() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*=\S*$").expect("valida"))
}

fn git_c_prefix() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^git\s+-C\s+\S+\s+").expect("valida"))
}

const WRITE_HINTS: &[&str] = &[
    " rm ", " rm -", "mv ", "cp ", "mkdir", "touch ", " > ", " >> ", "git commit", "git push",
    "git add", "git merge", "git rebase", "git checkout", "git reset", "git stash", "npm install",
    "pip install", "sed -i", "chmod", "kill ", "launchctl",
];

/// Un comando `Bash` è di sola lettura se nessun sottocomando (separato da
/// `;` o a capo) scrive niente. Porta `classify_bash` di `analyze2.py` — un
/// comando vuoto o senza nessun verbo riconosciuto NON è di sola lettura: il
/// caso vuoto è deliberatamente escluso, o un comando ignoto passerebbe come
/// meccanico per assenza di prove contrarie.
pub fn is_read_only_bash(command: &str) -> bool {
    let lower = command.to_lowercase();
    if WRITE_HINTS.iter().any(|h| lower.contains(h)) {
        return false;
    }
    let mut saw_any = false;
    for part in command.split(['\n', ';']) {
        let part = part.trim();
        if part.is_empty() || part.starts_with('#') {
            continue;
        }
        if var_assignment().is_match(part) {
            continue;
        }
        let stripped = git_c_prefix().replacen(part, 1, "git ");
        if read_verb().is_match(&stripped) {
            saw_any = true;
            continue;
        }
        return false;
    }
    saw_any
}

/// Il turno è delegabile a un modello più economico senza perdita di
/// giudizio: legge, non decide. Le due regole non si sommano — un subagent
/// che legge un file con `Read` non è meccanico per questo solo motivo, se
/// il suo mestiere non è in [`MECHANICAL_AGENT_TYPES`].
pub fn is_mechanical(t: &RawTurn) -> bool {
    if t.is_sidechain {
        t.attribution_agent
            .as_deref()
            .is_some_and(|a| MECHANICAL_AGENT_TYPES.contains(&a))
    } else {
        match t.first_tool.as_deref() {
            Some("Read") | Some("Glob") | Some("Grep") => true,
            Some("Bash") => is_read_only_bash(t.bash_command.as_deref().unwrap_or("")),
            _ => false,
        }
    }
}

/// Il mestiere di un turno: l'`attributionAgent` dichiarato, o `main` per un
/// turno principale senza attribuzione, o `unknown` per un turno in
/// sidechain a cui manca — il campo può assentarsi anche lì.
pub fn job_of(t: &RawTurn) -> String {
    match &t.attribution_agent {
        Some(a) => a.clone(),
        None if t.is_sidechain => "unknown".to_string(),
        None => "main".to_string(),
    }
}

// ============================================================
// La finestra temporale.
// ============================================================

/// L'istante di un timestamp ISO8601 UTC, in millisecondi dall'epoca. `None`
/// se la stringa è troppo corta, i campi non sono numeri, o la data non
/// esiste — un timestamp rotto non deve far crollare l'aggregazione, solo
/// non contribuire a una durata.
pub fn parse_iso_epoch_millis(ts: &str) -> Option<i64> {
    if ts.len() < 19 {
        return None;
    }
    let y: i64 = ts.get(0..4)?.parse().ok()?;
    let mo: i64 = ts.get(5..7)?.parse().ok()?;
    let d: i64 = ts.get(8..10)?.parse().ok()?;
    let h: i64 = ts.get(11..13)?.parse().ok()?;
    let mi: i64 = ts.get(14..16)?.parse().ok()?;
    let s: i64 = ts.get(17..19)?.parse().ok()?;
    let ms: i64 = if ts.len() >= 23 && ts.as_bytes().get(19) == Some(&b'.') {
        ts.get(20..23)?.parse().ok()?
    } else {
        0
    };
    let date = Date::new(y, mo, d)?;
    Some(date.days() * 86_400_000 + h * 3_600_000 + mi * 60_000 + s * 1000 + ms)
}

/// La mezzanotte UTC di `days` giorni prima di `now_epoch_secs`, nella stessa
/// forma dei timestamp del transcript — confrontabile a livello di stringa.
pub fn cutoff_iso(now_epoch_secs: i64, days: u64) -> String {
    let day = now_epoch_secs.div_euclid(86_400) - days as i64;
    format!("{}T00:00:00.000Z", Date::from_days(day).iso())
}

/// Il timestamp cade dentro la finestra: confronto lessicografico, valido
/// perché entrambe le stringhe sono ISO8601 UTC a larghezza fissa.
pub fn within_window(ts: &str, cutoff: &str) -> bool {
    ts >= cutoff
}

/// I primi dieci caratteri di un timestamp ISO8601 — la data, per il
/// raggruppamento giornaliero del backfill. Nessuna aritmetica: la data è già
/// scritta nella stringa.
pub fn day_bucket(ts: &str) -> &str {
    if ts.len() >= 10 {
        &ts[..10]
    } else {
        ts
    }
}

/// La durata fra due timestamp, in secondi. Zero se uno dei due non si legge
/// — un buco nella misura non deve produrre un numero negativo o un panico.
pub fn duration_seconds(first_ts: &str, last_ts: &str) -> f64 {
    match (
        parse_iso_epoch_millis(first_ts),
        parse_iso_epoch_millis(last_ts),
    ) {
        (Some(a), Some(b)) => (b - a).max(0) as f64 / 1000.0,
        _ => 0.0,
    }
}

// ============================================================
// Il cursore: quanto di un file è già stato letto.
// ============================================================

/// La coda di `text` non ancora letta rispetto a un cursore in byte, **tagliata
/// all'ultimo `\n` completo**, e il nuovo cursore. Una riga ancora a metà
/// scrittura (nessun `\n` finale) non è nella parte restituita: resta sul
/// disco, il cursore non la supera, e chi chiama la rilegge intera al giro
/// dopo — senza questo taglio un cursore avanzato fino a fine-file perdeva
/// per sempre la metà già letta di una riga troncata. Un cursore oltre la
/// lunghezza del testo (file troncato o ruotato) riparte da zero invece di
/// restituire il vuoto per sempre. Il punto di partenza si allinea al
/// confine di carattere più vicino non oltre: un cursore a metà di un
/// carattere multi-byte non deve far crollare la slice.
pub fn tail_from_cursor(text: &str, cursor: u64) -> (&str, u64) {
    let raw_start = if (cursor as usize) <= text.len() {
        cursor as usize
    } else {
        0
    };
    let mut start = raw_start;
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    let tail = &text[start..];
    let complete_len = tail.rfind('\n').map(|i| i + 1).unwrap_or(0);
    (&tail[..complete_len], start as u64 + complete_len as u64)
}

/// Un nome di file stabile per il filesystem, derivato da un percorso di
/// transcript — FNV-1a a 64 bit, esadecimale. Serve al cursore per file
/// (`state/costi-cursori/<chiave>.json`), uno per transcript invece di una
/// mappa unica: due `Stop` su transcript diversi non si contendono più lo
/// stesso file. Non deve resistere a collisioni intenzionali, solo
/// distinguere fra loro i pochi transcript che una sessione tocca.
pub fn cursor_key(path: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in path.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// Il percorso del transcript di un subagent, dedotto da quello della madre:
/// `.../<slug>/<sessionId>.jsonl` diventa
/// `.../<slug>/<sessionId>/subagents/agent-<agentId>.jsonl`. Nessun accesso
/// al disco: solo manipolazione del percorso, quindi è pura anche se produce
/// un percorso che potrebbe non esistere.
pub fn child_transcript_path(mother_transcript_path: &str, agent_id: &str) -> Option<String> {
    let p = std::path::Path::new(mother_transcript_path);
    let stem = p.file_stem()?.to_str()?;
    let parent = p.parent()?;
    Some(
        parent
            .join(stem)
            .join("subagents")
            .join(format!("agent-{agent_id}.jsonl"))
            .to_string_lossy()
            .into_owned(),
    )
}

// ============================================================
// L'aggregazione: da una lista di turni a un totale.
// ============================================================

/// La somma di un gruppo di turni, con la finestra temporale che coprono.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Totals {
    pub turns: u64,
    pub raw_records: u64,
    pub mechanical_turns: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub first_ts: String,
    pub last_ts: String,
}

/// Aggrega turni già deduplicati. `raw_records` arriva da fuori perché è un
/// conteggio pre-deduplica: qui dentro i turni sono già uno per `message.id`.
pub fn totals_of(deduped: &[RawTurn], raw_records: u64) -> Totals {
    let mut t = Totals {
        raw_records,
        ..Default::default()
    };
    for turn in deduped {
        t.turns += 1;
        if is_mechanical(turn) {
            t.mechanical_turns += 1;
        }
        t.input_tokens += turn.input_tokens;
        t.output_tokens += turn.output_tokens;
        t.cache_read += turn.cache_read;
        t.cache_write += turn.cache_write;
        if t.first_ts.is_empty() || turn.timestamp < t.first_ts {
            t.first_ts = turn.timestamp.clone();
        }
        if turn.timestamp > t.last_ts {
            t.last_ts = turn.timestamp.clone();
        }
    }
    t
}

// ============================================================
// Il listino: un sottoinsieme minimo di TOML, scritto a mano perché il
// workspace vieta nuove dipendenze (tempo di avvio dei binari).
// ============================================================

#[derive(Debug, Clone, PartialEq)]
pub enum TomlValue {
    Number(f64),
    Text(String),
}

/// Sezioni `[nome]`, righe `chiave = numero` o `chiave = "stringa"`,
/// commenti `#`. Non è un parser TOML generale: niente array, niente tabelle
/// annidate, niente stringhe multi-riga — basta e avanza per un listino di
/// prezzi. Una riga che non si legge (numero rotto, chiave fuori da ogni
/// sezione) si scarta da sola: il resto del file resta leggibile.
pub fn parse_toml(text: &str) -> BTreeMap<String, BTreeMap<String, TomlValue>> {
    let mut out: BTreeMap<String, BTreeMap<String, TomlValue>> = BTreeMap::new();
    let mut section = String::new();
    for raw_line in text.lines() {
        let line = strip_toml_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            section = name.trim().to_string();
            out.entry(section.clone()).or_default();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if section.is_empty() {
            continue; // una chiave prima di qualunque `[sezione]`: non ha una tabella
        }
        let key = key.trim().to_string();
        let value = value.trim();
        let parsed = if let Some(s) = value.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
            Some(TomlValue::Text(s.to_string()))
        } else {
            value.parse::<f64>().ok().map(TomlValue::Number)
        };
        if let Some(v) = parsed {
            out.entry(section.clone()).or_default().insert(key, v);
        }
    }
    out
}

fn strip_toml_comment(line: &str) -> &str {
    match line.find('#') {
        Some(i) => &line[..i],
        None => line,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Listino {
    pub usd_eur: f64,
    pub models: BTreeMap<String, ModelPrice>,
}

impl Default for Listino {
    fn default() -> Self {
        Listino {
            usd_eur: 1.0,
            models: BTreeMap::new(),
        }
    }
}

/// Il listino dal testo TOML. Una sezione `[cambio]` mancante lascia il
/// cambio a 1.0 (dollari, non euro — un difetto visibile, non un euro
/// inventato). Una sezione modello con un prezzo mancante o rotto non entra
/// nel listino: il rapporto la tratterà come «n/d», mai come zero.
pub fn listino_from_toml(text: &str) -> Listino {
    let tables = parse_toml(text);
    let as_number = |v: Option<&TomlValue>| match v {
        Some(TomlValue::Number(n)) => Some(*n),
        _ => None,
    };
    let usd_eur = tables
        .get("cambio")
        .and_then(|t| as_number(t.get("usd_eur")))
        .unwrap_or(1.0);
    let mut models = BTreeMap::new();
    for (name, table) in &tables {
        if name == "cambio" {
            continue;
        }
        let get = |k: &str| as_number(table.get(k));
        if let (Some(input), Some(output), Some(cache_read), Some(cache_write)) =
            (get("input"), get("output"), get("cache_read"), get("cache_write"))
        {
            models.insert(
                name.clone(),
                ModelPrice {
                    input,
                    output,
                    cache_read,
                    cache_write,
                },
            );
        }
    }
    Listino { usd_eur, models }
}

/// Il costo in euro di `tokens` token a `price_per_million_usd` dollari al
/// milione, cambiati al tasso `usd_eur`.
pub fn cost_eur(tokens: u64, price_per_million_usd: f64, usd_eur: f64) -> f64 {
    (tokens as f64 / 1_000_000.0) * price_per_million_usd * usd_eur
}

fn totals_cost_eur(t: &Totals, price: &ModelPrice, usd_eur: f64) -> f64 {
    cost_eur(t.input_tokens, price.input, usd_eur)
        + cost_eur(t.output_tokens, price.output, usd_eur)
        + cost_eur(t.cache_read, price.cache_read, usd_eur)
        + cost_eur(t.cache_write, price.cache_write, usd_eur)
}

// ============================================================
// La riga del registro (`state/costi.jsonl`) e il rapporto.
// ============================================================

#[derive(Debug, Clone, PartialEq)]
pub struct LedgerRow {
    pub ts: String,
    pub event: String,
    pub session: String,
    pub agent_id: Option<String>,
    pub agent_type: String,
    pub model: String,
    pub turns: u64,
    pub raw_records: u64,
    pub mechanical_turns: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub duration_s: f64,
    pub first_ts: String,
    pub last_ts: String,
    pub source: String,
}

pub fn render_ledger_row(row: &LedgerRow) -> String {
    serde_json::json!({
        "ts": row.ts,
        "event": row.event,
        "session": row.session,
        "agent_id": row.agent_id,
        "agent_type": row.agent_type,
        "model": row.model,
        "turns": row.turns,
        "raw_records": row.raw_records,
        "mechanical_turns": row.mechanical_turns,
        "input_tokens": row.input_tokens,
        "output_tokens": row.output_tokens,
        "cache_read": row.cache_read,
        "cache_write": row.cache_write,
        "duration_s": row.duration_s,
        "first_ts": row.first_ts,
        "last_ts": row.last_ts,
        "source": row.source,
    })
    .to_string()
}

pub fn parse_ledger_line(line: &str) -> Option<LedgerRow> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let u = |k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    Some(LedgerRow {
        ts: s("ts"),
        event: s("event"),
        session: s("session"),
        agent_id: v.get("agent_id").and_then(|x| x.as_str()).map(String::from),
        agent_type: {
            let a = s("agent_type");
            if a.is_empty() { "unknown".to_string() } else { a }
        },
        model: v.get("model")?.as_str()?.to_string(),
        turns: u("turns"),
        raw_records: u("raw_records"),
        mechanical_turns: u("mechanical_turns"),
        input_tokens: u("input_tokens"),
        output_tokens: u("output_tokens"),
        cache_read: u("cache_read"),
        cache_write: u("cache_write"),
        duration_s: v.get("duration_s").and_then(|x| x.as_f64()).unwrap_or(0.0),
        first_ts: s("first_ts"),
        last_ts: s("last_ts"),
        source: {
            let src = s("source");
            if src.is_empty() { "hook".to_string() } else { src }
        },
    })
}

/// La chiave di copertura di una riga: sessione, `agent_id` (stringa vuota
/// per la sessione principale), modello, giorno UTC di `first_ts`.
fn coverage_key(r: &LedgerRow) -> (String, String, String, String) {
    (
        r.session.clone(),
        r.agent_id.clone().unwrap_or_default(),
        r.model.clone(),
        day_bucket(&r.first_ts).to_string(),
    )
}

fn parse_ymd(s: &str) -> Option<Date> {
    if s.len() < 10 {
        return None;
    }
    Date::new(s.get(0..4)?.parse().ok()?, s.get(5..7)?.parse().ok()?, s.get(8..10)?.parse().ok()?)
}

/// I giorni UTC coperti da una riga hook: da `day_bucket(first_ts)` a
/// `day_bucket(last_ts)` inclusi. Una riga hook aggrega tutti i turni letti
/// in un giro solo — se il primo cade alle 23:59 e il successivo alle 00:01,
/// copre entrambi i giorni, non solo quello del primo: `backfill`, che
/// raggruppa per turno, avrebbe altrimenti trovato il turno di mezzanotte in
/// un giorno «scoperto» e lo avrebbe ricontato. `last_ts` vuoto (riga di
/// correzione, `turns:0`) copre solo il giorno di `first_ts`.
fn hook_days(r: &LedgerRow) -> Vec<String> {
    let start_str = day_bucket(&r.first_ts);
    let end_str = if r.last_ts.is_empty() { start_str } else { day_bucket(&r.last_ts) };
    match (parse_ymd(start_str), parse_ymd(end_str)) {
        (Some(a), Some(b)) if b.days() >= a.days() => {
            (a.days()..=b.days()).map(|d| Date::from_days(d).iso()).collect()
        }
        _ => vec![start_str.to_string()],
    }
}

/// Scarta le righe `source:"backfill"` la cui chiave di copertura combacia
/// con un giorno coperto da una riga `source:"hook"` già presente: il gancio,
/// una volta acceso, è la fonte più vera da lì in avanti, il backfill riempie
/// solo i buchi di prima. LIMITE ACCETTATO: un giorno coperto solo a metà dal
/// gancio non viene completato dal backfill per quella chiave — ma non si
/// conta mai due volte, ed è il gancio a coprire tutto da quando è acceso.
/// Restituisce le righe tenute e quante `backfill` sono state scartate.
pub fn dedupe_backfill_covered_by_hook(rows: Vec<LedgerRow>) -> (Vec<LedgerRow>, usize) {
    let mut hook_keys: BTreeSet<(String, String, String, String)> = BTreeSet::new();
    for r in rows.iter().filter(|r| r.source == "hook") {
        for day in hook_days(r) {
            hook_keys.insert((r.session.clone(), r.agent_id.clone().unwrap_or_default(), r.model.clone(), day));
        }
    }
    let mut discarded = 0usize;
    let kept = rows
        .into_iter()
        .filter(|r| {
            let covered = r.source == "backfill" && hook_keys.contains(&coverage_key(r));
            if covered {
                discarded += 1;
            }
            !covered
        })
        .collect();
    (kept, discarded)
}

/// La somma di più righe del registro, con il costo in euro se il listino
/// conosce il modello — e l'insieme dei modelli che non conosce, per marcare
/// il totale come parziale invece di far sparire la differenza.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GroupTotals {
    pub totals: Totals,
    pub cost_eur: f64,
    pub unpriced_models: BTreeSet<String>,
}

/// Raggruppa le righe del registro per una chiave (modello o mestiere) e
/// somma costi e token. `key` prende la riga intera perché la chiave può
/// essere il modello o il mestiere — i due assi del rapporto.
pub fn group_ledger(
    rows: &[LedgerRow],
    listino: &Listino,
    key: impl Fn(&LedgerRow) -> String,
) -> BTreeMap<String, GroupTotals> {
    let mut out: BTreeMap<String, GroupTotals> = BTreeMap::new();
    for r in rows {
        let g = out.entry(key(r)).or_default();
        g.totals.turns += r.turns;
        g.totals.raw_records += r.raw_records;
        g.totals.mechanical_turns += r.mechanical_turns;
        g.totals.input_tokens += r.input_tokens;
        g.totals.output_tokens += r.output_tokens;
        g.totals.cache_read += r.cache_read;
        g.totals.cache_write += r.cache_write;
        if g.totals.first_ts.is_empty() || r.first_ts < g.totals.first_ts {
            g.totals.first_ts = r.first_ts.clone();
        }
        if r.last_ts > g.totals.last_ts {
            g.totals.last_ts = r.last_ts.clone();
        }
        match listino.models.get(&r.model) {
            Some(price) => {
                let row_totals = Totals {
                    input_tokens: r.input_tokens,
                    output_tokens: r.output_tokens,
                    cache_read: r.cache_read,
                    cache_write: r.cache_write,
                    ..Default::default()
                };
                g.cost_eur += totals_cost_eur(&row_totals, price, listino.usd_eur);
            }
            None => {
                g.unpriced_models.insert(r.model.clone());
            }
        }
    }
    out
}

fn format_millions(n: u64) -> String {
    format!("{:.2}", n as f64 / 1_000_000.0)
}

fn format_percent(part: u64, total: u64) -> String {
    if total == 0 {
        "0.0%".to_string()
    } else {
        format!("{:.1}%", 100.0 * part as f64 / total as f64)
    }
}

fn format_cost(g: &GroupTotals) -> String {
    if g.unpriced_models.is_empty() {
        format!("{:.2}", g.cost_eur)
    } else {
        format!("{:.2}*", g.cost_eur)
    }
}

fn render_table(out: &mut String, title: &str, groups: &BTreeMap<String, GroupTotals>) {
    let _ = writeln!(out, "-- {title} --");
    let _ = writeln!(
        out,
        "chiave\tturni\trecord_grezzi\t%meccanici\toutput_M\tcache_letta_M\tcache_scritta_M\tcosto_eur"
    );
    for (key, g) in groups {
        let _ = writeln!(
            out,
            "{key}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            g.totals.turns,
            g.totals.raw_records,
            format_percent(g.totals.mechanical_turns, g.totals.turns),
            format_millions(g.totals.output_tokens),
            format_millions(g.totals.cache_read),
            format_millions(g.totals.cache_write),
            format_cost(g),
        );
    }
}

/// Il rapporto completo: per modello, per mestiere, i totali e il confronto
/// turni-reali/record-grezzi. Tabulato, non allineato a colonne fisse — così
/// resta un caso di prova esatto invece di dipendere dalla larghezza dei nomi.
pub fn render_report(rows: &[LedgerRow], listino: &Listino) -> String {
    let mut out = String::new();
    let by_model = group_ledger(rows, listino, |r| r.model.clone());
    let by_job = group_ledger(rows, listino, |r| r.agent_type.clone());
    render_table(&mut out, "Per modello", &by_model);
    out.push('\n');
    render_table(&mut out, "Per mestiere", &by_job);
    out.push('\n');

    let mut grand_totals = Totals::default();
    let mut grand_cost = 0.0;
    let mut unpriced: BTreeSet<String> = BTreeSet::new();
    for g in by_model.values() {
        grand_totals.turns += g.totals.turns;
        grand_totals.raw_records += g.totals.raw_records;
        grand_totals.mechanical_turns += g.totals.mechanical_turns;
        grand_totals.output_tokens += g.totals.output_tokens;
        grand_totals.cache_read += g.totals.cache_read;
        grand_totals.cache_write += g.totals.cache_write;
        grand_cost += g.cost_eur;
        unpriced.extend(g.unpriced_models.iter().cloned());
    }
    let _ = writeln!(
        out,
        "totale: {} turni, {} record grezzi, output {} M, cache letta {} M, cache scritta {} M, costo {:.2} eur{}",
        grand_totals.turns,
        grand_totals.raw_records,
        format_millions(grand_totals.output_tokens),
        format_millions(grand_totals.cache_read),
        format_millions(grand_totals.cache_write),
        grand_cost,
        if unpriced.is_empty() { String::new() } else { " (parziale)".to_string() },
    );
    let _ = writeln!(
        out,
        "turni reali vs record grezzi: {} / {} ({})",
        grand_totals.turns,
        grand_totals.raw_records,
        format_percent(grand_totals.turns, grand_totals.raw_records),
    );
    if !unpriced.is_empty() {
        let list: Vec<&str> = unpriced.iter().map(String::as_str).collect();
        let _ = writeln!(out, "senza prezzo nel listino: {}", list.join(", "));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str, out: u64, ts: &str, model: &str) -> String {
        serde_json::json!({
            "type": "assistant",
            "timestamp": ts,
            "sessionId": "sess1",
            "isSidechain": false,
            "message": {
                "id": id,
                "model": model,
                "usage": {
                    "input_tokens": 2,
                    "output_tokens": out,
                    "cache_read_input_tokens": 100,
                    "cache_creation_input_tokens": 10,
                },
                "content": [],
            },
        })
        .to_string()
    }

    // --- deduplica: tre chunk, stesso id, output crescente ---

    #[test]
    fn streaming_chunks_collapse_into_one_turn_with_max_output() {
        let turns: Vec<RawTurn> = [
            line("msg_1", 1, "2026-08-11T14:00:00.000Z", "claude-sonnet-5"),
            line("msg_1", 1, "2026-08-11T14:00:01.000Z", "claude-sonnet-5"),
            line("msg_1", 527, "2026-08-11T14:00:02.000Z", "claude-sonnet-5"),
        ]
        .iter()
        .filter_map(|l| parse_transcript_line(l))
        .collect();
        assert_eq!(turns.len(), 3, "il parser deve leggere tutti e tre i chunk grezzi");
        let deduped = dedupe_by_message_id(turns);
        assert_eq!(deduped.len(), 1, "un message.id vale un turno");
        assert_eq!(deduped[0].output_tokens, 527);
        // cache e input si contano una volta sola: quella del chunk tenuto,
        // non la somma dei tre.
        assert_eq!(deduped[0].cache_read, 100);
        assert_eq!(deduped[0].input_tokens, 2);
    }

    #[test]
    fn two_different_ids_stay_two_turns() {
        let turns: Vec<RawTurn> = [
            line("msg_a", 10, "2026-08-11T14:00:00.000Z", "claude-sonnet-5"),
            line("msg_b", 20, "2026-08-11T14:00:01.000Z", "claude-sonnet-5"),
        ]
        .iter()
        .filter_map(|l| parse_transcript_line(l))
        .collect();
        assert_eq!(dedupe_by_message_id(turns).len(), 2);
    }

    #[test]
    fn a_tie_on_output_tokens_keeps_the_chunk_that_arrived_first() {
        // Due chunk con lo stesso `message.id` e lo stesso `output_tokens`:
        // resta il primo, perché il secondo non porta niente di più. Se
        // vincesse l'ultimo, il primo timestamp del turno slitterebbe in
        // avanti e la durata del batch si accorcerebbe da sola.
        let turns: Vec<RawTurn> = [
            line("msg_1", 42, "2026-08-11T14:00:00.000Z", "claude-sonnet-5"),
            line("msg_1", 42, "2026-08-11T14:00:09.000Z", "claude-sonnet-5"),
        ]
        .iter()
        .filter_map(|l| parse_transcript_line(l))
        .collect();
        let deduped = dedupe_by_message_id(turns);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].timestamp, "2026-08-11T14:00:00.000Z");
    }

    // --- il primo `tool_use` dentro il contenuto del messaggio ---

    fn line_with_content(id: &str, content: serde_json::Value) -> String {
        serde_json::json!({
            "type": "assistant",
            "timestamp": "2026-08-11T14:00:00.000Z",
            "sessionId": "sess1",
            "isSidechain": false,
            "message": {
                "id": id,
                "model": "claude-sonnet-5",
                "usage": { "input_tokens": 1, "output_tokens": 1 },
                "content": content,
            },
        })
        .to_string()
    }

    #[test]
    fn the_first_tool_use_is_found_past_a_text_block() {
        // Il turno tipico apre con un blocco `text` e chiama lo strumento
        // subito dopo: se la ricerca prendesse il primo blocco qualunque,
        // ogni turno con del testo davanti risulterebbe senza strumento — e
        // quindi mai meccanico.
        let l = line_with_content(
            "msg_1",
            serde_json::json!([
                { "type": "text", "text": "guardo il registro" },
                { "type": "tool_use", "name": "Bash", "input": { "command": "git status" } },
            ]),
        );
        let t = parse_transcript_line(&l).expect("è un turno assistant con usage");
        assert_eq!(t.first_tool.as_deref(), Some("Bash"));
        assert_eq!(t.bash_command.as_deref(), Some("git status"));
        assert!(is_mechanical(&t), "un git status dietro del testo resta meccanico");
    }

    #[test]
    fn only_a_bash_tool_use_carries_the_command() {
        // Un campo `input.command` può comparire anche su strumenti che non
        // sono Bash: il comando si legge solo quando lo strumento è davvero
        // Bash, o un parametro omonimo finirebbe nel giudizio di scrittura.
        let l = line_with_content(
            "msg_2",
            serde_json::json!([
                { "type": "tool_use", "name": "Read", "input": { "command": "rm -rf /x" } },
            ]),
        );
        let t = parse_transcript_line(&l).expect("turno valido");
        assert_eq!(t.first_tool.as_deref(), Some("Read"));
        assert_eq!(t.bash_command, None, "solo Bash porta un comando");
    }

    // --- finestra per timestamp ---

    #[test]
    fn within_window_compares_lexicographically() {
        let cutoff = "2026-08-16T00:00:00.000Z";
        assert!(within_window("2026-08-16T00:00:00.000Z", cutoff));
        assert!(within_window("2026-08-20T09:00:00.000Z", cutoff));
        assert!(!within_window("2026-08-15T23:59:59.999Z", cutoff));
    }

    #[test]
    fn cutoff_iso_goes_back_the_right_number_of_days() {
        // 2026-08-23T12:00:00Z: sette giorni prima cade a mezzanotte del 16.
        let now = Date::new(2026, 8, 23).unwrap().days() * 86_400 + 12 * 3600;
        assert_eq!(cutoff_iso(now, 7), "2026-08-16T00:00:00.000Z");
        assert_eq!(cutoff_iso(now, 0), "2026-08-23T00:00:00.000Z");
    }

    #[test]
    fn day_bucket_slices_the_date_without_arithmetic() {
        assert_eq!(day_bucket("2026-08-11T14:05:14.688Z"), "2026-08-11");
        assert_eq!(day_bucket("corto"), "corto");
    }

    #[test]
    fn duration_seconds_reads_the_gap_between_two_instants() {
        assert_eq!(
            duration_seconds("2026-08-11T14:00:00.000Z", "2026-08-11T14:00:10.500Z"),
            10.5
        );
        assert_eq!(duration_seconds("boh", "2026-08-11T14:00:00.000Z"), 0.0);
    }

    #[test]
    fn parse_iso_epoch_millis_lands_on_the_exact_instant() {
        // Il valore atteso non viene da questa stessa formula: è l'istante di
        // 2026-08-11T14:05:14.688Z contato fuori di qui. Ogni pezzo — giorni,
        // ore, minuti, secondi, millesimi — pesa col proprio fattore, e un
        // fattore sbagliato sposta il totale invece di sparire nella somma.
        assert_eq!(
            parse_iso_epoch_millis("2026-08-11T14:05:14.688Z"),
            Some(1_786_457_114_688)
        );
    }

    #[test]
    fn a_timestamp_without_milliseconds_is_still_an_instant() {
        // Diciannove caratteri sono già un istante completo al secondo: la
        // soglia esclude ciò che è più corto, non questo. E ciò che segue i
        // secondi si legge come millesimi solo se è davvero un punto
        // decimale a tre cifre — un fuso o una frazione troncata valgono
        // zero millesimi, non una riga scartata.
        let base = parse_iso_epoch_millis("2026-08-11T14:05:14").expect("19 caratteri bastano");
        assert_eq!(base, 1_786_457_114_000);
        assert_eq!(parse_iso_epoch_millis("2026-08-11T14:05:14+00:00"), Some(base));
        assert_eq!(parse_iso_epoch_millis("2026-08-11T14:05:14.6Z"), Some(base));
        assert_eq!(parse_iso_epoch_millis("2026-08-11T14:05"), None, "troppo corto");
    }

    // --- classificazione meccanica: almeno sei casi ---

    fn turn(is_sidechain: bool, attribution: Option<&str>, tool: Option<&str>, bash: Option<&str>) -> RawTurn {
        RawTurn {
            message_id: "m".into(),
            model: "claude-sonnet-5".into(),
            timestamp: "2026-08-20T00:00:00.000Z".into(),
            session: "s".into(),
            is_sidechain,
            attribution_agent: attribution.map(String::from),
            input_tokens: 1,
            output_tokens: 1,
            cache_read: 0,
            cache_write: 0,
            first_tool: tool.map(String::from),
            bash_command: bash.map(String::from),
        }
    }

    #[test]
    fn a_read_call_is_mechanical() {
        assert!(is_mechanical(&turn(false, None, Some("Read"), None)));
    }

    #[test]
    fn a_read_only_bash_chain_is_mechanical() {
        assert!(is_mechanical(&turn(
            false,
            None,
            Some("Bash"),
            Some("git status && ls")
        )));
    }

    #[test]
    fn a_git_push_is_not_mechanical() {
        assert!(!is_mechanical(&turn(false, None, Some("Bash"), Some("git push"))));
    }

    #[test]
    fn an_edit_is_not_mechanical() {
        assert!(!is_mechanical(&turn(false, None, Some("Edit"), None)));
    }

    #[test]
    fn a_measurer_sidechain_is_mechanical() {
        assert!(is_mechanical(&turn(true, Some("measurer"), Some("Bash"), Some("echo hi"))));
    }

    #[test]
    fn a_builder_sidechain_reading_a_file_is_not_mechanical() {
        // la regola sidechain guarda SOLO l'agentType, non lo strumento: un
        // builder che legge con Read non diventa meccanico per questo.
        assert!(!is_mechanical(&turn(true, Some("builder"), Some("Read"), None)));
    }

    #[test]
    fn plain_text_without_a_tool_is_not_mechanical() {
        assert!(!is_mechanical(&turn(false, None, None, None)));
    }

    // --- il verbo di lettura porta il caso ---

    #[test]
    fn is_read_only_bash_handles_var_assignment_and_git_dash_c() {
        assert!(is_read_only_bash("FOO=bar; git -C /x status"));
        assert!(!is_read_only_bash("touch /tmp/a"));
        assert!(!is_read_only_bash(""));
    }

    #[test]
    fn an_empty_or_commented_subcommand_does_not_sink_a_read_only_chain() {
        // Un `;` finale lascia un sottocomando vuoto, e una riga di commento
        // non è un comando: né l'uno né l'altro scrive niente, quindi non
        // devono far cadere il giudizio di sola lettura del resto della
        // catena. Un turno di lettura contato come non meccanico sposta la
        // percentuale su cui si decide il listino.
        assert!(is_read_only_bash("git status; "));
        assert!(is_read_only_bash("# guardo e basta\nls"));
    }

    // --- job_of ---

    #[test]
    fn job_of_falls_back_by_sidechain() {
        assert_eq!(job_of(&turn(false, None, None, None)), "main");
        assert_eq!(job_of(&turn(true, None, None, None)), "unknown");
        assert_eq!(job_of(&turn(true, Some("measurer"), None, None)), "measurer");
    }

    // --- l'aggregazione di un gruppo di turni ---

    fn turn_at(ts: &str, tokens: [u64; 4], tool: Option<&str>) -> RawTurn {
        let mut t = turn(false, None, tool, None);
        t.timestamp = ts.into();
        t.input_tokens = tokens[0];
        t.output_tokens = tokens[1];
        t.cache_read = tokens[2];
        t.cache_write = tokens[3];
        t
    }

    #[test]
    fn totals_of_sums_every_column_and_spans_the_real_window() {
        // I turni arrivano nell'ordine del transcript, non in ordine di
        // timestamp: la finestra deve restare il minimo e il massimo veri.
        // Ogni colonna qui vale un numero diverso da tutte le altre, così una
        // voce persa o scambiata non si nasconde dietro una somma che torna
        // lo stesso. È il totale su cui si legge il costo di una sessione:
        // uno zero ben formato è peggio di un errore.
        let turns = vec![
            turn_at("2026-08-20T10:00:00.000Z", [1, 20, 300, 4_000], Some("Read")),
            turn_at("2026-08-20T09:00:00.000Z", [2, 40, 600, 8_000], Some("Edit")),
            turn_at("2026-08-20T11:00:00.000Z", [4, 80, 900, 16_000], Some("Edit")),
        ];
        let t = totals_of(&turns, 9);
        assert_eq!(t.turns, 3);
        assert_eq!(t.raw_records, 9, "il conteggio pre-deduplica arriva da fuori");
        assert_eq!(t.mechanical_turns, 1, "solo il turno che legge");
        assert_eq!(t.input_tokens, 7);
        assert_eq!(t.output_tokens, 140);
        assert_eq!(t.cache_read, 1_800);
        assert_eq!(t.cache_write, 28_000);
        assert_eq!(t.first_ts, "2026-08-20T09:00:00.000Z");
        assert_eq!(t.last_ts, "2026-08-20T11:00:00.000Z");
    }

    #[test]
    fn totals_of_without_turns_still_carries_the_raw_count() {
        // Nessun turno da contare non vuol dire nessun record letto: il
        // conteggio grezzo resta, ed è quello che dice che il transcript era
        // pieno di righe non contabili invece che vuoto.
        let t = totals_of(&[], 12);
        assert_eq!(t.turns, 0);
        assert_eq!(t.raw_records, 12);
        assert_eq!(t.first_ts, "");
    }

    // --- parser TOML ---

    #[test]
    fn toml_parser_reads_comments_and_sections() {
        let text = "# un commento\n[cambio]\nusd_eur = 0.92 # un altro\n[claude-sonnet-5]\ninput = 3\noutput = 15\n";
        let t = parse_toml(text);
        assert_eq!(
            t.get("cambio").and_then(|s| s.get("usd_eur")),
            Some(&TomlValue::Number(0.92))
        );
        assert_eq!(
            t.get("claude-sonnet-5").and_then(|s| s.get("input")),
            Some(&TomlValue::Number(3.0))
        );
    }

    #[test]
    fn toml_parser_ignores_a_missing_section() {
        let t = parse_toml("chiave_orfana = 5\n[vera]\nx = 1\n");
        assert!(!t.contains_key(""));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn toml_parser_drops_an_invalid_number_but_keeps_the_rest() {
        let t = parse_toml("[m]\nrotto = non-un-numero\nbuono = 2\n");
        let section = t.get("m").expect("la sezione esiste");
        assert!(!section.contains_key("rotto"));
        assert_eq!(section.get("buono"), Some(&TomlValue::Number(2.0)));
    }

    #[test]
    fn listino_defaults_the_exchange_rate_when_the_section_is_missing() {
        let l = listino_from_toml("[claude-sonnet-5]\ninput = 3\noutput = 15\ncache_read = 0.3\ncache_write = 3.75\n");
        assert_eq!(l.usd_eur, 1.0);
        assert!(l.models.contains_key("claude-sonnet-5"));
    }

    #[test]
    fn listino_drops_a_model_with_a_missing_price() {
        let l = listino_from_toml("[claude-x]\ninput = 3\noutput = 15\n"); // manca cache_read/cache_write
        assert!(!l.models.contains_key("claude-x"));
    }

    // --- calcolo euro ---

    #[test]
    fn cost_eur_scales_tokens_by_millions_and_exchange_rate() {
        // 2_000_000 token a 10$/M, cambio 0.5 -> 2*10*0.5 = 10
        assert_eq!(cost_eur(2_000_000, 10.0, 0.5), 10.0);
        assert_eq!(cost_eur(0, 999.0, 1.0), 0.0);
    }

    fn row(model: &str, job: &str, output_tokens: u64, cache_read: u64) -> LedgerRow {
        LedgerRow {
            ts: "2026-08-20T00:00:00.000Z".into(),
            event: "Stop".into(),
            session: "s".into(),
            agent_id: None,
            agent_type: job.into(),
            model: model.into(),
            turns: 1,
            raw_records: 1,
            mechanical_turns: 0,
            input_tokens: 0,
            output_tokens,
            cache_read,
            cache_write: 0,
            duration_s: 0.0,
            first_ts: "2026-08-20T00:00:00.000Z".into(),
            last_ts: "2026-08-20T00:00:00.000Z".into(),
            source: "hook".into(),
        }
    }

    #[test]
    fn group_ledger_marks_a_group_as_partial_when_a_model_has_no_price() {
        let listino = listino_from_toml("[claude-sonnet-5]\ninput=3\noutput=15\ncache_read=0.3\ncache_write=3.75\n");
        let rows = vec![
            row("claude-sonnet-5", "main", 1_000_000, 0),
            row("claude-mistero", "main", 1_000_000, 0),
        ];
        let by_model = group_ledger(&rows, &listino, |r| r.model.clone());
        assert!(by_model["claude-sonnet-5"].unpriced_models.is_empty());
        assert!(!by_model["claude-mistero"].unpriced_models.is_empty());
        assert_eq!(by_model["claude-sonnet-5"].cost_eur, 15.0); // 1M output a 15$/M
    }

    #[test]
    fn group_ledger_sums_every_column_and_the_widest_window() {
        // Due righe della stessa chiave, in ordine sparso di tempo: la somma
        // prende ogni colonna e la finestra si allarga a entrambe. Valori
        // tutti diversi fra loro, per lo stesso motivo dell'aggregazione dei
        // turni: uno scambio di colonna deve vedersi.
        let mut a = row("claude-sonnet-5", "main", 20, 300);
        a.turns = 2;
        a.raw_records = 5;
        a.mechanical_turns = 1;
        a.input_tokens = 1;
        a.cache_write = 4_000;
        a.first_ts = "2026-08-20T10:00:00.000Z".into();
        a.last_ts = "2026-08-20T10:30:00.000Z".into();
        let mut b = row("claude-sonnet-5", "main", 40, 600);
        b.turns = 8;
        b.raw_records = 11;
        b.mechanical_turns = 6;
        b.input_tokens = 2;
        b.cache_write = 8_000;
        b.first_ts = "2026-08-20T09:00:00.000Z".into();
        b.last_ts = "2026-08-20T11:00:00.000Z".into();
        let groups = group_ledger(&[a, b], &Listino::default(), |r| r.model.clone());
        let g = &groups["claude-sonnet-5"];
        assert_eq!(g.totals.turns, 10);
        assert_eq!(g.totals.raw_records, 16);
        assert_eq!(g.totals.mechanical_turns, 7);
        assert_eq!(g.totals.input_tokens, 3);
        assert_eq!(g.totals.output_tokens, 60);
        assert_eq!(g.totals.cache_read, 900);
        assert_eq!(g.totals.cache_write, 12_000);
        assert_eq!(g.totals.first_ts, "2026-08-20T09:00:00.000Z");
        assert_eq!(g.totals.last_ts, "2026-08-20T11:00:00.000Z");
    }

    #[test]
    fn group_ledger_prices_all_four_kinds_of_token() {
        // Prezzi e quantità diversi per ogni voce: input, output, cache letta
        // e cache scritta entrano nel costo ciascuna col proprio peso. Con
        // quantità o prezzi uguali, una voce persa o addizionata al posto
        // sbagliato darebbe lo stesso totale — e la cache scritta, la più
        // cara, è anche quella che una prova a zero non guarda mai.
        let listino = listino_from_toml(
            "[cambio]\nusd_eur = 1.0\n[claude-sonnet-5]\ninput=1\noutput=10\ncache_read=100\ncache_write=1000\n",
        );
        let mut r = row("claude-sonnet-5", "main", 2_000_000, 4_000_000);
        r.input_tokens = 1_000_000;
        r.cache_write = 8_000_000;
        let g = group_ledger(&[r], &listino, |r| r.model.clone());
        // 1*1 + 2*10 + 4*100 + 8*1000
        assert_eq!(g["claude-sonnet-5"].cost_eur, 8_421.0);
        assert!(g["claude-sonnet-5"].unpriced_models.is_empty());
    }

    #[test]
    fn ledger_row_round_trips_through_json() {
        let r = row("claude-opus-5", "builder", 42, 7);
        let text = render_ledger_row(&r);
        let back = parse_ledger_line(&text).expect("deve rileggersi");
        assert_eq!(back, r);
    }

    #[test]
    fn a_ledger_row_without_agent_type_reads_back_as_unknown() {
        let text = r#"{"ts":"2026-08-20T00:00:00.000Z","event":"Stop","session":"s","model":"m","turns":1,"raw_records":1,"mechanical_turns":0,"input_tokens":0,"output_tokens":0,"cache_read":0,"cache_write":0,"duration_s":0.0,"first_ts":"2026-08-20T00:00:00.000Z","last_ts":"2026-08-20T00:00:00.000Z"}"#;
        let back = parse_ledger_line(text).expect("deve leggersi anche senza agent_type/source");
        assert_eq!(back.agent_type, "unknown");
        assert_eq!(back.source, "hook");
    }

    // --- cursore: la seconda lettura non riconta ---

    #[test]
    fn a_second_read_from_the_saved_cursor_finds_nothing_new() {
        let text = "riga1\nriga2\n";
        let (first, cursor) = tail_from_cursor(text, 0);
        assert_eq!(first, "riga1\nriga2\n");
        assert_eq!(cursor, text.len() as u64);
        let (second, cursor2) = tail_from_cursor(text, cursor);
        assert_eq!(second, "");
        assert_eq!(cursor2, cursor);
    }

    #[test]
    fn a_cursor_past_a_truncated_file_restarts_from_zero_but_still_waits_for_a_newline() {
        // "corto" non ha nessun `\n`: dopo il riavvio da zero non c'è ancora
        // niente di completo da leggere, e il cursore resta a zero.
        let text = "corto";
        let (slice, cursor) = tail_from_cursor(text, 9_999);
        assert_eq!(slice, "");
        assert_eq!(cursor, 0);
    }

    #[test]
    fn a_trailing_partial_line_is_held_back_until_it_closes() {
        // Una riga a metà scrittura (senza `\n` finale) non entra nella coda
        // restituita: il cursore si ferma subito dopo l'ultima riga completa,
        // e la riga a metà si rilegge per intero al giro dopo.
        let text = "riga1\nriga2 a meta senza newline";
        let (slice, cursor) = tail_from_cursor(text, 0);
        assert_eq!(slice, "riga1\n");
        assert_eq!(cursor, 6);
        // Quando la riga si completa, il giro successivo la vede intera.
        let full = "riga1\nriga2 a meta senza newline\n";
        let (slice2, cursor2) = tail_from_cursor(full, cursor);
        assert_eq!(slice2, "riga2 a meta senza newline\n");
        assert_eq!(cursor2, full.len() as u64);
    }

    #[test]
    fn cursor_key_is_deterministic_and_distinguishes_paths() {
        assert_eq!(cursor_key("/a/b.jsonl"), cursor_key("/a/b.jsonl"));
        assert_ne!(cursor_key("/a/b.jsonl"), cursor_key("/a/c.jsonl"));
        assert_eq!(cursor_key("/a/b.jsonl").len(), 16, "esadecimale a 64 bit");
    }

    #[test]
    fn cursor_key_matches_the_published_fnv1a_vectors() {
        // I vettori di prova pubblicati di FNV-1a a 64 bit: un oracolo di
        // fuori, non la stessa formula riscritta qui. Servono perché due
        // chiavi restano diverse fra loro anche con un algoritmo sbagliato —
        // e una chiave che cambia forma spezza i cursori già sul disco.
        assert_eq!(cursor_key(""), "cbf29ce484222325");
        assert_eq!(cursor_key("a"), "af63dc4c8601ec8c");
        assert_eq!(cursor_key("foobar"), "85944171f73967e8");
    }

    #[test]
    fn a_cursor_inside_a_character_slides_back_to_its_start() {
        // Un cursore salvato a metà di un carattere multi-byte non deve far
        // crollare la slice né mangiare il carattere: si arretra al confine e
        // la riga si rilegge intera. Arretrare, non avanzare — avanzando si
        // perderebbe per sempre il carattere spezzato.
        let text = "héllo\n"; // 'é' occupa i byte 1 e 2
        let (tail, cursor) = tail_from_cursor(text, 2);
        assert_eq!(tail, "éllo\n");
        assert_eq!(cursor, text.len() as u64);
    }

    // --- percorso del figlio da agent_id ---

    #[test]
    fn child_transcript_path_is_built_from_the_mothers_session_id() {
        let mother = "/home/someone/.claude/projects/-Users-theo-x/abc123.jsonl";
        let child = child_transcript_path(mother, "def456").expect("percorso costruibile");
        assert_eq!(
            child,
            "/home/someone/.claude/projects/-Users-theo-x/abc123/subagents/agent-def456.jsonl"
        );
    }

    #[test]
    fn child_transcript_path_is_none_without_a_usable_stem() {
        assert_eq!(child_transcript_path("", "id"), None);
    }

    // --- rapporto ---

    // --- dedup fra gancio e backfill sulla stessa copertura ---

    fn row_with(model: &str, source: &str, session: &str, agent_id: Option<&str>, first_ts: &str) -> LedgerRow {
        LedgerRow {
            ts: first_ts.into(),
            event: "Stop".into(),
            session: session.into(),
            agent_id: agent_id.map(String::from),
            agent_type: "main".into(),
            model: model.into(),
            turns: 1,
            raw_records: 1,
            mechanical_turns: 0,
            input_tokens: 0,
            output_tokens: 100,
            cache_read: 0,
            cache_write: 0,
            duration_s: 0.0,
            first_ts: first_ts.into(),
            last_ts: first_ts.into(),
            source: source.into(),
        }
    }

    #[test]
    fn a_backfill_row_covered_by_a_hook_row_is_discarded() {
        let rows = vec![
            row_with("claude-sonnet-5", "hook", "sess1", None, "2026-08-20T00:00:00.000Z"),
            row_with("claude-sonnet-5", "backfill", "sess1", None, "2026-08-20T09:00:00.000Z"),
        ];
        let (kept, discarded) = dedupe_backfill_covered_by_hook(rows);
        assert_eq!(discarded, 1, "stessa sessione/modello/giorno: il backfill è coperto");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].source, "hook");
    }

    #[test]
    fn a_backfill_row_on_a_different_day_is_kept() {
        let rows = vec![
            row_with("claude-sonnet-5", "hook", "sess1", None, "2026-08-20T00:00:00.000Z"),
            row_with("claude-sonnet-5", "backfill", "sess1", None, "2026-08-21T00:00:00.000Z"),
        ];
        let (kept, discarded) = dedupe_backfill_covered_by_hook(rows);
        assert_eq!(discarded, 0, "un giorno diverso non è coperto dal gancio");
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn a_backfill_row_for_a_different_agent_is_kept() {
        let rows = vec![
            row_with("claude-sonnet-5", "hook", "sess1", None, "2026-08-20T00:00:00.000Z"),
            row_with("claude-sonnet-5", "backfill", "sess1", Some("agent-x"), "2026-08-20T00:00:00.000Z"),
        ];
        let (kept, discarded) = dedupe_backfill_covered_by_hook(rows);
        assert_eq!(discarded, 0, "un agent_id diverso ha la sua chiave a parte");
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn no_hook_rows_means_backfill_is_never_discarded() {
        let rows = vec![row_with("claude-sonnet-5", "backfill", "sess1", None, "2026-08-20T00:00:00.000Z")];
        let (kept, discarded) = dedupe_backfill_covered_by_hook(rows);
        assert_eq!(discarded, 0);
        assert_eq!(kept.len(), 1);
    }

    fn row_span(model: &str, source: &str, first_ts: &str, last_ts: &str) -> LedgerRow {
        let mut r = row_with(model, source, "sess1", None, first_ts);
        r.last_ts = last_ts.into();
        r
    }

    #[test]
    fn a_hook_row_spanning_utc_midnight_covers_both_days() {
        let rows = vec![
            row_span("claude-sonnet-5", "hook", "2026-08-19T23:59:00.000Z", "2026-08-20T00:01:00.000Z"),
            row_span("claude-sonnet-5", "backfill", "2026-08-19T23:59:00.000Z", "2026-08-19T23:59:00.000Z"),
            row_span("claude-sonnet-5", "backfill", "2026-08-20T00:01:00.000Z", "2026-08-20T00:01:00.000Z"),
        ];
        let (kept, discarded) = dedupe_backfill_covered_by_hook(rows);
        assert_eq!(discarded, 2, "entrambi i giorni scavalcati dalla riga hook sono coperti");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].source, "hook");
    }

    #[test]
    fn a_hook_row_whose_span_runs_backwards_still_covers_its_first_day() {
        // `last_ts` prima di `first_ts` è una riga malmessa, non un permesso
        // di non coprire niente: se la copertura tornasse vuota, il backfill
        // dello stesso giorno rientrerebbe e quel giorno finirebbe contato
        // due volte.
        let rows = vec![
            row_span("claude-sonnet-5", "hook", "2026-08-20T10:00:00.000Z", "2026-08-19T10:00:00.000Z"),
            row_span("claude-sonnet-5", "backfill", "2026-08-20T12:00:00.000Z", "2026-08-20T12:00:00.000Z"),
        ];
        let (kept, discarded) = dedupe_backfill_covered_by_hook(rows);
        assert_eq!(discarded, 1, "il giorno di first_ts resta coperto");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].source, "hook");
    }

    #[test]
    fn a_turns0_correction_row_has_an_empty_day_bucket() {
        // La riga di correzione (`turns:0`) non porta un `first_ts` reale: il
        // suo giorno è la stringa vuota, non «il giorno dopo» — non copre mai
        // per sbaglio un giorno vero.
        assert_eq!(day_bucket(""), "");
    }

    #[test]
    fn render_report_contains_the_real_vs_raw_line() {
        let listino = listino_from_toml("[claude-sonnet-5]\ninput=3\noutput=15\ncache_read=0.3\ncache_write=3.75\n");
        let rows = vec![row("claude-sonnet-5", "main", 1_000_000, 0)];
        let text = render_report(&rows, &listino);
        assert!(text.contains("turni reali vs record grezzi: 1 / 1"));
        assert!(text.contains("claude-sonnet-5"));
    }

    #[test]
    fn render_report_is_exact_down_to_the_last_column() {
        // Il rapporto per intero, colonna per colonna: è il numero su cui si
        // decide il listino, e un rendiconto ben formato con dentro uno zero
        // sbagliato non lo vede nessuno. Ogni cifra qui è diversa dalle
        // altre, e la percentuale non è né 0 né 100.
        let listino = listino_from_toml(
            "[cambio]\nusd_eur = 1.0\n[claude-sonnet-5]\ninput=1\noutput=10\ncache_read=100\ncache_write=1000\n",
        );
        let mut r = row("claude-sonnet-5", "main", 2_000_000, 4_000_000);
        r.turns = 4;
        r.raw_records = 5;
        r.mechanical_turns = 1;
        r.input_tokens = 1_000_000;
        r.cache_write = 8_000_000;
        let header =
            "chiave\tturni\trecord_grezzi\t%meccanici\toutput_M\tcache_letta_M\tcache_scritta_M\tcosto_eur";
        let expected = [
            "-- Per modello --",
            header,
            "claude-sonnet-5\t4\t5\t25.0%\t2.00\t4.00\t8.00\t8421.00",
            "",
            "-- Per mestiere --",
            header,
            "main\t4\t5\t25.0%\t2.00\t4.00\t8.00\t8421.00",
            "",
            "totale: 4 turni, 5 record grezzi, output 2.00 M, cache letta 4.00 M, cache scritta 8.00 M, costo 8421.00 eur",
            "turni reali vs record grezzi: 4 / 5 (80.0%)",
            "",
        ]
        .join("\n");
        assert_eq!(render_report(&[r], &listino), expected);
    }

    #[test]
    fn a_model_without_a_price_is_marked_partial_and_named() {
        // Il modello che il listino non conosce non vale zero in silenzio:
        // la sua riga porta l'asterisco, il totale si dichiara parziale e il
        // nome è elencato in fondo. Senza quell'elenco, un costo mancante si
        // legge come un costo basso.
        let listino = listino_from_toml("[claude-sonnet-5]\ninput=1\noutput=1\ncache_read=1\ncache_write=1\n");
        let text = render_report(&[row("claude-mistero", "main", 1_000_000, 0)], &listino);
        assert!(text.contains("\t0.00*\n"), "la riga del gruppo è marcata: {text}");
        assert!(text.contains("costo 0.00 eur (parziale)"), "{text}");
        assert!(text.contains("senza prezzo nel listino: claude-mistero"), "{text}");
    }

    #[test]
    fn a_report_where_every_model_has_a_price_says_nothing_about_missing_ones() {
        // La riga finale compare solo quando manca davvero un prezzo: se
        // comparisse sempre, l'avviso smetterebbe di voler dire qualcosa.
        let listino = listino_from_toml("[claude-sonnet-5]\ninput=1\noutput=1\ncache_read=1\ncache_write=1\n");
        let text = render_report(&[row("claude-sonnet-5", "main", 1_000_000, 0)], &listino);
        assert!(!text.contains("senza prezzo nel listino"), "{text}");
        assert!(!text.contains("(parziale)"), "{text}");
        assert!(text.contains("\t1.00\n"), "il costo non è marcato: {text}");
    }
}
