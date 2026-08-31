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
use terminal::{Buffer, Opening, Passed, Routed, Router, Size, Terminals, Workspace};

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
        .open(
            scratch.workspace.clone(),
            &shell(),
            Arc::clone(&seen) as Arc<dyn terminal::Output>,
        )
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
        .open(
            scratch.workspace.clone(),
            &shell(),
            Arc::clone(&seen) as Arc<dyn terminal::Output>,
        )
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
        .open(
            scratch.workspace.clone(),
            &shell(),
            Arc::clone(&seen) as Arc<dyn terminal::Output>,
        )
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

/// **IL RIDIMENSIONAMENTO ARRIVA AL PROGRAMMA DENTRO.** Non si guarda una
/// variabile nostra: si chiede al terminale, con `stty`, quanto crede di essere
/// grande. È l'unico modo in cui questa prova poteva venire diversa.
#[test]
fn a_resize_is_seen_by_the_program_inside() {
    let scratch = Scratch::make("size");
    let terminals = plain_terminals();
    let seen = Arc::new(Buffer::new());
    let terminal = terminals
        .open(
            scratch.workspace.clone(),
            &shell(),
            Arc::clone(&seen) as Arc<dyn terminal::Output>,
        )
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
        .open(here.workspace.clone(), &shell(), Arc::clone(&quiet))
        .expect("aprire il primo");
    let second = terminals
        .open(there.workspace.clone(), &shell(), Arc::clone(&quiet))
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
        .open(scratch.workspace.clone(), &shell(), quiet)
        .expect("aprire");
    assert!(terminal.alive());

    terminals
        .close(terminal.id())
        .expect("l'identificativo esiste")
        .expect("chiudere");

    assert!(!terminal.alive(), "chiuso e ancora vivo");
    assert!(terminals.list().is_empty(), "{:?}", terminals.list());
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
        .open(scratch.workspace.clone(), &shell(), quiet)
        .expect("aprire");

    match terminal.submit("ls").expect("scrivere") {
        Routed::Command { why, .. } => assert!(
            matches!(why, Passed::NoRuleMatched | Passed::RunnableFirstWord(_)),
            "motivo inatteso: {why:?}"
        ),
        other => panic!("`ls` non è una richiesta di flusso: {other:?}"),
    }
}
