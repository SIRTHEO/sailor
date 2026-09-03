//! **La finestra dice che la ricostruzione è fallita, invece di sparire.**
//!
//! Metà del guasto 11 è che la finestra sopravviva; l'altra metà è che lo
//! **dichiari**. Una finestra che resta aperta mostrando codice vecchio senza
//! dirlo è peggio di una che sparisce: chi guarda crede che la sua modifica non
//! abbia avuto effetto e va a cercare il difetto dove non c'è. È il vincolo
//! permanente «un'interfaccia che nasconde cosa succede è il contrario del
//! prodotto», e il guasto 30 l'ha già pagato una volta.
//!
//! **PERCHÉ IL TITOLO, E NON SOLO UN EVENTO.** L'evento `live-status` c'è, e la
//! tela può disegnarci sopra quello che vuole. Ma il programma che deve dare la
//! notizia è quello **già acceso** — cioè costruito prima che il guasto
//! esistesse — e la sua pagina è quella vecchia: se la notizia vivesse solo
//! nella pagina, la prima volta che serve non arriverebbe. Il titolo della
//! finestra lo scrive il guscio nativo, quindi arriva comunque.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use supervisor::{LiveState, LiveStatus};

/// Ogni quanto si guarda il file di stato.
///
/// Mezzo secondo: sotto la soglia in cui si nota, e sopra quella in cui un
/// sondaggio costa. Il file è una decina di righe.
const LOOK_EVERY: Duration = Duration::from_millis(500);

/// Il titolo di riposo. Sta scritto anche in `tauri.conf.json`, ed è l'unico
/// posto da cui questo modulo lo può rimettere a posto.
const CALM_TITLE: &str = "Sailor";

/// Quello che la finestra restituisce a chi chiede com'è messa la modalità viva.
#[tauri::command]
pub fn live_status() -> Option<LiveStatus> {
    LiveStatus::read(&status_path())
}

fn status_path() -> std::path::PathBuf {
    ledger::sailor_home()
        .map(|home| LiveStatus::path_in(&home))
        .unwrap_or_else(|| std::env::temp_dir().join(supervisor::STATUS_FILE))
}

/// Accende il filo che guarda il file di stato e riferisce.
///
/// **NON FA MORIRE NIENTE, MAI.** Ogni errore qui — il file assente, illeggibile,
/// a metà — si legge come «non so», e la finestra continua. Un guardiano della
/// modalità viva che facesse cadere la finestra rifarebbe il guasto 11 dalla
/// parte da cui nessuno lo cercherebbe.
pub fn watch(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let mut last: Option<LiveStatus> = None;
        loop {
            std::thread::sleep(LOOK_EVERY);
            let current = LiveStatus::read(&status_path());
            if current == last {
                continue;
            }
            if let Some(status) = current.as_ref() {
                announce(&app, status);
            }
            last = current;
        }
    });
}

fn announce(app: &AppHandle, status: &LiveStatus) {
    // La tela ci disegna sopra quello che vuole; il titolo è il minimo che
    // arriva anche senza di lei.
    let _ = app.emit("live-status", status);
    crate::events::emit(app, "build", status);

    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let title = match status.state {
        LiveState::BuildFailed => {
            // **IL NUMERO DI SECONDI NON CI STA NEL TITOLO, E VA BENE.** Qui
            // serve che chi guarda sappia che *quello che vede è vecchio*; da
            // quanto lo dice l'evento, dove c'è spazio.
            "Sailor — rebuild FAILED: you are looking at the last good version".to_owned()
        }
        LiveState::Building => "Sailor — rebuilding…".to_owned(),
        LiveState::Running => CALM_TITLE.to_owned(),
    };
    let _ = window.set_title(&title);
}
