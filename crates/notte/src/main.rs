//! Il ciclo di notte: smista i compiti di
//! `~/.claude/state/plancia/coda-notte/*.task` ai motori già pagati
//! (OpenRouter :free, Codex sulla quota di Theo) invece di far lavorare un
//! subagente Claude, che paga da solo ~60k token di prologo.
//!
//! QUESTO FILE FA SOLO I/O E PROCESSI: legge file, scrive file, sposta file,
//! chiama `curl`/`codex`/`sh -c`, dorme fra una chiamata OpenRouter e
//! l'altra. Ogni giudizio — è un compito malformato? cita una credenziale?
//! sta sotto il tetto in byte? è verde o rosso? — vive in `lib.rs`, dove le
//! prove lo controllano senza toccare rete o disco.
//!
//! UN 429 NON SI RITENTA A RAFFICA: conta nella quota di 20/minuto. Qui non
//! esiste un ciclo di ritentativo — un 429 è semplicemente un rosso, come
//! ogni altro errore del motore.

use notte::{
    already_done_today, alert_markdown, contains_secret, enriched_path, parse_codex_tokens,
    parse_openrouter_body, resolve_bin, stamped_for_next_night,
};
use notte::{parse_idle_seconds, parse_loadavg_1min, parse_mem_free_percent};
use notte::{parse_task, prompt_over_cap, report_line, status_line};
use notte::{decide, OpenRouterResult, Outcome, ParsedTask, WatchDecision, WatchInputs, WatchThresholds, Weight};
use notte::{attempts_field, parse_lock_pid, process_exists, set_attempts_field, split_receipt_name, strip_receipt_suffix};
use notte::MAX_TASK_ATTEMPTS;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Config {
    queue_dir: PathBuf,
    done_dir: PathBuf,
    alerts_dir: PathBuf,
    codex_bin: String,
    codex_dir: PathBuf,
    gemini_bin: String,
    gemini_model: String,
    gemini_timeout_secs: u64,
    openrouter_model: String,
    openrouter_key_file: PathBuf,
    openrouter_fetch_override: Option<String>,
    max_failures: u32,
    openrouter_pause: u64,
    max_prompt_bytes: usize,
    today: String,
    report_path: PathBuf,
    log_path: PathBuf,
    last_output_path: PathBuf,
    // ── il lucchetto e la ricevuta (difetti 2 e 4 del 25/08) ─────────
    lock_path: PathBuf,
    in_progress_dir: PathBuf,
    // ── le scadenze (difetto 3 del 25/08) ────────────────────────────
    codex_timeout_secs: u64,
    check_timeout_secs: u64,
    // ── il ciclo continuo (--watch) ─────────────────────────────────
    watch_interval_secs: u64,
    idle_threshold_secs: u64,
    idle_load_ratio_cap: f64,
    busy_load_ratio_cap: f64,
    light_load_ratio_cap: f64,
    mem_free_min_percent: u32,
    hourly_cap: u32,
    failure_cooldown_secs: u64,
    decision_heartbeat_ticks: u32,
    // La finestra di notte (difetto 1 del 25/08): fuori da qui si lavora
    // solo a macchina ferma da molto.
    window_start_hour: u32,
    window_end_hour: u32,
    very_idle_seconds: u64,
    // Sostituti per le prove: bypassano `ioreg`/`sysctl`/`memory_pressure`
    // con un numero fisso, come `openrouter_fetch_override` fa per la rete.
    idle_seconds_override: Option<u64>,
    load1_override: Option<f64>,
    mem_free_percent_override: Option<u32>,
    core_count_override: Option<u32>,
    hour_override: Option<u32>,
    // Solo per le prove di integrazione: fa uscire `--watch` dopo N giri
    // invece di girare per sempre.
    watch_max_ticks: Option<u32>,
}

fn env_or(key: &str, default: String) -> String {
    std::env::var(key).unwrap_or(default)
}

/// La data di adesso, non quella dell'avvio.
///
/// IL GUASTO CHE RIPARA, misurato il 27/08/2026: `Config::today` si calcola una
/// volta sola, quando il processo nasce. Il ciclo residente vive per giorni, e
/// dal giorno dopo l'avvio confrontava ogni ricorrente con una data ferma: tutte
/// portavano `ultima-esecuzione` uguale a quel giorno, quindi tutte risultavano
/// «già fatte oggi» e la coda si dichiarava vuota. **Per sempre.** Il registro
/// del 27/08 porta «salto: coda vuota (x340)» con dodici lavorazioni ferme
/// dentro, e nessun rapporto per quel giorno.
///
/// È il difetto che faceva sembrare il servizio attivo solo di notte: ogni
/// riavvio — un rilascio, un riavvio della macchina — gli ridava esattamente un
/// giorno di lavoro, e poi taceva.
///
/// `NOTTE_DATE_OVERRIDE` resta sovrano: chi fissa la data lo fa apposta, e le
/// prove ci contano.
fn today_now() -> String {
    env_or("NOTTE_DATE_OVERRIDE", shell_date(&["+%Y-%m-%d"]))
}

