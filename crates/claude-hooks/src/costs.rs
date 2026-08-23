//! La colla del registro dei costi: stdin del gancio, i percorsi dei
//! transcript, i cursori, e le tre uscite (`record`, `backfill`, `report`).
//! La logica pura sta in `guards::cost_ledger`. `record` è un gancio su
//! `Stop`/`SubagentStop`, fail-open: qualunque errore finisce su stderr e il
//! codice resta 0. `backfill` e `report` sono strumenti da riga di comando,
//! nessuna radice li invoca.

// La madre si legge da un file aperto e non da `fs::read_to_string`: cresce
// per tutta la sessione, e rileggerla intera a ogni `Stop` sarebbe proprio la
// rilettura di contesto che questo registro esiste per misurare. Si apre il
// file e si va al byte del cursore salvato.
use guards::cost_ledger::{self, LedgerRow, RawTurn};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read as _, Seek, SeekFrom};
use std::path::{Path, PathBuf};

fn home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home/someone"))
}

fn state_dir() -> PathBuf {
    home().join(".claude").join("state")
}

/// Le righe scritte dal gancio (`source:"hook"`), append-only.
fn ledger_path() -> PathBuf {
    state_dir().join("costi.jsonl")
}

/// Le righe di `backfill`, in un file separato da `costi.jsonl` — così
/// riscriverlo ad ogni corsa non può mai far sparire una riga del gancio
/// appesa nel frattempo (`report` legge entrambi). Vedi `backfill()`.
fn backfill_path() -> PathBuf {
    state_dir().join("costi-backfill.jsonl")
}

fn listino_path() -> PathBuf {
    state_dir().join("listino-modelli.toml")
}

fn cursors_dir() -> PathBuf {
    state_dir().join("costi-cursori")
}

// ============================================================
// Il cursore, uno per file di transcript.
// ============================================================

/// Il turno che al giro precedente era ancora potenzialmente in streaming:
/// contato una volta, ma con un `output_tokens` che poteva non essere
/// definitivo.
#[derive(Debug, Clone, Default, PartialEq)]
struct OpenTurn {
    message_id: String,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cache_read: u64,
    cache_write: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct CursorState {
    cursor: u64,
    open: Option<OpenTurn>,
}

/// Un file per transcript sotto `state/costi-cursori/`, invece di un'unica
/// mappa: due `Stop` su transcript diversi non leggono e non scrivono più lo
/// stesso file, quindi non si sovrascrivono a vicenda in un
/// leggi-modifica-scrivi senza lucchetto. La chiave è l'hash del percorso
/// (`guards::cost_ledger::cursor_key`), non il percorso stesso: un nome di
/// file troppo lungo o con caratteri strani non deve poter far fallire la
/// scrittura dello stato.
fn cursor_file_for(transcript_path: &str) -> PathBuf {
    cursors_dir().join(format!("{}.json", cost_ledger::cursor_key(transcript_path)))
}

fn load_cursor(transcript_path: &str) -> CursorState {
    let Ok(text) = fs::read_to_string(cursor_file_for(transcript_path)) else {
        return CursorState::default();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return CursorState::default();
    };
    let cursor = v.get("cursor").and_then(|x| x.as_u64()).unwrap_or(0);
    let get_u64 = |k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    let open = v
        .get("open_id")
        .and_then(|x| x.as_str())
        .map(|id| OpenTurn {
            message_id: id.to_string(),
            model: v.get("open_model").and_then(|x| x.as_str()).unwrap_or("unknown").to_string(),
            input_tokens: get_u64("open_input"),
            output_tokens: get_u64("open_output"),
            cache_read: get_u64("open_cache_read"),
            cache_write: get_u64("open_cache_write"),
        });
    CursorState { cursor, open }
}

fn save_cursor(transcript_path: &str, state: &CursorState) {
    let mut obj = serde_json::json!({ "cursor": state.cursor });
    if let Some(o) = &state.open {
        obj["open_id"] = serde_json::json!(o.message_id);
        obj["open_model"] = serde_json::json!(o.model);
        obj["open_input"] = serde_json::json!(o.input_tokens);
        obj["open_output"] = serde_json::json!(o.output_tokens);
        obj["open_cache_read"] = serde_json::json!(o.cache_read);
        obj["open_cache_write"] = serde_json::json!(o.cache_write);
    }
    let _ = atomic_write(&cursor_file_for(transcript_path), &obj.to_string());
}

/// Scrittura senza stato a metà strada: un file temporaneo nella stessa
/// cartella, poi `rename` — su questo filesystem `rename` è atomico, quindi
/// chi legge nel frattempo vede il file vecchio intero o quello nuovo
/// intero, mai un taglio a metà. Il PID nel nome del temporaneo basta a
/// separare due processi che scrivono nello stesso istante; non c'è bisogno
/// di un lucchetto perché non c'è un leggi-modifica-scrivi da difendere: ogni
/// cursore vive nel proprio file.
fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("no parent directory"))?;
    fs::create_dir_all(dir)?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("costs-state");
    let tmp = dir.join(format!(".{file_name}.tmp{}", std::process::id()));
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)
}

