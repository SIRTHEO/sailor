//! L'involucro di `role-claim`/`role-vacancy`: mkdir, `kill -0`, `ps`,
//! `orca terminal list`, la serratura sul disco, la lettura e la scrittura dei
//! marcatori. Il giudizio — occupato/libero/non-so, il terzo stato, il
//! verdetto sul titolare — sta in `guards::role_claim`, che non tocca niente
//! di tutto questo.
//!
//! PERCHÉ INSIEME A `role-claim.sh` INVECE DI UN SECONDO COMANDO PER
//! `role-vacancy.sh`: sono un solo giudizio, portato da un solo modulo per
//! parte — il primo script chiamava il secondo con un `.`, e la ronda della
//! coda legge lo stesso terzo stato. Vedi `guards::role_claim` per il resto.

use guards::role_claim::{self as judge, HandoffState, HolderScan, KillProbe, Mode, Reachable, RoleFile, Verdict};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ── La configurazione, letta dall'ambiente con gli stessi nomi e le stesse
// soglie di serie dello script ───────────────────────────────────────────────

pub struct Config {
    pub roles_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub projects_dir: PathBuf,
    pub log_file: PathBuf,
    pub orca_bin: String,
    pub handoff_stale_s: u64,
    pub active_s: u64,
    pub expiry_s: u64,
    pub lock_stale_s: u64,
    pub lock_wait_s: u64,
    pub orca_cap_s: u64,
    pub vacancy_dawn_hour: i64,
    pub vacancy_max_hours: u32,
}

