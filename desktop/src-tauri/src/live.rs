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

use supervisor::{LiveState, LiveStatus, SwapRequest};

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
    said_by_somebody_still_there(LiveStatus::read(&status_path()))
}

/// **A STATUS OUTLIVES WHOEVER WROTE IT.** The file stays on disk when the
/// supervisor stops, so a window opened tomorrow — or a released one, which
/// never had a supervisor — would read «a build is waiting» and offer a
/// gesture nobody is listening for. A pid of `0` comes from a file written
/// before the field existed: it is «cannot tell», and it is shown.
fn said_by_somebody_still_there(status: Option<LiveStatus>) -> Option<LiveStatus> {
    let status = status?;
    if status.supervisor_pid == 0 || ledger::pid_is_alive(status.supervisor_pid) {
        Some(status)
    } else {
        None
    }
}

fn status_path() -> std::path::PathBuf {
    ledger::sailor_home()
        .map(|home| LiveStatus::path_in(&home))
        .unwrap_or_else(|| std::env::temp_dir().join(supervisor::STATUS_FILE))
}

fn swap_path() -> std::path::PathBuf {
    ledger::sailor_home()
        .map(|home| SwapRequest::path_in(&home))
        .unwrap_or_else(|| std::env::temp_dir().join(supervisor::SWAP_FILE))
}

/// Asks for the build that is waiting. **This window ends when it is granted**,
/// which is why nothing else asks: the gesture belongs to the person.
#[tauri::command]
pub fn take_new_build() -> Result<(), String> {
    SwapRequest::ask(&swap_path())
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
            let current = said_by_somebody_still_there(LiveStatus::read(&status_path()));
            if current == last {
                continue;
            }
            match current.as_ref() {
                Some(status) => announce(&app, status),
                // The supervisor went: the title goes back to being a name.
                None => calm(&app),
            }
            last = current;
        }
    });
}

/// Puts the title back to the resting one. A window that kept «rebuilding…»
/// after the supervisor stopped would be waiting for news that cannot come.
fn calm(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_title(CALM_TITLE);
    }
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
        LiveState::Ready => "Sailor — a new build is waiting".to_owned(),
        LiveState::Running => CALM_TITLE.to_owned(),
    };
    let _ = window.set_title(&title);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(pid: u32) -> LiveStatus {
        LiveStatus {
            state: LiveState::Ready,
            message: String::new(),
            changed_at: 100,
            running_since: Some(90),
            supervisor_pid: pid,
        }
    }

    /// **A BUILD NOBODY IS HOLDING IS NOT A BUILD YOU CAN TAKE.** The status
    /// file stays on disk after the supervisor stops, and a released window
    /// never had one at all: read as it is, both would offer a gesture that
    /// reaches nobody.
    #[test]
    fn a_status_whose_supervisor_is_gone_is_no_status_at_all() {
        // This process is alive by definition, and no pid is 0 but the file
        // written before the field existed.
        assert!(said_by_somebody_still_there(Some(status(std::process::id()))).is_some());
        assert!(said_by_somebody_still_there(Some(status(0))).is_some());
        assert!(said_by_somebody_still_there(None).is_none());

        // A number that is not a pid at all. It matters that this is a no:
        // read as a signed integer it would address a whole process group.
        assert!(said_by_somebody_still_there(Some(status(u32::MAX))).is_none());
    }
}
