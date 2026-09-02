//! Uno pseudo-terminale vero, aperto dentro uno spazio di lavoro vero.
//!
//! **QUESTE PROVE AVVIANO PROCESSI.** Non simulano niente: aprono un terminale,
//! ci scrivono dentro come farebbe un dito su una tastiera, e leggono ciò che ne
//! esce. Se la macchina non avesse pseudo-terminali, queste prove non
//! passerebbero — ed è il punto: una prova che non tocca il sistema operativo
//! non dice niente su un crate che esiste per toccarlo.
//!
//! **PERCHÉ `echo ci"a"o` E NON `echo ciao`.** Un terminale riscrive ciò che gli
//! si digita: `echo ciao` comparirebbe nell'uscita *due volte* — una perché il
//! terminale rimanda indietro i tasti, una perché la shell ha eseguito — e una
//! prova che cerca «ciao» resterebbe **verde anche se la shell non avesse
//! eseguito niente**. Con le virgolette in mezzo le due cose si distinguono: la
//! riga rimandata indietro è `echo ci"a"o`, e la parola `ciao` compare solo se
//! qualcuno l'ha eseguita. È la stessa trappola per cui l'uscita di un terminale
//! non si può leggere come si legge una pipe.

use std::sync::Arc;
use std::time::Duration;
use terminal::{Buffer, Ending, Opening, Passed, Routed, Router, Size, Terminals, Workspace};

/// Quanto si aspetta una risposta prima di dire che non è arrivata. Largo: su
/// una macchina che sta compilando, avviare una shell può prendere secondi.
const PATIENCE: Duration = Duration::from_secs(10);

/// Una cartella vuota che fa da spazio di lavoro, cancellata alla fine.
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

/// Un terminale che non smista niente: qui si prova lo pseudo-terminale, e una
/// regola di smistamento di mezzo renderebbe ambiguo quale dei due ha fallito.
fn plain_terminals() -> Terminals {
    Terminals::with_router(Arc::new(Router::without_routes(Arc::new(
        terminal::PathLookup::current(),
    ))))
}

fn shell() -> Opening {
    Opening {
        // `/bin/sh` e non la shell dell'utente: `zsh` con la configurazione di
        // qualcuno stampa banner, cambia l'invito e a volte esegue cose. La
        // prova deve parlare del terminale, non della casa di chi la lancia.
        program: "/bin/sh".into(),
        ..Opening::default()
    }
}

/// **IL TERMINALE È VERO: GLI SI SCRIVE E RISPONDE.**
///
/// Il mutante che fa cadere questa prova è una scrittura che non arriva
/// all'ingresso del figlio — ed è il difetto da cui difende, perché un terminale
/// che accetta una riga e non la esegue sembra funzionare.
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

/// **NASCE DENTRO LO SPAZIO DI LAVORO, NON CI VA DOPO.** Nessuno ha scritto un
/// `cd`: la cartella è parte di com'è stato aperto.
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

/// **L'USCITA ARRIVA MENTRE ESCE, NON QUANDO IL COMANDO FINISCE.**
///
/// La misura è il secondo di attesa in mezzo: se i pezzi arrivassero alla fine,
/// «primo» e «secondo» comparirebbero insieme, e la seconda asserzione
/// cadrebbe. È la stessa proprietà che `actions` prova sulle pipe, e il motivo
/// per cui il filo che legge esiste invece di un `read_to_end`.
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

/// **LA FINE SI ANNUNCIA, E DICE COM'È ANDATA.**
///
/// Senza questo annuncio chi guarda non ha modo di distinguere un terminale
/// morto da uno che tace: continuerebbe a mostrarlo vivo per sempre. È la
/// proprietà su cui poggia l'evento `terminal_closed` del contratto.
///
/// LA MISURA CHE POTEVA VENIRE DIVERSA: si sceglie `exit 7` e non `exit`,
/// perché un codice qualunque diverso da zero cade su ogni scorciatoia — un
/// annuncio che non arriva, e anche un annuncio che arriva con un esito
/// inventato riuscito.
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