fn env_str(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn env_u64(name: &str, default: u64) -> u64 {
    env_str(name).and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl Config {
    pub fn from_env() -> Config {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let lock_stale_s = env_u64("ROLE_CLAIM_LOCK_STALE_S", 120);
        Config {
            roles_dir: env_str("ROLE_CLAIM_ROLES_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".claude/state/ruoli")),
            sessions_dir: env_str("ROLE_CLAIM_SESSIONS_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".claude/state/sessioni-vive")),
            projects_dir: env_str("ROLE_CLAIM_PROJECTS_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".claude/projects")),
            log_file: env_str("ROLE_CLAIM_LOG")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".claude/state/role-claim.log")),
            orca_bin: env_str("ROLE_CLAIM_ORCA_BIN").unwrap_or_else(|| "orca".to_string()),
            handoff_stale_s: env_u64("ROLE_CLAIM_HANDOFF_STALE_S", 1800),
            active_s: env_u64("ROLE_CLAIM_ACTIVE_S", 3600),
            expiry_s: env_u64("ROLE_CLAIM_EXPIRY_S", 21600),
            lock_stale_s,
            lock_wait_s: env_u64("ROLE_CLAIM_LOCK_WAIT_S", lock_stale_s + 15),
            orca_cap_s: env_u64("ROLE_CLAIM_ORCA_CAP_S", 10),
            vacancy_dawn_hour: env_u64("ROLE_VACANCY_DAWN_HOUR", 6) as i64,
            vacancy_max_hours: env_u64("ROLE_VACANCY_MAX_HOURS", 24) as u32,
        }
    }
}

pub fn run() -> i32 {
    let argv: Vec<String> = std::env::args().skip(2).collect();
    let args = match judge::parse_args(&argv) {
        Ok(a) => a,
        Err(judge::ArgError::MissingRole) => {
            usage();
            return 64;
        }
        Err(judge::ArgError::Unknown(a)) => {
            eprintln!("role-claim: unknown argument '{a}'");
            usage();
            return 64;
        }
        Err(judge::ArgError::VacancyFlagsWithoutLeaveEmpty) => {
            eprintln!("role-claim: --for-hours and --why only mean something with --leave-empty");
            return 64;
        }
    };
    let cfg = Config::from_env();
    let session_id = judge::resolve_session_id(
        env_str("CLAUDE_CODE_SESSION_ID").as_deref(),
        env_str("CLAUDE_SESSION_ID").as_deref(),
        env_str("CLAUDE_TRANSCRIPT_PATH").as_deref(),
    );
    let Some(session_id) = session_id else {
        eprintln!("role-claim: no session id available, cannot declare {}", args.role);
        return 65;
    };
    let own = judge::short_id(&session_id);
    run_with(&args, &cfg, &own, now())
}

fn usage() {
    eprintln!("role-claim: usage: role-claim.sh [--who-holds|--handing-over|--fill-again] ROLE");
    eprintln!("role-claim:        role-claim.sh --leave-empty ROLE [--for-hours N] [--why TEXT]");
}

// ── Il mondo ────────────────────────────────────────────────────────────────

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn age_of(path: &Path, now: i64) -> Option<u64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let secs = modified.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    Some((now - secs).max(0) as u64)
}

/// Come `sed -n 'Np' | tr -d '\r\n'`, ma su un file: `None` se non si legge.
fn read_lossy(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn note(log: &Path, now: i64, message: &str) {
    if let Some(dir) = log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log) {
        // `date -u '+%Y-%m-%d %H:%M:%S'`, due spazi prima del messaggio: la
        // stessa forma di `printf '%s UTC  %s\n'`.
        let stamp = hook_io::local_time::utc_iso_seconds(now).replace('T', " ");
        let _ = writeln!(f, "{stamp} UTC  {message}");
    }
}

fn who_or_unnamed(w: &str) -> &str {
    if w.is_empty() { "an unnamed session" } else { w }
}

/// Lo scarto dall'UTC come `i64`: `hook_io::local_time::local_offset` lo rende
/// in `i32`, e i porti puri di `guards::role_claim` lo vogliono in `i64` per
/// restare coerenti con gli epoch che maneggiano altrove.
///
/// NOTA PER IL REVISORE: `vacancy_until_text` scrive `%Y-%m-%d %H:%M` con lo
/// scarto numerico, non la sigla del fuso (`%Z` di `date -r`, es. "CEST") che
/// porta lo script shell originale. `hook-io::local_time` espone solo
/// l'offset in secondi, non i nomi delle zone: riprodurre `%Z` servirebbe una
/// tabella di nomi che oggi non esiste in nessun crate di casa. Interpretato
/// come non bloccante — il numero resta corretto, cambia solo la resa.
///
/// LO SCARTO SI CHIEDE PER L'ISTANTE DI CUI SI PARLA, mai per «adesso» (rilievo
/// del revisore, 25/08/2026): l'argomento è l'epoch da rendere o da raggiungere,
/// non l'ora della chiamata. Applicare a un istante lo scarto di un altro momento
/// sbaglia di un'ora a cavallo del cambio d'ora legale — innocuo quando si stampa
/// soltanto, uno stato scritto sbagliato quando si calcola una scadenza.
fn local_offset(at: i64) -> i64 {
    hook_io::local_time::local_offset(at) as i64
}

/// Il messaggio comune a «scaduta» e «senza scadenza leggibile»: si dice a
/// voce in tutti e due i casi, e chi scrive (non chi chiede) ripulisce il
/// marcatore — una domanda che ripulisce è una domanda che scrive.
fn report_expired_vacancy(cfg: &Config, role: &str, handoff_path: &Path, now: i64, read_only: bool, said: &str) {
    eprintln!("role-claim: the empty-by-decision declaration for {role} {said} -- the post is an ordinary empty post again");
    if !read_only {
        let _ = std::fs::remove_file(handoff_path);
        note(&cfg.log_file, now, &format!("{role}: removed an empty-by-decision declaration that {said}"));
    }
}

/// Il messaggio da dare a chi lancia dove non può scrivere. Prova con una
/// scrittura vera, come `claude-hooks::fault_deposit::not_writable`: dentro il
/// perimetro di una sessione i bit dicono di sì e la scrittura torna «operation
/// not permitted».
fn not_writable(dir: &Path) -> Option<String> {
    let probe = dir.join(".role-claim-probe");
    let done = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&probe, b"x"));
    let _ = std::fs::remove_file(&probe);
    match done {
        Ok(()) => None,
        Err(e) => Some(format!("cannot write in {} ({e})", dir.display())),
    }
}

// ── La serratura ──────────────────────────────────────────────────────────────

/// Rilascia se stessa uscendo, MA SOLO SE È ANCORA MIA: se dentro c'è un nome
/// che non è il mio, qualcuno me l'ha rotta sotto e sta lavorando adesso.
struct RoleLock {
    dir: PathBuf,
    owner_file: PathBuf,
    own: String,
    role: String,
    log: PathBuf,
    held: bool,
}

