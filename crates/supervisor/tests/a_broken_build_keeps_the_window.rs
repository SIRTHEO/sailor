//! **The window survives a build that failed — fault 11.**
//!
//! The register said a build error kills the window; reading `tauri-cli` the
//! mechanism is another, and the difference is the repair. `run_dev_watcher`
//! kills the running program *before* it compiles, so the window dies on every
//! file touched and the error is only why none comes back: the defect is the
//! order. Hence the repair, one line of sequence — build first, and touch what
//! is running only if the build went. Break the order, not a message, to see
//! these red.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use supervisor::{rebuild_then_swap, BuildOutcome, LiveState, LiveStatus, Rebuild, Running};

/// A fake program that counts how many times it was stopped.
struct Fake {
    label: String,
    stops: std::rc::Rc<std::cell::Cell<usize>>,
}

impl Running for Fake {
    fn stop(&mut self) -> Result<(), String> {
        self.stops.set(self.stops.get() + 1);
        Ok(())
    }
}

/// **THE HEART OF FAULT 11.**
///
/// Under `tauri-cli`'s order — stop, then build — this proof is red on all
/// three assertions: `stops` is 1, `running` is `None`, and the outcome is not
/// `KeptRunning`. That is the mutation to make to verify it, not an error
/// message to change: the original defect was the sequence.
#[test]
fn a_failed_build_never_stops_what_is_running() {
    let stops = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut running = Some(Fake {
        label: "la finestra buona".to_owned(),
        stops: stops.clone(),
    });

    let outcome = rebuild_then_swap(
        &mut running,
        || BuildOutcome::Failed {
            message: "error[E0425]: cannot find value `x`".to_owned(),
        },
        || panic!("non si riavvia niente quando la costruzione è fallita"),
    );

    assert_eq!(
        stops.get(),
        0,
        "la finestra è stata fermata pur essendo fallita la costruzione: è \
         l'ordine di `tauri-cli`, cioè il guasto 11"
    );
    assert!(
        running.is_some(),
        "l'ultima versione buona non è più in mano a nessuno"
    );
    assert_eq!(
        running.as_ref().map(|fake| fake.label.as_str()),
        Some("la finestra buona"),
        "è rimasto acceso qualcosa, ma non quello di prima"
    );
    match outcome {
        Rebuild::KeptRunning { message } => assert!(
            message.contains("E0425"),
            "l'errore del compilatore non arriva a chi guarda: {message}"
        ),
        other => panic!("una costruzione fallita ha dato {other:?}"),
    }
}

/// The other half: when the build succeeds, the old one **must** give up its
/// place. Without this proof the most convenient repair — never stop anything —
/// would pass the proof above and leave live mode stuck on the first version
/// for ever.
#[test]
fn a_good_build_replaces_what_is_running() {
    let stops = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut running = Some(Fake {
        label: "quella di prima".to_owned(),
        stops: stops.clone(),
    });
    let started = stops.clone();

    let outcome = rebuild_then_swap(
        &mut running,
        || BuildOutcome::Succeeded,
        || {
            Ok(Fake {
                label: "quella nuova".to_owned(),
                stops: started.clone(),
            })
        },
    );

    assert_eq!(stops.get(), 1, "la vecchia non è stata fermata");
    assert_eq!(
        running.as_ref().map(|fake| fake.label.as_str()),
        Some("quella nuova"),
        "la nuova non ha preso il posto"
    );
    assert!(matches!(outcome, Rebuild::Replaced), "esito: {outcome:?}");
}

/// **A REAL PROCESS, NOT A FAKE.**
///
/// The two proofs above measure the order on an object built to obey. This one
/// lights an operating-system process, makes the build fail, then asks the
/// system whether that pid still breathes. It is the difference between "I
/// wrote the code that should" and "I saw it".
#[test]
fn a_real_child_is_still_breathing_after_a_broken_build() {
    let child = std::process::Command::new("/bin/sh")
        .args(["-c", "while true; do sleep 1; done"])
        .spawn()
        .expect("accendere un processo lungo");
    let pid = child.id();

    struct Child(std::process::Child);
    impl Running for Child {
        fn stop(&mut self) -> Result<(), String> {
            self.0.kill().map_err(|error| error.to_string())?;
            let _ = self.0.wait();
            Ok(())
        }
    }

    /// **THE CLEAN-UP CANNOT LIVE ON THE HAPPY PATH**: a panic below unwinds
    /// past the `stop()` at the end, and what this lit outlives the suite.
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let mut running = Some(Child(child));
    assert!(
        ledger::pid_is_alive(pid),
        "il processo non è nemmeno partito"
    );

    let outcome = rebuild_then_swap(
        &mut running,
        || BuildOutcome::Failed {
            message: "could not compile `ledger`".to_owned(),
        },
        || panic!("non si riavvia niente"),
    );

    assert!(
        matches!(outcome, Rebuild::KeptRunning { .. }),
        "esito: {outcome:?}"
    );
    assert!(
        ledger::pid_is_alive(pid),
        "il processo acceso è morto per una compilazione fallita: pid {pid}"
    );

    // And now it really stops, or this proof would leave fault 4's own orphan.
    running
        .as_mut()
        .expect("è ancora acceso")
        .stop()
        .expect("spegnerlo");
    assert!(
        !ledger::pid_is_alive(pid),
        "resta acceso dopo lo stop: pid {pid}"
    );
}

static NEXT: AtomicU64 = AtomicU64::new(0);

fn temporary_path(label: &str) -> PathBuf {
    let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "sailor-live-{label}-{}-{sequence}.json",
        std::process::id()
    ))
}

/// **SURVIVING IS NOT ENOUGH: IT MUST SAY SO.**
///
/// The permanent constraint is "an interface that hides what is happening is
/// the opposite of the product", and a window that stays open showing old code
/// *without saying so* is worse than one that vanishes — whoever looks believes
/// their change had no effect. The message leaves the supervisor by a file,
/// because its reader is the program **already running**: the old one, which
/// has no channel to a supervisor just born.
#[test]
fn the_failure_message_reaches_whoever_is_watching() {
    let path = temporary_path("stato");

    assert!(
        LiveStatus::read(&path).is_none(),
        "un file che non esiste ha risposto qualcosa"
    );

    let failure = LiveStatus {
        supervisor_pid: std::process::id(),
        state: LiveState::BuildFailed,
        message: "error[E0425]: cannot find value `x` in this scope".to_owned(),
        changed_at: 1_700_000_000,
        running_since: Some(1_699_999_000),
    };
    failure.write(&path).expect("scrivere lo stato");

    let read = LiveStatus::read(&path).expect("rileggere lo stato");
    assert_eq!(read.state, LiveState::BuildFailed);
    assert!(
        read.message.contains("E0425"),
        "il messaggio è arrivato monco: {}",
        read.message
    );
    assert_eq!(
        read.running_since,
        Some(1_699_999_000),
        "chi guarda deve poter dire da quando è vecchio quello che vede"
    );

    let _ = std::fs::remove_file(&path);
}
