//! A real pseudo-terminal, opened inside a real workspace.
//!
//! **THESE PROOFS START PROCESSES.** They simulate nothing: they open a
//! terminal, type into it as a finger on a keyboard would, and read what comes
//! out. On a machine with no pseudo-terminals they would not pass — which is
//! the point: a proof that never touches the operating system says nothing
//! about a crate that exists to touch it.
//!
//! **WHY `echo ci"a"o` AND NOT `echo ciao`.** A terminal echoes back what is
//! typed at it: `echo ciao` would appear in the output *twice* — once because
//! the terminal sends the keys back, once because the shell ran — and a proof
//! looking for "ciao" would stay **green even had the shell run nothing**. With
//! the quotes in the middle the two are told apart: the echoed line is
//! `echo ci"a"o`, and the word `ciao` appears only if somebody ran it. It is
//! the same trap that stops a terminal's output being read like a pipe's.

use std::sync::Arc;
use std::time::Duration;
use terminal::{Buffer, Ending, Opening, Passed, Routed, Router, Size, Terminals, Workspace};

/// How long an answer is waited for before it is called missing. Generous: on
/// a machine that is compiling, starting a shell can take seconds.
const PATIENCE: Duration = Duration::from_secs(10);

/// An empty directory standing in for a workspace, deleted at the end.
struct Scratch {
    workspace: Workspace,
}

