//! `sailor-live` — la modalità viva che non fa sparire la finestra.
//!
//! **PRENDE IL POSTO DI `cargo tauri dev`**, e per una ragione sola: quel
//! comando ferma il programma acceso **prima** di ricompilare
//! (`tauri-cli 2.11.4`, `src/interface/rust.rs`, `run_dev_watcher`), quindi ogni
//! file toccato spegne la finestra e una compilazione fallita è solo il motivo
//! per cui non ne ritorna una. Qui l'ordine è rovesciato — si costruisce, e la
//! finestra si sostituisce **solo se** la costruzione è riuscita — e quando
//! fallisce lo si dice, invece di lasciare uno schermo vuoto.
//!
//! E ogni processo che accende passa dal deposito, che è la riparazione del
//! guasto 4: chi arriva domani e trova la porta occupata ha un posto dove
//! chiedere di chi è.

use std::path::{Path, PathBuf};
use std::time::Duration;

use supervisor::child::{cargo_build, newest_change, Process, Spec};
use supervisor::{
    close_the_ones_that_stopped_breathing, left_running, now, rebuild_then_swap, LiveState,
    LiveStatus, Rebuild,
};

use supervisor::DEV_PORT;

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let root = repository_root(&arguments);

    let store = open_ledger();
    if store.is_none() {
        eprintln!(
            "avviso: nessun deposito. I processi accesi non verranno registrati, \
             e un orfano di stanotte domani non avrà un padrone."
        );
    }

    match arguments.first().map(String::as_str) {
        Some("--list") => list_left_running(store.as_ref()),
        Some("--stop") => stop_left_running(store.as_ref()),
        _ => run_live(&root, store.as_ref()),
    }
}