impl Config {
    fn from_env() -> Self {
        let home = std::env::var("HOME").expect("HOME must be set");
        let state_dir = env_or("NOTTE_STATE_DIR", format!("{home}/.claude/state/notte"));
        let queue_dir = env_or(
            "NOTTE_QUEUE_DIR",
            format!("{home}/.claude/state/plancia/coda-notte"),
        );
        let done_dir = env_or("NOTTE_DONE_DIR", format!("{queue_dir}/fatti"));
        let alerts_dir = env_or(
            "NOTTE_ALERTS_DIR",
            format!("{home}/.claude/state/plancia/segnalazioni"),
        );
        let today = env_or("NOTTE_DATE_OVERRIDE", shell_date(&["+%Y-%m-%d"]));
        let report_path = env_or(
            "NOTTE_REPORT",
            format!("{state_dir}/rapporto-{today}.md"),
        );
        let log_path = env_or("NOTTE_LOG", format!("{state_dir}/notte.log"));
        let last_output_path = env_or(
            "NOTTE_LAST_OUTPUT",
            format!("{state_dir}/.last-output"),
        );
        let lock_path = env_or("NOTTE_LOCK_PATH", format!("{state_dir}/notte.lock"));
        let in_progress_dir = env_or("NOTTE_IN_PROGRESS_DIR", format!("{queue_dir}/in-corso"));
        Config {
            queue_dir: PathBuf::from(queue_dir),
            done_dir: PathBuf::from(done_dir),
            alerts_dir: PathBuf::from(alerts_dir),
            codex_bin: env_or("NOTTE_CODEX_BIN", "codex".to_string()),
            codex_dir: PathBuf::from(env_or(
                "NOTTE_CODEX_DIR",
                format!("{home}/.claude/docs"),
            )),
            // `agy`, non `gemini`: è la riga di comando di Antigravity, e usa
            // l'accesso già fatto. Vive in `~/.local/bin`, che `resolve_bin`
            // guarda anche quando il percorso ereditato non ci arriva.
            gemini_bin: env_or("NOTTE_GEMINI_BIN", "agy".to_string()),
            gemini_model: env_or("NOTTE_GEMINI_MODEL", "gemini-3.7-flash-low".to_string()),
            gemini_timeout_secs: env_or("NOTTE_GEMINI_TIMEOUT_SECS", "300".to_string())
                .parse()
                .unwrap_or(300),
            openrouter_model: env_or(
                "NOTTE_OPENROUTER_MODEL",
                "nvidia/nemotron-3-super-120b-a12b:free".to_string(),
            ),
            openrouter_key_file: PathBuf::from(env_or(
                "NOTTE_OPENROUTER_KEY_FILE",
                format!("{home}/.claude/state/openrouter.key"),
            )),
            openrouter_fetch_override: std::env::var("NOTTE_OPENROUTER_FETCH").ok(),
            max_failures: env_or("NOTTE_MAX_FAILURES", "3".to_string())
                .parse()
                .unwrap_or(3),
            openrouter_pause: env_or("NOTTE_OPENROUTER_PAUSE", "3".to_string())
                .parse()
                .unwrap_or(3),
            max_prompt_bytes: env_or("NOTTE_MAX_PROMPT_BYTES", "8000".to_string())
                .parse()
                .unwrap_or(8000),
            today,
            report_path: PathBuf::from(report_path),
            log_path: PathBuf::from(log_path),
            last_output_path: PathBuf::from(last_output_path),
            lock_path: PathBuf::from(lock_path),
            in_progress_dir: PathBuf::from(in_progress_dir),
            codex_timeout_secs: env_or("NOTTE_CODEX_TIMEOUT_SECS", "300".to_string())
                .parse()
                .unwrap_or(300),
            check_timeout_secs: env_or("NOTTE_CHECK_TIMEOUT_SECS", "120".to_string())
                .parse()
                .unwrap_or(120),
            watch_interval_secs: env_or("NOTTE_WATCH_INTERVAL_SECS", "90".to_string())
                .parse()
                .unwrap_or(90),
            idle_threshold_secs: env_or("NOTTE_IDLE_THRESHOLD_SECS", "600".to_string())
                .parse()
                .unwrap_or(600),
            idle_load_ratio_cap: env_or("NOTTE_IDLE_LOAD_CAP", "0.6".to_string())
                .parse()
                .unwrap_or(0.6),
            busy_load_ratio_cap: env_or("NOTTE_BUSY_LOAD_CAP", "0.25".to_string())
                .parse()
                .unwrap_or(0.25),
            // 1 volta il numero di core: sotto quel numero la macchina non
            // sta mettendo lavoro in coda. Il perché coi numeri di oggi è nel
            // commento dentro `decide()`, in `lib.rs`.
            light_load_ratio_cap: env_or("NOTTE_LIGHT_LOAD_CAP", "1.0".to_string())
                .parse()
                .unwrap_or(1.0),
            mem_free_min_percent: env_or("NOTTE_MEM_FREE_MIN_PERCENT", "20".to_string())
                .parse()
                .unwrap_or(20),
            hourly_cap: env_or("NOTTE_HOURLY_CAP", "6".to_string()).parse().unwrap_or(6),
            failure_cooldown_secs: env_or("NOTTE_FAILURE_COOLDOWN_SECS", "1800".to_string())
                .parse()
                .unwrap_or(1800),
            decision_heartbeat_ticks: env_or("NOTTE_DECISION_HEARTBEAT_TICKS", "20".to_string())
                .parse()
                .unwrap_or(20),
            window_start_hour: env_or("NOTTE_WINDOW_START_HOUR", "1".to_string()).parse().unwrap_or(1),
            window_end_hour: env_or("NOTTE_WINDOW_END_HOUR", "7".to_string()).parse().unwrap_or(7),
            very_idle_seconds: env_or("NOTTE_VERY_IDLE_SECS", "7200".to_string())
                .parse()
                .unwrap_or(7200),
            idle_seconds_override: std::env::var("NOTTE_IDLE_SECONDS_OVERRIDE")
                .ok()
                .and_then(|s| s.parse().ok()),
            load1_override: std::env::var("NOTTE_LOAD1_OVERRIDE").ok().and_then(|s| s.parse().ok()),
            mem_free_percent_override: std::env::var("NOTTE_MEM_FREE_PERCENT_OVERRIDE")
                .ok()
                .and_then(|s| s.parse().ok()),
            core_count_override: std::env::var("NOTTE_CORE_COUNT_OVERRIDE")
                .ok()
                .and_then(|s| s.parse().ok()),
            hour_override: std::env::var("NOTTE_HOUR_OVERRIDE").ok().and_then(|s| s.parse().ok()),
            watch_max_ticks: std::env::var("NOTTE_WATCH_MAX_TICKS").ok().and_then(|s| s.parse().ok()),
        }
    }
}

/// `date` di sistema invece di una libreria di calendario: un binario in più
/// per formattare una data non vale la pena qui, e i coreutils ci sono
/// sempre.
fn shell_date(args: &[&str]) -> String {
    Command::new("date")
        .args(args)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// ORA LOCALE, COL FUSO SCRITTO. Fino al 26/08/2026 il registro stampava in
/// UTC mentre la finestra di notte si decide sull'ora locale: chi rileggeva
/// il registro vedeva «eseguo» alle 23:00 e concludeva che la finestra non
/// veniva rispettata. Due orologi diversi nello stesso file sono un difetto,
/// non un dettaglio.
fn note(log_path: &Path, msg: &str) {
    let stamp = shell_date(&["+%Y-%m-%d %H:%M:%S %z"]);
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(log_path) {
        let _ = writeln!(f, "{stamp}  {msg}");
    }
}

/// APPEND LIMITATO: sotto launchd il file cresce ogni notte per sempre.
/// Oltre 5000 righe si tiene solo la coda (2000).
fn rotate_log(log_path: &Path) {
    let Ok(content) = fs::read_to_string(log_path) else {
        return;
    };
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() > 5000 {
        let tail = lines[lines.len() - 2000..].join("\n") + "\n";
        let _ = fs::write(log_path, tail);
    }
}

// `process_exists` sta in `lib.rs`, accanto a chi legge le ricevute: la via di
// rilascio del servizio deve fare la stessa domanda prima di riavviare, e da
// qui dentro non la può raggiungere.

/// Il lucchetto dell'intero giro (difetto 2 del 25/08): un file con dentro
/// il pid, creato con `O_EXCL` così due istanze non possono prenderlo
/// insieme, tolto all'uscita. Un lucchetto di un pid morto si scavalca.
struct RunLock {
    path: PathBuf,
}

impl Drop for RunLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

enum LockOutcome {
    Taken(RunLock),
    /// Un'altra istanza, con questo pid, ce l'ha già: si esce puliti, non è
    /// un errore.
    Held(Option<u32>),
}

fn try_create_lock_file(path: &Path) -> bool {
    use std::os::unix::fs::OpenOptionsExt;
    let Ok(mut file) = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path) else {
        return false;
    };
    let _ = writeln!(file, "{}", std::process::id());
    true
}

/// Oltre questa età un lucchetto senza un pid leggibile è uno scarto (crash
/// a metà scrittura), non la corsa di un vincitore appena partito.
const LOCK_STALE_SECS: u64 = 600;

fn lock_age_secs(path: &Path) -> Option<u64> {
    fs::metadata(path).ok()?.modified().ok()?.elapsed().ok().map(|d| d.as_secs())
}

/// PROVA DAL VIVO IL 25/08/2026: due istanze lanciate insieme prendevano
/// ENTRAMBE il lucchetto. La causa era qui — fra `create_new` (atomico) e
/// `writeln!` del pid c'è una finestra reale, non nanoscopica: due processi
/// partiti insieme fanno lo stesso lavoro fino a questo punto, e la seconda
/// arrivava a leggere il file del vincitore proprio a metà scrittura, lo
/// giudicava "senza pid quindi stantio" e lo scavalcava. Ora un file senza
/// pid leggibile si tratta come stantio solo se è vecchio (`LOCK_STALE_SECS`),
/// altrimenti si aspetta che il vincitore finisca di scriverlo.
fn take_run_lock(path: &Path) -> LockOutcome {
    if try_create_lock_file(path) {
        return LockOutcome::Taken(RunLock { path: path.to_path_buf() });
    }
    let Ok(text) = fs::read_to_string(path) else {
        // Sparito fra il tentativo e la lettura: chi lo teneva ha già
        // finito. Si riprova una volta sola.
        return retry_create_or_held(path, None);
    };
    if let Some(pid) = parse_lock_pid(&text) {
        if process_exists(pid) {
            return LockOutcome::Held(Some(pid));
        }
        let _ = fs::remove_file(path);
        return retry_create_or_held(path, None);
    }
    match lock_age_secs(path) {
        Some(age) if age >= LOCK_STALE_SECS => {
            let _ = fs::remove_file(path);
            retry_create_or_held(path, None)
        }
        // Fresco e senza pid: quasi certamente il vincitore lo sta ancora
        // scrivendo. Non si scavalca — si aspetta il prossimo giro.
        _ => LockOutcome::Held(None),
    }
}