/// La coda di un file dal byte del cursore salvato, senza rileggerlo intero
/// e senza mai restituire una riga a metà scrittura — la logica di dove
/// tagliare vive in `guards::cost_ledger::tail_from_cursor`, l'unica
/// implementazione: qui si apre il file e si salta al cursore, il taglio
/// all'ultimo `\n` completo lo fa quella funzione.
fn read_tail(path: &Path, cursor: u64) -> Result<(String, u64), String> {
    let mut file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let len = file
        .metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    let start = if cursor <= len { cursor } else { 0 };
    file.seek(SeekFrom::Start(start))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let (complete, relative_end) = cost_ledger::tail_from_cursor(&buf, 0);
    Ok((complete.to_string(), start + relative_end))
}

fn append_rows(rows: &[LedgerRow]) {
    if rows.is_empty() {
        return;
    }
    let dir = state_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let mut text = String::new();
    for r in rows {
        text.push_str(&cost_ledger::render_ledger_row(r));
        text.push('\n');
    }
    use std::io::Write as _;
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ledger_path())
    {
        let _ = f.write_all(text.as_bytes());
    }
}

// ============================================================
// `costs record` — il gancio.
// ============================================================

/// Letto lo stdin del gancio, aggrega i turni nuovi e scrive. Fail-open: ogni
/// errore va su stderr, il codice resta sempre 0.
pub fn record() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    if let Err(e) = record_from_payload(&raw) {
        eprintln!("costs record: {e}");
    }
    0
}

