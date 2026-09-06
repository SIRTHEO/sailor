//! **The window says the rebuild failed, instead of vanishing.**
//!
//! Half of fault 11 is that the window survives; the other half is that it
//! **declares** it. A window that stays open showing old code without saying so
//! is worse than one that vanishes: the watcher believes their change had no
//! effect and hunts the defect where it is not. It is the permanent constraint
//! "an interface that hides what happens is the opposite of the product", and
//! fault 30 has already paid for it once.
//!
//! **WHY THE TITLE, AND NOT AN EVENT ALONE.** The `live-status` event exists,
//! and the canvas can draw whatever it likes on top. But the program that must
//! break the news is the one **already running** — built before the fault
//! existed — and its page is the old one: were the news to live in the page
//! alone, it would fail to arrive the very first time it was needed. The window
//! title is written by the native shell, so it arrives anyway.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use supervisor::{LiveState, LiveStatus, SwapRequest};

/// How often the status file is looked at.
///
/// Half a second: under the threshold where it is noticed, over the threshold
/// where a poll costs. The file is about ten lines.
const LOOK_EVERY: Duration = Duration::from_millis(500);

/// The resting title. It is written in `tauri.conf.json` as well, and this is
/// the only place from which this module can put it back.
const CALM_TITLE: &str = "Sailor";

/// What the window hands back to whoever asks how live mode stands.
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

/// Starts the thread that watches the status file and reports.
///
/// **IT KILLS NOTHING, EVER.** Every error here — the file absent, unreadable,
/// half written — reads as "I do not know", and the window carries on. A live
/// mode watchman that brought the window down would redo fault 11 from the side
/// nobody would look at.
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
    // The canvas draws whatever it likes on top; the title is the minimum that
    // arrives even without it.
    let _ = app.emit("live-status", status);
    crate::events::emit(app, "build", status);

    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let title = match status.state {
        LiveState::BuildFailed => {
            // **THE SECOND COUNT DOES NOT FIT THE TITLE, AND THAT IS FINE.**
            // What matters here is that the watcher knows *what they see is
            // old*; how long for is said by the event, where there is room.
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
