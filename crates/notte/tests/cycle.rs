//! Prove di integrazione sul binario vero: ogni prova lancia
//! `target/debug/notte` (via `CARGO_BIN_EXE_notte`) su una cartella di coda
//! usa-e-getta, coi motori sostituiti da finti eseguibili in `tests/fixtures/`
//! — niente rete, niente quota Codex spesa dalle prove.
//!
//! IL BRACCIO CHE CONTA È IL COMPITO ROSSO CHE APRE UNA SEGNALAZIONE: un
//! ciclo che dichiara verde tutto è facile da rendere verde per sbaglio.
//! Questa batteria rompe apposta una `verifica:` (§ `red_check_writes_alert`)
//! e controlla che il rosso e la segnalazione escano davvero, non solo che il
//! programma non vada in crash.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

struct Workspace {
    root: PathBuf,
}

impl Workspace {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "notte-cycle-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for sub in ["queue", "done", "alerts", "state", "in-corso"] {
            fs::create_dir_all(root.join(sub)).unwrap();
        }
        Workspace { root }
    }

    fn path(&self, sub: &str) -> PathBuf {
        self.root.join(sub)
    }

    /// Scrive un compito nella coda, nel formato di casa.
    fn write_task(&self, name: &str, engine: &str, prompt: &str, check: &str) -> PathBuf {
        let text = format!("motore: {engine}\nperimetro: pubblico\n---prompt---\n{prompt}\n---verifica---\n{check}\n");
        let path = self.path("queue").join(name);
        fs::write(&path, text).unwrap();
        path
    }

    /// Scrive una ricevuta a mano in `in-corso/`, come se un giro precedente
    /// l'avesse presa in carico e fosse morto lì: simula il difetto 4 senza
    /// dover davvero uccidere un processo a metà lavoro. Il numero di
    /// tentativi non vive più nel testo del compito (lo tiene il deposito:
    /// vedi `write_open_attempt`/`write_closed_broke_attempt`), quindi qui
    /// il testo è sempre quello nudo del compito.
    fn write_receipt(&self, task_name: &str, pid: u32, engine: &str, prompt: &str, check: &str) -> PathBuf {
        let text = format!("motore: {engine}\nperimetro: pubblico\n---prompt---\n{prompt}\n---verifica---\n{check}\n");
        let path = self.path("in-corso").join(format!("{task_name}.{pid}"));
        fs::write(&path, text).unwrap();
        path
    }
}

/// Lo stesso formato di `task_run_id` in `main.rs` (privato, non
/// raggiungibile da qui perché `main.rs` è un binario, non una libreria):
/// duplicato apposta, perché la prova deve costruire l'identità di corsa
/// esattamente come la costruisce il binario, per scrivere nel deposito
/// esattamente dove il binario andrà a cercare.
fn expected_run_id(task_name: &str, today: &str) -> String {
    format!("notte-{today}-{task_name}")
}

/// Il record del passo "motore" per un tentativo lasciato aperto da un
/// processo morto a metà: la controparte, nel deposito, della vecchia
/// ricevuta orfana in `in-corso/`. `dead_pid` è il pid che
/// `NotteProcessProbe` troverà nell'ingresso del record — nessun processo
/// vero su questa macchina userà mai 999999.
fn write_open_attempt(ledger: &ledger::Ledger, run_id: &str, attempt: u32, dead_pid: u32) {
    let record = flow::StepRecord::started(
        run_id,
        "motore",
        attempt,
        attempt as u64,
        vec![],
        serde_json::json!({ "pid": dead_pid }),
        vec![],
        0,
    );
    ledger.append_step_started(&record).expect("scrivere l'intenzione del tentativo interrotto");
}

/// Un tentativo già chiuso come interrotto: la storia di un'interruzione
/// passata, già recuperata in una notte precedente.
fn write_closed_broke_attempt(ledger: &ledger::Ledger, run_id: &str, attempt: u32) {
    write_open_attempt(ledger, run_id, attempt, 999_999);
    ledger
        .close_step(
            run_id,
            "motore",
            attempt,
            attempt as u64,
            flow::Completion {
                outcome: flow::Outcome::Broke,
                output: None,
                said: None,
                failure_class: Some("process_disappeared".to_string()),
                ended_at: 1,
                bytes_seen: None,
                bytes_discarded: None,
            },
        )
        .expect("chiudere il tentativo interrotto");
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    p.to_string_lossy().to_string()
}

/// Lancia il ciclo con un ambiente isolato. `extra` aggiunge o sovrascrive
/// variabili per una singola prova (soglie, pause, motori finti).
fn run(ws: &Workspace, extra: &[(&str, &str)]) -> std::process::ExitStatus {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_notte"));
    cmd.env("NOTTE_QUEUE_DIR", ws.path("queue"))
        .env("NOTTE_DONE_DIR", ws.path("done"))
        .env("NOTTE_ALERTS_DIR", ws.path("alerts"))
        .env("NOTTE_STATE_DIR", ws.path("state"))
        .env("NOTTE_IN_PROGRESS_DIR", ws.path("in-corso"))
        .env("NOTTE_LOCK_PATH", ws.path("state").join("notte.lock"))
        .env("NOTTE_DATE_OVERRIDE", "2026-08-25")
        .env("NOTTE_MAX_FAILURES", "3")
        .env("NOTTE_OPENROUTER_PAUSE", "0")
        .env("NOTTE_MAX_PROMPT_BYTES", "8000")
        // Senza questa riga il ponte cade sul percorso predefinito, che è il
        // deposito VERO: il 27/08/2026 una batteria ha scritto 55 esecuzioni
        // finte nella pagina di controllo, con token di laboratorio, e chi la
        // guardava leggeva una giornata di lavoro che non era mai successa.
        // Ogni altra variabile qui sopra isola già la prova: questa mancava
        // perché il ponte è arrivato dopo.
        .env("NOTTE_LEDGER_DIR", ws.path("state").join("flussi"))
        .env("NOTTE_OPENROUTER_FETCH", fixture("fixture-openrouter-ok.test.sh"));
    for (k, v) in extra {
        cmd.env(k, v);
    }
    cmd.status().expect("il binario deve poter partire")
}