fn retry_create_or_held(path: &Path, pid_if_held: Option<u32>) -> LockOutcome {
    if try_create_lock_file(path) {
        LockOutcome::Taken(RunLock { path: path.to_path_buf() })
    } else {
        LockOutcome::Held(pid_if_held)
    }
}

/// L'assertion `caffeinate` vive quanto il singolo compito, mai quanto il
/// demone (difetto 1b del 25/08): `-t <secondi>` la fa cadere da sola anche
/// se l'uccisione fallisse, e viene comunque ammazzata appena il compito
/// finisce — non può sopravvivere al processo che l'ha creata.
struct CaffeineGuard {
    child: Option<Child>,
}

impl CaffeineGuard {
    fn start(seconds: u64) -> Self {
        let child = Command::new("/usr/bin/caffeinate")
            .args(["-i", "-t", &seconds.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok();
        CaffeineGuard { child }
    }
}

impl Drop for CaffeineGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// L'esito di un comando eseguito con un tetto di tempo. Niente `timeout(1)`:
/// non esiste su questa macchina (difetto 3 del 25/08); il tetto è un ciclo
/// di `try_wait` con `kill` alla scadenza.
enum RunOutcome {
    Finished { status: std::process::ExitStatus, stdout: Vec<u8>, stderr: Vec<u8> },
    TimedOut,
    SpawnFailed,
}

fn run_with_timeout(mut cmd: Command, limit: Duration) -> RunOutcome {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return RunOutcome::SpawnFailed,
    };
    // Drenare in due fili, non a fine corsa: un figlio che riempie la pipe
    // prima che la leggiamo resterebbe bloccato in scrittura per sempre.
    let mut out_pipe = child.stdout.take().expect("stdout è piped");
    let mut err_pipe = child.stderr.take().expect("stderr è piped");
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out_pipe.read_to_end(&mut buf);
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err_pipe.read_to_end(&mut buf);
        buf
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if start.elapsed() >= limit {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => break None,
        }
    };
    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    match status {
        Some(status) => RunOutcome::Finished { status, stdout, stderr },
        None => RunOutcome::TimedOut,
    }
}

struct Report {
    path: PathBuf,
}

impl Report {
    fn open(path: &Path, today: &str) -> Self {
        if !path.exists() {
            let _ = fs::write(path, format!("# Rapporto della notte {today}\n"));
        }
        let stamp = shell_date(&["+%H:%M %z"]);
        if let Ok(mut f) = fs::OpenOptions::new().append(true).open(path) {
            let _ = writeln!(f, "\n## Giro delle {stamp}\n");
        }
        Report { path: path.to_path_buf() }
    }

    fn line(&self, s: &str) {
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = writeln!(f, "{s}");
        }
    }
}

fn write_alert(cfg: &Config, name: &str, engine: &str, reason: &str, detail: &str) {
    let base = name.strip_suffix(".task").unwrap_or(name);
    let out_path = cfg.alerts_dir.join(format!("{}-notte-{base}.md", cfg.today));
    let content = alert_markdown(name, engine, &cfg.today, reason, detail);
    let _ = fs::write(out_path, content);
}

/// Sposta il compito eseguito in `fatti/`, con la riga `notte-status:` che
/// dice come è finito: il segno che questa notte l'ha già provato. `path`
/// può essere in coda o già una ricevuta in `in-corso/<nome>.<pid>` — il
/// nome in `fatti/` perde comunque il suffisso pid, il compito ci torna col
/// nome con cui è nato.
fn finish(path: &Path, done_dir: &Path, status: &str) {
    let mut content = fs::read_to_string(path).unwrap_or_default();
    content.push_str(&status_line(status));
    let _ = fs::write(path, &content);
    if let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) {
        let _ = fs::rename(path, done_dir.join(strip_receipt_suffix(&name)));
    }
}

/// La porta unica per chiudere un compito che è stato davvero eseguito:
/// sceglie fra archivio e ritorno in coda guardando il compito, così il
/// bivio sta in un posto solo invece che a ogni chiamata.
fn close_task(path: &Path, cfg: &Config, task: &notte::Task, status: &str) {
    if task.recurring {
        finish_recurring(path, cfg, status);
    } else {
        finish(path, &cfg.done_dir, status);
    }
}

/// Chiude un compito che si ripete: l'esito va in archivio con la data nel
/// nome, e il compito **torna in coda** con la data di oggi, pronto per la
/// notte dopo. Vale anche quando l'esito è rosso: una sentinella spenta al
/// primo rosso è una sentinella che non serve più proprio quando serve.
///
/// Se il ritorno in coda non riesce, il compito viene archiviato come un
/// compito normale: meglio perderne la ricorrenza che perderlo del tutto.
fn finish_recurring(path: &Path, cfg: &Config, status: &str) {
    let original = fs::read_to_string(path).unwrap_or_default();
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else {
        return;
    };
    let bare = strip_receipt_suffix(&name);
    let stem = bare.strip_suffix(".task").unwrap_or(&bare);

    let mut archived = original.clone();
    archived.push_str(&status_line(status));
    let archive_at = cfg.done_dir.join(format!("{stem}-{}.task", cfg.today));
    let _ = fs::write(&archive_at, &archived);

    let back = stamped_for_next_night(&original, &cfg.today);
    let queued_at = cfg.queue_dir.join(&bare);
    if fs::write(&queued_at, &back).is_ok() {
        if path != queued_at {
            let _ = fs::remove_file(path);
        }
        note(&cfg.log_path, &format!("{bare}: ricorrente, torna in coda per domani"));
    } else {
        note(
            &cfg.log_path,
            &format!("{bare}: ricorrente, ma non sono riuscito a rimetterlo in coda: lo archivio"),
        );
        finish(path, &cfg.done_dir, status);
    }
}

enum EngineResult {
    Ok { tokens: String },
    Failed { kind: String, tokens: String },
}

fn fetch_openrouter_body(cfg: &Config, prompt: &str) -> String {
    if let Some(cmd) = &cfg.openrouter_fetch_override {
        let child = Command::new(cmd)
            .arg(&cfg.openrouter_model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn();
        let Ok(mut child) = child else {
            return String::new();
        };
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(prompt.as_bytes());
        }
        return child
            .wait_with_output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();
    }
    let key = match fs::read_to_string(&cfg.openrouter_key_file) {
        Ok(k) => k.trim().to_string(),
        Err(_) => {
            return format!(
                "{{\"error\":{{\"message\":\"OpenRouter key not readable: {}\"}}}}",
                cfg.openrouter_key_file.display()
            )
        }
    };
    let payload = serde_json::json!({
        "model": cfg.openrouter_model,
        "temperature": 0,
        "max_tokens": 2000,
        "messages": [{"role": "user", "content": prompt}],
    })
    .to_string();
    Command::new("curl")
        .args([
            "-sS",
            "-m",
            "180",
            "https://openrouter.ai/api/v1/chat/completions",
            "-H",
            &format!("Authorization: Bearer {key}"),
            "-H",
            "Content-Type: application/json",
            "-d",
            &payload,
        ])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
}

fn call_openrouter(cfg: &Config, prompt: &str) -> EngineResult {
    let body = fetch_openrouter_body(cfg, prompt);
    match parse_openrouter_body(&body) {
        OpenRouterResult::Ok { content, tokens } => {
            let _ = fs::write(&cfg.last_output_path, &content);
            EngineResult::Ok { tokens }
        }
        OpenRouterResult::RateLimited => {
            let _ = fs::write(&cfg.last_output_path, &body);
            EngineResult::Failed { kind: "429".to_string(), tokens: "?".to_string() }
        }
        OpenRouterResult::Error(msg) => {
            let _ = fs::write(&cfg.last_output_path, &msg);
            EngineResult::Failed { kind: "errore".to_string(), tokens: "?".to_string() }
        }
    }
}