impl Scratch {
    fn make(label: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!(
            "sailor-terminal-{}-{}-{label}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("l'orologio non va indietro")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("creare la cartella di prova");
        Scratch {
            workspace: Workspace::open(&root).expect("la cartella appena creata è uno spazio"),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.workspace.root);
    }
}

/// A terminal that routes nothing: what is proved here is the pseudo-terminal,
/// and a routing rule in the middle would blur which of the two failed.
fn plain_terminals() -> Terminals {
    Terminals::with_router(Arc::new(Router::without_routes(Arc::new(
        terminal::PathLookup::on(toolbox::Machine::bare(std::path::PathBuf::from(
            toolbox::probe::NOWHERE,
        ))),
    ))))
}

fn shell() -> Opening {
    Opening {
        // `/bin/sh` and not the user's shell: `zsh` with somebody's own
        // configuration prints banners, changes the prompt and sometimes runs
        // things. The proof must speak of the terminal, not of whose house it
        // is launched in.
        program: "/bin/sh".into(),
        ..Opening::default()
    }
}

/// **THE TERMINAL IS REAL: WRITE TO IT AND IT ANSWERS.**
///
/// The mutant that brings this proof down is a write that never reaches the
/// child's input — the very defect it guards against, because a terminal that
/// takes a line and does not run it looks as though it works.
#[test]
fn a_terminal_runs_what_is_written_into_it() {
    let scratch = Scratch::make("echo");
    let terminals = plain_terminals();
    let seen = Arc::new(Buffer::new());
    let terminal = terminals
        .open(scratch.workspace.clone(), &shell(), |_| {
            Arc::clone(&seen) as Arc<dyn terminal::Output>
        })
        .expect("aprire uno pseudo-terminale");

    let decision = terminal.submit("echo ci\"a\"o").expect("scrivere la riga");
    assert!(
        matches!(decision, Routed::Command { .. }),
        "un comando resta un comando: {decision:?}"
    );

    assert!(
        seen.wait_for("ciao", PATIENCE),
        "la shell non ha risposto; ciò che è uscito finora: {:?}",
        seen.text()
    );
}

/// **IT IS BORN INSIDE THE WORKSPACE, IT DOES NOT WALK THERE.** Nobody typed a
/// `cd`: the directory is part of how it was opened.
#[test]
fn a_terminal_starts_inside_its_workspace() {
    let scratch = Scratch::make("pwd");
    let terminals = plain_terminals();
    let seen = Arc::new(Buffer::new());
    let terminal = terminals
        .open(scratch.workspace.clone(), &shell(), |_| {
            Arc::clone(&seen) as Arc<dyn terminal::Output>
        })
        .expect("aprire uno pseudo-terminale");

    terminal.submit("pwd").expect("scrivere la riga");

    let root = scratch.workspace.root.to_string_lossy().into_owned();
    assert!(
        seen.wait_for(&root, PATIENCE),
        "il terminale non è nato in {root}; ciò che è uscito: {:?}",
        seen.text()
    );
}

/// **THE OUTPUT ARRIVES AS IT LEAVES, NOT WHEN THE COMMAND ENDS.**
///
/// The measure is the second of waiting in between: were the pieces to arrive
/// at the end, "primo" and "secondo" would appear together and the second
/// assertion would fall. It is the property `actions` proves on pipes, and the
/// reason the reading thread exists instead of a `read_to_end`.
#[test]
fn the_output_arrives_while_it_is_being_produced() {
    let scratch = Scratch::make("live");
    let terminals = plain_terminals();
    let seen = Arc::new(Buffer::new());
    let terminal = terminals
        .open(scratch.workspace.clone(), &shell(), |_| {
            Arc::clone(&seen) as Arc<dyn terminal::Output>
        })
        .expect("aprire uno pseudo-terminale");

    terminal
        .submit("printf 'pri''mo\\n'; sleep 2; printf 'secon''do\\n'")
        .expect("scrivere la riga");

    assert!(
        seen.wait_for("primo", PATIENCE),
        "il primo pezzo non è arrivato: {:?}",
        seen.text()
    );
    assert!(
        !seen.text().contains("secondo"),
        "il secondo pezzo è arrivato insieme al primo: l'uscita non è in diretta, è stata consegnata tutta alla fine. Uscita: {:?}",
        seen.text()
    );
    assert!(
        seen.wait_for("secondo", PATIENCE),
        "il secondo pezzo non è mai arrivato: {:?}",
        seen.text()
    );
}

/// **THE END ANNOUNCES ITSELF, AND SAYS HOW IT WENT.**
///
/// Without that announcement whoever watches cannot tell a dead terminal from
/// a silent one: it would go on showing it alive for ever. It is the property
/// the contract's `terminal_closed` event rests on.
///
/// THE MEASURE THAT COULD HAVE COME OUT OTHERWISE: `exit 7` is chosen and not
/// `exit`, because a code other than zero falls on every shortcut — an
/// announcement that never comes, and equally one that comes with an invented
/// successful outcome.
#[test]
fn the_end_of_a_terminal_is_announced_with_how_it_ended() {
    let scratch = Scratch::make("fine");
    let terminals = plain_terminals();
    let seen = Arc::new(Buffer::new());
    let terminal = terminals
        .open(scratch.workspace.clone(), &shell(), |_| {
            Arc::clone(&seen) as Arc<dyn terminal::Output>
        })
        .expect("aprire uno pseudo-terminale");

    assert_eq!(
        seen.ending(),
        None,
        "un terminale appena aperto non è finito, e «non ancora» non è un esito"
    );

    terminal.submit("exit 7").expect("scrivere la riga");

    assert_eq!(
        seen.wait_for_end(PATIENCE),
        Some(Ending::Exited(7)),
        "la fine non è arrivata, o è arrivata con l'esito sbagliato; \
         ciò che è uscito: {:?}",
        seen.text()
    );
}

/// **THE SINK KNOWS WHICH TERMINAL IT BELONGS TO BEFORE THE FIRST BYTE.**
///
/// Whoever carries the output out of here — the window — must mark every piece
/// with the terminal's identifier, and that identifier is assigned by `open`.
/// Taking a ready-made sink would leave an instant where the bytes exist and
/// the name does not, and the shell's prompt falls into it: the first piece
/// whoever watches expects to see.
///
/// THE MEASURE THAT COULD HAVE COME OUT OTHERWISE: the name is recorded **by
/// the sink, at its own birth**, not asked of the terminal afterwards. Were
/// `open` to build the sink nameless — or receive it ready-made — an empty
/// string would be left here.
#[test]
fn the_output_is_told_which_terminal_it_belongs_to() {
    let scratch = Scratch::make("nome");
    let terminals = plain_terminals();

    /// A sink that remembers the name it was given at birth and the one it had
    /// when the first piece arrived.
    struct Named {
        at_birth: String,
        at_first_chunk: std::sync::Mutex<Option<String>>,
    }

    impl terminal::Output for Named {
        fn chunk(&self, _bytes: &[u8]) {
            let mut first = self.at_first_chunk.lock().expect("non panica");
            if first.is_none() {
                *first = Some(self.at_birth.clone());
            }
        }
    }

    let named = std::sync::Mutex::new(None::<Arc<Named>>);
    let terminal = terminals
        .open(scratch.workspace.clone(), &shell(), |id| {
            let made = Arc::new(Named {
                at_birth: id.to_owned(),
                at_first_chunk: std::sync::Mutex::new(None),
            });
            *named.lock().expect("non panica") = Some(Arc::clone(&made));
            made as Arc<dyn terminal::Output>
        })
        .expect("aprire uno pseudo-terminale");

    let made = named
        .lock()
        .expect("non panica")
        .clone()
        .expect("il destinatario è stato fabbricato");
    assert_eq!(
        made.at_birth,
        terminal.id(),
        "il nome dato al destinatario non è quello del terminale"
    );

    // Something must come out, or the second assertion measures nothing.
    terminal.submit("printf 'ec''co\\n'").expect("scrivere");
    assert!(
        seen_something(&made, PATIENCE),
        "nessun pezzo è mai arrivato"
    );
    assert_eq!(
        made.at_first_chunk.lock().expect("non panica").as_deref(),
        Some(terminal.id()),
        "il primo pezzo è arrivato a un destinatario che non sapeva ancora il proprio nome"
    );

    fn seen_something(made: &Named, limit: Duration) -> bool {
        let until = std::time::Instant::now() + limit;
        loop {
            if made.at_first_chunk.lock().expect("non panica").is_some() {
                return true;
            }
            if std::time::Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// **THE RESIZE REACHES THE PROGRAM INSIDE.** No variable of ours is consulted:
/// the terminal is asked, with `stty`, how big it believes it is. It is the
/// only way this proof could have come out otherwise.
#[test]
fn a_resize_is_seen_by_the_program_inside() {
    let scratch = Scratch::make("size");
    let terminals = plain_terminals();
    let seen = Arc::new(Buffer::new());
    let terminal = terminals
        .open(scratch.workspace.clone(), &shell(), |_| {
            Arc::clone(&seen) as Arc<dyn terminal::Output>
        })
        .expect("aprire uno pseudo-terminale");

    terminal
        .resize(Size {
            rows: 40,
            columns: 100,
        })
        .expect("ridimensionare");
    terminal.submit("stty size").expect("scrivere la riga");

    assert!(
        seen.wait_for("40 100", PATIENCE),
        "il terminale dentro non ha visto 40 righe per 100 colonne; ciò che è uscito: {:?}",
        seen.text()
    );
}

/// **THE LIST SAYS WHICH ARE OPEN AND IN WHICH WORKSPACE**, the question that
/// must be answered before an interface can be built on top.
#[test]
fn the_list_says_which_terminals_are_open_and_where() {
    let here = Scratch::make("qui");
    let there = Scratch::make("altrove");
    let terminals = plain_terminals();
    let quiet: Arc<dyn terminal::Output> = Arc::new(Buffer::new());

    let first = terminals
        .open(here.workspace.clone(), &shell(), |_| Arc::clone(&quiet))
        .expect("aprire il primo");
    let second = terminals
        .open(there.workspace.clone(), &shell(), |_| Arc::clone(&quiet))
        .expect("aprire il secondo");

    let listed = terminals.list();
    assert_eq!(listed.len(), 2, "due aperti, {listed:?}");
    assert_ne!(first.id(), second.id(), "due terminali, due identificativi");

    let of_first = listed
        .iter()
        .find(|row| row.id == first.id())
        .expect("il primo è nell'elenco");
    assert_eq!(
        of_first.workspace_root,
        here.workspace.root.to_string_lossy()
    );
    assert_eq!(of_first.workspace_name, here.workspace.name);
    assert!(of_first.alive);
    assert!(of_first.process_id > 0);

    let of_second = listed
        .iter()
        .find(|row| row.id == second.id())
        .expect("il secondo è nell'elenco");
    assert_eq!(
        of_second.workspace_root,
        there.workspace.root.to_string_lossy()
    );
}

/// Closing stops the process and takes the row out of the list. Without the
/// `wait` inside `close` the process would stay a zombie and `alive` would go
/// on saying yes.
#[test]
fn closing_a_terminal_stops_it_and_takes_it_off_the_list() {
    let scratch = Scratch::make("chiusura");
    let terminals = plain_terminals();
    let quiet: Arc<dyn terminal::Output> = Arc::new(Buffer::new());
    let terminal = terminals
        .open(scratch.workspace.clone(), &shell(), |_| quiet)
        .expect("aprire");
    assert!(terminal.alive());

    terminals
        .close(terminal.id())
        .expect("l'identificativo esiste")
        .expect("chiudere");

    assert!(!terminal.alive(), "chiuso e ancora vivo");
    assert!(terminals.list().is_empty(), "{:?}", terminals.list());
}

/// **THE LIST'S ROW COMES OUT WITH THE NAMES THE WINDOW READS.**
///
/// `docs/the-terminal-contract.md` says two things at once: that the row is
/// this type, and that its fields are called `workspaceRoot`, `workspaceName`,
/// `processId`. Both hold only if this struct comes out that way: copying a
/// version of it into TypeScript or the shell would be fault 10.
///
/// THE MEASURE THAT COULD HAVE COME OUT OTHERWISE: take `rename_all =
/// "camelCase"` off [`terminal::Summary`] and the names come back with
/// underscores, the window reads `undefined` on three fields of five, and
/// nobody notices — a missing field in JavaScript is not an error, it is a void.
#[test]
fn the_list_row_carries_the_names_the_window_reads() {
    let row = terminal::Summary {
        id: "qui-1".to_owned(),
        workspace_root: "/tmp/qui".to_owned(),
        workspace_name: "qui".to_owned(),
        alive: true,
        process_id: 4242,
        device: "ttys004".to_owned(),
        moved: 512,
        estimated_tokens: 60_477,
        program: "codex".to_owned(),
        profile: Some("prove".to_owned()),
        directory: "/tmp/qui/altrove".to_owned(),
        attached: 1,
    };
    let written = serde_json::to_value(&row).expect("la riga si serializza");
    let object = written.as_object().expect("è un oggetto");
    let mut names: Vec<&str> = object.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "alive",
            "attached",
            "device",
            "directory",
            "estimatedTokens",
            "id",
            "moved",
            "processId",
            "profile",
            "program",
            "workspaceName",
            "workspaceRoot"
        ],
        "i nomi della riga non sono quelli del contratto: {written}"
    );
    assert_eq!(written["program"], "codex");
    assert_eq!(written["profile"], "prove");
    // A row from an older host, without the two names, still reads.
    let older: terminal::Summary = serde_json::from_str(
        r#"{"id":"a","workspaceRoot":"/r","workspaceName":"r","alive":true,"processId":1,"device":"ttys001","moved":0,"estimatedTokens":0}"#,
    )
    .expect("an older row reads");
    assert_eq!((older.program.as_str(), older.profile), ("", None));
    assert_eq!(written["workspaceRoot"], "/tmp/qui");
    assert_eq!(written["processId"], 4242);
    // The tty is the anchor of a tab: it travels as the device, short form.
    assert_eq!(written["device"], "ttys004");
    assert_eq!(written["moved"], 512);
    assert_eq!(written["estimatedTokens"], 60_477);
}

/// **THE ESTIMATE IN THE ROW IS THE RELAY'S, NOT A SECOND FIT.** The window
/// shows a token count next to the bytes, and it must be the number the relay
/// compares to its ceiling: the same model, the same intercept, on the same
/// bytes. A row that carried zero, or a count fitted elsewhere, would show a
/// session as empty right up to the moment the baton is handed on.
#[test]
fn the_estimate_in_the_row_is_the_one_the_relay_measures_with() {
    let model = sessions::fullness::Model::default();
    for moved in [0_u64, 512, 1 << 20] {
        let expected = sessions::fullness::measure(moved, &model, 0).estimated_tokens;
        assert_eq!(terminal::estimated_tokens(moved), expected, "at {moved} bytes");
    }
    assert!(
        terminal::estimated_tokens(1 << 20) > terminal::estimated_tokens(0),
        "more bytes must estimate more tokens"
    );
}

/// **A TAB IS ANCHORED ON THE TTY, AND THE TTY IS THE PROGRAM'S OWN.** The
/// program inside is asked which terminal it is on, and the answer must be the
/// device the list carries: a device read from anywhere else would name a
/// session the program does not recognise as its own.
#[test]
fn the_device_in_the_list_is_the_one_the_program_inside_reports() {
    let scratch = Scratch::make("device");
    let terminals = plain_terminals();
    let seen = Arc::new(Buffer::new());
    let terminal = terminals
        .open(scratch.workspace.clone(), &shell(), |_| {
            Arc::clone(&seen) as Arc<dyn terminal::Output>
        })
        .expect("aprire uno pseudo-terminale");

    let listed = terminals.list();
    let device = listed[0].device.clone();
    assert!(
        device.starts_with("tty") || device.starts_with("pts"),
        "not a tty name: {device}"
    );
    assert!(
        !device.starts_with("/dev/"),
        "the short name, as `ps` writes it: {device}"
    );

    terminal.submit("tty").expect("scrivere la riga");
    assert!(
        seen.wait_for(&format!("/dev/{device}"), PATIENCE),
        "the program inside reports a different terminal; what came out: {:?}",
        seen.text()
    );
}

/// A workspace that does not exist is refused at the open, not at the `spawn`:
/// otherwise the message would speak of the shell instead of the directory,
/// and whoever reads it would look in the wrong place.
#[test]
fn a_workspace_that_is_not_there_is_refused_before_anything_starts() {
    let missing = std::env::temp_dir().join(format!(
        "sailor-terminal-non-esiste-davvero-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&missing);
    assert!(Workspace::open(&missing).is_err());
}

/// An ordinary command passes, and the reason says why: without the reason, a
/// routing that does not fire is mute.
#[test]
fn a_plain_command_passes_and_says_why() {
    let scratch = Scratch::make("motivo");
    let terminals = plain_terminals();
    let quiet: Arc<dyn terminal::Output> = Arc::new(Buffer::new());
    let terminal = terminals
        .open(scratch.workspace.clone(), &shell(), |_| quiet)
        .expect("aprire");

    match terminal.submit("ls").expect("scrivere") {
        Routed::Command { why, .. } => assert!(
            matches!(why, Passed::NoRuleMatched | Passed::RunnableFirstWord(_)),
            "motivo inatteso: {why:?}"
        ),
        other => panic!("`ls` non è una richiesta di flusso: {other:?}"),
    }
}