/// Lancia il ciclo residente per un numero fisso di giri, con la macchina
/// finta ferma e dentro la finestra di notte: serve a provare ciò che
/// dipende dal «un compito per giro», invisibile in modalità `--once`.
fn run_watch(ws: &Workspace, ticks: u32, extra: &[(&str, &str)]) -> std::process::ExitStatus {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_notte"));
    cmd.arg("--watch")
        .env("NOTTE_QUEUE_DIR", ws.path("queue"))
        .env("NOTTE_DONE_DIR", ws.path("done"))
        .env("NOTTE_ALERTS_DIR", ws.path("alerts"))
        .env("NOTTE_STATE_DIR", ws.path("state"))
        .env("NOTTE_IN_PROGRESS_DIR", ws.path("in-corso"))
        .env("NOTTE_LOCK_PATH", ws.path("state").join("notte.lock"))
        .env("NOTTE_DATE_OVERRIDE", "2026-08-25")
        .env("NOTTE_MAX_FAILURES", "3")
        .env("NOTTE_OPENROUTER_PAUSE", "0")
        .env("NOTTE_MAX_PROMPT_BYTES", "8000")
        .env("NOTTE_OPENROUTER_FETCH", fixture("fixture-openrouter-ok.test.sh"))
        // Stessa ragione della gemella `run`: senza, il ciclo residente scrive
        // nel deposito vero.
        .env("NOTTE_LEDGER_DIR", ws.path("state").join("flussi"))
        .env("NOTTE_WATCH_MAX_TICKS", ticks.to_string())
        .env("NOTTE_WATCH_INTERVAL_SECS", "0")
        .env("NOTTE_IDLE_SECONDS_OVERRIDE", "99999")
        .env("NOTTE_LOAD1_OVERRIDE", "0.1")
        .env("NOTTE_MEM_FREE_PERCENT_OVERRIDE", "90")
        .env("NOTTE_CORE_COUNT_OVERRIDE", "8")
        .env("NOTTE_HOUR_OVERRIDE", "3");
    for (k, v) in extra {
        cmd.env(k, v);
    }
    cmd.status().expect("il binario deve poter partire")
}

fn report_text(ws: &Workspace) -> String {
    fs::read_to_string(ws.path("state").join("rapporto-2026-08-25.md")).unwrap_or_default()
}

fn done_names(ws: &Workspace) -> Vec<String> {
    fs::read_dir(ws.path("done"))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect()
}

fn queue_names(ws: &Workspace) -> Vec<String> {
    fs::read_dir(ws.path("queue"))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect()
}

fn alert_files(ws: &Workspace) -> Vec<String> {
    fs::read_dir(ws.path("alerts"))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect()
}

fn in_progress_names(ws: &Workspace) -> Vec<String> {
    fs::read_dir(ws.path("in-corso"))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect()
}

/// Il caso di base: un compito che passa la sua verifica finisce verde, si
/// sposta in `done/` con lo stato scritto, e non apre nessuna segnalazione.
#[test]
fn a_matching_check_turns_green() {
    let ws = Workspace::new("green");
    ws.write_task("2026-08-25-a.task", "openrouter", "una domanda", "grep -q 'answer: 42' \"$NOTTE_OUTPUT_FILE\"");
    let status = run(&ws, &[]);
    assert!(status.success());
    let report = report_text(&ws);
    assert!(report.contains("VERDE"), "{report}");
    assert_eq!(done_names(&ws), vec!["2026-08-25-a.task"]);
    assert!(queue_names(&ws).is_empty());
    assert!(alert_files(&ws).is_empty(), "un verde non apre segnalazioni");
    let moved = fs::read_to_string(ws.path("done").join("2026-08-25-a.task")).unwrap();
    assert!(moved.contains("notte-status: green"));
}

/// IL BRACCIO CHE CONTA: rompo apposta la verifica (cerco una risposta che
/// il motore non ha dato) e controllo che diventi rosso **e** che nasca una
/// segnalazione — non solo che il programma non vada in crash.
///
/// MUTANTE: se `run_check` tornasse sempre `true`, questo braccio resta
/// verde anche qui, e la battuta d'arresto non lo vedrebbe.
#[test]
fn a_broken_check_turns_red_and_writes_an_alert() {
    let ws = Workspace::new("red-check");
    ws.write_task(
        "2026-08-25-b.task",
        "openrouter",
        "una domanda",
        "grep -q 'answer: 99' \"$NOTTE_OUTPUT_FILE\"",
    );
    let status = run(&ws, &[]);
    assert!(
        status.success(),
        "un rosso solo non supera la soglia di fallimenti consecutivi: il processo esce comunque a zero"
    );
    let report = report_text(&ws);
    assert!(report.contains("ROSSO (verifica fallita)"), "{report}");
    let alerts = alert_files(&ws);
    assert_eq!(alerts.len(), 1, "un rosso apre esattamente una segnalazione: {alerts:?}");
    let alert = fs::read_to_string(ws.path("alerts").join(&alerts[0])).unwrap();
    assert!(alert.contains("la verifica non ha confermato"), "{alert}");
}