/// Il terzo motore, dal 26/08/2026.
///
/// IL NOME DEL COMANDO È `agy`, NON `gemini`, e la differenza è costata mezza
/// mattina. La CLI che si chiama `gemini` risponde `UNSUPPORTED_CLIENT`:
/// Google ha chiuso quel livello gratuito ai comandi da terminale e rimanda
/// ad Antigravity. Ma Antigravity **ha** la sua riga di comando, `agy`, che
/// usa l'accesso già fatto — nessuna chiave, nessuna quota separata. Prima di
/// concludere che una strada è chiusa conviene chiedere come si chiama la
/// porta.
///
/// SENZA MOTORE SI RIMANDA, NON SI FALLISCE. Come per il motore locale
/// inerte: un compito che aspetta uno strumento non installato non è un
/// difetto del compito, e non deve consumare il contatore dei fallimenti
/// consecutivi né aprire una segnalazione ogni notte.
fn call_gemini(cfg: &Config, prompt: &str) -> EngineResult {
    let bin = match resolve_engine_bin(&cfg.gemini_bin) {
        Ok(b) => b,
        Err(looked) => {
            let _ = fs::write(
                &cfg.last_output_path,
                format!(
                    "«{}» non è eseguibile in nessuno dei posti guardati:\n{}\n",
                    cfg.gemini_bin,
                    looked.iter().map(|p| format!("  {p}")).collect::<Vec<_>>().join("\n")
                ),
            );
            return EngineResult::Failed {
                kind: "motore assente".to_string(),
                tokens: "0".to_string(),
            };
        }
    };
    let workdir = cfg.codex_dir.join(".lavoro-usa-e-getta");
    let _ = fs::create_dir_all(&workdir);
    let mut cmd = Command::new(&bin);
    cmd.arg("--model")
        .arg(&cfg.gemini_model)
        .arg("--print")
        .arg(prompt)
        .env("PATH", child_path())
        // Stessa cartella usa-e-getta di Codex, per lo stesso motivo: il
        // motore legge tutta la macchina e può scrivere solo dove non serve
        // a nessuno.
        .current_dir(&workdir)
        .stdin(Stdio::null());
    match run_with_timeout(cmd, Duration::from_secs(cfg.gemini_timeout_secs)) {
        RunOutcome::Finished { status, stdout, stderr } => {
            let mut combined = String::from_utf8_lossy(&stdout).to_string();
            combined.push_str(&String::from_utf8_lossy(&stderr));
            let _ = fs::write(&cfg.last_output_path, &combined);
            if status.success() {
                // La CLI non stampa un conteggio di token come fa Codex:
                // meglio un punto interrogativo onesto di un numero inventato.
                EngineResult::Ok { tokens: "?".to_string() }
            } else {
                EngineResult::Failed { kind: "errore".to_string(), tokens: "?".to_string() }
            }
        }
        RunOutcome::TimedOut => {
            let _ = fs::write(
                &cfg.last_output_path,
                format!("gemini non ha risposto entro {}s\n", cfg.gemini_timeout_secs),
            );
            EngineResult::Failed { kind: "timeout".to_string(), tokens: "?".to_string() }
        }
        RunOutcome::SpawnFailed => {
            let _ = fs::write(&cfg.last_output_path, "gemini non è partito\n");
            EngineResult::Failed { kind: "errore".to_string(), tokens: "?".to_string() }
        }
    }
}

/// Il percorso che ricevono tutti i figli: il motore, la sua catena di
/// interpreti, e la riga di shell della `verifica:`.
fn child_path() -> String {
    enriched_path(
        &std::env::var("PATH").unwrap_or_default(),
        &std::env::var("HOME").unwrap_or_default(),
    )
}

/// Il ponte fra la ricerca del motore — che è logica, e sta in `lib.rs` con
/// le sue prove — e il disco vero, l'unica cosa che qui non si può fingere.
fn resolve_engine_bin(name: &str) -> Result<String, Vec<String>> {
    let path_var = std::env::var("PATH").unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    resolve_bin(name, &path_var, &home, |candidate| {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(candidate)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    })
}

/// Un compito impiantato (`sleep 100000` nella `verifica:`, o un `codex` che
/// non torna) fermava la notte per sempre: solo `curl` aveva un `-m`
/// (difetto 3 del 25/08). Qui il guardiano è `run_with_timeout`.
fn call_codex(cfg: &Config, prompt: &str) -> EngineResult {
    let bin = match resolve_engine_bin(&cfg.codex_bin) {
        Ok(b) => b,
        Err(looked) => {
            let _ = fs::write(
                &cfg.last_output_path,
                format!(
                    "«{}» non è eseguibile in nessuno dei posti guardati.\n\
                     Sotto launchd il percorso ereditato non è quello della shell.\n\
                     Guardati:\n{}\n",
                    cfg.codex_bin,
                    looked.iter().map(|p| format!("  {p}")).collect::<Vec<_>>().join("\n")
                ),
            );
            return EngineResult::Failed { kind: "errore".to_string(), tokens: "?".to_string() };
        }
    };
    // LA CARTELLA DI LAVORO È USA-E-GETTA, E IL MOTIVO NON È L'ORDINE.
    //
    // Dal 26/08/2026 il motore gira in `workspace-write` invece che in sola
    // lettura, perché l'indice semantico di casa passa da un servizio locale
    // che la sola lettura non lascia raggiungere: senza, il motore non sa
    // dove stanno le cose e inventa percorsi — è successo lo stesso giorno,
    // con tre compiti che puntavano a cartelle inesistenti.
    //
    // In cambio la cartella scrivibile è una cartella che non serve a
    // nessuno, ricreata a ogni compito: il motore legge tutto il resto della
    // macchina, e può scrivere solo lì. Prima poteva scrivere in
    // `~/.claude/docs`, che è dove nessuno vuole trovarsi modifiche.
    let workdir = cfg.codex_dir.join(".lavoro-usa-e-getta");
    let _ = fs::create_dir_all(&workdir);
    let mut cmd = Command::new(&bin);
    cmd.args(["exec", "-s", "workspace-write", "-c"])
        .arg("sandbox_workspace_write={network_access=true}")
        .arg("-C")
        .arg(&workdir)
        .arg(prompt)
        .env("PATH", child_path())
        .stdin(Stdio::null());
    match run_with_timeout(cmd, Duration::from_secs(cfg.codex_timeout_secs)) {
        RunOutcome::Finished { status, stdout, stderr } => {
            let mut combined = String::from_utf8_lossy(&stdout).to_string();
            combined.push_str(&String::from_utf8_lossy(&stderr));
            let _ = fs::write(&cfg.last_output_path, &combined);
            let tokens = parse_codex_tokens(&combined);
            if status.success() {
                EngineResult::Ok { tokens }
            } else {
                EngineResult::Failed { kind: "errore".to_string(), tokens }
            }
        }
        RunOutcome::TimedOut => {
            let _ = fs::write(
                &cfg.last_output_path,
                format!("codex timed out after {}s\n", cfg.codex_timeout_secs),
            );
            EngineResult::Failed { kind: "timeout".to_string(), tokens: "?".to_string() }
        }
        RunOutcome::SpawnFailed => {
            let _ = fs::write(&cfg.last_output_path, "codex not found on PATH\n");
            EngineResult::Failed { kind: "errore".to_string(), tokens: "?".to_string() }
        }
    }
}

/// L'esito della `verifica:`: distinto da un `Failed` normale, perché il
/// rapporto deve poter dire "timeout" e non "check failed" quando è stato il
/// tempo a fermarla, non il giudizio.
enum CheckOutcome {
    Passed,
    Failed,
    TimedOut,
}