impl Drop for RoleLock {
    fn drop(&mut self) {
        if !self.held {
            return;
        }
        let cur = read_lossy(&self.owner_file).map(|s| s.trim_end_matches(['\r', '\n']).to_string());
        if cur.as_deref() == Some(self.own.as_str()) {
            let _ = std::fs::remove_file(&self.owner_file);
            let _ = std::fs::remove_dir(&self.dir);
        } else {
            note(
                &self.log,
                now(),
                &format!(
                    "did not release the lock on {}: it now belongs to {}, not {}",
                    self.role,
                    cur.as_deref().map(who_or_unnamed).unwrap_or("someone else"),
                    self.own
                ),
            );
        }
    }
}

/// `None` se la serratura non serve (`--who-holds`), altrimenti la prende —
/// aspettando, rompendo quella scaduta, o arrendendosi. `Err(2)` è il codice
/// da restituire subito, come lo script.
fn acquire_lock(cfg: &Config, role: &str, own: &str) -> Result<Option<RoleLock>, i32> {
    let dir = cfg.roles_dir.join(format!(".lock.{role}"));
    let owner_file = dir.join("owner");
    // Calcolata una volta sola, come `lock_deadline=$(( $(now) + LOCK_WAIT_S ))`
    // nel guscio: un'epoca fissa contro cui confrontare l'orologio reale a ogni
    // giro del ciclo.
    let deadline_epoch = now() + cfg.lock_wait_s as i64;
    loop {
        if std::fs::create_dir(&dir).is_ok() {
            let _ = std::fs::write(&owner_file, own);
            return Ok(Some(RoleLock {
                dir,
                owner_file,
                own: own.to_string(),
                role: role.to_string(),
                log: cfg.log_file.clone(),
                held: true,
            }));
        }
        let dir_exists = dir.is_dir();
        let cur_now = now();
        let lock_age = age_of(&dir, cur_now).unwrap_or(0);
        let stale_owner = read_lossy(&owner_file).map(|s| s.trim_end_matches(['\r', '\n']).to_string());
        let stale_owner = stale_owner.as_deref().filter(|s| !s.is_empty());
        let step = judge::classify_lock_wait(dir_exists, lock_age, cfg.lock_stale_s, cur_now, deadline_epoch, stale_owner);
        match step {
            judge::LockStep::NotAnOccupiedLock => {
                note(&cfg.log_file, cur_now, &format!("cannot create the lock on {role} (not an occupied lock), {own} did not claim"));
                eprintln!("role-claim: cannot create the lock directory for {role}, not claiming");
                return Err(2);
            }
            judge::LockStep::BreakStale { owner } => {
                note(&cfg.log_file, cur_now, &format!("broke the stale lock on {role} held by {owner} ({lock_age}s old)"));
                let _ = std::fs::remove_file(&owner_file);
                let _ = std::fs::remove_dir(&dir);
                continue;
            }
            judge::LockStep::TimedOut => {
                note(&cfg.log_file, cur_now, &format!("timed out waiting for the lock on {role}, {own} did not claim"));
                eprintln!("role-claim: timed out waiting for the lock on {role}, not claiming");
                return Err(2);
            }
            judge::LockStep::Wait => {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

// ── Un comando col tempo massimo, come in `hook_census::run_with_timeout` ──

fn run_with_timeout(cmd: &mut Command, seconds: u64) -> String {
    let Ok(mut child) = cmd.stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
        return String::new();
    };
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }
    let mut out = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        use std::io::Read;
        let _ = pipe.read_to_end(&mut out);
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ── Il posto degli altri titolari ────────────────────────────────────────────

fn list_role_files(roles_dir: &Path) -> Vec<RoleFile> {
    let Ok(entries) = std::fs::read_dir(roles_dir) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let first_line = read_lossy(&e.path())
                .map(|t| t.lines().next().unwrap_or("").trim_end_matches(['\r', '\n']).to_string())
                .unwrap_or_default();
            Some(RoleFile { name, first_line })
        })
        .collect()
}

/// Il record di sessione del titolare: pid, tab, handle, percorso della
/// trascrizione — quello che il guscio legge con `jq`.
struct HolderRecord {
    pid: Option<u32>,
    tab: Option<String>,
    handle: Option<String>,
    transcript: Option<String>,
}

fn read_holder_record(cfg: &Config, holder: &str) -> HolderRecord {
    let path = cfg.sessions_dir.join(format!("{holder}.json"));
    let Some(text) = read_lossy(&path) else {
        return HolderRecord { pid: None, tab: None, handle: None, transcript: None };
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return HolderRecord { pid: None, tab: None, handle: None, transcript: None };
    };
    HolderRecord {
        pid: v.get("session_pid").and_then(|x| x.as_u64()).map(|x| x as u32),
        tab: v.get("tab_id").and_then(|x| x.as_str()).filter(|s| !s.is_empty()).map(String::from),
        handle: v.get("terminal_handle").and_then(|x| x.as_str()).filter(|s| !s.is_empty()).map(String::from),
        transcript: v.get("transcript_path").and_then(|x| x.as_str()).filter(|s| !s.is_empty()).map(String::from),
    }
}

/// La trascrizione del titolare: il percorso del record se è leggibile,
/// altrimenti il primo file che porta i suoi otto caratteri in testa, sotto
/// `PROJECTS_DIR/*/`.
fn find_transcript(cfg: &Config, from_record: Option<&str>, holder: &str) -> Option<PathBuf> {
    if let Some(p) = from_record {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let entries = std::fs::read_dir(&cfg.projects_dir).ok()?;
    for tree in entries.flatten() {
        if !tree.path().is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(tree.path()) else { continue };
        for f in files.flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            if name.starts_with(holder) && name.ends_with(".jsonl") {
                return Some(f.path());
            }
        }
    }
    None
}

fn parse_ps_comm(output: &str) -> Option<String> {
    let line = output.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    let mut parts = line.splitn(2, char::is_whitespace);
    let _pid = parts.next()?;
    let comm = parts.next()?.trim();
    if comm.is_empty() { None } else { Some(comm.to_string()) }
}

/// `kill -0 PID`: la sonda letta nel testo, mai nel codice d'uscita.
fn probe_process(pid: u32) -> KillProbe {
    match Command::new("kill").args(["-0", &pid.to_string()]).output() {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            judge::classify_kill_probe(out.status.success(), &stderr)
        }
        // `kill` non è nemmeno partito: non è una risposta, è un non-so.
        Err(_) => KillProbe::Denied,
    }
}