/// Il motore stesso può rispondere male (429): anche questo è rosso, non un
/// caso a parte, e non si ritenta.
#[test]
fn an_engine_429_is_red_too() {
    let ws = Workspace::new("429");
    ws.write_task("2026-08-25-c.task", "openrouter", "una domanda", "true");
    let status = run(&ws, &[("NOTTE_OPENROUTER_FETCH", &fixture("fixture-openrouter-429.test.sh"))]);
    assert!(status.success());
    let report = report_text(&ws);
    assert!(report.contains("ROSSO (motore: 429)"), "{report}");
}

/// `ollama` è vivo ma senza generalista: rimandato, non rosso, e non tocca
/// il contatore dei fallimenti consecutivi.
#[test]
fn an_ollama_task_is_deferred_not_failed() {
    let ws = Workspace::new("ollama");
    ws.write_task("2026-08-25-d.task", "ollama", "una domanda", "true");
    let status = run(&ws, &[]);
    assert!(status.success());
    let report = report_text(&ws);
    assert!(report.contains("RIMANDATO"), "{report}");
    // Si cerca la forma esatta con cui un vero rosso compare nel rapporto,
    // non la parola sciolta: finché era in inglese, «DEFERRED» conteneva
    // «RED» e la prova sarebbe passata per un motivo sbagliato.
    assert!(!report.contains("ROSSO ("), "{report}");
    let moved = fs::read_to_string(ws.path("done").join("2026-08-25-d.task")).unwrap();
    assert!(moved.contains("notte-status: rimandato"), "{moved}");
}

/// La sola esclusione che resta: un prompt che cita una credenziale non esce
/// di casa, qualunque sia il motore o il perimetro dichiarato.
#[test]
fn a_credential_in_the_prompt_is_skipped_and_flagged() {
    let ws = Workspace::new("secret");
    ws.write_task(
        "2026-08-25-e.task",
        "openrouter",
        "usa la chiave in ~/.claude/state/openrouter.key per firmare",
        "true",
    );
    let status = run(&ws, &[]);
    assert!(status.success());
    let report = report_text(&ws);
    assert!(report.contains("SALTATO: l'istruzione nomina una credenziale"), "{report}");
    assert_eq!(alert_files(&ws).len(), 1);
}

/// Il tetto sta sul prompt, non sul motore: un prompt troppo grande viene
/// saltato con una riga che dice quanti byte e quale tetto.
#[test]
fn a_prompt_over_the_byte_cap_is_skipped() {
    let ws = Workspace::new("cap");
    let long_prompt = "x".repeat(50);
    ws.write_task("2026-08-25-f.task", "openrouter", &long_prompt, "true");
    let status = run(&ws, &[("NOTTE_MAX_PROMPT_BYTES", "10")]);
    assert!(status.success());
    let report = report_text(&ws);
    assert!(report.contains("SALTATO: l'istruzione è"), "{report}");
    assert!(report.contains("oltre il tetto di 10 byte"), "{report}");
}

/// Codex come motore: stesso ciclo, un binario diverso, lo stesso giudizio.
#[test]
fn a_codex_task_can_turn_green() {
    let ws = Workspace::new("codex-green");
    ws.write_task("2026-08-25-g.task", "codex", "conta qualcosa", "grep -q 'answer: 7' \"$NOTTE_OUTPUT_FILE\"");
    let status = run(&ws, &[("NOTTE_CODEX_BIN", &fixture("fixture-codex-ok.test.sh"))]);
    assert!(status.success());
    let report = report_text(&ws);
    assert!(report.contains("codex"), "{report}");
    assert!(report.contains("VERDE"), "{report}");
    assert!(report.contains("1234 token"), "{report}");
}

/// Un codex che esce in errore è rosso come qualunque altro motore.
#[test]
fn a_failing_codex_binary_is_red() {
    let ws = Workspace::new("codex-red");
    ws.write_task("2026-08-25-h.task", "codex", "conta qualcosa", "true");
    let status = run(&ws, &[("NOTTE_CODEX_BIN", &fixture("fixture-codex-fail.test.sh"))]);
    assert!(status.success());
    let report = report_text(&ws);
    assert!(report.contains("ROSSO (motore: errore)"), "{report}");
}

/// LA BATTUTA D'ARRESTO: con un tetto di due fallimenti consecutivi, il
/// terzo compito non viene nemmeno toccato — resta in coda, non in `done/`.
#[test]
fn the_cycle_stops_after_consecutive_failures_and_leaves_the_rest_queued() {
    let ws = Workspace::new("stop");
    ws.write_task("2026-08-25-i1.task", "openrouter", "x", "false");
    ws.write_task("2026-08-25-i2.task", "openrouter", "x", "false");
    ws.write_task("2026-08-25-i3.task", "openrouter", "x", "true");
    let status = run(&ws, &[("NOTTE_MAX_FAILURES", "2")]);
    assert!(!status.success(), "un'uscita fermata anzitempo non è zero");
    let report = report_text(&ws);
    assert!(report.contains("Fermato dopo 2 fallimenti di fila"), "{report}");
    let done = done_names(&ws);
    assert_eq!(done.len(), 2, "solo i due che hanno fallito sono stati toccati: {done:?}");
    assert_eq!(queue_names(&ws), vec!["2026-08-25-i3.task"], "il terzo resta in coda, intoccato");
}