fn run_check(check: &str, last_output_path: &Path, limit: Duration) -> CheckOutcome {
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(check)
        .env("NOTTE_OUTPUT_FILE", last_output_path)
        .env("PATH", child_path());
    let Ok(mut child) = cmd.spawn() else { return CheckOutcome::Failed };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() { CheckOutcome::Passed } else { CheckOutcome::Failed };
            }
            Ok(None) => {
                if start.elapsed() >= limit {
                    let _ = child.kill();
                    let _ = child.wait();
                    return CheckOutcome::TimedOut;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return CheckOutcome::Failed,
        }
    }
}

/// I compiti che questa notte ha ancora senso prendere.
///
/// Un ricorrente già passato oggi è escluso **qui**, non al momento di
/// eseguirlo: il ciclo prende un compito per giro, e uno già fatto lasciato
/// in testa all'elenco fermerebbe la coda invece di lasciar passare quelli
/// dietro.
fn list_queued_tasks(queue_dir: &Path, today: &str) -> Vec<PathBuf> {
    let mut tasks: Vec<PathBuf> = fs::read_dir(queue_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "task").unwrap_or(false))
        .filter(|p| match parse_task(&fs::read_to_string(p).unwrap_or_default()) {
            ParsedTask::Ok(t) => !already_done_today(&t, today),
            // Un compito illeggibile passa: lo giudica chi lo esegue, che
            // sa archiviarlo e dirlo nel rapporto. Filtrarlo qui lo
            // renderebbe invisibile per sempre.
            ParsedTask::Malformed => true,
        })
        .collect();
    tasks.sort();
    tasks
}

fn ensure_dirs(cfg: &Config) {
    for dir in [
        cfg.queue_dir.parent().map(|p| p.to_path_buf()).unwrap_or_default(),
        cfg.queue_dir.clone(),
        cfg.done_dir.clone(),
        cfg.alerts_dir.clone(),
        cfg.in_progress_dir.clone(),
    ] {
        let _ = fs::create_dir_all(dir);
    }
    for path in [&cfg.log_path, &cfg.lock_path] {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
    }
}

/// La ricevuta prima del lavoro (difetto 4 del 25/08): il compito passa da
/// `coda-notte/` a `in-corso/<nome>.<pid>` PRIMA di chiamare il motore, così
/// un processo ucciso a metà lascia un segno — non torna in coda muto.
fn claim_task(in_progress_dir: &Path, path: &Path, name: &str) -> Option<PathBuf> {
    let receipt = in_progress_dir.join(format!("{name}.{}", std::process::id()));
    fs::rename(path, &receipt).ok()?;
    Some(receipt)
}

/// Le ricevute rimaste da un processo ucciso a metà: pid morto, contatore
/// tentativi incrementato. Oltre `MAX_TASK_ATTEMPTS` il compito è
/// avvelenato e finisce dritto in `fatti/`; altrimenti torna in coda per un
/// altro giro. Va chiamata una sola volta, all'avvio.
fn recover_orphaned_receipts(cfg: &Config) {
    let Ok(entries) = fs::read_dir(&cfg.in_progress_dir) else { return };
    for entry in entries.flatten() {
        let receipt = entry.path();
        let Some(file_name) = receipt.file_name().map(|n| n.to_string_lossy().to_string()) else {
            continue;
        };
        let (base_name, pid) = split_receipt_name(&file_name);
        // Un nome senza suffisso pid non è mai stato scritto da `claim_task`:
        // non si sa di chi sia, si lascia stare piuttosto che perderlo.
        let Some(pid) = pid else { continue };
        if process_exists(pid) {
            continue; // al lavoro davvero (o pid riciclato: si aspetta)
        }

        let text = fs::read_to_string(&receipt).unwrap_or_default();
        let attempts = attempts_field(&text) + 1;
        let content = set_attempts_field(&text, attempts);
        let _ = fs::remove_file(&receipt);

        if attempts > MAX_TASK_ATTEMPTS {
            let mut poisoned = content.clone();
            poisoned.push_str(&status_line("red (avvelenato)"));
            let _ = fs::write(cfg.done_dir.join(&base_name), &poisoned);
            let engine = match parse_task(&content) {
                ParsedTask::Ok(t) => t.engine,
                ParsedTask::Malformed => "sconosciuto".to_string(),
            };
            write_alert(
                cfg,
                &base_name,
                &engine,
                &format!("interrotto {attempts} volte (pid morto ogni volta): avvelenato, non si ritenta più"),
                &notte::truncate_chars(&content, 500),
            );
            note(&cfg.log_path, &format!("{base_name}: avvelenato dopo {attempts} tentativi, in fatti/"));
        } else {
            let _ = fs::write(cfg.queue_dir.join(&base_name), &content);
            note(&cfg.log_path, &format!("{base_name}: ricevuta orfana (pid {pid} morto), tentativo {attempts}, torna in coda"));
        }
    }
}

/// Il bilancio di un compito eseguito: in che secchio finisce, e se conta
/// nel totale del rapporto (un compito malformato no — non è mai arrivato
/// a essere un compito).
struct TaskOutcome {
    counts_total: bool,
    bucket: &'static str,
}

/// Un compito, dalla lettura del file alla riga di rapporto. Condivisa fra
/// il ciclo che svuota tutta la coda in un colpo (`notte`) e il ciclo
/// continuo che ne fa uno alla volta (`notte --watch`): il giudizio su un
/// singolo compito è lo stesso, cambia solo quanti gliene si dà in pasto.
/// Il rapporto del giorno **corrente**, non di quello in cui il ciclo è partito.
///
/// IL DIFETTO CHE RIPARA, misurato il 26/08/2026: la data si calcolava una
/// volta sola, all'avvio del processo. Il ciclo è residente e sopravvive alla
/// mezzanotte, quindi continuava a scrivere nel rapporto del giorno prima —
/// cinque righe datate 26 stavano dentro `rapporto-2026-08-25.md`, e per il 26
/// non esisteva nessun file. Chi cercava cosa avesse fatto il sistema oggi non
/// trovava niente, mentre il sistema aveva lavorato.
///
/// `today_now` arriva da fuori invece di essere letto qui: è ciò che rende
/// questa scelta provabile senza aspettare mezzanotte.
fn report_for(configured: &Path, configured_day: &str, today_now: &str) -> (PathBuf, String) {
    // Le due variabili d'ambiente restano sovrane: chi fissa la data o il
    // percorso lo fa apposta — le prove, e chi rigenera un rapporto a mano.
    if std::env::var("NOTTE_REPORT").is_ok() || std::env::var("NOTTE_DATE_OVERRIDE").is_ok() {
        return (configured.to_path_buf(), configured_day.to_string());
    }
    if today_now == configured_day || today_now.is_empty() {
        return (configured.to_path_buf(), configured_day.to_string());
    }
    let dir = configured.parent().unwrap_or_else(|| Path::new("."));
    (dir.join(format!("rapporto-{today_now}.md")), today_now.to_string())
}