fn record_from_payload(raw: &str) -> Result<(), String> {
    let payload: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let event = payload
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let session = payload
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mother_path = payload
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let agent_id = payload.get("agent_id").and_then(|v| v.as_str());
    let is_subagent = hook_io::agent_id_says_subagent(agent_id);

    let path = if is_subagent {
        let id = agent_id.unwrap_or("");
        cost_ledger::child_transcript_path(mother_path, id)
            .ok_or_else(|| "cannot build the child's transcript path".to_string())?
    } else {
        mother_path.to_string()
    };
    if path.is_empty() {
        return Err("no transcript_path in the payload".into());
    }
    let path_buf = PathBuf::from(&path);

    // Il figlio è piccolo e finito una volta per tutte quando arriva
    // `SubagentStop`: si legge per intero, nessun cursore e nessun turno
    // aperto da tenere a mente per un file che non cresce più. La madre
    // invece cresce per tutta la sessione: si legge dal cursore salvato, e
    // il turno che era ancora aperto al giro scorso si porta avanti per
    // poterlo correggere se ricompare.
    let (slice, mother_state) = if is_subagent {
        let text = fs::read_to_string(&path_buf).map_err(|e| format!("{path}: {e}"))?;
        (text, None)
    } else {
        let state = load_cursor(&path);
        let (text, new_cursor) = read_tail(&path_buf, state.cursor)?;
        (text, Some((state, new_cursor)))
    };

    let mut raw_by_model: BTreeMap<String, u64> = BTreeMap::new();
    let mut turns: Vec<RawTurn> = Vec::new();
    for line in slice.lines() {
        if let Some(t) = cost_ledger::parse_transcript_line(line) {
            *raw_by_model.entry(t.model.clone()).or_default() += 1;
            turns.push(t);
        }
    }
    if turns.is_empty() {
        // Ancora niente di completo da leggere (o solo righe che non sono
        // turni): si salva comunque il cursore, il turno aperto resta quello
        // di prima.
        if let Some((mut state, new_cursor)) = mother_state {
            state.cursor = new_cursor;
            save_cursor(&path, &state);
        }
        return Ok(());
    }

    let deduped = cost_ledger::dedupe_by_message_id(turns);
    let job = if is_subagent {
        deduped
            .iter()
            .find_map(|t| t.attribution_agent.clone())
            .unwrap_or_else(|| "unknown".to_string())
    } else {
        "main".to_string()
    };

    // Il turno che avevamo lasciato aperto al giro scorso: se lo stesso
    // `message.id` ricompare qui, quello che avevamo scritto era parziale.
    // Misurato in revisione: senza questo, un `message.id` a cavallo di due
    // letture del gancio (chunk 1→1 scritti, poi 527 arrivato dopo) diventava
    // due turni e 528 token di uscita invece di uno solo da 527. Se invece
    // non arriva più nessuno `Stop` dopo l'ultimo turno aperto (fine sessione,
    // crash), quel turno resta scritto col valore parziale per sempre: al
    // massimo un turno sottocontato per transcript, mai un doppio conteggio.
    let previously_open = mother_state.as_ref().and_then(|(s, _)| s.open.clone());

    // Il candidato al turno aperto per IL PROSSIMO giro è sempre l'ultimo
    // `message.id` di questo lotto, in ordine di prima comparsa — che, senza
    // interleaving fra i turni, è anche l'ultimo per cui è arrivato un chunk.
    let next_open = deduped.last().map(|t| OpenTurn {
        message_id: t.message_id.clone(),
        model: t.model.clone(),
        input_tokens: t.input_tokens,
        output_tokens: t.output_tokens,
        cache_read: t.cache_read,
        cache_write: t.cache_write,
    });

    let mut by_model: BTreeMap<String, Vec<RawTurn>> = BTreeMap::new();
    let mut correction: Option<(String, OpenTurn)> = None;
    for t in deduped {
        if let Some(open) = &previously_open {
            if t.message_id == open.message_id {
                // Il modello ricompare diverso da quello con cui il turno era
                // stato aperto: non dovrebbe succedere (lo stesso message.id
                // non cambia modello), ma se capita la correzione va sul
                // modello originale, non su quello nuovo — altrimenti la riga
                // aggiunta finirebbe sotto un modello che non l'ha mai avuta.
                if t.model != open.model {
                    eprintln!(
                        "costs record: message.id {:?} reappeared under model {:?}, was open under {:?}; correcting the original model",
                        t.message_id, t.model, open.model
                    );
                }
                let regressed = t.input_tokens < open.input_tokens
                    || t.output_tokens < open.output_tokens
                    || t.cache_read < open.cache_read
                    || t.cache_write < open.cache_write;
                if regressed {
                    eprintln!(
                        "costs record: message.id {:?} reappeared with lower usage than previously recorded (in {}->{}, out {}->{}, cache_read {}->{}, cache_write {}->{}); no negative correction is written",
                        t.message_id, open.input_tokens, t.input_tokens, open.output_tokens, t.output_tokens,
                        open.cache_read, t.cache_read, open.cache_write, t.cache_write
                    );
                }
                let delta = OpenTurn {
                    message_id: t.message_id.clone(),
                    model: open.model.clone(),
                    input_tokens: t.input_tokens.saturating_sub(open.input_tokens),
                    output_tokens: t.output_tokens.saturating_sub(open.output_tokens),
                    cache_read: t.cache_read.saturating_sub(open.cache_read),
                    cache_write: t.cache_write.saturating_sub(open.cache_write),
                };
                if delta.input_tokens > 0
                    || delta.output_tokens > 0
                    || delta.cache_read > 0
                    || delta.cache_write > 0
                {
                    correction = Some((open.model.clone(), delta));
                }
                continue; // già contato al giro scorso: non è un turno nuovo
            }
        }
        by_model.entry(t.model.clone()).or_default().push(t);
    }

    let now = hook_io::journal::now_iso8601_seconds();
    let mut rows = Vec::new();
    for (model, group) in by_model {
        let raw = raw_by_model.get(&model).copied().unwrap_or(0);
        let mut totals = cost_ledger::totals_of(&group, raw);
        if let Some((corrected_model, delta)) = &correction {
            if corrected_model == &model {
                totals.input_tokens += delta.input_tokens;
                totals.output_tokens += delta.output_tokens;
                totals.cache_read += delta.cache_read;
                totals.cache_write += delta.cache_write;
                correction = None; // agganciata a una riga vera: non serve più a parte
            }
        }
        rows.push(LedgerRow {
            ts: now.clone(),
            event: event.to_string(),
            session: session.clone(),
            agent_id: agent_id.map(String::from),
            agent_type: job.clone(),
            model,
            turns: totals.turns,
            raw_records: totals.raw_records,
            mechanical_turns: totals.mechanical_turns,
            input_tokens: totals.input_tokens,
            output_tokens: totals.output_tokens,
            cache_read: totals.cache_read,
            cache_write: totals.cache_write,
            duration_s: cost_ledger::duration_seconds(&totals.first_ts, &totals.last_ts),
            first_ts: totals.first_ts,
            last_ts: totals.last_ts,
            source: "hook".to_string(),
        });
    }
    // La correzione non ha trovato un modello nuovo a cui agganciarsi in
    // questo lotto (il solo turno del lotto era proprio quello che si stava
    // correggendo): una riga a parte, turni e record grezzi a zero — porta
    // solo il delta.
    if let Some((model, delta)) = correction {
        rows.push(LedgerRow {
            ts: now,
            event: event.to_string(),
            session: session.clone(),
            agent_id: agent_id.map(String::from),
            agent_type: job,
            model,
            turns: 0,
            raw_records: 0,
            mechanical_turns: 0,
            input_tokens: delta.input_tokens,
            output_tokens: delta.output_tokens,
            cache_read: delta.cache_read,
            cache_write: delta.cache_write,
            duration_s: 0.0,
            first_ts: String::new(),
            last_ts: String::new(),
            source: "hook".to_string(),
        });
    }
    append_rows(&rows);

    if let Some((mut state, new_cursor)) = mother_state {
        state.cursor = new_cursor;
        state.open = next_open;
        save_cursor(&path, &state);
    }
    Ok(())
}