/// La pausa vale per ogni chiamata OpenRouter: due compiti con un secondo di
/// pausa impiegano almeno un secondo in più del tempo di una chiamata sola.
#[test]
fn the_openrouter_pause_is_actually_slept() {
    let ws = Workspace::new("pause");
    ws.write_task("2026-08-25-j1.task", "openrouter", "x", "true");
    ws.write_task("2026-08-25-j2.task", "openrouter", "x", "true");
    let start = Instant::now();
    let status = run(&ws, &[("NOTTE_OPENROUTER_PAUSE", "1")]);
    let elapsed = start.elapsed();
    assert!(status.success());
    assert!(
        elapsed >= Duration::from_secs(1),
        "due chiamate con pausa di 1s devono impiegare almeno 1s, non {elapsed:?}"
    );
}

/// Un compito senza uno dei quattro campi non manda in crash il ciclo: viene
/// saltato e non conta come motore sconosciuto.
#[test]
fn a_task_missing_a_field_is_skipped_not_crashed() {
    let ws = Workspace::new("malformed");
    let path = ws.path("queue").join("2026-08-25-k.task");
    fs::write(&path, "motore: openrouter\n---prompt---\nsolo il prompt, niente verifica\n").unwrap();
    let status = run(&ws, &[]);
    assert!(status.success());
    let report = report_text(&ws);
    assert!(report.contains("SALTATO: campi mancanti"), "{report}");
}

// ── difetto 3: la scadenza sul motore e sulla verifica ──────────────────

/// LA VERIFICA CHE NON TORNA MAI: prima restava impiantata per sempre (solo
/// `curl` aveva un tetto). Ora `run_check` la ammazza da sola e il compito
/// finisce rosso — non impiccato in coda.
#[test]
fn a_hanging_check_times_out_and_is_moved_to_done() {
    let ws = Workspace::new("check-timeout");
    ws.write_task("2026-08-25-l.task", "openrouter", "una domanda", "sleep 100000");
    let status = run(&ws, &[("NOTTE_CHECK_TIMEOUT_SECS", "1")]);
    assert!(status.success());
    let report = report_text(&ws);
    assert!(report.contains("ROSSO (verifica: timeout dopo 1s)"), "{report}");
    assert_eq!(done_names(&ws), vec!["2026-08-25-l.task"], "non deve restare impiantato in coda");
    assert!(queue_names(&ws).is_empty());
    assert!(in_progress_names(&ws).is_empty(), "la ricevuta deve essere stata smaltita");
    let moved = fs::read_to_string(ws.path("done").join("2026-08-25-l.task")).unwrap();
    assert!(moved.contains("notte-status: red (timeout verifica)"), "{moved}");
}

/// LO STESSO PER IL MOTORE: un `codex` che non torna è rosso per timeout,
/// non un compito perso per sempre.
#[test]
fn a_hanging_engine_times_out_and_is_moved_to_done() {
    let ws = Workspace::new("engine-timeout");
    ws.write_task("2026-08-25-n.task", "codex", "conta qualcosa", "true");
    let status = run(
        &ws,
        &[("NOTTE_CODEX_BIN", &fixture("fixture-codex-hang.test.sh")), ("NOTTE_CODEX_TIMEOUT_SECS", "1")],
    );
    assert!(status.success());
    let report = report_text(&ws);
    assert!(report.contains("ROSSO (motore: timeout)"), "{report}");
    assert_eq!(done_names(&ws), vec!["2026-08-25-n.task"]);
    assert!(queue_names(&ws).is_empty());
    assert!(in_progress_names(&ws).is_empty());
}

// ── difetto 2: il lucchetto sull'intero giro ─────────────────────────────

/// Un lucchetto tenuto da un pid vivo (qui, il processo di prova stesso)
/// ferma la seconda istanza subito: esce a zero, senza toccare la coda.
#[test]
fn a_lock_held_by_a_live_pid_makes_the_run_exit_cleanly_without_touching_the_queue() {
    let ws = Workspace::new("lock-held");
    ws.write_task("2026-08-25-o.task", "openrouter", "x", "true");
    let lock_path = ws.path("state").join("notte.lock");
    fs::write(&lock_path, format!("{}\n", std::process::id())).unwrap();
    let status = run(&ws, &[]);
    assert!(status.success(), "il lucchetto preso non è un errore, si esce puliti");
    assert_eq!(queue_names(&ws), vec!["2026-08-25-o.task"], "il compito non è mai stato toccato");
    assert!(done_names(&ws).is_empty());
    // Il lucchetto altrui non si tocca: resta lì per chi lo tiene davvero.
    assert!(lock_path.exists());
}

/// Un lucchetto di un pid morto (999_999: nessun processo reale su questa
/// macchina lo userà mai) non blocca la raccolta per sempre — si scavalca.
#[test]
fn a_stale_lock_from_a_dead_pid_is_taken_over() {
    let ws = Workspace::new("lock-stale");
    ws.write_task("2026-08-25-p.task", "openrouter", "x", "true");
    fs::write(ws.path("state").join("notte.lock"), "999999\n").unwrap();
    let status = run(&ws, &[]);
    assert!(status.success());
    assert_eq!(done_names(&ws), vec!["2026-08-25-p.task"], "il compito è stato eseguito davvero");
}