fn execute_task(cfg: &Config, path: &Path, report: &Report, fails: &mut u32, today: &str) -> TaskOutcome {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let text = fs::read_to_string(path).unwrap_or_default();

    let task = match parse_task(&text) {
        ParsedTask::Ok(t) => t,
        ParsedTask::Malformed => {
            note(&cfg.log_path, &format!("{name}: campi mancanti, salto"));
            report.line(&report_line(
                &name,
                &Outcome::Skipped { reason: "campi mancanti (motore/prompt/verifica)".into() },
            ));
            finish(path, &cfg.done_dir, "saltato (campi mancanti)");
            return TaskOutcome { counts_total: false, bucket: "skipped" };
        }
    };

    // Un ricorrente già passato oggi resta dov'è, intoccato: non finisce in
    // archivio, non apre una segnalazione, non conta come lavoro. Senza
    // questo, i sei giri all'ora della finestra notturna lo rieseguirebbero
    // fino a riempire la notte con un compito solo.
    if already_done_today(&task, today) {
        return TaskOutcome { counts_total: false, bucket: "skipped" };
    }

    let prompt_bytes = task.prompt.len();
    if prompt_over_cap(&task.prompt, cfg.max_prompt_bytes) {
        note(
            &cfg.log_path,
            &format!("{name}: l'istruzione è {prompt_bytes} byte, oltre il tetto di {} byte, salto", cfg.max_prompt_bytes),
        );
        report.line(&report_line(
            &name,
            &Outcome::Skipped {
                reason: format!(
                    "l'istruzione è {prompt_bytes} byte, oltre il tetto di {} byte: dividi il compito o accorcialo a monte",
                    cfg.max_prompt_bytes
                ),
            },
        ));
        finish(path, &cfg.done_dir, &format!("saltato (istruzione oltre il tetto: {prompt_bytes} byte)"));
        return TaskOutcome { counts_total: true, bucket: "skipped" };
    }

    if contains_secret(&task.prompt) {
        note(&cfg.log_path, &format!("{name}: l'istruzione nomina una credenziale, salto"));
        report.line(&report_line(
            &name,
            &Outcome::Skipped { reason: "l'istruzione nomina una credenziale, resta in casa".into() },
        ));
        write_alert(
            cfg,
            &name,
            &task.engine,
            "l'istruzione nomina una credenziale, e le credenziali non escono",
            &notte::truncate_chars(&task.prompt, 300),
        );
        finish(path, &cfg.done_dir, "saltato (credenziale nell'istruzione)");
        return TaskOutcome { counts_total: true, bucket: "skipped" };
    }

    if task.engine == "ollama" {
        // Vivo ma senza generalista (misurato il 25/08): non può ricevere
        // compiti di codice oggi. Non è un fallimento, non tocca `fails`.
        note(&cfg.log_path, &format!("{name}: motore locale inerte, rimando"));
        report.line(&report_line(
            &name,
            &Outcome::Deferred { reason: "motore locale inerte: nessun modello generalista installato".into() },
        ));
        finish(path, &cfg.done_dir, "rimandato (motore locale inerte)");
        return TaskOutcome { counts_total: true, bucket: "deferred" };
    }

    // LA RICEVUTA (difetto 4): da qui in poi il motore può appendere o
    // essere ucciso — il compito passa in `in-corso/` PRIMA di chiamarlo, non
    // dopo. Se anche la ricevuta stessa non si riesce a scrivere, meglio
    // saltare il compito che perderlo in un posto che nessuno ricontrolla.
    let Some(claimed) = claim_task(&cfg.in_progress_dir, path, &name) else {
        note(&cfg.log_path, &format!("{name}: non ho potuto scrivere la ricevuta in in-corso/, salto"));
        report.line(&report_line(
            &name,
            &Outcome::Skipped { reason: "non ho potuto scrivere la ricevuta in in-corso/".into() },
        ));
        return TaskOutcome { counts_total: true, bucket: "skipped" };
    };

    let engine_cap_secs = match task.engine.as_str() {
        "codex" => cfg.codex_timeout_secs,
        // Il fetch OpenRouter ha già `curl -m 180`: qui basta un margine.
        _ => 200,
    };
    let caffeine_secs = engine_cap_secs + cfg.check_timeout_secs + 30;

    let start = Instant::now();
    let bucket = {
        // Il caffeinate vive quanto questo blocco (difetto 1b): appena si
        // esce — verde, rosso o timeout — viene ammazzato.
        let _caffeine = CaffeineGuard::start(caffeine_secs);

        let (label, result) = match task.engine.as_str() {
            "openrouter" => (
                format!("openrouter/{}", cfg.openrouter_model),
                call_openrouter(cfg, &task.prompt),
            ),
            "codex" => ("codex".to_string(), call_codex(cfg, &task.prompt)),
            "gemini" => (
                format!("gemini/{}", cfg.gemini_model),
                call_gemini(cfg, &task.prompt),
            ),
            other => {
                note(&cfg.log_path, &format!("{name}: motore sconosciuto «{other}»"));
                report.line(&report_line(
                    &name,
                    &Outcome::Skipped { reason: format!("motore sconosciuto «{other}»") },
                ));
                finish(&claimed, &cfg.done_dir, "saltato (motore sconosciuto)");
                return TaskOutcome { counts_total: true, bucket: "skipped" };
            }
        };
        let elapsed = start.elapsed().as_secs();

        match result {
            EngineResult::Ok { tokens } => {
                match run_check(&task.check, &cfg.last_output_path, Duration::from_secs(cfg.check_timeout_secs)) {
                    CheckOutcome::Passed => {
                        *fails = 0;
                        report.line(&report_line(
                            &name,
                            &Outcome::Green { engine_label: label, tokens, seconds: elapsed },
                        ));
                        close_task(&claimed, cfg, &task, "green");
                        "green"
                    }
                    CheckOutcome::Failed => {
                        *fails += 1;
                        report.line(&report_line(
                            &name,
                            &Outcome::Red { engine_label: label, tokens, seconds: elapsed, reason: "verifica fallita".into() },
                        ));
                        let detail = fs::read_to_string(&cfg.last_output_path).unwrap_or_default();
                        write_alert(
                            cfg,
                            &name,
                            &task.engine,
                            "la verifica non ha confermato la risposta",
                            &notte::truncate_chars(&detail, 500),
                        );
                        close_task(&claimed, cfg, &task, "red (verifica fallita)");
                        "red"
                    }
                    CheckOutcome::TimedOut => {
                        *fails += 1;
                        report.line(&report_line(
                            &name,
                            &Outcome::Red {
                                engine_label: label,
                                tokens,
                                seconds: elapsed,
                                reason: format!("verifica: timeout dopo {}s", cfg.check_timeout_secs),
                            },
                        ));
                        write_alert(
                            cfg,
                            &name,
                            &task.engine,
                            "the check did not finish within the time allowed",
                            &notte::truncate_chars(&task.check, 500),
                        );
                        close_task(&claimed, cfg, &task, "red (timeout verifica)");
                        "red"
                    }
                }
            }
            // Aspettare uno strumento non installato non è fallire: non tocca
            // il contatore dei fallimenti consecutivi (tre di fila
            // fermerebbero la notte intera) e non apre una segnalazione nuova
            // ogni notte. Il compito resta lì, pronto per quando arriva.
            EngineResult::Failed { kind, .. } if kind == "motore assente" => {
                note(&cfg.log_path, &format!("{name}: il motore non è installato, rimando"));
                report.line(&report_line(
                    &name,
                    &Outcome::Deferred {
                        reason: format!("{}: il motore non è installato", task.engine),
                    },
                ));
                close_task(
                    &claimed,
                    cfg,
                    &task,
                    &format!("rimandato ({}: motore assente)", task.engine),
                );
                return TaskOutcome { counts_total: true, bucket: "deferred" };
            }
            EngineResult::Failed { kind, tokens } => {
                *fails += 1;
                report.line(&report_line(
                    &name,
                    &Outcome::Red { engine_label: label, tokens, seconds: elapsed, reason: format!("motore: {kind}") },
                ));
                let detail = fs::read_to_string(&cfg.last_output_path).unwrap_or_default();
                write_alert(
                    cfg,
                    &name,
                    &task.engine,
                    &format!("il motore ha risposto: {kind}"),
                    &notte::truncate_chars(&detail, 500),
                );
                close_task(&claimed, cfg, &task, &format!("red (engine: {kind})"));
                "red"
            }
        }
    };

    // Un 429 non si ritenta a raffica: la pausa vale per ogni chiamata
    // OpenRouter, riuscita o no, perché è il limite di 20/minuto a
    // imporla, non l'esito.
    if task.engine == "openrouter" {
        std::thread::sleep(Duration::from_secs(cfg.openrouter_pause));
    }

    TaskOutcome { counts_total: true, bucket }
}