/// **IL DESTINATARIO SA DI QUALE TERMINALE È PRIMA DI RICEVERE IL PRIMO BYTE.**
///
/// Chi porta l'uscita fuori di qui — la finestra — deve marcare ogni pezzo con
/// l'identificativo del terminale, e quell'identificativo lo assegna `open`.
/// Ricevendo un destinatario già fatto resterebbe un istante in cui i byte
/// esistono e il nome no, e ci cade dentro l'invito della shell: il primo pezzo
/// che chi guarda si aspetta di vedere.
///
/// LA MISURA CHE POTEVA VENIRE DIVERSA: il nome viene registrato **dal
/// destinatario, alla propria nascita**, non chiesto al terminale dopo. Se
/// `open` fabbricasse il destinatario senza nome — o lo ricevesse già fatto —
/// qui resterebbe una stringa vuota.
#[test]
fn the_output_is_told_which_terminal_it_belongs_to() {
    let scratch = Scratch::make("nome");
    let terminals = plain_terminals();

    /// Un destinatario che si ricorda il nome ricevuto alla nascita e quello
    /// che aveva quando è arrivato il primo pezzo.
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

    // Qualcosa deve pur uscire, o la seconda asserzione non misurerebbe niente.
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

/// **IL RIDIMENSIONAMENTO ARRIVA AL PROGRAMMA DENTRO.** Non si guarda una
/// variabile nostra: si chiede al terminale, con `stty`, quanto crede di essere
/// grande. È l'unico modo in cui questa prova poteva venire diversa.
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

/// **L'ELENCO DICE QUALI SONO APERTI E IN QUALE SPAZIO**, che è la domanda a cui
/// serve rispondere per attaccarci sopra un'interfaccia.
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

/// Chiudere spegne il processo e toglie la riga dall'elenco. Senza il `wait`
/// dentro `close`, il processo resterebbe zombie e `alive` continuerebbe a dire
/// di sì.
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

/// **LA RIGA DELL'ELENCO ESCE COI NOMI CHE LA FINESTRA LEGGE.**
///
/// `docs/2026-09-01-il-contratto-del-terminale.md` dice due cose insieme: che la
/// riga è questo tipo, e che i suoi campi si chiamano `workspaceRoot`,
/// `workspaceName`, `processId`. Reggono solo se questa struttura esce così: chi
/// ne ricopiasse una versione in TypeScript o nel guscio farebbe il guasto 10.
///
/// LA MISURA CHE POTEVA VENIRE DIVERSA: togliendo `rename_all = "camelCase"` da
/// [`terminal::Summary`] i nomi tornano con l'underscore, la finestra legge
/// `undefined` su tre campi su cinque, e non se ne accorge nessuno — un campo
/// assente in JavaScript non è un errore, è un vuoto.
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
    };
    let written = serde_json::to_value(&row).expect("la riga si serializza");
    let object = written.as_object().expect("è un oggetto");
    let mut names: Vec<&str> = object.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "alive",
            "device",
            "estimatedTokens",
            "id",
            "moved",
            "processId",
            "workspaceName",
            "workspaceRoot"
        ],
        "i nomi della riga non sono quelli del contratto: {written}"
    );
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

/// Uno spazio di lavoro che non esiste si rifiuta all'apertura, non allo
/// `spawn`: altrimenti il messaggio parlerebbe della shell invece che della
/// cartella, e chi legge cercherebbe nel posto sbagliato.
#[test]
fn a_workspace_that_is_not_there_is_refused_before_anything_starts() {
    let missing = std::env::temp_dir().join("sailor-terminal-non-esiste-davvero");
    let _ = std::fs::remove_dir_all(&missing);
    assert!(Workspace::open(&missing).is_err());
}

/// Un comando ordinario passa, e il motivo dice perché: senza il motivo, uno
/// smistamento che non scatta è muto.
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