// ── difetto 4: la ricevuta prima del lavoro ──────────────────────────────

/// Una ricevuta orfana (pid morto) viene recuperata al riavvio: la
/// riconciliazione del motore chiude il tentativo interrotto come "Broke",
/// il compito rientra in coda e viene rieseguito — un secondo tentativo per
/// lo stesso passo "motore" — nello stesso giro.
///
/// IL BRACCIO CHE CONTA, sostituisce la vecchia `tentativi: 1` nel testo del
/// compito: qui il numero di tentativi si legge nel deposito, non nel file.
#[test]
fn an_orphaned_receipt_is_retried_and_the_ledger_shows_two_attempts() {
    let ws = Workspace::new("receipt-recovered");
    ws.write_receipt("2026-08-25-q.task", 999_999, "openrouter", "una domanda", "grep -q 'answer: 42' \"$NOTTE_OUTPUT_FILE\"");
    let ledger_dir = ws.path("state").join("flussi");
    let run_id = expected_run_id("2026-08-25-q.task", "2026-08-25");
    {
        let ledger = ledger::Ledger::open(&ledger_dir).expect("il deposito di prova deve aprirsi");
        write_open_attempt(&ledger, &run_id, 1, 999_999);
    }

    let status = run(&ws, &[("NOTTE_LEDGER_DIR", ledger_dir.to_str().unwrap())]);
    assert!(status.success());
    assert!(in_progress_names(&ws).is_empty(), "la ricevuta orfana non deve restare lì");
    assert_eq!(done_names(&ws), vec!["2026-08-25-q.task"], "recuperata, rimessa in coda, eseguita nello stesso giro");
    let moved = fs::read_to_string(ws.path("done").join("2026-08-25-q.task")).unwrap();
    assert!(moved.contains("notte-status: green"), "{moved}");
    assert!(!moved.contains("tentativi:"), "il contatore non vive più nel testo del compito: {moved}");

    let ledger = ledger::Ledger::open(&ledger_dir).expect("il deposito deve rileggersi");
    let steps = ledger.steps(&run_id).expect("i passi devono leggersi");
    assert_eq!(steps.len(), 2, "un tentativo interrotto più uno riuscito: {steps:?}");
    assert_eq!(steps[0].attempt, 1);
    assert_eq!(steps[0].outcome, Some(flow::Outcome::Broke), "{steps:?}");
    assert_eq!(steps[0].failure_class.as_deref(), Some("process_disappeared"), "{steps:?}");
    assert_eq!(steps[1].attempt, 2);
    assert_eq!(steps[1].outcome, Some(flow::Outcome::Went), "{steps:?}");
}

/// LA TRAPPOLA VERA: un compito già interrotto due volte (due tentativi già
/// chiusi come "Broke" nel deposito) e interrotto una terza — pid morto,
/// ricevuta orfana — non si ritenta più: va dritto in `fatti/` come rosso
/// avvelenato, senza che il motore chiami mai `NotteTaskAction::execute`.
///
/// MUTANTE: se il controllo del grafo fosse `attempt > max_attempts` invece
/// di `>=`, un compito interrotto tre volte verrebbe ritentato una quarta —
/// questa prova lo becca solo se il terzo tentativo resta "Broke" e non
/// lascia mai un'uscita (`TaskStepOutput`) nel deposito.
#[test]
fn a_third_interrupted_attempt_is_poisoned_without_calling_the_engine() {
    let ws = Workspace::new("receipt-poisoned");
    ws.write_receipt("2026-08-25-r.task", 999_999, "openrouter", "una domanda", "true");
    let ledger_dir = ws.path("state").join("flussi");
    let run_id = expected_run_id("2026-08-25-r.task", "2026-08-25");
    {
        let ledger = ledger::Ledger::open(&ledger_dir).expect("il deposito di prova deve aprirsi");
        write_closed_broke_attempt(&ledger, &run_id, 1);
        write_closed_broke_attempt(&ledger, &run_id, 2);
        write_open_attempt(&ledger, &run_id, 3, 999_999);
    }

    let status = run(&ws, &[("NOTTE_LEDGER_DIR", ledger_dir.to_str().unwrap())]);
    assert!(status.success());
    assert!(in_progress_names(&ws).is_empty());
    assert!(queue_names(&ws).is_empty(), "un avvelenato non torna in coda");
    assert_eq!(done_names(&ws), vec!["2026-08-25-r.task"]);
    let moved = fs::read_to_string(ws.path("done").join("2026-08-25-r.task")).unwrap();
    assert!(moved.contains("notte-status: red (avvelenato)"), "{moved}");
    assert!(!moved.contains("tentativi:"), "{moved}");
    let alerts = alert_files(&ws);
    assert_eq!(alerts.len(), 1, "{alerts:?}");
    let alert = fs::read_to_string(ws.path("alerts").join(&alerts[0])).unwrap();
    assert!(alert.contains("avvelenato"), "{alert}");

    let ledger = ledger::Ledger::open(&ledger_dir).expect("il deposito deve rileggersi");
    let steps = ledger.steps(&run_id).expect("i passi devono leggersi");
    assert_eq!(steps.len(), 3, "nessun quarto tentativo: {steps:?}");
    assert!(
        steps.iter().all(|s| s.outcome == Some(flow::Outcome::Broke)),
        "il motore non deve essere mai stato chiamato: {steps:?}"
    );
}