/// La radice del repository su cui lavorare.
///
/// **Viene da chi lancia, mai scritta dentro.** È la lezione del guasto 25: un
/// percorso assoluto dentro il programma lo rende eseguibile in un posto solo, e
/// lanciato da un clone lavora — e commette — nel repository principale.
fn repository_root(arguments: &[String]) -> PathBuf {
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        if argument == "--root" {
            if let Some(path) = arguments.next() {
                return PathBuf::from(path);
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn open_ledger() -> Option<ledger::Ledger> {
    let directory = ledger::default_directory()?;
    match ledger::Ledger::open(&directory) {
        Ok(store) => Some(store),
        Err(error) => {
            eprintln!(
                "il deposito in {} non si apre: {error}",
                directory.display()
            );
            None
        }
    }
}

/// Cosa è rimasto acceso, e chi respira ancora.
fn list_left_running(store: Option<&ledger::Ledger>) {
    let Some(store) = store else {
        eprintln!("senza deposito non c'è niente da elencare");
        return;
    };
    let left = match left_running(store) {
        Ok(left) => left,
        Err(error) => {
            eprintln!("leggere il deposito: {error}");
            return;
        }
    };
    if left.is_empty() {
        println!("nessun processo lasciato acceso.");
        return;
    }
    for item in left {
        let breath = if item.still_alive { "vivo" } else { "morto" };
        let port = item
            .record
            .port
            .map(|port| format!(", porta {port}"))
            .unwrap_or_default();
        println!(
            "{}  pid {} [{breath}]{port}  — {} {}  (acceso da {}, {})",
            item.record.process_id,
            item.record.pid,
            item.record.command,
            item.record.args.join(" "),
            item.record.started_by,
            item.record.working_directory,
        );
    }
}

/// Spegne quello che è rimasto acceso, e lo scrive.
///
/// **È la seconda metà del guasto 4**: «non può né spegnerli né riprenderli».
/// Chi trova la porta occupata aveva bisogno di questo comando, e la volta
/// scorsa ha dovuto cercare il pid a mano — due persone, due volte.
fn stop_left_running(store: Option<&ledger::Ledger>) {
    let Some(store) = store else {
        eprintln!("senza deposito non c'è niente da spegnere");
        return;
    };
    // Prima si tolgono i fantasmi: chiudere nel deposito chi è già morto evita
    // di annunciare che si sta spegnendo qualcosa che non c'è.
    match close_the_ones_that_stopped_breathing(store, now()) {
        Ok(0) => {}
        Ok(closed) => println!("{closed} voci chiuse: erano processi già morti."),
        Err(error) => eprintln!("chiudere i morti: {error}"),
    }

    let left = match left_running(store) {
        Ok(left) => left,
        Err(error) => {
            eprintln!("leggere il deposito: {error}");
            return;
        }
    };
    for item in left.into_iter().filter(|item| item.still_alive) {
        // SAFETY: `kill` legge e scrive interi. Il pid viene dal deposito, cioè
        // da qualcosa che Sailor ha acceso: non si spegne roba di altri.
        let sent = unsafe { libc_kill(item.record.pid) };
        if sent {
            let _ = store.record_process_ended(&ledger::ProcessEndRecord {
                process_id: item.record.process_id.clone(),
                exit_code: None,
                ended_at: now(),
            });
            println!(
                "spento {} (pid {})",
                item.record.process_id, item.record.pid
            );
        } else {
            eprintln!(
                "non si è riusciti a spegnere {} (pid {}): resta scritto come acceso, \
                 che è meglio di dichiararlo morto senza esserne sicuri",
                item.record.process_id, item.record.pid
            );
        }
    }
}

/// SIGTERM su un pid solo. Non `pkill`, non `killall`: un numero conosciuto.
unsafe fn libc_kill(pid: u32) -> bool {
    extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    const SIGTERM: i32 = 15;
    kill(pid as i32, SIGTERM) == 0
}

fn run_live(root: &Path, store: Option<&ledger::Ledger>) {
    let desktop = root.join("desktop");
    let manifest = desktop.join("src-tauri/Cargo.toml");
    let binary = desktop.join("src-tauri/target/debug/sailor-desktop");
    if !manifest.exists() {
        eprintln!(
            "non c'è nessuna finestra in {}: serve --root sulla radice del repository",
            manifest.display()
        );
        std::process::exit(2);
    }

    let status_path = ledger::sailor_home()
        .map(|home| LiveStatus::path_in(&home))
        .unwrap_or_else(|| std::env::temp_dir().join(supervisor::STATUS_FILE));

    if let Some(store) = store {
        match close_the_ones_that_stopped_breathing(store, now()) {
            Ok(closed) if closed > 0 => {
                println!("{closed} voci di processi morti chiuse nel deposito.")
            }
            Ok(_) => {}
            Err(error) => eprintln!("chiudere i morti: {error}"),
        }
        // **QUESTO È IL CASO DEL GUASTO 4, PRESO PRIMA CHE FACCIA MALE.** La
        // volta scorsa l'avvio falliva con un errore di porta occupata e nessuno
        // sapeva di chi fosse. Adesso lo sa il deposito, e lo dice qui.
        if let Ok(Some(holder)) = store.process_holding_port(DEV_PORT) {
            if ledger::pid_is_alive(holder.pid) {
                eprintln!(
                    "la porta {DEV_PORT} è tenuta da {} (pid {}), acceso da {} in {}.\n\
                     Spegnilo con `sailor-live --stop`, oppure usalo com'è.",
                    holder.process_id, holder.pid, holder.started_by, holder.working_directory
                );
                std::process::exit(3);
            }
        }
    }

    // Il servitore di sviluppo della pagina. È il processo che nel guasto 4
    // teneva la porta.
    let vite = Process::start(
        Spec {
            process_id: format!("live-frontend-{DEV_PORT}"),
            command: "npm".to_owned(),
            args: vec!["run".to_owned(), "dev".to_owned()],
            working_directory: desktop.clone(),
            port: Some(DEV_PORT),
            purpose: "live".to_owned(),
            started_by: started_by(),
        },
        store,
    );
    let _vite = match vite {
        Ok(process) => {
            println!(
                "pagina in sviluppo: pid {} sulla porta {DEV_PORT}",
                process.pid()
            );
            Some(process)
        }
        Err(error) => {
            eprintln!("il servitore della pagina non parte: {error}");
            std::process::exit(4);
        }
    };

    let roots = watched_roots(root);
    let mut seen = newest_change(&roots);

    publish(&status_path, LiveState::Building, String::new(), None);
    let mut window: Option<Process> = None;
    let mut running_since: Option<i64> = None;

    match cargo_build(&manifest, Some(1)) {
        supervisor::BuildOutcome::Succeeded => match start_window(&binary, &desktop, store) {
            Ok(process) => {
                println!("finestra accesa: pid {}", process.pid());
                running_since = Some(now());
                window = Some(process);
                publish(
                    &status_path,
                    LiveState::Running,
                    String::new(),
                    running_since,
                );
            }
            Err(error) => {
                eprintln!("la finestra non parte: {error}");
                publish(&status_path, LiveState::BuildFailed, error, None);
            }
        },
        supervisor::BuildOutcome::Failed { message } => {
            eprintln!("{message}");
            eprintln!(
                "la prima costruzione è fallita: non c'è ancora nessuna finestra da tenere \
                 accesa. Correggi e salva: si riprova da solo."
            );
            publish(&status_path, LiveState::BuildFailed, message, None);
        }
    }

    println!("in ascolto. Ctrl-C per chiudere.");
    loop {
        std::thread::sleep(Duration::from_millis(500));

        // Se la finestra è stata chiusa a mano, la si scrive chiusa e si smette:
        // tenerla nel deposito come accesa fabbricherebbe un fantasma.
        if let Some(process) = window.as_mut() {
            if let Some(code) = process.exited() {
                process.record_end(code);
                println!("la finestra è stata chiusa da chi la guardava.");
                return;
            }
        }

        let changed = newest_change(&roots);
        if changed <= seen {
            continue;
        }
        seen = changed;

        println!("qualcosa è cambiato: ricostruisco senza toccare la finestra.");
        publish(
            &status_path,
            LiveState::Building,
            String::new(),
            running_since,
        );

        let outcome = rebuild_then_swap(
            &mut window,
            || cargo_build(&manifest, Some(1)),
            || start_window(&binary, &desktop, store),
        );

        match outcome {
            Rebuild::Replaced => {
                running_since = Some(now());
                println!("finestra sostituita.");
                publish(
                    &status_path,
                    LiveState::Running,
                    String::new(),
                    running_since,
                );
            }
            Rebuild::KeptRunning { message } => {
                eprintln!("{message}");
                eprintln!("costruzione fallita: la finestra resta all'ultima versione buona.");
                publish(&status_path, LiveState::BuildFailed, message, running_since);
            }
            Rebuild::StartFailed { message } => {
                eprintln!("costruita, ma non riparte: {message}");
                publish(&status_path, LiveState::BuildFailed, message, None);
                running_since = None;
            }
        }
    }
}

/// Dove si guarda per sapere che qualcosa è cambiato.
///
/// **`crates/` c'è di proposito, e non c'era in `cargo tauri dev`.**
/// `get_in_workspace_dependency_paths` di `tauri-cli` segue solo le dipendenze
/// per percorso che sono **membri dello stesso workspace**, e
/// `desktop/src-tauri/Cargo.toml` dichiara un `[workspace]` vuoto apposta per
/// stare fuori da quello alla radice. Risultato: con `cargo tauri dev` una
/// modifica a `crates/ledger` non fa ricostruire niente, e la finestra continua
/// a mostrare il motore vecchio senza dirlo.
fn watched_roots(root: &Path) -> Vec<PathBuf> {
    vec![
        root.join("crates"),
        root.join("desktop/src-tauri/src"),
        root.join("desktop/src-tauri/Cargo.toml"),
    ]
}

fn start_window(
    binary: &Path,
    working_directory: &Path,
    store: Option<&ledger::Ledger>,
) -> Result<Process, String> {
    Process::start(
        Spec {
            process_id: "live-window".to_owned(),
            command: binary.display().to_string(),
            args: Vec::new(),
            working_directory: working_directory.to_path_buf(),
            port: None,
            purpose: "live".to_owned(),
            started_by: started_by(),
        },
        store,
    )
}

/// Chi ha acceso: la persona più il pid di questo supervisore.
///
/// **Il nome da solo non basta**, perché due supervisori della stessa persona
/// sono precisamente il caso del guasto 4 — due cantieri, due porte, una notte.
fn started_by() -> String {
    let who = std::env::var("USER").unwrap_or_else(|_| "ignoto".to_owned());
    format!("sailor-live/{who}/{}", std::process::id())
}

fn publish(path: &Path, state: LiveState, message: String, running_since: Option<i64>) {
    let status = LiveStatus {
        state,
        message,
        changed_at: now(),
        running_since,
    };
    if let Err(error) = status.write(path) {
        eprintln!("lo stato non si scrive ({}): {error}", path.display());
    }
}