fn ps_comm(pid: u32) -> Option<String> {
    let out = Command::new("ps").args(["-o", "pid=,comm=", "-p", &pid.to_string()]).output().ok()?;
    parse_ps_comm(&String::from_utf8_lossy(&out.stdout))
}

fn orca_terminals(cfg: &Config) -> Option<(Vec<String>, Vec<String>)> {
    let mut cmd = Command::new(&cfg.orca_bin);
    cmd.args(["terminal", "list", "--json"]);
    let raw = run_with_timeout(&mut cmd, cfg.orca_cap_s);
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    if v.get("ok").and_then(|x| x.as_bool()) != Some(true) {
        return None;
    }
    let terminals = v.get("result").and_then(|r| r.get("terminals")).and_then(|t| t.as_array())?;
    let tabs = terminals.iter().filter_map(|t| t.get("tabId").and_then(|x| x.as_str()).map(String::from)).collect();
    let handles = terminals.iter().filter_map(|t| t.get("handle").and_then(|x| x.as_str()).map(String::from)).collect();
    Some((tabs, handles))
}

// ── Il giro ──────────────────────────────────────────────────────────────────

pub fn run_with(args: &judge::ParsedArgs, cfg: &Config, own: &str, now: i64) -> i32 {
    let read_only = args.mode == Mode::WhoHolds;
    let role = &args.role;
    let _ = std::fs::create_dir_all(&cfg.roles_dir);

    // La domanda si può rispondere leggendo; dichiarare no. Solo chi scrive
    // deve poter contare su un registro scrivibile.
    if !read_only {
        if let Some(why) = not_writable(&cfg.roles_dir) {
            eprintln!("role-claim: {why}");
            return 2;
        }
    }

    let handoff_path = cfg.roles_dir.join(role);

    let _lock = if read_only {
        None
    } else {
        match acquire_lock(cfg, role, own) {
            Ok(l) => l,
            Err(code) => return code,
        }
    };

    match args.mode {
        Mode::HandingOver => {
            let _ = std::fs::write(&handoff_path, format!("in-ricambio\n{own}\n"));
            note(&cfg.log_file, now, &format!("{role} marked as handing over by {own}"));
            println!("role-claim: {role} marked as handing over by {own}");
            return 0;
        }
        Mode::LeaveEmpty => {
            return do_leave_empty(args, cfg, own, now, &handoff_path);
        }
        Mode::FillAgain => {
            return do_fill_again(cfg, own, now, role, &handoff_path);
        }
        Mode::Claim | Mode::WhoHolds => {}
    }

    // --- Il terzo stato: chi prende supera la decisione, chi chiede la legge.
    let vacancy_text = read_lossy(&handoff_path);
    match judge::read_vacancy(vacancy_text.as_deref(), now) {
        judge::VacancyRead::Active(v) => {
            if read_only {
                println!("{}", judge::status_vacant(role, &v.who, v.until, v.left));
                println!(
                    "role-claim: {role} is empty BY DECISION until {} ({}min left), decided by {}{}. This is not an uncovered post: do not open one.",
                    judge::vacancy_until_text(v.until, local_offset(v.until)),
                    v.left / 60,
                    who_or_unnamed(&v.who),
                    if v.why.is_empty() { String::new() } else { format!(" -- {}", v.why) }
                );
                return 3;
            }
            let _ = std::fs::remove_file(&handoff_path);
            note(&cfg.log_file, now, &format!(
                "{role} was declared empty by decision by {} until {}; {own} is taking the post anyway, declaration cleared",
                who_or_unnamed(&v.who), v.until
            ));
            eprintln!(
                "role-claim: {role} was declared empty by decision until {}; taking the post clears that declaration",
                judge::vacancy_until_text(v.until, local_offset(v.until))
            );
        }
        // NON UN OR-PATTERN: `Expired` e `Malformed` non condividono gli stessi
        // campi (`until: i64` contro `until_raw: String`), e un ramo unico non
        // può legarli entrambi. Stesso messaggio finale, costruito da un
        // piccolo aiuto invece che duplicato riga per riga.
        judge::VacancyRead::Expired { until, .. } => {
            let said = format!("ran out at {}", judge::vacancy_until_text(until, local_offset(until)));
            report_expired_vacancy(cfg, role, &handoff_path, now, read_only, &said);
        }
        judge::VacancyRead::Malformed { until_raw, .. } => {
            let said = format!("has no readable deadline ('{until_raw}'), so it never counted");
            report_expired_vacancy(cfg, role, &handoff_path, now, read_only, &said);
        }
        judge::VacancyRead::None => {}
    }

    // --- Il marcatore «in-ricambio», riletto dopo il terzo stato: il file è
    // uno, i due marcatori si escludono per costruzione.
    let handoff_text = read_lossy(&handoff_path);
    if let Some(owner) = judge::read_handoff(handoff_text.as_deref()) {
        let age = age_of(&handoff_path, now).unwrap_or(0);
        match judge::evaluate_handoff(Some(owner), age, own, cfg.handoff_stale_s) {
            HandoffState::Blocking { owner } => {
                if read_only {
                    println!("{}", judge::status_handoff(role, &owner, age));
                }
                eprintln!("role-claim: {role} has a handoff in progress (marked {age}s ago by {owner}), not claiming");
                return 1;
            }
            HandoffState::Stale { owner } => {
                if read_only {
                    eprintln!("role-claim: {role} has a stale handoff marker ({age}s old, left by {owner}) -- whoever takes the post will clear it");
                } else {
                    let _ = std::fs::remove_file(&handoff_path);
                    note(&cfg.log_file, now, &format!("{role}: removed a handoff marker {age}s old left by {owner}"));
                }
            }
            HandoffState::NotHandoff => {}
        }
    }

    // --- 1. Chi ha già dichiarato questo mestiere? ------------------------
    let files = list_role_files(&cfg.roles_dir);
    let holder = match judge::scan_holder(&files, own, role, read_only) {
        HolderScan::HeldByYou => {
            println!("{}", judge::status_held_by_you(role, own));
            println!("role-claim: {role} is held by you ({own})");
            return 1;
        }
        HolderScan::Free => {
            if read_only {
                println!("{}", judge::status_free(role));
                println!("role-claim: nobody holds {role}");
                return 0;
            }
            let _ = std::fs::write(cfg.roles_dir.join(own), format!("{role}\n"));
            note(&cfg.log_file, now, &format!("{role} claimed by {own}, nobody held it"));
            println!("role-claim: nobody held {role}, claimed by {own}");
            return 0;
        }
        HolderScan::Held(h) => h,
    };

    // --- 2. Il titolare è vivo? ---------------------------------------------
    let record = read_holder_record(cfg, &holder);
    let mut comm = None;
    let mut probe = None;
    if let Some(pid) = record.pid {
        let p = probe_process(pid);
        if p == KillProbe::Succeeded {
            comm = ps_comm(pid);
        }
        probe = Some(p);
    }
    let proc_status = judge::proc_status_from(record.pid.is_some(), probe, comm.as_deref());

    let transcript = find_transcript(cfg, record.transcript.as_deref(), &holder);
    let talk_age = transcript.as_deref().and_then(|p| age_of(p, now));

    let Some((live_tabs, live_handles)) = orca_terminals(cfg) else {
        note(&cfg.log_file, now, &format!("cannot read the live pane list, not touching {role} (held by {holder})"));
        eprintln!("role-claim: cannot read the live pane list, not claiming and not removing {role} (held by {holder})");
        return 2;
    };
    let reachable = judge::compute_reachable(record.tab.as_deref(), record.handle.as_deref(), &live_tabs, &live_handles);

    // La chiamata a `orca` può aver mangiato secondi: la serratura torna
    // giovane, o il prossimo che passa la crede abbandonata mentre la sto
    // usando. SOLO SE LA SERRATURA C'È: un `touch` su una cartella inesistente
    // creerebbe un file con quel nome, e nessun `mkdir` riuscirebbe più lì.
    if !read_only && _lock.is_some() {
        let lock_dir = cfg.roles_dir.join(format!(".lock.{role}"));
        if lock_dir.is_dir() {
            let _ = Command::new("touch").arg(&lock_dir).status();
        }
    }

    // --- 3. Il verdetto -------------------------------------------------------
    match judge::evaluate_holder(proc_status, talk_age, reachable, cfg.active_s, cfg.expiry_s) {
        Verdict::AlreadyHeldLive => {
            if read_only {
                println!("{}", judge::status_held(role, &holder));
            }
            note(&cfg.log_file, now, &format!("{role} already held by live session {holder} (process running), {own} did not claim"));
            eprintln!("role-claim: {role} is already held by a live session ({holder}), not claiming -- write to it and ask if it is handing over");
            1
        }
        Verdict::AlreadyHeldByTranscript => {
            if read_only {
                println!("{}", judge::status_held(role, &holder));
            }
            note(&cfg.log_file, now, &format!(
                "{role} already held by {holder} (transcript {}s old, pane {}), {own} did not claim",
                talk_age.unwrap_or(0), reachable_word(reachable)
            ));
            eprintln!("role-claim: {role} is already held by a live session ({holder}), not claiming -- write to it and ask if it is handing over");
            1
        }
        Verdict::Replaceable { why } => {
            if read_only {
                println!("{}", judge::status_takeable(role, &holder));
                println!("role-claim: {holder} holds {role} but is not alive ({why}) -- the post can be taken");
                return 0;
            }
            let _ = std::fs::remove_file(cfg.roles_dir.join(&holder));
            let _ = std::fs::write(cfg.roles_dir.join(own), format!("{role}\n"));
            note(&cfg.log_file, now, &format!("{role}: {holder} was not alive ({why}), replaced by {own}"));
            println!("role-claim: {holder} held {role} but was not alive ({why}), replaced by {own}");
            0
        }
        Verdict::Unknown => {
            if read_only {
                println!("{}", judge::status_unknown(role, &holder));
                println!("role-claim: cannot tell whether {holder} ({role}) is alive");
                return 2;
            }
            note(&cfg.log_file, now, &format!("cannot tell whether {holder} ({role}) is alive, {own} did not claim and did not remove"));
            eprintln!("role-claim: cannot tell whether {holder} is alive, not claiming and not removing {role}");
            2
        }
    }
}