// ── i compiti che si ripetono ──────────────────────────────────────────
//
// LA MISURA CHE LI HA RESI NECESSARI. Nella notte fra il 25 e il 26/08/2026
// la macchina è stata sveglia sei ore e la coda si è svuotata in tre minuti:
// ogni compito, una volta eseguito, finiva in archivio e non tornava.

/// Una sentinella verde deve tornare in coda, timbrata con la data di oggi,
/// e lasciare in archivio l'esito di stanotte sotto un nome che porta la
/// data — così le notti non si sovrascrivono a vicenda.
#[test]
fn a_recurring_task_goes_back_to_the_queue() {
    let ws = Workspace::new("ricorrente-verde");
    let text = "motore: openrouter\nricorrenza: ogni-notte\n---prompt---\nuna domanda\n---verifica---\ngrep -q 'answer: 42' \"$NOTTE_OUTPUT_FILE\"\n";
    fs::write(ws.path("queue").join("sentinella.task"), text).unwrap();

    let status = run(&ws, &[]);
    assert!(status.success());
    assert!(report_text(&ws).contains("VERDE"), "{}", report_text(&ws));

    assert_eq!(queue_names(&ws), vec!["sentinella.task"], "deve essere tornata in coda");
    let back = fs::read_to_string(ws.path("queue").join("sentinella.task")).unwrap();
    assert!(back.contains("ultima-esecuzione: 2026-08-25"), "manca il timbro: {back}");

    let done = done_names(&ws);
    assert_eq!(done, vec!["sentinella-2026-08-25.task"], "l'esito va in archivio con la data: {done:?}");
    assert!(in_progress_names(&ws).is_empty(), "la ricevuta deve essere stata smaltita");
}

/// Anche rossa deve tornare in coda: una sentinella spenta al primo rosso
/// smette di guardare proprio quando c'è qualcosa da guardare.
#[test]
fn a_recurring_task_comes_back_even_when_red() {
    let ws = Workspace::new("ricorrente-rosso");
    let text = "motore: openrouter\nricorrenza: ogni-notte\n---prompt---\nuna domanda\n---verifica---\nfalse\n";
    fs::write(ws.path("queue").join("sentinella.task"), text).unwrap();

    let status = run(&ws, &[]);
    assert!(status.success());
    assert!(report_text(&ws).contains("ROSSO ("), "{}", report_text(&ws));
    assert_eq!(queue_names(&ws), vec!["sentinella.task"], "una rossa deve tornare lo stesso");
    assert_eq!(alert_files(&ws).len(), 1, "e deve aver aperto la sua segnalazione");
}

/// Il freno contro le sei esecuzioni per notte, provato **dove il difetto
/// vive**: nel ciclo residente, che prende un compito per giro.
///
/// Con un giro solo a disposizione, una sentinella già timbrata oggi e in
/// testa all'ordine alfabetico non deve rubarlo: se il filtro non escludesse
/// i ricorrenti già fatti, il giro finirebbe su di lei e il compito dietro
/// non partirebbe mai. In modalità «un giro solo» il difetto non si vede,
/// perché lì la coda viene percorsa tutta.
#[test]
fn a_recurring_task_already_done_today_does_not_steal_the_tick() {
    let ws = Workspace::new("ricorrente-gia-fatta");
    let done_today = "motore: openrouter\nricorrenza: ogni-notte\nultima-esecuzione: 2026-08-25\n---prompt---\nuna domanda\n---verifica---\ntrue\n";
    fs::write(ws.path("queue").join("a-sentinella.task"), done_today).unwrap();
    ws.write_task("b-normale.task", "openrouter", "una domanda", "grep -q 'answer: 42' \"$NOTTE_OUTPUT_FILE\"");

    let status = run_watch(&ws, 1, &[]);
    assert!(status.success());

    let report = report_text(&ws);
    assert!(report.contains("b-normale.task"), "il compito dietro doveva prendersi il giro: {report}");
    assert!(!report.contains("a-sentinella.task"), "quella già fatta non doveva comparire: {report}");
    assert_eq!(
        queue_names(&ws),
        vec!["a-sentinella.task"],
        "la sentinella resta in coda intatta, il normale se n'è andato"
    );
    let untouched = fs::read_to_string(ws.path("queue").join("a-sentinella.task")).unwrap();
    assert!(!untouched.contains("notte-status:"), "non doveva essere toccata: {untouched}");
}