/// Il ciclo che svuota tutta la coda in un colpo, fermandosi solo al tetto
/// di fallimenti consecutivi: comportamento invariato rispetto a prima del
/// ciclo continuo, e le prove di integrazione lo controllano lanciando il
/// binario senza `--watch`.
fn run_batch(cfg: &Config) -> i32 {
    ensure_dirs(cfg);
    rotate_log(&cfg.log_path);
    recover_orphaned_receipts(cfg);
    let report = Report::open(&cfg.report_path, &cfg.today);

    let tasks = list_queued_tasks(&cfg.queue_dir, &cfg.today);
    if tasks.is_empty() {
        note(&cfg.log_path, &format!("no tasks in {}, nothing to do", cfg.queue_dir.display()));
        report.line("no tasks queued.");
        return 0;
    }

    let mut fails: u32 = 0;
    let (mut total, mut green, mut red, mut deferred, mut skipped) = (0u32, 0u32, 0u32, 0u32, 0u32);
    let mut stopped = false;

    for path in tasks {
        // Il giro `--once` nasce e muore nello stesso minuto: la data
        // dell'avvio è ancora quella di adesso.
        let outcome = execute_task(cfg, &path, &report, &mut fails, &cfg.today);
        if outcome.counts_total {
            total += 1;
        }
        match outcome.bucket {
            "green" => green += 1,
            "red" => red += 1,
            "deferred" => deferred += 1,
            _ => skipped += 1,
        }

        if fails >= cfg.max_failures {
            note(&cfg.log_path, &format!("{fails} fallimenti di fila, mi fermo"));
            report.line("");
            report.line(&format!("**Fermato dopo {fails} fallimenti di fila.**"));
            stopped = true;
            break;
        }
    }

    report.line("");
    report.line(&format!(
        "**In tutto**: {total} compiti — {green} verdi, {red} rossi, {deferred} rimandati, {skipped} saltati."
    ));
    note(
        &cfg.log_path,
        &format!(
            "giro finito: {green} verdi, {red} rossi, {deferred} rimandati, {skipped} saltati (fermato prima: {stopped})"
        ),
    );

    if stopped {
        1
    } else {
        0
    }
}

// ── il ciclo continuo (--watch) ─────────────────────────────────────────