// ============================================================
// `costs backfill` — lo strumento.
// ============================================================

fn flag_value(args: &[String], name: &str) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == name {
            return it.next().cloned();
        }
        if let Some(rest) = a.strip_prefix(&format!("{name}=")) {
            return Some(rest.to_string());
        }
    }
    None
}

fn parse_days_flag(args: &[String], default: u64) -> u64 {
    flag_value(args, "--days")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(default)
}

fn now_epoch_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Cammina l'albero e raccoglie ogni `.jsonl`. Non distingue file di sessione
/// da file di subagent: lo fa `parse_transcript_line`, che legge gli stessi
/// campi in entrambi.
fn collect_jsonl(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

fn mtime_epoch(path: &Path) -> i64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// L'`agentId` di un file di subagent, dal suo nome (`agent-<id>.jsonl`), o
/// `None` per il transcript di una sessione principale.
fn agent_id_from_filename(path: &Path) -> Option<String> {
    let name = path.file_stem()?.to_str()?;
    name.strip_prefix("agent-").map(String::from)
}

/// Rilegge il file intero e ne ricava le righe del registro, una per
/// (modello, giorno) — è la stessa unità con cui `record` scrive una riga
/// per (modello, evento), solo raggruppata sul giorno perché qui non c'è un
/// singolo evento a fissare la finestra.
fn rows_from_file(path: &Path, cutoff: &str) -> Vec<LedgerRow> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut raw_turns: Vec<RawTurn> = Vec::new();
    for line in text.lines() {
        if let Some(t) = cost_ledger::parse_transcript_line(line) {
            if cost_ledger::within_window(&t.timestamp, cutoff) {
                raw_turns.push(t);
            }
        }
    }
    if raw_turns.is_empty() {
        return Vec::new();
    }
    let mut raw_by_bucket: BTreeMap<(String, String), u64> = BTreeMap::new();
    for t in &raw_turns {
        let key = (t.model.clone(), cost_ledger::day_bucket(&t.timestamp).to_string());
        *raw_by_bucket.entry(key).or_default() += 1;
    }
    let deduped = cost_ledger::dedupe_by_message_id(raw_turns);
    let mut by_bucket: BTreeMap<(String, String), Vec<RawTurn>> = BTreeMap::new();
    for t in deduped {
        let key = (t.model.clone(), cost_ledger::day_bucket(&t.timestamp).to_string());
        by_bucket.entry(key).or_default().push(t);
    }
    let now = hook_io::journal::now_iso8601_seconds();
    let agent_id = agent_id_from_filename(path);
    let mut rows = Vec::new();
    for ((model, day), group) in by_bucket {
        let raw = raw_by_bucket.get(&(model.clone(), day.clone())).copied().unwrap_or(0);
        let job = group
            .iter()
            .find_map(|t| t.attribution_agent.clone())
            .unwrap_or_else(|| {
                if group.iter().any(|t| t.is_sidechain) {
                    "unknown".to_string()
                } else {
                    "main".to_string()
                }
            });
        let totals = cost_ledger::totals_of(&group, raw);
        rows.push(LedgerRow {
            ts: now.clone(),
            event: "backfill".to_string(),
            session: group.first().map(|t| t.session.clone()).unwrap_or_default(),
            agent_id: agent_id.clone(),
            agent_type: job,
            model,
            turns: totals.turns,
            raw_records: totals.raw_records,
            mechanical_turns: totals.mechanical_turns,
            input_tokens: totals.input_tokens,
            output_tokens: totals.output_tokens,
            cache_read: totals.cache_read,
            cache_write: totals.cache_write,
            duration_s: cost_ledger::duration_seconds(&totals.first_ts, &totals.last_ts),
            first_ts: totals.first_ts,
            last_ts: totals.last_ts,
            source: "backfill".to_string(),
        });
    }
    rows
}

