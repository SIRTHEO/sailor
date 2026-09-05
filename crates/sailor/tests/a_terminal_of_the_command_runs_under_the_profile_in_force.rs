//! **THE WINDOW OPENED TERMINALS UNDER THE PROFILE IN FORCE AND THE COMMAND
//! DID NOT.** One of the two was right, and the other lit engines that
//! answered as whoever the outer shell happened to be.
//!
//! The real binary, in a real pseudo-terminal, with a store of this test's
//! own: what the program inside prints is what it was actually given.

use std::ffi::OsStr;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use terminal::pty::{Pty, Size};
use terminal::Workspace;

fn scratch(name: &str) -> PathBuf {
    terminal::scratch::directory(name).expect("a scratch directory")
}

/// A store naming an active profile for a command line whose home moves.
fn a_store_with_a_profile_in_force(at: &PathBuf, home: &str) {
    let store = format!(
        r#"{{"profiles":[{{"name":"la-mia","cli_id":"codex","home_dir":"{home}"}}],
             "active":{{"codex":"la-mia"}}}}"#
    );
    std::fs::write(at, store).expect("the store");
}

fn seen(shown: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8_lossy(&shown.lock().expect("the lock does not panic").clone()).into_owned()
}

#[test]
fn the_engine_inside_is_given_the_home_the_active_profile_names() {
    let directory = scratch("carburante");
    let store = directory.join("profili.json");
    let home = directory.join("una-casa");
    std::fs::create_dir_all(&home).expect("the home");
    a_store_with_a_profile_in_force(&store, &home.display().to_string());

    let workspace = Workspace::open("/tmp").expect("open the workspace");
    let binary = env!("CARGO_BIN_EXE_sailor");
    let arguments: Vec<&OsStr> = vec![
        OsStr::new("terminal"),
        OsStr::new("run"),
        OsStr::new("--store"),
        directory.as_os_str(),
        OsStr::new("--"),
        OsStr::new("/bin/sh"),
        OsStr::new("-c"),
        OsStr::new("echo the-home-is-[$CODEX_HOME]"),
    ];
    let outer = Arc::new(
        Pty::open(
            &workspace,
            OsStr::new(binary),
            &arguments,
            Size::default(),
            // The store this test wrote, and nothing of the machine's own.
            &[(
                "PROFILES_STATE_PATH".to_owned(),
                store.display().to_string(),
            )],
        )
        .expect("open a terminal with sailor terminal inside"),
    );

    let shown = Arc::new(Mutex::new(Vec::new()));
    let mut reader = outer.reader().expect("a second end for whoever reads");
    let filling = Arc::clone(&shown);
    std::thread::spawn(move || {
        let mut buffer = [0u8; 1024];
        while let Ok(read) = reader.read(&mut buffer) {
            if read == 0 {
                return;
            }
            filling
                .lock()
                .expect("the lock does not panic")
                .extend_from_slice(&buffer[..read]);
        }
    });

    let wanted = format!("the-home-is-[{}]", home.display());
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && !seen(&shown).contains(&wanted) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let said = seen(&shown);
    assert!(
        said.contains(&wanted),
        "the terminal did not open under the profile in force. It said: {said}"
    );
    // THE ABSURD CONTROL: an empty variable would satisfy a looser check.
    assert!(
        !said.contains("the-home-is-[]"),
        "the variable arrived empty: {said}"
    );
    let _ = outer.close();
}
