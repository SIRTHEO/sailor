//! **Sailor knows what it started — fault 4.**
//!
//! "Sailor starts processes and does not know which it started, so it can
//! neither stop nor resume them. Seen: an orphan development process held a
//! port and blocked the start — twice, for two different people, in the same
//! night."
//!
//! The proofs here do not simulate the register: they light real processes,
//! write them in the real ledger, and find them again from a reopened `Ledger`
//! — the position of whoever arrives the next day knowing nothing.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use supervisor::child::Spec;
use supervisor::{
    close_the_ones_that_stopped_breathing, left_running, Running, Supervisor, DEV_PORT,
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        // A counter as well as the clock: `cargo test` runs the proofs in one
        // process and the macOS clock has no nanosecond resolution — fault 21,
        // already paid for in `crates/profiles`.
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sailor-supervisor-{label}-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("creare la cartella");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Stopping a process stops what it lit too.
///
/// `sailor-live` starts `cargo` and the window's server, and both have
/// children: a surviving grandchild is the port still held after a successful
/// `stop`. The judge is the system — `pid_is_alive` — not the other
/// implementation: two copies wrong the same way would agree with each other.
#[test]
fn stopping_a_process_also_stops_the_one_it_started() {
    let dir = TestDirectory::new("nipote");
    let pidfile = dir.0.join("nipote.pid");
    let mut process = Supervisor::over(None)
        .start(Spec {
            command: "/bin/sh".to_owned(),
            args: vec![
                "-c".to_owned(),
                format!("sleep 300 & echo $! > {}; wait", pidfile.display()),
            ],
            ..sleeper("padre-di-qualcuno", None)
        })
        .expect("accendere il padre");

    let grandchild = wait_for_pid(&pidfile);
    assert!(
        ledger::pid_is_alive(grandchild),
        "il nipote non è mai partito: la prova non sta provando niente"
    );

    process.stop().expect("spegnere il padre");

    // A freshly killed pid stays a zombie until somebody reaps it, and a zombie
    // still answers the null signal: a handful of turns are allowed before
    // accusing.
    let mut still_here = true;
    for _ in 0..100 {
        if !ledger::pid_is_alive(grandchild) {
            still_here = false;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if still_here {
        let _ = std::process::Command::new("kill")
            .args(["-9", &grandchild.to_string()])
            .status();
    }
    assert!(
        !still_here,
        "il nipote {grandchild} è vivo dopo lo stop del padre: è l'orfano che \
         questo crate esiste per non lasciare"
    );
}

/// Waits for the script to have written the grandchild's pid, and reads it.
fn wait_for_pid(pidfile: &std::path::Path) -> u32 {
    for _ in 0..200 {
        if let Ok(text) = std::fs::read_to_string(pidfile) {
            if let Ok(pid) = text.trim().parse() {
                return pid;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("il nipote non ha scritto il proprio pid entro quattro secondi");
}

fn sleeper(process_id: &str, port: Option<u16>) -> Spec {
    Spec {
        process_id: process_id.to_owned(),
        command: "/bin/sh".to_owned(),
        args: vec!["-c".to_owned(), "while true; do sleep 1; done".to_owned()],
        working_directory: std::env::temp_dir(),
        port,
        purpose: "live".to_owned(),
        started_by: "una prova".to_owned(),
        environment: Vec::new(),
        speaks: true,
    }
}

/// **FAULT 4'S CASE, REMADE WHOLE.**
///
/// A process with a port is lit, every trace in memory is thrown away — the
/// `Ledger` is reopened from scratch, as tomorrow's person would — and the
/// question is who holds the port. Before this repair the answer did not exist:
/// the pid had to be hunted by hand, and two people did it twice in the same
/// night without knowing of each other.
#[test]
fn tomorrow_someone_can_ask_who_holds_the_port() {
    let directory = TestDirectory::new("orfano");

    let supervisor = Supervisor::over(Some(
        ledger::Ledger::open(&directory.0).expect("aprire il deposito"),
    ));
    let mut process = supervisor
        .start(sleeper("live-frontend", Some(DEV_PORT)))
        .expect("accendere il processo");
    let pid = process.pid();
    drop(supervisor);

    // From here on this is the next day's person: the process is still running,
    // but nothing of whoever started it remains in memory — only the disk. It
    // is the exact situation fault 4 turned up in.
    let store = ledger::Ledger::open(&directory.0).expect("riaprire il deposito");
    let holder = store
        .process_holding_port(DEV_PORT)
        .expect("chiedere della porta")
        .expect("nessuno tiene la porta, ma qualcosa la sta tenendo davvero");

    assert_eq!(holder.pid, pid, "il deposito indica un altro pid");
    assert_eq!(holder.process_id, "live-frontend");
    assert_eq!(
        holder.command, "/bin/sh",
        "senza la riga di comando chi lo trova non sa se può spegnerlo"
    );
    assert_eq!(
        holder.started_by, "una prova",
        "l'orfano è di nuovo senza padrone, che era metà del guasto"
    );
    assert!(
        ledger::pid_is_alive(pid),
        "il deposito dice che c'è, e non c'è: pid {pid}"
    );

    // And now it can be stopped, which was the second half: "it can neither
    // stop nor resume them".
    process
        .stop()
        .expect("spegnere quello che il deposito ha trovato");
    assert!(!ledger::pid_is_alive(pid), "l'orfano non si è spento");
    assert!(
        store
            .process_holding_port(DEV_PORT)
            .expect("richiedere della porta")
            .is_none(),
        "la porta risulta ancora occupata: chi arriva dopo non partirà lo stesso"
    );
}

/// **A LIST THAT NEVER CLEANS ITSELF STOPS BEING READ.**
///
/// A process killed from outside writes no ending of its own, so it stays
/// "running" for ever. The sweep closes it — and must not close what still
/// breathes, or it would declare a held port free and remake the fault from the
/// opposite side.
#[test]
fn the_dead_are_closed_and_the_living_are_left_alone() {
    let directory = TestDirectory::new("fantasmi");
    let supervisor = Supervisor::over(Some(
        ledger::Ledger::open(&directory.0).expect("aprire il deposito"),
    ));
    let store = supervisor.ledger().expect("il deposito appena aperto");

    let mut alive = supervisor
        .start(sleeper("ancora-qui", None))
        .expect("accendere il vivo");

    // A process that dies on its own and that **nobody records as ended**: the
    // ghost. `exited` reaps it — really takes it out of the process table — and
    // `forget` stops the destructor writing its ending, which is what happens
    // when the one dying is whoever held it.
    let mut doomed = supervisor
        .start(Spec {
            command: "/bin/sh".to_owned(),
            args: vec!["-c".to_owned(), "exit 0".to_owned()],
            ..sleeper("gia-morto", None)
        })
        .expect("accendere il condannato");
    let mut reaped = false;
    for _ in 0..200 {
        if doomed.exited().is_some() {
            reaped = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(reaped, "il condannato non è morto entro quattro secondi");
    std::mem::forget(doomed);

    let before = left_running(store).expect("leggere prima");
    assert_eq!(before.len(), 2, "il deposito non ha scritto tutti e due");
    assert_eq!(
        before.iter().filter(|item| item.still_alive).count(),
        1,
        "la conferma pid per pid non distingue il vivo dal morto"
    );

    let closed =
        close_the_ones_that_stopped_breathing(store, 1_700_000_000).expect("chiudere i morti");
    assert_eq!(closed, 1, "chiusi {closed} invece di uno solo");

    let after = left_running(store).expect("leggere dopo");
    let names: Vec<&str> = after
        .iter()
        .map(|item| item.record.process_id.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["ancora-qui"],
        "la pulizia ha chiuso un processo che stava lavorando"
    );

    alive.stop().expect("spegnere il vivo");
    assert!(
        left_running(store).expect("leggere alla fine").is_empty(),
        "spegnere non ha scritto la chiusura"
    );
}

/// **TWO COPIES OF THE SAME PORT DRIFT APART.**
///
/// The number lives in `supervisor` and in `desktop/src-tauri/tauri.conf.json`.
/// If somebody changes `devUrl` and not this constant, the register will
/// declare the wrong port and whoever hunts the orphan will look at an empty
/// place — fault 4 back with a register apparently working. It is fault 10
/// ("the same list written in two places"): while there are two copies, at
/// least one proof compares them.
#[test]
fn the_dev_port_matches_the_tauri_config() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("il crate sta in <radice>/crates/supervisor")
        .to_path_buf();
    let path = root.join("desktop/src-tauri/tauri.conf.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("desktop/src-tauri/tauri.conf.json is gone, so the port cannot be compared: {error}")
    });
    workspace::measured_against(
        text.lines().count(),
        "lines of the Tauri configuration read",
        1,
        "port the process register declares",
    );
    let config: serde_json::Value =
        serde_json::from_str(&text).expect("la configurazione di Tauri non è JSON valido");

    let declared = config["build"]["devUrl"]
        .as_str()
        .expect("`build.devUrl` non c'è più: la modalità viva non ha più una porta nota");
    assert!(
        declared.ends_with(&format!(":{DEV_PORT}")),
        "la finestra si sviluppa su {declared} mentre il registro dei processi \
         scrive la porta {DEV_PORT}"
    );
}

/// **A ROW OVER A NUMBER SOMEBODY ELSE HOLDS NOW STOPS NOTHING.** The start
/// refuses when the ledger names a live holder of the port; naming one that
/// died and whose number came round would refuse for ever, on a name that
/// belongs to a stranger.
#[test]
fn the_port_is_held_only_by_the_process_the_row_actually_names() {
    let directory = TestDirectory::new("porta-tenuta");
    let store = ledger::Ledger::open(&directory.0).expect("the store");
    let mine = std::process::id();
    let born = ledger::born_second_of(mine).expect("this machine says when a process was born");

    written_holding(&store, "p-mio", mine, Some(born));
    assert_eq!(
        supervisor::who_the_ledger_says_holds(&store, DEV_PORT).map(|holder| holder.process_id),
        Some("p-mio".to_owned()),
        "the process the row names is right here"
    );

    written_holding(&store, "p-mio", mine, Some(born - 1));
    assert!(
        supervisor::who_the_ledger_says_holds(&store, DEV_PORT).is_none(),
        "the same number under another second is not the holder the row named"
    );
}

fn written_holding(store: &ledger::Ledger, process_id: &str, pid: u32, born_at: Option<i64>) {
    store
        .record_process_started(&ledger::ProcessRecord {
            process_id: process_id.to_owned(),
            pid,
            command: "npm".to_owned(),
            args: vec!["run".to_owned(), "dev".to_owned()],
            working_directory: "/somewhere".to_owned(),
            port: Some(DEV_PORT),
            purpose: "live".to_owned(),
            started_by: "the test".to_owned(),
            run_id: None,
            started_at: supervisor::now(),
            born_at,
        })
        .expect("write the start");
}
