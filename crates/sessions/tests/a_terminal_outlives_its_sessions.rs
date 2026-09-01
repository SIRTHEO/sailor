//! Un terminale dura più delle sessioni che ci passano dentro.
//!
//! Le tre cose che questo stato deve reggere, scritte una per prova:
//! la stessa sessione che manda molti eventi; due sessioni sullo stesso tty in
//! momenti diversi; una riga rimasta aperta perché il terminale è stato ucciso
//! senza dirlo.
//!
//! E la quarta, che è quella che decide se il disegno è giusto: **lo stacco
//! vive sul tty, non sulla sessione**. Staccare un terminale lo stacca anche
//! per gli agenti che ci apriranno dopo — è quello che una persona intende
//! quando dice «lascia stare questa finestra».
//!
//! **NESSUNA PROVA QUI APRE IL DEPOSITO PREDEFINITO.** Ogni prova ha il suo
//! file usa-e-getta: il deposito di questa macchina è alla versione 8 mentre
//! questo codice ne conosce un'altra, e una prova che lo aprisse misurerebbe la
//! macchina di chi la esegue — è il guasto 5.

use sessions::{Anchor, Arrival, SessionError, Sessions, TerminalEvent, SESSIONS_FILE};
use std::path::PathBuf;

/// Una cartella usa-e-getta per una prova sola.
struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory = std::env::temp_dir().join(format!(
            "sailor-sessions-{label}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&directory).expect("creare la cartella della prova");
        Scratch { directory }
    }

    fn store(&self) -> Sessions {
        Sessions::open(self.directory.join(SESSIONS_FILE)).expect("aprire le sessioni")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn anchor(tty: &str, worktree: &str) -> Anchor {
    Anchor {
        tty: tty.to_owned(),
        worktree: worktree.to_owned(),
        ancestor: Some("Whatever".to_owned()),
    }
}

fn arrival(tty: &str, worktree: &str, session: &str, at: i64) -> Arrival {
    Arrival {
        anchor: anchor(tty, worktree),
        session_id: Some(session.to_owned()),
        transcript_path: Some(format!("/tmp/{session}.jsonl")),
        at,
    }
}

fn event(tty: &str, session: &str, name: &str, at: i64) -> TerminalEvent {
    TerminalEvent {
        tty: tty.to_owned(),
        session_id: Some(session.to_owned()),
        worktree: Some("/work/sailor".to_owned()),
        ancestor: Some("Whatever".to_owned()),
        name: name.to_owned(),
        transcript_path: None,
        occurred_at: at,
        payload: None,
    }
}

#[test]
fn the_same_session_sends_many_events_and_the_terminal_stays_one() {
    let scratch = Scratch::new("many-events");
    let store = scratch.store();
    store
        .open_terminal(&arrival("ttys001", "/work/sailor", "aaa", 100))
        .expect("aprire");
    for (index, name) in ["SessionStart", "UserPromptSubmit", "Stop"].iter().enumerate() {
        store
            .record_event(&event("ttys001", "aaa", name, 100 + index as i64))
            .expect("registrare");
    }
    assert_eq!(store.terminals().expect("leggere").len(), 1);
    let recorded = store.events_on("ttys001").expect("leggere gli eventi");
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[0].name, "SessionStart");
    assert_eq!(recorded[2].name, "Stop");
    assert_eq!(store.sessions_on("ttys001").expect("le sessioni"), vec!["aaa"]);
}

#[test]
fn two_sessions_can_share_one_terminal_at_different_times() {
    let scratch = Scratch::new("two-sessions");
    let store = scratch.store();

    store
        .open_terminal(&arrival("ttys001", "/first", "aaa", 100))
        .expect("aprire la prima");
    store
        .record_event(&event("ttys001", "aaa", "SessionStart", 100))
        .expect("registrare");
    assert!(store.close_terminal("ttys001", 200).expect("chiudere"));

    store
        .open_terminal(&arrival("ttys001", "/second", "bbb", 300))
        .expect("aprire la seconda");
    store
        .record_event(&event("ttys001", "bbb", "SessionStart", 300))
        .expect("registrare");

    let terminals = store.terminals().expect("leggere");
    assert_eq!(terminals.len(), 1, "il tty è uno: {terminals:?}");
    let row = &terminals[0];
    assert_eq!(row.session_id.as_deref(), Some("bbb"));
    assert_eq!(row.worktree, "/second");
    assert_eq!(row.opened_at, 300, "la seconda apertura riparte da capo");
    assert!(row.is_open(), "riaprire toglie la chiusura di prima");

    assert_eq!(
        store.sessions_on("ttys001").expect("le sessioni"),
        vec!["aaa", "bbb"],
        "la successione delle sessioni si legge dalla coda, che non si riscrive"
    );
}

/// Un terminale ucciso non chiude niente. La riga resta aperta, e resta aperta
/// **visibilmente**: chi legge lo stato deve poter dire «questa non è viva, è
/// rimasta lì», invece di credere a una sessione che non c'è più.
#[test]
fn a_terminal_killed_without_saying_leaves_its_row_open() {
    let scratch = Scratch::new("killed");
    let store = scratch.store();
    store
        .open_terminal(&arrival("ttys004", "/somewhere", "ccc", 10))
        .expect("aprire");
    let row = store.terminal("ttys004").expect("leggere").expect("c'è");
    assert!(row.is_open());
    assert_eq!(row.closed_at, None);
    assert!(
        !store.close_terminal("ttys009", 20).expect("chiudere"),
        "chiudere un tty che non si è mai aperto non deve fingere di aver chiuso"
    );
}

/// **LA PROVA CHE DECIDE IL DISEGNO.** Lo stacco è sul tty: sopravvive alla
/// sessione che c'era, e vale per quella che arriva.
#[test]
fn detaching_holds_the_terminal_and_not_the_session() {
    let scratch = Scratch::new("detach");
    let store = scratch.store();

    store
        .open_terminal(&arrival("ttys002", "/here", "aaa", 100))
        .expect("aprire");
    store.detach(&anchor("ttys002", "/here"), 150).expect("staccare");
    assert!(store.terminal("ttys002").expect("leggere").expect("c'è").is_detached());

    // Arriva un altro agente, sullo stesso terminale, dopo.
    store
        .open_terminal(&arrival("ttys002", "/here", "bbb", 200))
        .expect("aprire la seconda");
    let row = store.terminal("ttys002").expect("leggere").expect("c'è");
    assert_eq!(row.session_id.as_deref(), Some("bbb"));
    assert!(
        row.is_detached(),
        "una finestra staccata è staccata anche per chi ci arriva dopo: \
         se lo stacco cade a ogni apertura dura quanto una sessione, che non è \
         quello che chiede chi lo dice"
    );

    assert!(store.attach("ttys002").expect("riattaccare"));
    assert!(!store.terminal("ttys002").expect("leggere").expect("c'è").is_detached());
    assert!(
        !store.attach("ttys002").expect("riattaccare due volte"),
        "riattaccare ciò che è già attaccato non ha cambiato niente, e lo dice"
    );
}

/// Staccare un terminale di cui nessuno si è ancora presentato deve restare
/// scritto: altrimenti `/sailor-off` su una finestra appena aperta non fa
/// niente, e non lo dice.
#[test]
fn a_terminal_can_be_detached_before_anyone_has_arrived() {
    let scratch = Scratch::new("detach-first");
    let store = scratch.store();
    store.detach(&anchor("ttys007", "/here"), 50).expect("staccare");
    let row = store.terminal("ttys007").expect("leggere").expect("c'è");
    assert!(row.is_detached());
    assert_eq!(row.session_id, None);

    store
        .open_terminal(&arrival("ttys007", "/here", "zzz", 60))
        .expect("aprire dopo lo stacco");
    assert!(store.terminal("ttys007").expect("leggere").expect("c'è").is_detached());
}

/// Un evento che arriva da un terminale mai annunciato apre lo stesso la riga:
/// i ganci non arrivano in ordine, e un evento perso è informazione persa.
#[test]
fn an_event_from_an_unannounced_terminal_still_lands() {
    let scratch = Scratch::new("unannounced");
    let store = scratch.store();
    store
        .remember_terminal(&arrival("ttys005", "/elsewhere", "ddd", 10))
        .expect("ricordare");
    store
        .record_event(&event("ttys005", "ddd", "PostToolUse", 11))
        .expect("registrare");
    let row = store.terminal("ttys005").expect("leggere").expect("c'è");
    assert_eq!(row.worktree, "/elsewhere");
    assert_eq!(store.events_on("ttys005").expect("gli eventi").len(), 1);
}

/// **LA VERSIONE È NOSTRA, E IL FILE ANCHE.** Il deposito delle corse ha la sua
/// `user_version` e la alza quando cambiano le sue proiezioni; questa non
/// c'entra e non deve muoversi con quella. La prova guarda le due cose che lo
/// rendono vero: il numero, e il fatto che aprire le sessioni **non crea**
/// `state.db`.
#[test]
fn the_sessions_have_their_own_file_and_their_own_version() {
    let scratch = Scratch::new("version");
    let store = scratch.store();
    assert_eq!(store.schema_version().expect("la versione"), 1);
    assert!(store.path().ends_with(SESSIONS_FILE));
    assert!(
        !scratch.directory.join("state.db").exists(),
        "le sessioni hanno toccato il deposito delle corse"
    );
    assert!(
        !scratch.directory.join("events.db").exists(),
        "le sessioni hanno toccato il registro degli eventi delle corse"
    );
}

/// Un file scritto da una versione più nuova si dichiara, non si ripara.
#[test]
fn a_file_from_a_newer_version_is_refused_by_name() {
    let scratch = Scratch::new("newer");
    let path = scratch.directory.join(SESSIONS_FILE);
    {
        let connection = rusqlite::Connection::open(&path).expect("creare il file");
        connection
            .pragma_update(None, "user_version", 99_i64)
            .expect("scrivere la versione");
    }
    match Sessions::open(&path) {
        Err(SessionError::UnsupportedSchema(found)) => assert_eq!(found, 99),
        Err(other) => panic!("una versione ignota va detta per quello che è, non «{other}»"),
        Ok(_) => panic!("una versione ignota è passata come se fosse la nostra"),
    }
}