/// Riscrive `costi-backfill.jsonl` da zero, un file **separato** da
/// `costi.jsonl`. Non è la stessa scelta della prima versione: quella
/// rileggeva `costi.jsonl`, teneva le righe `source:"hook"` e riscriveva
/// tutto — fra la lettura e la scrittura, una riga appesa da `record` in
/// quella finestra spariva, perché la scrittura finale si basava su uno
/// scatto vecchio. Un file a parte toglie la finestra del tutto: `backfill`
/// non legge mai `costi.jsonl`, quindi non può mai perderne una riga, e la
/// scrittura è comunque atomica (temp+rename) contro un `report` in corsa
/// nello stesso istante. IDEMPOTENTE PER COSTRUZIONE: rilanciarlo sostituisce
/// il proprio file, non lo accresce.
pub fn backfill(args: &[String]) -> i32 {
    let days = parse_days_flag(args, 30);
    let now = now_epoch_secs();
    let cutoff = cost_ledger::cutoff_iso(now, days);
    // Margine di due giorni sul prefiltro mtime: un file ancora aperto lo
    // tocca ben oltre l'istante del suo primo turno nella finestra, e il
    // fuso orario locale può spostare la mezzanotte UTC di un giorno intero.
    let mtime_floor = now - (days as i64 + 2) * 86_400;

    let root = home().join(".claude").join("projects");
    let mut files = Vec::new();
    collect_jsonl(&root, &mut files);

    let mut rows: Vec<LedgerRow> = Vec::new();
    let mut scanned = 0usize;
    for path in &files {
        if mtime_epoch(path) < mtime_floor {
            continue;
        }
        scanned += 1;
        rows.extend(rows_from_file(path, &cutoff));
    }

    let mut out = String::new();
    for r in &rows {
        out.push_str(&cost_ledger::render_ledger_row(r));
        out.push('\n');
    }
    if let Err(e) = atomic_write(&backfill_path(), &out) {
        eprintln!("costs backfill: cannot write the backfill ledger: {e}");
        return 1;
    }
    println!(
        "costs backfill: {} files scanned under mtime (of {} found in total), {} rows written",
        scanned,
        files.len(),
        rows.len()
    );
    0
}

// ============================================================
// `costs report` — lo strumento.
// ============================================================

