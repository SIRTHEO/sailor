//! **La finestra sopravvive a una compilazione fallita — guasto 11.**
//!
//! COSA DICEVA IL GUASTO, E PERCHÉ NON ERA ESATTO. La voce 11 di
//! `docs/guasti-incontrati.md` diceva: «in modalità viva un errore di
//! compilazione in un crate qualunque **uccide la finestra**». Leggendo
//! `tauri-cli` 2.11.4 il meccanismo è un altro, e la differenza cambia la
//! riparazione. In `src/interface/rust.rs`, dentro `run_dev_watcher`, il giro è:
//!
//! ```text
//! child.kill()          // il programma acceso viene ucciso PRIMA
//! let _ = child.wait();
//! child = run(...)?;    // e solo dopo si compila
//! ```
//!
//! La finestra non muore *per* l'errore di compilazione: muore a **ogni** file
//! toccato, sempre, e l'errore di compilazione è soltanto il motivo per cui non
//! ne ritorna una. Il difetto non è nella gestione dell'errore — è
//! **nell'ordine**.
//!
//! Da qui la forma della riparazione, che è tutta in una riga di sequenza:
//! **prima si costruisce, e si tocca ciò che è acceso solo se la costruzione è
//! riuscita.** Queste prove esistono per tenere quell'ordine, ed è l'ordine —
//! non un messaggio d'errore — quello che va rotto per vederle rosse.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use supervisor::{rebuild_then_swap, BuildOutcome, LiveState, LiveStatus, Rebuild, Running};

/// Un programma finto che conta quante volte lo hanno fermato.
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

/// **IL CUORE DEL GUASTO 11.**
///
/// Con l'ordine di `tauri-cli` — fermare, poi costruire — questa prova è rossa
/// su tutte e tre le asserzioni: `stops` vale 1, `running` è `None`, e l'esito
/// non è `KeptRunning`. È la mutazione da fare per verificarla, e non un
/// messaggio d'errore da cambiare: il difetto originale era la sequenza.
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

/// L'altra metà: quando la costruzione riesce, il vecchio **deve** cedere il
/// posto. Senza questa prova la riparazione più comoda — non fermare mai
/// niente — passerebbe la prova di sopra e lascerebbe la modalità viva ferma
/// alla prima versione per sempre.
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

/// **UN PROCESSO VERO, NON UN FINTO.**
///
/// Le due prove di sopra misurano l'ordine su un oggetto costruito apposta per
/// obbedire. Questa accende un processo del sistema operativo, fa fallire la
/// costruzione, e poi chiede al sistema se quel pid respira ancora. È la
/// differenza fra «ho scritto il codice che dovrebbe» e «l'ho visto».
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

    let mut running = Some(Child(child));
    assert!(ledger::pid_is_alive(pid), "il processo non è nemmeno partito");

    let outcome = rebuild_then_swap(
        &mut running,
        || BuildOutcome::Failed {
            message: "could not compile `ledger`".to_owned(),
        },
        || panic!("non si riavvia niente"),
    );

    assert!(matches!(outcome, Rebuild::KeptRunning { .. }), "esito: {outcome:?}");
    assert!(
        ledger::pid_is_alive(pid),
        "il processo acceso è morto per una compilazione fallita: pid {pid}"
    );

    // E ora si spegne davvero, o questa prova lascerebbe l'orfano del guasto 4.
    running.as_mut().expect("è ancora acceso").stop().expect("spegnerlo");
    assert!(!ledger::pid_is_alive(pid), "resta acceso dopo lo stop: pid {pid}");
}

static NEXT: AtomicU64 = AtomicU64::new(0);

fn temporary_path(label: &str) -> PathBuf {
    let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "sailor-live-{label}-{}-{sequence}.json",
        std::process::id()
    ))
}

/// **NON BASTA SOPRAVVIVERE: DEVE DIRLO.**
///
/// Il vincolo permanente è «un'interfaccia che nasconde cosa succede è il
/// contrario del prodotto», e una finestra che resta aperta mostrando codice
/// vecchio *senza dirlo* è peggio di una che sparisce — chi guarda crede che
/// la sua modifica non abbia avuto effetto. Il messaggio esce dal supervisore
/// per un file, perché chi deve leggerlo è il programma **già acceso**: quello
/// vecchio, che non ha nessun canale col supervisore appena nato.
#[test]
fn the_failure_message_reaches_whoever_is_watching() {
    let path = temporary_path("stato");

    assert!(
        LiveStatus::read(&path).is_none(),
        "un file che non esiste ha risposto qualcosa"
    );

    let failure = LiveStatus {
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
