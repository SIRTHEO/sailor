//! `sailor-live` — the live mode that does not make the window vanish.
//!
//! **IT TAKES THE PLACE OF `cargo tauri dev`**, for one reason: that command
//! stops the running program **before** recompiling (`tauri-cli 2.11.4`,
//! `src/interface/rust.rs`, `run_dev_watcher`), so every file touched puts the
//! window out and a failed build is merely the reason none comes back. Here the
//! order is reversed — build, and swap the window **only if** the build
//! succeeded — and when it fails it is said, instead of leaving a blank screen.
//!
//! And every process it lights goes through the ledger, which is the repair of
//! fault 4: whoever arrives tomorrow and finds the port taken has somewhere to
//! ask whose it is.

use std::path::{Path, PathBuf};
use std::time::Duration;

use supervisor::child::{cargo_build, newest_change, Process, Spec};
use supervisor::{
    close_the_ones_that_stopped_breathing, left_running, now, rebuild_then_swap, turn_now,
    LiveState, LiveStatus, Rebuild, Supervisor, SwapRequest, Turn,
};

use supervisor::DEV_PORT;

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let root = repository_root(&arguments);

    let supervisor = Supervisor::over(open_ledger());
    if supervisor.ledger().is_none() {
        eprintln!(
            "avviso: nessun deposito. I processi accesi non verranno registrati, \
             e un orfano di stanotte domani non avrà un padrone."
        );
    }

    match arguments.first().map(String::as_str) {
        Some("--list") => list_left_running(supervisor.ledger()),
        Some("--stop") => stop_left_running(supervisor.ledger()),
        _ => run_live(
            &root,
            &supervisor,
            arguments.iter().any(|one| one == "--at-once"),
        ),
    }
}

/// The repository to work on. **It comes from whoever starts this, never
/// written inside** — fault 25: an absolute path in the program makes it
/// runnable in one place, and from a clone it works in the main tree.
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

/// What was left running, and who still breathes.
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