fn measure_idle_seconds(cfg: &Config) -> u64 {
    cfg.idle_seconds_override.unwrap_or_else(|| {
        Command::new("ioreg")
            .args(["-c", "IOHIDSystem"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| parse_idle_seconds(&s))
            .unwrap_or(0)
    })
}

// Se il carico non si legge, meglio presumerlo alto e restare a guardare:
// un falso "libero" ruberebbe risorse a Theo, un falso "occupato" al
// massimo rimanda un compito che può aspettare.
fn measure_load1(cfg: &Config) -> f64 {
    cfg.load1_override.unwrap_or_else(|| {
        Command::new("sysctl")
            .args(["-n", "vm.loadavg"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| parse_loadavg_1min(&s))
            .unwrap_or(f64::MAX)
    })
}

fn measure_mem_free_percent(cfg: &Config) -> u32 {
    cfg.mem_free_percent_override.unwrap_or_else(|| {
        Command::new("memory_pressure")
            .args(["-Q"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| parse_mem_free_percent(&s))
            .unwrap_or(0)
    })
}

fn measure_core_count(cfg: &Config) -> u32 {
    cfg.core_count_override.unwrap_or_else(|| {
        Command::new("sysctl")
            .args(["-n", "hw.ncpu"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1)
    })
}

/// L'ora locale (0-23), per la finestra di notte (difetto 1 del 25/08).
fn measure_hour(cfg: &Config) -> u32 {
    cfg.hour_override.unwrap_or_else(|| shell_date(&["+%H"]).parse().unwrap_or(0))
}

/// Scrive nel registro una riga di decisione solo quando il motivo cambia,
/// o ogni `heartbeat_every` ripetizioni dello stesso: un salto identico a
/// ogni giro (coda vuota di notte, per ore) non deve riempire il file, ma
/// deve restare visibile che il ciclo è vivo e sta ancora aspettando.
struct DecisionLog<'a> {
    log_path: &'a Path,
    last_reason: Option<String>,
    repeat_count: u32,
    heartbeat_every: u32,
}

impl<'a> DecisionLog<'a> {
    fn new(log_path: &'a Path, heartbeat_every: u32) -> Self {
        DecisionLog { log_path, last_reason: None, repeat_count: 0, heartbeat_every: heartbeat_every.max(1) }
    }

    fn record(&mut self, line: String, reason_key: &str) {
        if self.last_reason.as_deref() == Some(reason_key) {
            self.repeat_count += 1;
            if self.repeat_count % self.heartbeat_every != 0 {
                return;
            }
            note(self.log_path, &format!("{line} (x{})", self.repeat_count));
            return;
        }
        self.last_reason = Some(reason_key.to_string());
        self.repeat_count = 1;
        note(self.log_path, &line);
    }
}

/// Il ciclo continuo: ogni `watch_interval_secs` misura macchina ferma o
/// carico libero e decide se c'è un compito da eseguire — uno solo per
/// giro, mai a raffica, perché l'attesa fra un giro e l'altro è già il
/// distanziatore. `watch_max_ticks` (solo per le prove) lo fa uscire dopo
/// N giri invece di girare per sempre.
fn run_watch(cfg: &Config) -> i32 {
    ensure_dirs(cfg);
    note(
        &cfg.log_path,
        &format!(
            "ciclo avviato: giro ogni {}s · ferma da {}s · carico max da fermo {} · carico max da occupata {} · carico max per una leggera {} · memoria libera min {}% · tetto orario {} · finestra {:02}:00-{:02}:00 · fermissima {}s",
            cfg.watch_interval_secs,
            cfg.idle_threshold_secs,
            cfg.idle_load_ratio_cap,
            cfg.busy_load_ratio_cap,
            cfg.light_load_ratio_cap,
            cfg.mem_free_min_percent,
            cfg.hourly_cap,
            cfg.window_start_hour,
            cfg.window_end_hour,
            cfg.very_idle_seconds,
        ),
    );
    recover_orphaned_receipts(cfg);

    let th = WatchThresholds {
        idle_seconds: cfg.idle_threshold_secs,
        idle_load_ratio_cap: cfg.idle_load_ratio_cap,
        busy_load_ratio_cap: cfg.busy_load_ratio_cap,
        light_load_ratio_cap: cfg.light_load_ratio_cap,
        mem_free_min_percent: cfg.mem_free_min_percent,
        hourly_cap: cfg.hourly_cap,
        window_start_hour: cfg.window_start_hour,
        window_end_hour: cfg.window_end_hour,
        very_idle_seconds: cfg.very_idle_seconds,
    };

    let mut recent_runs: Vec<Instant> = Vec::new();
    let mut fails: u32 = 0;
    let mut cooldown_until: Option<Instant> = None;
    let mut decision_log = DecisionLog::new(&cfg.log_path, cfg.decision_heartbeat_ticks);
    let mut ticks: u32 = 0;

    loop {
        rotate_log(&cfg.log_path);
        recent_runs.retain(|t| t.elapsed() < Duration::from_secs(3600));
        if let Some(until) = cooldown_until {
            if Instant::now() >= until {
                cooldown_until = None;
            }
        }

        // LA DATA SI RILEGGE A OGNI GIRO, e va letta **qui**, prima del filtro.
        // La correzione del 26/08 la rileggeva solo per scegliere il file del
        // rapporto, dentro il ramo che esegue — cioè a valle del punto che si
        // blocca: per arrivarci serve una coda non vuota, e la coda si svuotava
        // proprio qui. Una correzione giusta messa dopo l'ostacolo non si vede
        // mai girare.
        let today = today_now();
        let tasks = list_queued_tasks(&cfg.queue_dir, &today);
        // IL PESO SI GUARDA SU TUTTA LA CODA, NON SULLA SOLA TESTA.
        //
        // Il difetto che questo ripara, misurato sul servizio vivo il
        // 27/08/2026: il peso si leggeva dalla prima lavorazione dell'elenco
        // ordinato, e quello diventava il peso dell'intero giro. La coda aveva
        // dieci compiti leggeri su dodici, ma in testa — per ordine alfabetico,
        // non per scelta di nessuno — stava `giudica-la-riparazione`, che è
        // pesante. Risultato: il giro veniva giudicato pesante, la soglia si
        // stringeva a quella dei pesanti, e con la macchina di Theo al lavoro
        // **i dieci leggeri dietro non partivano mai**.
        //
        // Il peso serve esattamente a far passare il lavoro piccolo mentre la
        // macchina è occupata. Deciderlo sulla testa lo annulla ogni volta che
        // un pesante capita davanti, cioè per pura fortuna alfabetica.
        let weighed: Vec<(PathBuf, Weight)> = tasks
            .iter()
            .map(|p| {
                // Un compito illeggibile o senza il campo `peso` conta come
                // pesante: stessa prudenza di `parse_task`.
                let w = fs::read_to_string(p)
                    .ok()
                    .map(|text| match parse_task(&text) {
                        ParsedTask::Ok(t) => t.weight,
                        ParsedTask::Malformed => Weight::Heavy,
                    })
                    .unwrap_or(Weight::Heavy);
                (p.clone(), w)
            })
            .collect();
        let idle_seconds = measure_idle_seconds(cfg);
        let load1 = measure_load1(cfg);
        let mem_free_percent = measure_mem_free_percent(cfg);
        let core_count = measure_core_count(cfg);
        let hour = measure_hour(cfg);

        let inputs_for = |w: Weight| WatchInputs {
            idle_seconds,
            load1,
            mem_free_percent,
            core_count,
            tasks_this_hour: recent_runs.len() as u32,
            queue_empty: tasks.is_empty(),
            in_cooldown: cooldown_until.is_some(),
            hour,
            next_task_weight: w,
        };
        let idle_word = if idle_seconds >= th.idle_seconds { "si" } else { "no" };

        // La prima lavorazione che le condizioni di adesso ammettono davvero.
        // `decide` resta il solo giudice — gli si chiede solo una volta per
        // peso, invece di una volta per la testa: gli altri freni (finestra,
        // tetto orario, riposo dopo i fallimenti) non dipendono dal peso e
        // quindi negano tutti i candidati insieme, come prima.
        let chosen = weighed
            .iter()
            .find(|(_, w)| matches!(decide(&inputs_for(*w), &th), WatchDecision::Run))
            .map(|(p, _)| p.clone());

        // Quando non passa nessuno, la ragione da registrare è quella del
        // compito in testa: è il candidato che chi legge il registro si aspetta
        // di vedere nominato.
        let decision = match &chosen {
            Some(_) => WatchDecision::Run,
            None => decide(
                &inputs_for(weighed.first().map(|(_, w)| *w).unwrap_or(Weight::Heavy)),
                &th,
            ),
        };

        match decision {
            WatchDecision::Skip(reason) => {
                let line = format!("idle={idle_word} carico={load1:.2} mem={mem_free_percent}% → salto: {reason}");
                decision_log.record(line, &reason);
            }
            WatchDecision::Run => {
                decision_log.record(
                    format!("idle={idle_word} carico={load1:.2} mem={mem_free_percent}% → eseguo"),
                    "__run__",
                );
                if let Some(path) = chosen {
                    // La data si rilegge a ogni esecuzione: il ciclo resta
                    // acceso per giorni e la mezzanotte non lo riavvia.
                    let (report_path, report_day) =
                        report_for(&cfg.report_path, &cfg.today, &today);
                    let report = Report::open(&report_path, &report_day);
                    execute_task(cfg, &path, &report, &mut fails, &today);
                    recent_runs.push(Instant::now());
                    if fails >= cfg.max_failures {
                        note(
                            &cfg.log_path,
                            &format!("{fails} fallimenti di fila, mi fermo per {}s", cfg.failure_cooldown_secs),
                        );
                        cooldown_until = Some(Instant::now() + Duration::from_secs(cfg.failure_cooldown_secs));
                        fails = 0;
                    }
                }
            }
        }

        ticks += 1;
        if let Some(max) = cfg.watch_max_ticks {
            if ticks >= max {
                return 0;
            }
        }
        std::thread::sleep(Duration::from_secs(cfg.watch_interval_secs));
    }
}

fn main() {
    let cfg = Config::from_env();
    ensure_dirs(&cfg);

    // IL LUCCHETTO SULL'INTERO GIRO (difetto 2 del 25/08): due istanze non
    // eseguono più lo stesso compito insieme — la seconda esce pulita.
    let lock = match take_run_lock(&cfg.lock_path) {
        LockOutcome::Taken(l) => l,
        LockOutcome::Held(pid) => {
            let who = pid.map(|p| p.to_string()).unwrap_or_else(|| "sconosciuto".to_string());
            println!("notte: lucchetto preso (pid {who} attivo), esco pulito");
            note(&cfg.log_path, &format!("lucchetto già preso dal pid {who}, esco pulito"));
            std::process::exit(0);
        }
    };

    let watch_mode = std::env::args().any(|a| a == "--watch");
    let exit_code = if watch_mode { run_watch(&cfg) } else { run_batch(&cfg) };
    // `std::process::exit` non esegue i `Drop`: il lucchetto si toglie a
    // mano, altrimenti resterebbe per sempre dopo un giro normale.
    drop(lock);
    std::process::exit(exit_code);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LA MEZZANOTTE, senza aspettarla — difetto misurato il 26/08/2026.
    ///
    /// Il ciclo residente calcolava la data all'avvio e la teneva per giorni:
    /// cinque righe datate 26 sono finite in `rapporto-2026-08-25.md`, e per il
    /// 26 non esisteva nessun file. Qui la giornata nuova si passa a mano.
    #[test]
    fn after_midnight_the_report_follows_the_new_day() {
        let configured = PathBuf::from("/tmp/notte-x/rapporto-2026-08-25.md");
        let (path, day) = report_for(&configured, "2026-08-25", "2026-08-26");
        assert_eq!(day, "2026-08-26");
        assert_eq!(path, PathBuf::from("/tmp/notte-x/rapporto-2026-08-26.md"));
        assert_eq!(
            path.parent(),
            configured.parent(),
            "la cartella non cambia: cambia solo il giorno"
        );
    }

    /// Nello stesso giorno non si tocca niente: senza questo caso, un criterio
    /// che ricalcola sempre il percorso passerebbe la prova sopra e nessuno si
    /// accorgerebbe che ha smesso di rispettare `NOTTE_REPORT`.
    #[test]
    fn within_the_same_day_the_report_path_is_untouched() {
        let configured = PathBuf::from("/tmp/notte-x/rapporto-2026-08-25.md");
        let (path, day) = report_for(&configured, "2026-08-25", "2026-08-25");
        assert_eq!(day, "2026-08-25");
        assert_eq!(path, configured);
    }

    /// LA CORSA VERA, misurata dal vivo il 25/08/2026: due istanze lanciate
    /// insieme prendevano ENTRAMBE il lucchetto. La causa era leggere un
    /// file appena creato dall'altra, ancora senza il pid dentro (la
    /// finestra fra `create_new` e `writeln!`), e giudicarlo stantio. Qui la
    /// si riproduce senza corse vere: un file vuoto, appena nato, non deve
    /// mai passare per scavalcabile.
    #[test]
    fn a_freshly_created_empty_lock_is_not_stolen() {
        let dir = std::env::temp_dir().join(format!(
            "notte-lock-race-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notte.lock");
        fs::write(&path, "").unwrap(); // il momento esatto della corsa: creato, non ancora scritto
        let outcome = take_run_lock(&path);
        assert!(
            matches!(outcome, LockOutcome::Held(None)),
            "un lucchetto appena nato, senza ancora un pid, non si scavalca"
        );
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }
}