fn reachable_word(r: Reachable) -> &'static str {
    match r {
        Reachable::Yes => "yes",
        Reachable::No => "no",
        Reachable::Unknown => "unknown",
    }
}

fn do_leave_empty(args: &judge::ParsedArgs, cfg: &Config, own: &str, now: i64, handoff_path: &Path) -> i32 {
    let role = &args.role;
    let (until_epoch, span) = if let Some(raw) = &args.for_hours {
        match judge::validate_hours(raw, cfg.vacancy_max_hours) {
            Ok(h) => (now + i64::from(h) * 3600, format!("{h}h")),
            Err(e) => {
                eprintln!("{e}");
                return 64;
            }
        }
    } else {
        // `local_offset` come FUNZIONE, non come numero: l'alba può cadere oltre un
        // cambio d'ora, e allora vuole lo scarto di quel momento (vedi la nota su
        // `vacancy_next_dawn`).
        (judge::vacancy_next_dawn(now, local_offset, cfg.vacancy_dawn_hour), "until dawn".to_string())
    };
    let why = args.why.clone().unwrap_or_default();

    let was = read_lossy(handoff_path)
        .and_then(|t| judge::read_handoff(Some(&t)))
        .map(|owner| format!(" (this replaced a handoff marker left by {owner})"))
        .unwrap_or_default();

    let _ = std::fs::write(
        handoff_path,
        format!("{}\n{own}\n{until_epoch}\n{why}\n", judge::VACANCY_MARKER),
    );
    note(
        &cfg.log_file,
        now,
        &format!(
            "{role} declared empty by decision by {own} until {until_epoch} ({span}){}{was}",
            if why.is_empty() { String::new() } else { format!(": {why}") }
        ),
    );
    println!("{}", judge::status_vacant(role, own, until_epoch, until_epoch - now));
    println!(
        "role-claim: {role} is now declared empty by decision until {} ({span}). Nothing will be opened for it until then, and after then it goes back to being an ordinary empty post.{was}",
        judge::vacancy_until_text(until_epoch, local_offset(until_epoch))
    );
    0
}