/// IL DIFETTO CHE QUESTA PROVA TIENE CHIUSO, misurato sul servizio vivo il
/// 27/08/2026: il peso del giro si leggeva dalla **sola testa** della coda
/// ordinata, e diventava il peso di tutto il giro. La coda reale aveva dieci
/// lavorazioni leggere su dodici, ma in testa stava una pesante — per ordine
/// alfabetico, non per scelta di nessuno. Il giro veniva quindi giudicato
/// pesante, la soglia di carico si stringeva a quella dei pesanti, e con la
/// macchina al lavoro **le dieci leggere dietro non partivano mai**.
///
/// È il difetto che annulla il peso proprio nel caso per cui il peso esiste:
/// far passare il lavoro piccolo mentre la macchina è occupata.
///
/// La prova mette il carico a 5, che sta **sopra** la soglia dei pesanti e
/// **sotto** quella delle leggere: senza la correzione il giro salta e la coda
/// resta intatta.
#[test]
fn a_heavy_task_at_the_head_does_not_hold_back_the_light_ones_behind_it() {
    let ws = Workspace::new("pesante-in-testa");
    let heavy = "motore: openrouter\npeso: pesante\n---prompt---\nuna domanda\n---verifica---\ntrue\n";
    fs::write(ws.path("queue").join("a-pesante.task"), heavy).unwrap();
    let light = "motore: openrouter\npeso: leggero\n---prompt---\nuna domanda\n---verifica---\ngrep -q 'answer: 42' \"$NOTTE_OUTPUT_FILE\"\n";
    fs::write(ws.path("queue").join("b-leggera.task"), light).unwrap();

    // Carico 5.0 su 8 core: oltre il tetto dei pesanti, entro quello delle
    // leggere. La macchina non è ferma, che è la condizione di tutti i giorni.
    let status = run_watch(
        &ws,
        1,
        &[
            ("NOTTE_LOAD1_OVERRIDE", "5.0"),
            ("NOTTE_IDLE_SECONDS_OVERRIDE", "0"),
        ],
    );
    assert!(status.success());

    let report = report_text(&ws);
    assert!(
        report.contains("b-leggera.task"),
        "la leggera dietro doveva partire malgrado la pesante in testa; \
         col difetto il giro è giudicato pesante e salta. Rapporto: {report:?}"
    );
    assert!(
        !report.contains("a-pesante.task"),
        "la pesante non doveva partire con la macchina a questo carico: {report}"
    );
    assert_eq!(
        queue_names(&ws),
        vec!["a-pesante.task"],
        "la pesante resta in coda, la leggera se n'è andata"
    );
}