pub fn report(args: &[String]) -> i32 {
    let ledger = flag_value(args, "--ledger")
        .map(PathBuf::from)
        .unwrap_or_else(ledger_path);
    let backfill_ledger = flag_value(args, "--backfill-ledger")
        .map(PathBuf::from)
        .unwrap_or_else(backfill_path);
    let listino_file = flag_value(args, "--listino")
        .map(PathBuf::from)
        .unwrap_or_else(listino_path);

    // Un file mancante è normale (nessun backfill ancora lanciato, o
    // installazione appena fatta): si tratta come vuoto, non come un errore.
    let mut rows: Vec<LedgerRow> = Vec::new();
    for path in [&ledger, &backfill_ledger] {
        if let Ok(text) = fs::read_to_string(path) {
            rows.extend(text.lines().filter_map(cost_ledger::parse_ledger_line));
        }
    }
    // Il gancio, una volta acceso, è la fonte più vera per una copertura: le
    // righe di backfill che coprono la stessa (sessione, agent_id, modello,
    // giorno) di una riga del gancio si scartano, o lo stesso turno si
    // conterebbe due volte fra i due file.
    let (mut rows, discarded) = cost_ledger::dedupe_backfill_covered_by_hook(rows);
    if discarded > 0 {
        eprintln!("costs report: discarded {discarded} backfill rows already covered by the hook");
    }

    if let Some(days_raw) = flag_value(args, "--days") {
        let Ok(days) = days_raw.parse::<u64>() else {
            eprintln!("costs report: --days wants a number, got {days_raw:?}");
            return 1;
        };
        let cutoff = cost_ledger::cutoff_iso(now_epoch_secs(), days);
        rows.retain(|r| cost_ledger::within_window(&r.first_ts, &cutoff));
    }

    let listino_text = fs::read_to_string(&listino_file).unwrap_or_default();
    let listino = cost_ledger::listino_from_toml(&listino_text);

    let mut models: Vec<&String> = rows.iter().map(|r| &r.model).collect();
    models.sort();
    models.dedup();
    for model in models {
        if !listino.models.contains_key(model) {
            eprintln!("costs report: no price in the price list for {model:?}, cost n/d");
        }
    }

    println!("{}", cost_ledger::render_report(&rows, &listino));
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::HomeIsolata;

    fn assistant_line(id: &str, out: u64, ts: &str) -> String {
        serde_json::json!({
            "type": "assistant",
            "timestamp": ts,
            "sessionId": "sess1",
            "isSidechain": false,
            "message": {
                "id": id,
                "model": "claude-sonnet-5",
                "usage": {"input_tokens": 1, "output_tokens": out, "cache_read_input_tokens": 5, "cache_creation_input_tokens": 0},
                "content": [{"type": "text", "text": "ciao"}],
            },
        })
        .to_string()
    }

    #[test]
    fn record_writes_once_and_a_second_call_writes_nothing_new() {
        let home = HomeIsolata::nuova("costs-record-cursor");
        let projects = home.dir.join(".claude").join("projects").join("slug");
        fs::create_dir_all(&projects).unwrap();
        let transcript = projects.join("sess1.jsonl");
        fs::write(&transcript, format!("{}\n", assistant_line("m1", 42, "2026-08-20T00:00:00.000Z"))).unwrap();

        let payload = serde_json::json!({
            "hook_event_name": "Stop",
            "session_id": "sess1",
            "transcript_path": transcript.to_string_lossy(),
        })
        .to_string();

        record_from_payload(&payload).expect("prima lettura pulita");
        let after_first = fs::read_to_string(ledger_path()).unwrap_or_default();
        assert_eq!(after_first.lines().count(), 1, "una riga scritta al primo giro");

        record_from_payload(&payload).expect("seconda lettura pulita");
        let after_second = fs::read_to_string(ledger_path()).unwrap_or_default();
        assert_eq!(
            after_second, after_first,
            "il cursore impedisce di ricontare lo stesso turno"
        );

        // una riga NUOVA dopo il cursore deve invece contarsi
        let mut f = fs::OpenOptions::new().append(true).open(&transcript).unwrap();
        use std::io::Write as _;
        writeln!(f, "{}", assistant_line("m2", 7, "2026-08-20T00:01:00.000Z")).unwrap();
        record_from_payload(&payload).expect("terza lettura pulita");
        let after_third = fs::read_to_string(ledger_path()).unwrap_or_default();
        assert_eq!(after_third.lines().count(), 2, "solo la riga nuova si aggiunge");
    }

    #[test]
    fn record_reads_the_childs_full_file_from_the_agent_id() {
        let home = HomeIsolata::nuova("costs-record-subagent");
        let sess_dir = home.dir.join(".claude").join("projects").join("slug").join("sess1");
        let subdir = sess_dir.join("subagents");
        fs::create_dir_all(&subdir).unwrap();
        let child = subdir.join("agent-abc123.jsonl");
        let line = serde_json::json!({
            "type": "assistant",
            "timestamp": "2026-08-20T00:00:00.000Z",
            "sessionId": "sess1",
            "isSidechain": true,
            "agentId": "abc123",
            "attributionAgent": "measurer",
            "message": {
                "id": "m1",
                "model": "claude-sonnet-5",
                "usage": {"input_tokens": 1, "output_tokens": 9, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
                "content": [],
            },
        })
        .to_string();
        fs::write(&child, format!("{line}\n")).unwrap();

        let mother = home.dir.join(".claude").join("projects").join("slug").join("sess1.jsonl");
        fs::write(&mother, "").unwrap();

        let payload = serde_json::json!({
            "hook_event_name": "SubagentStop",
            "session_id": "sess1",
            "transcript_path": mother.to_string_lossy(),
            "agent_id": "abc123",
        })
        .to_string();
        record_from_payload(&payload).expect("il figlio si trova da sé");
        let ledger = fs::read_to_string(ledger_path()).unwrap_or_default();
        let row = cost_ledger::parse_ledger_line(ledger.lines().next().expect("una riga")).unwrap();
        assert_eq!(row.agent_type, "measurer");
        assert_eq!(row.output_tokens, 9);
    }

    #[test]
    fn a_payload_without_a_transcript_fails_open() {
        // fail-open: nessun panico, nessuna scrittura, l'errore torna a chi
        // chiama (che `record()` scrive su stderr e ignora).
        let err = record_from_payload(r#"{"hook_event_name":"Stop","session_id":"x"}"#);
        assert!(err.is_err());
    }

    // --- correzioni della revisione indipendente, 23/08/2026 ---

    fn line_at(id: &str, out: u64, ts: &str) -> String {
        assistant_line(id, out, ts)
    }

    #[test]
    fn review_streaming_chunks_split_across_a_cursor_boundary_are_double_counted() {
        // Simula: al momento di Stop, sul disco sono già arrivati i primi due
        // chunk dello streaming di un messaggio (output crescente 1 -> 1), ma
        // NON ancora il chunk finale (out=527) che chiude davvero il turno.
        let home = HomeIsolata::nuova("costs-review-streaming-split");
        let projects = home.dir.join(".claude").join("projects").join("slug");
        fs::create_dir_all(&projects).unwrap();
        let transcript = projects.join("sess1.jsonl");
        fs::write(
            &transcript,
            format!(
                "{}\n{}\n",
                line_at("m1", 1, "2026-08-20T00:00:00.000Z"),
                line_at("m1", 1, "2026-08-20T00:00:01.000Z"),
            ),
        )
        .unwrap();

        let payload = serde_json::json!({
            "hook_event_name": "Stop",
            "session_id": "sess1",
            "transcript_path": transcript.to_string_lossy(),
        })
        .to_string();

        record_from_payload(&payload).expect("primo giro pulito");

        // Il chunk finale arriva DOPO che il cursore ha già superato i primi due.
        let mut f = fs::OpenOptions::new().append(true).open(&transcript).unwrap();
        use std::io::Write as _;
        writeln!(f, "{}", line_at("m1", 527, "2026-08-20T00:00:02.000Z")).unwrap();
        record_from_payload(&payload).expect("secondo giro pulito");

        let ledger = fs::read_to_string(ledger_path()).unwrap_or_default();
        let rows: Vec<_> = ledger.lines().filter_map(cost_ledger::parse_ledger_line).collect();
        let total_turns: u64 = rows.iter().map(|r| r.turns).sum();
        let total_output: u64 = rows.iter().map(|r| r.output_tokens).sum();
        // Un solo turno reale (message.id m1) da 527 token di uscita:
        assert_eq!(total_turns, 1, "un solo message.id deve valere un turno solo");
        assert_eq!(total_output, 527, "l'output del turno reale non deve raddoppiare");
    }

    #[test]
    fn review_a_line_cut_mid_write_is_lost_forever() {
        // Simula: al momento di Stop, la riga è a metà scrittura sul disco
        // (nessun newline finale). Il gancio deve aspettare che si completi,
        // non tagliarla e perderne l'inizio per sempre.
        let home = HomeIsolata::nuova("costs-review-partial-line");
        let projects = home.dir.join(".claude").join("projects").join("slug");
        fs::create_dir_all(&projects).unwrap();
        let transcript = projects.join("sess1.jsonl");

        let full_line = line_at("m1", 42, "2026-08-20T00:00:00.000Z");
        let cut_point = full_line.len() / 2;
        let (head, tail) = full_line.split_at(cut_point);
        fs::write(&transcript, head).unwrap(); // riga a metà, niente newline

        let payload = serde_json::json!({
            "hook_event_name": "Stop",
            "session_id": "sess1",
            "transcript_path": transcript.to_string_lossy(),
        })
        .to_string();
        record_from_payload(&payload).expect("primo giro pulito (niente da scrivere)");

        // La riga si completa.
        let mut f = fs::OpenOptions::new().append(true).open(&transcript).unwrap();
        use std::io::Write as _;
        writeln!(f, "{tail}").unwrap();
        record_from_payload(&payload).expect("secondo giro pulito");

        let ledger = fs::read_to_string(ledger_path()).unwrap_or_default();
        let rows: Vec<_> = ledger.lines().filter_map(cost_ledger::parse_ledger_line).collect();
        let total_turns: u64 = rows.iter().map(|r| r.turns).sum();
        assert_eq!(total_turns, 1, "il turno completato deve comunque essere contato una volta");
    }

    #[test]
    fn backfill_never_touches_the_hook_ledger() {
        // Il gancio scrive una riga in `costi.jsonl`; `backfill` (che qui non
        // trova nessun transcript da scandire, la HOME è vuota) non deve
        // toccarla: le due scritture vivono in file diversi.
        let home = HomeIsolata::nuova("costs-backfill-does-not-clobber");
        let projects = home.dir.join(".claude").join("projects").join("slug");
        fs::create_dir_all(&projects).unwrap();
        let transcript = projects.join("sess1.jsonl");
        fs::write(&transcript, format!("{}\n", assistant_line("m1", 5, "2026-08-20T00:00:00.000Z"))).unwrap();
        let payload = serde_json::json!({
            "hook_event_name": "Stop",
            "session_id": "sess1",
            "transcript_path": transcript.to_string_lossy(),
        })
        .to_string();
        record_from_payload(&payload).expect("il gancio scrive la sua riga");
        let before = fs::read_to_string(ledger_path()).unwrap_or_default();
        assert_eq!(before.lines().count(), 1);

        assert_eq!(backfill(&["--days".to_string(), "30".to_string()]), 0);

        let after = fs::read_to_string(ledger_path()).unwrap_or_default();
        assert_eq!(after, before, "costi.jsonl non cambia per una corsa di backfill");
        assert!(backfill_path().exists(), "backfill scrive comunque il proprio file");
    }

    #[test]
    fn review_report_double_counts_when_hook_and_backfill_cover_the_same_turn() {
        // Il gancio ha già registrato un turno in costi.jsonl (source:"hook").
        // Poi qualcuno lancia `backfill`, che rilegge lo STESSO transcript e
        // scrive una riga per lo STESSO turno in costi-backfill.jsonl
        // (source:"backfill"). `report` deve dedplicare le due fonti sulla
        // stessa copertura, non sommarle.
        let home = HomeIsolata::nuova("costs-review-hook-plus-backfill");
        let projects = home.dir.join(".claude").join("projects").join("slug");
        fs::create_dir_all(&projects).unwrap();
        let transcript = projects.join("sess1.jsonl");
        fs::write(&transcript, format!("{}\n", assistant_line("m1", 100, "2026-08-20T00:00:00.000Z"))).unwrap();

        let payload = serde_json::json!({
            "hook_event_name": "Stop",
            "session_id": "sess1",
            "transcript_path": transcript.to_string_lossy(),
        })
        .to_string();
        record_from_payload(&payload).expect("il gancio registra il turno");

        assert_eq!(backfill(&["--days".to_string(), "30".to_string()]), 0);

        // report legge entrambi i file e li deduplica con la stessa funzione
        // pura usata da `report()`.
        let mut rows: Vec<LedgerRow> = Vec::new();
        for path in [ledger_path(), backfill_path()] {
            if let Ok(text) = fs::read_to_string(&path) {
                rows.extend(text.lines().filter_map(cost_ledger::parse_ledger_line));
            }
        }
        let (rows, discarded) = cost_ledger::dedupe_backfill_covered_by_hook(rows);
        let total_turns: u64 = rows.iter().map(|r| r.turns).sum();
        let total_output: u64 = rows.iter().map(|r| r.output_tokens).sum();
        // Un solo turno reale è stato prodotto (m1, 100 token di uscita):
        assert_eq!(total_turns, 1, "un solo turno reale non deve contarsi due volte fra hook e backfill");
        assert_eq!(total_output, 100, "l'output di un solo turno non deve raddoppiare fra hook e backfill");
        assert_eq!(discarded, 1, "la riga di backfill coperta dal gancio va scartata");
    }

    #[test]
    fn each_transcript_gets_its_own_cursor_file() {
        let home = HomeIsolata::nuova("costs-cursor-per-file");
        let projects = home.dir.join(".claude").join("projects").join("slug");
        fs::create_dir_all(&projects).unwrap();
        let a = projects.join("a.jsonl");
        let b = projects.join("b.jsonl");
        fs::write(&a, format!("{}\n", assistant_line("ma", 3, "2026-08-20T00:00:00.000Z"))).unwrap();
        fs::write(&b, format!("{}\n", assistant_line("mb", 4, "2026-08-20T00:00:00.000Z"))).unwrap();
        for t in [&a, &b] {
            let payload = serde_json::json!({
                "hook_event_name": "Stop",
                "session_id": "sess1",
                "transcript_path": t.to_string_lossy(),
            })
            .to_string();
            record_from_payload(&payload).expect("ognuno legge il proprio file");
        }
        let state_a = load_cursor(&a.to_string_lossy());
        let state_b = load_cursor(&b.to_string_lossy());
        assert!(state_a.cursor > 0 && state_b.cursor > 0);
        assert_ne!(
            cursor_file_for(&a.to_string_lossy()),
            cursor_file_for(&b.to_string_lossy()),
            "due transcript diversi non condividono il file del cursore"
        );
    }
}