fn do_fill_again(cfg: &Config, own: &str, now: i64, role: &str, handoff_path: &Path) -> i32 {
    let text = read_lossy(handoff_path);
    let (who, state_word) = match judge::read_vacancy(text.as_deref(), now) {
        judge::VacancyRead::None => {
            println!("role-claim: {role} was not declared empty by decision, nothing to call off");
            return 0;
        }
        judge::VacancyRead::Active(v) => (v.who, "active"),
        judge::VacancyRead::Expired { who, .. } => (who, "expired"),
        judge::VacancyRead::Malformed { who, .. } => (who, "malformed"),
    };
    let _ = std::fs::remove_file(handoff_path);
    note(
        &cfg.log_file,
        now,
        &format!("{role}: the empty-by-decision declaration (state {state_word}, left by {}) was called off by {own}", who_or_unnamed(&who)),
    );
    println!("role-claim: {role} is no longer declared empty by decision -- it is an ordinary empty post again");
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use hook_io::testing::test_dir;

    /// Un banco: le quattro cartelle e il finto `orca`, come nel banco shell.
    struct Bench {
        cfg: Config,
        own: String,
        now: i64,
    }

    const NOW: i64 = 1_787_598_000;

    fn bench(name: &str) -> Bench {
        let dir = test_dir(name);
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let orca = bin.join("orca");
        std::fs::write(&orca, "#!/bin/sh\necho '{\"ok\": true, \"result\": {\"terminals\": []}}'\n").unwrap();
        std::process::Command::new("chmod").arg("+x").arg(&orca).status().unwrap();
        Bench {
            cfg: Config {
                roles_dir: dir.join("ruoli"),
                sessions_dir: dir.join("sessioni"),
                projects_dir: dir.join("projects"),
                log_file: dir.join("role-claim.log"),
                orca_bin: orca.to_string_lossy().into_owned(),
                handoff_stale_s: 1800,
                active_s: 3600,
                expiry_s: 21600,
                lock_stale_s: 120,
                lock_wait_s: 135,
                orca_cap_s: 5,
                vacancy_dawn_hour: 6,
                vacancy_max_hours: 24,
            },
            own: "bbbbbbbb".to_string(),
            now: NOW,
        }
    }

    impl Bench {
        // Maiuscolo come lo farebbe `judge::parse_args`: costruire il valore a
        // mano qui non deve poter dare un mestiere diverso da quello che un
        // avvio vero produrrebbe.
        fn args(&self, mode: Mode, role: &str) -> judge::ParsedArgs {
            judge::ParsedArgs { mode, role: judge::ascii_upper(role), for_hours: None, why: None }
        }
        fn role_file(&self, name: &str) -> String {
            std::fs::read_to_string(self.cfg.roles_dir.join(name)).unwrap_or_default()
        }
    }

    /// Braccio 1: un mestiere libero si dichiara.
    #[test]
    fn a_free_post_is_claimed() {
        let b = bench("role-claim-libero");
        let args = b.args(Mode::Claim, "macchinista");
        assert_eq!(run_with(&args, &b.cfg, &b.own, b.now), 0);
        assert_eq!(b.role_file("bbbbbbbb").trim(), "MACCHINISTA");
    }

    /// Braccio 10: un giro normale non lascia dietro la serratura.
    #[test]
    fn a_normal_run_leaves_no_lock_behind() {
        let b = bench("role-claim-serratura-pulita");
        let args = b.args(Mode::Claim, "macchinista");
        run_with(&args, &b.cfg, &b.own, b.now);
        assert!(!b.cfg.roles_dir.join(".lock.MACCHINISTA").exists());
    }

    /// Braccio 14: la domanda non scrive niente, nemmeno per un mestiere libero.
    #[test]
    fn asking_about_a_free_post_writes_nothing() {
        let b = bench("role-claim-domanda-libera");
        let args = b.args(Mode::WhoHolds, "nostromo");
        std::fs::create_dir_all(&b.cfg.roles_dir).unwrap();
        assert_eq!(run_with(&args, &b.cfg, &b.own, b.now), 0);
        assert!(std::fs::read_dir(&b.cfg.roles_dir).unwrap().next().is_none(), "niente è stato scritto");
    }

    /// Braccio 17: il terzo stato risponde diversamente da un posto libero.
    #[test]
    fn a_post_left_empty_by_decision_answers_exit_3() {
        let b = bench("role-claim-terzo-stato");
        let leave = judge::ParsedArgs {
            mode: Mode::LeaveEmpty,
            role: "CAPITANO".to_string(),
            for_hours: None,
            why: Some("una sola figura di guardia stanotte".to_string()),
        };
        assert_eq!(run_with(&leave, &b.cfg, &b.own, b.now), 0);
        let ask = b.args(Mode::WhoHolds, "capitano");
        assert_eq!(run_with(&ask, &b.cfg, "dddddddd", b.now), 3, "vuoto per decisione, non libero e basta");
    }

    /// Braccio 21: chi apre la figura scavalca la decisione, e lo dice.
    #[test]
    fn taking_the_post_clears_the_declaration() {
        let b = bench("role-claim-scavalca");
        let leave = judge::ParsedArgs { mode: Mode::LeaveEmpty, role: "CAPITANO".to_string(), for_hours: None, why: None };
        run_with(&leave, &b.cfg, &b.own, b.now);
        let take = b.args(Mode::Claim, "capitano");
        assert_eq!(run_with(&take, &b.cfg, "dddddddd", b.now), 0);
        assert_eq!(b.role_file("dddddddd").trim(), "CAPITANO");
        assert!(!b.cfg.roles_dir.join("CAPITANO").exists(), "la dichiarazione è sparita");
    }

    #[test]
    fn calling_off_a_missing_declaration_is_not_an_error() {
        let b = bench("role-claim-disdetta-vuota");
        std::fs::create_dir_all(&b.cfg.roles_dir).unwrap();
        let fill = b.args(Mode::FillAgain, "capitano");
        assert_eq!(run_with(&fill, &b.cfg, &b.own, b.now), 0);
    }
}