/// Stops what was left running, and writes it down. **The second half of
/// fault 4**: whoever found the port taken had to hunt the pid by hand.
fn stop_left_running(store: Option<&ledger::Ledger>) {
    let Some(store) = store else {
        eprintln!("senza deposito non c'è niente da spegnere");
        return;
    };
    // Ghosts go first: closing in the ledger what is already dead avoids
    // announcing the stop of something that is not there.
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
        // SAFETY: `kill` reads and writes integers. The pid comes from the
        // ledger, so from something Sailor lit: nobody else's is stopped.
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

/// SIGTERM on one pid. Not `pkill`, not `killall`: a known number.
unsafe fn libc_kill(pid: u32) -> bool {
    extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    const SIGTERM: i32 = 15;
    kill(pid as i32, SIGTERM) == 0
}

/// `--at-once` puts every build on the screen the moment it is done, which is
/// what this did before the window learnt to wait. Kept for whoever is not
/// working inside the window they are building.
fn run_live(root: &Path, supervisor: &Supervisor, at_once: bool) {
    let store = supervisor.ledger();
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

    let home = ledger::sailor_home();
    let status_path = home
        .as_ref()
        .map(|home| LiveStatus::path_in(home))
        .unwrap_or_else(|| std::env::temp_dir().join(supervisor::STATUS_FILE));
    let swap_path = home
        .as_ref()
        .map(|home| SwapRequest::path_in(home))
        .unwrap_or_else(|| std::env::temp_dir().join(supervisor::SWAP_FILE));
    // An ask left behind by whoever ran this before would swap the first
    // window this one lights, before anybody has looked at it.
    SwapRequest::take(&swap_path);

    if let Some(store) = store {
        match close_the_ones_that_stopped_breathing(store, now()) {
            Ok(closed) if closed > 0 => {
                println!("{closed} voci di processi morti chiuse nel deposito.")
            }
            Ok(_) => {}
            Err(error) => eprintln!("chiudere i morti: {error}"),
        }
        // **THIS IS FAULT 4, CAUGHT BEFORE IT HURTS.** The start used to fail
        // with a port-taken error and nobody knew whose the port was. Now the
        // ledger knows, and says so here.
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

    // **AND THE PORT IS ASKED OF THE PORT.** The ledger knows who Sailor
    // started, and nothing about a page server left running by hand — which
    // vite answers by moving to the next port while the window keeps loading
    // from this one. A bind, not a process list: fault 12.
    if let Some(taken) = supervisor::who_holds(DEV_PORT) {
        eprintln!(
            "la porta {DEV_PORT} è occupata ({taken}) e non risulta a Sailor.\n\
             Il servitore della pagina ne prenderebbe un'altra in silenzio, e la\n\
             finestra continuerebbe a leggere da questa.\n\
             `lsof -nP -iTCP:{DEV_PORT} -sTCP:LISTEN` dice di chi è."
        );
        std::process::exit(3);
    }

    // The page's development server. It is the process that in fault 4 was
    // holding the port.
    let vite = supervisor.start(Spec {
        process_id: format!("live-frontend-{DEV_PORT}"),
        command: "npm".to_owned(),
        args: vec!["run".to_owned(), "dev".to_owned()],
        working_directory: desktop.clone(),
        port: Some(DEV_PORT),
        purpose: "live".to_owned(),
        started_by: started_by(),
        // The environment of whoever typed `sailor-live`, which is what a
        // development server and the window both want.
        environment: Vec::new(),
    });
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
        supervisor::BuildOutcome::Succeeded => match start_window(&binary, &desktop, supervisor) {
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
    // A build that nobody has taken yet, and the ask that is still standing.
    let mut waiting = false;
    let mut asked = at_once;
    loop {
        std::thread::sleep(Duration::from_millis(500));

        // If the window was closed by hand, it is written closed and dropped:
        // keeping it in the ledger as running would manufacture a ghost.
        if let Some(process) = window.as_mut() {
            if let Some(code) = process.exited() {
                process.record_end(code);
                println!("la finestra è stata chiusa da chi la guardava.");
                return;
            }
        }

        asked |= SwapRequest::take(&swap_path);
        let changed = newest_change(&roots);
        match turn_now(changed > seen, waiting, asked, window.is_none()) {
            Turn::Wait => {}
            Turn::Build => {
                seen = changed;
                println!("qualcosa è cambiato: ricostruisco senza toccare la finestra.");
                publish(&status_path, LiveState::Building, String::new(), running_since);
                match cargo_build(&manifest, Some(1)) {
                    supervisor::BuildOutcome::Succeeded => {
                        waiting = true;
                        // ON SCREEN IS STILL THE ONE BEFORE THIS, and it stays
                        // there: what was being worked in is not taken away by
                        // the act of proving the code compiles.
                        println!("costruita. Aspetta: la finestra la prende quando gliela chiedi.");
                        publish(&status_path, LiveState::Ready, String::new(), running_since);
                    }
                    supervisor::BuildOutcome::Failed { message } => {
                        eprintln!("{message}");
                        eprintln!("costruzione fallita: la finestra resta all'ultima versione buona.");
                        publish(&status_path, LiveState::BuildFailed, message, running_since);
                    }
                }
            }
            Turn::Swap => {
                asked = false;
                waiting = false;
                publish(&status_path, LiveState::Building, String::new(), running_since);
                let outcome = rebuild_then_swap(
                    &mut window,
                    || supervisor::BuildOutcome::Succeeded,
                    || start_window(&binary, &desktop, supervisor),
                );
                match outcome {
                    Rebuild::Replaced => {
                        running_since = Some(now());
                        println!("finestra sostituita.");
                        publish(&status_path, LiveState::Running, String::new(), running_since);
                    }
                    Rebuild::KeptRunning { message } | Rebuild::StartFailed { message } => {
                        eprintln!("costruita, ma non riparte: {message}");
                        publish(&status_path, LiveState::BuildFailed, message, None);
                        running_since = None;
                    }
                }
            }
        }
    }
}

/// Where one looks to know something changed.
///
/// **`crates/` IS THERE ON PURPOSE, AND WAS NOT IN `cargo tauri dev`.**
/// `tauri-cli`'s `get_in_workspace_dependency_paths` follows only path
/// dependencies that are **members of the same workspace**, and
/// `desktop/src-tauri/Cargo.toml` declares an empty `[workspace]` precisely to
/// stay out of the root one. Result: under `cargo tauri dev` a change to
/// `crates/ledger` rebuilds nothing, and the window goes on showing the old
/// engine without saying so.
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
    supervisor: &Supervisor,
) -> Result<Process, String> {
    supervisor.start(Spec {
        process_id: "live-window".to_owned(),
        command: binary.display().to_string(),
        args: Vec::new(),
        working_directory: working_directory.to_path_buf(),
        port: None,
        purpose: "live".to_owned(),
        started_by: started_by(),
        // The environment of whoever typed `sailor-live`, which is what a
        // development server and the window both want.
        environment: Vec::new(),
    })
}

/// Who lit it: the person plus this supervisor's pid. **The name alone is not
/// enough** — two supervisors of the same person are fault 4 exactly.
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
        supervisor_pid: std::process::id(),
    };
    if let Err(error) = status.write(path) {
        eprintln!("lo stato non si scrive ({}): {error}", path.display());
    }
}
