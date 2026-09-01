//! **Sailor sa cosa ha avviato — guasto 4.**
//!
//! «Sailor avvia processi e non sa quali ha avviato, quindi non può né
//! spegnerli né riprenderli. Visto: un processo di sviluppo orfano occupava una
//! porta e impediva l'avvio — due volte, a due persone diverse, nella stessa
//! notte.»
//!
//! Le prove qui non simulano il registro: accendono processi veri, li scrivono
//! nel deposito vero, e li ritrovano da un `Ledger` riaperto — che è la
//! posizione di chi arriva il giorno dopo e non sa niente.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use supervisor::child::{Process, Spec};
use supervisor::{close_the_ones_that_stopped_breathing, left_running, Running, DEV_PORT};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        // Un contatore oltre all'orologio: `cargo test` manda le prove sullo
        // stesso processo e l'orologio di macOS non ha la risoluzione del
        // nanosecondo — è il guasto 21, già pagato in `crates/profiles`.
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

/// Spegnere un processo spegne anche chi lui ha acceso.
///
/// `sailor-live` avvia `cargo` e il server della finestra, che di figli ne
/// fanno: un nipote che sopravvive è la porta che resta occupata dopo uno
/// `stop` riuscito. Chi giudica è il sistema — `pid_is_alive` — non l'altra
/// implementazione: due copie che sbagliano uguale si darebbero ragione.
#[test]
fn stopping_a_process_also_stops_the_one_it_started() {
    let dir = TestDirectory::new("nipote");
    let pidfile = dir.0.join("nipote.pid");
    let mut process = Process::start(
        Spec {
            command: "/bin/sh".to_owned(),
            args: vec![
                "-c".to_owned(),
                format!("sleep 300 & echo $! > {}; wait", pidfile.display()),
            ],
            ..sleeper("padre-di-qualcuno", None)
        },
        None,
    )
    .expect("accendere il padre");

    let grandchild = wait_for_pid(&pidfile);
    assert!(
        ledger::pid_is_alive(grandchild),
        "il nipote non è mai partito: la prova non sta provando niente"
    );

    process.stop().expect("spegnere il padre");

    // Un pid appena ucciso resta zombie finché qualcuno lo raccoglie, e uno
    // zombie risponde ancora al segnale nullo: si concede una manciata di giri
    // prima di accusare.
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

/// Aspetta che lo script abbia scritto il pid del nipote, e lo legge.
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
    }
}

/// **IL CASO DEL GUASTO 4, RIFATTO PER INTERO.**
///
/// Si accende un processo con una porta, si butta via ogni traccia in memoria —
/// il `Ledger` viene riaperto da zero, come farebbe la persona di domani — e si
/// chiede chi tiene la porta. Prima di questa riparazione la risposta non
/// esisteva: bisognava cercare il pid a mano, e due persone lo hanno fatto due
/// volte nella stessa notte senza sapere l'una dell'altra.
#[test]
fn tomorrow_someone_can_ask_who_holds_the_port() {
    let directory = TestDirectory::new("orfano");

    let store = ledger::Ledger::open(&directory.0).expect("aprire il deposito");
    let mut process = Process::start(sleeper("live-frontend", Some(DEV_PORT)), Some(&store))
        .expect("accendere il processo");
    let pid = process.pid();
    drop(store);

    // Da qui in poi si è la persona del giorno dopo: il processo è ancora
    // acceso, ma di chi l'ha avviato non resta niente in memoria — solo il
    // disco. È la situazione esatta in cui il guasto 4 si è presentato.
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

    // E adesso si può spegnere, che era la seconda metà: «non può né spegnerli
    // né riprenderli».
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

/// **UN ELENCO CHE NON SI RIPULISCE SMETTE DI ESSERE LETTO.**
///
/// Un processo ucciso da fuori non scrive la propria chiusura, quindi resta
/// «acceso» per sempre. La passata di pulizia lo chiude — e non deve chiudere
/// chi respira ancora, altrimenti dichiarerebbe libera una porta occupata e
/// rifarebbe il guasto dalla parte opposta.
#[test]
fn the_dead_are_closed_and_the_living_are_left_alone() {
    let directory = TestDirectory::new("fantasmi");
    let store = ledger::Ledger::open(&directory.0).expect("aprire il deposito");

    let mut alive =
        Process::start(sleeper("ancora-qui", None), Some(&store)).expect("accendere il vivo");

    // Un processo che muore da solo e che **nessuno registra come finito**: è
    // il fantasma. `exited` lo raccoglie — cioè lo toglie davvero dalla tabella
    // dei processi — e `forget` impedisce al distruttore di scriverne la
    // chiusura, che è ciò che succede quando a morire è chi lo teneva.
    let mut doomed = Process::start(
        Spec {
            command: "/bin/sh".to_owned(),
            args: vec!["-c".to_owned(), "exit 0".to_owned()],
            ..sleeper("gia-morto", None)
        },
        Some(&store),
    )
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

    let before = left_running(&store).expect("leggere prima");
    assert_eq!(before.len(), 2, "il deposito non ha scritto tutti e due");
    assert_eq!(
        before.iter().filter(|item| item.still_alive).count(),
        1,
        "la conferma pid per pid non distingue il vivo dal morto"
    );

    let closed =
        close_the_ones_that_stopped_breathing(&store, 1_700_000_000).expect("chiudere i morti");
    assert_eq!(closed, 1, "chiusi {closed} invece di uno solo");

    let after = left_running(&store).expect("leggere dopo");
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
        left_running(&store).expect("leggere alla fine").is_empty(),
        "spegnere non ha scritto la chiusura"
    );
}

/// **DUE COPIE DELLA STESSA PORTA DIVERGONO.**
///
/// Il numero sta in `supervisor` e in `desktop/src-tauri/tauri.conf.json`. Se
/// qualcuno cambia il `devUrl` e non questa costante, il registro dichiarerà la
/// porta sbagliata e chi cerca l'orfano guarderà nel posto vuoto — un guasto 4
/// che si ripresenta con un registro apparentemente in funzione. È il guasto 10
/// («la stessa lista scritta in due punti»): finché le copie sono due, almeno
/// una prova le confronta.
#[test]
fn the_dev_port_matches_the_tauri_config() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("il crate sta in <radice>/crates/supervisor")
        .to_path_buf();
    let path = root.join("desktop/src-tauri/tauri.conf.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("leggere {}: {error}", path.display()));
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