/// Un orologio finto che passa la mezzanotte fra l'avvio e il primo giro.
///
/// `shell_date` invoca `date` **senza percorso assoluto**, quindi rispetta il
/// `PATH`: è questo che rende il tempo iniettabile senza toccare il codice di
/// produzione. La prima richiesta di una data nuda risponde col giorno prima —
/// è quella che il processo fa nascendo, per fissare `Config::today` — e da lì
/// in poi risponde col giorno dopo. Ogni altra forma (l'ora col fuso, che il
/// registro stampa a ogni riga) passa al `date` vero.
///
/// Senza questo, il difetto è **invisibile per costruzione**: in una prova il
/// processo nasce e muore nello stesso istante, quindi la data dell'avvio e
/// quella di adesso coincidono sempre.
fn install_clock_that_crosses_midnight(ws: &Workspace) -> PathBuf {
    let bin = ws.path("bin");
    fs::create_dir_all(&bin).unwrap();
    let counter = ws.path("state").join("date-calls");
    let script = format!(
        "#!/bin/sh\n\
         for a in \"$@\"; do\n\
         \x20 if [ \"$a\" = \"+%Y-%m-%d\" ]; then\n\
         \x20   if [ -f {c} ]; then echo 2026-08-27; else : > {c}; echo 2026-08-26; fi\n\
         \x20   exit 0\n\
         \x20 fi\n\
         done\n\
         exec /bin/date \"$@\"\n",
        c = counter.display()
    );
    let date_path = bin.join("date");
    fs::write(&date_path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&date_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
}

/// IL DIFETTO CHE QUESTA PROVA TIENE CHIUSO, misurato sul servizio vivo il
/// 27/08/2026: il ciclo residente filtrava i ricorrenti con la data **dell'avvio**
/// invece che con quella di adesso. Dal giorno dopo l'avvio, ogni lavorazione
/// portava `ultima-esecuzione` uguale a quella data ferma, quindi tutte
/// risultavano già fatte e la coda si dichiarava vuota **per sempre**: il
/// registro portava «salto: coda vuota (x340)» con dodici lavorazioni dentro, e
/// per quel giorno non esisteva nessun rapporto.
///
/// Il servizio sembrava così attivo solo di notte: ogni riavvio gli ridava
/// esattamente un giorno di lavoro, poi taceva fino al successivo.
///
/// La prova rompe apposta il presupposto — l'orologio avanza di un giorno fra
/// l'avvio e il primo giro — e pretende che la lavorazione di ieri venga ripresa.
#[test]
fn a_resident_cycle_picks_up_yesterdays_recurring_task_after_midnight() {
    let ws = Workspace::new("mezzanotte-ricorrenti");
    let done_yesterday = "motore: openrouter\nricorrenza: ogni-notte\nultima-esecuzione: 2026-08-26\n---prompt---\nuna domanda\n---verifica---\ngrep -q 'answer: 42' \"$NOTTE_OUTPUT_FILE\"\n";
    fs::write(ws.path("queue").join("sentinella.task"), done_yesterday).unwrap();

    let bin = install_clock_that_crosses_midnight(&ws);
    let path_with_fake_date = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    // Niente `NOTTE_DATE_OVERRIDE`: qui la data deve venire dall'orologio, che
    // è il pezzo sotto esame. Con l'override la prova resterebbe verde anche
    // col difetto dentro, perché la data non cambierebbe mai.
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_notte"));
    cmd.arg("--watch")
        .env("PATH", &path_with_fake_date)
        .env("NOTTE_QUEUE_DIR", ws.path("queue"))
        .env("NOTTE_DONE_DIR", ws.path("done"))
        .env("NOTTE_ALERTS_DIR", ws.path("alerts"))
        .env("NOTTE_STATE_DIR", ws.path("state"))
        .env("NOTTE_IN_PROGRESS_DIR", ws.path("in-corso"))
        .env("NOTTE_LOCK_PATH", ws.path("state").join("notte.lock"))
        .env("NOTTE_MAX_FAILURES", "3")
        .env("NOTTE_OPENROUTER_PAUSE", "0")
        .env("NOTTE_MAX_PROMPT_BYTES", "8000")
        .env("NOTTE_OPENROUTER_FETCH", fixture("fixture-openrouter-ok.test.sh"))
        // Questa prova costruisce l'ambiente a mano invece di passare da `run`,
        // e per questo era l'ultima a scrivere ancora nel deposito vero dopo la
        // riparazione delle altre due: l'unica esecuzione sopravvissuta si
        // chiamava `sentinella.task`, che è il compito qui sotto.
        .env("NOTTE_LEDGER_DIR", ws.path("state").join("flussi"))
        .env("NOTTE_WATCH_MAX_TICKS", "1")
        .env("NOTTE_WATCH_INTERVAL_SECS", "0")
        .env("NOTTE_IDLE_SECONDS_OVERRIDE", "99999")
        .env("NOTTE_LOAD1_OVERRIDE", "0.1")
        .env("NOTTE_MEM_FREE_PERCENT_OVERRIDE", "90")
        .env("NOTTE_CORE_COUNT_OVERRIDE", "8")
        .env("NOTTE_HOUR_OVERRIDE", "3");
    let status = cmd.status().expect("il binario deve poter partire");
    assert!(status.success());

    // Il rapporto porta la data di **oggi**, non quella dell'avvio.
    let report = fs::read_to_string(ws.path("state").join("rapporto-2026-08-27.md"))
        .unwrap_or_default();
    assert!(
        report.contains("sentinella.task"),
        "la ricorrente di ieri doveva essere ripresa dopo la mezzanotte; \
         col difetto la coda risulta vuota e il rapporto non esiste. Rapporto: {report:?}"
    );
}

// ── il ponte verso il deposito durevole (`ledger_bridge`) ───────────────

/// Il caso di base: un compito verde finisce anche nel deposito, con la
/// stessa intenzione (`run`, `step`) e lo stesso esito che il rapporto di
/// testo racconta — il deposito è un secondo osservatore, non un sostituto.
#[test]
fn a_green_task_is_recorded_in_the_ledger_too() {
    let ws = Workspace::new("ledger-green");
    ws.write_task("2026-08-25-u.task", "openrouter", "una domanda", "grep -q 'answer: 42' \"$NOTTE_OUTPUT_FILE\"");
    let ledger_dir = ws.path("state").join("flussi");
    let status = run(&ws, &[("NOTTE_LEDGER_DIR", ledger_dir.to_str().unwrap())]);
    assert!(status.success());
    assert_eq!(done_names(&ws), vec!["2026-08-25-u.task"]);

    let ledger = ledger::Ledger::open(&ledger_dir).expect("il deposito deve essersi aperto davvero");
    let dump = ledger.projection_dump().expect("il dump deve leggersi");
    let runs = dump["runs"].as_array().expect("colonna runs");
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0][5].as_str(), Some("green"), "colonna status: {runs:?}");
    let run_id = runs[0][0].as_str().expect("run_id").to_string();
    let steps = ledger.steps(&run_id).expect("passi leggibili");
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert_eq!(steps[0].outcome, Some(flow::Outcome::Went));
    let calls = dump["model_calls"].as_array().expect("colonna model_calls");
    assert_eq!(calls.len(), 1, "il token noto doveva finire in una riga: {calls:?}");
}

/// IL BRACCIO CHE CONTA: un file al posto della cartella del deposito fa
/// fallire `Ledger::open` a ogni compito, eppure il ciclo deve arrivare in
/// fondo come se il ponte non esistesse — verde, spostato in `done/`,
/// nessun blocco. Un deposito rotto non è mai un guasto della lavorazione.
///
/// MUTANTE PROVATO A MANO: sostituendo temporaneamente `.ok()` con
/// `.expect(...)` in `LedgerHandle::begin_in`, questa prova va in panico
/// (rosso); ripristinato l'originale torna verde. Il file è tornato
/// identico — vedi il rapporto di consegna per l'impronta prima/dopo.
#[test]
fn a_broken_ledger_directory_does_not_stop_a_green_task() {
    let ws = Workspace::new("ledger-broken");
    ws.write_task("2026-08-25-v.task", "openrouter", "una domanda", "grep -q 'answer: 42' \"$NOTTE_OUTPUT_FILE\"");
    let blocked = ws.path("state").join("flussi-rotta");
    fs::write(&blocked, "non è una cartella, il deposito non potrà aprirla").unwrap();

    let status = run(&ws, &[("NOTTE_LEDGER_DIR", blocked.to_str().unwrap())]);
    assert!(status.success(), "un deposito rotto non deve fermare il ciclo");
    let report = report_text(&ws);
    assert!(report.contains("VERDE"), "{report}");
    assert_eq!(done_names(&ws), vec!["2026-08-25-v.task"]);
    assert!(alert_files(&ws).is_empty(), "un verde non apre segnalazioni, deposito rotto o no");
}
