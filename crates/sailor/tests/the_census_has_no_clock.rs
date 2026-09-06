//! **THE CENSUS IS TRIGGERED, NOT CLOCKED.** Theo ruled timeouts out: no timer,
//! no wait loop, no watching in the background. The census is a function called
//! when an event arrives, and at no other moment.
//!
//! **A TEST AND NOT A LINE OF DESIGN**, because a poller is the most natural
//! thing to add: whoever comes next wants the state fresh, drops in a five-second
//! loop, and from then on Sailor eats the machine it claims to watch — a machine
//! that sleeps some seventy times a day, where a loop wakes to a world it did not
//! leave. A constraint written only in a document never goes red.
//!
//! The check reads the **shape** of the code, not its behaviour: it is coarse and
//! can let through a timer written in a form it does not know. It cannot accuse
//! wrongly, and adding a word to its list costs one line.

use std::path::{Path, PathBuf};

/// The signs of a clock inside the code.
///
/// **`now()` IS NOT HERE AND MUST NOT BE.** Reading the hour to date a fact is
/// the opposite of a timer: it is what makes a fact reconstructible. What is
/// forbidden is **waiting**, not **looking at the clock**.
const SIGNS_OF_A_CLOCK: &[&str] = &[
    "thread::sleep",
    "sleep(",
    "Duration::from",
    "loop {",
    "spawn(",
    "interval",
    "timeout",
    "recv_timeout",
    "set_interval",
];

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("il crate sta in <radice>/crates/sailor")
        .to_path_buf()
}

fn code_part(line: &str) -> &str {
    match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    }
}

fn tracking_sources() -> Vec<PathBuf> {
    let root = repository_root();
    let mut found = vec![root.join("crates/sailor/src/session_cmd.rs")];
    collect_under(&root.join("crates/sessions"), &mut found);
    found.retain(|path| path.exists());
    found
}

fn collect_under(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            collect_under(&path, found);
            continue;
        }
        if path.extension().and_then(|kind| kind.to_str()) == Some("rs") {
            found.push(path);
        }
    }
}

fn clocks_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let code = code_part(line);
        if let Some(sign) = SIGNS_OF_A_CLOCK.iter().find(|sign| code.contains(**sign)) {
            found.push(format!("riga {}: «{sign}» in: {}", number + 1, line.trim()));
        }
    }
    found
}

#[test]
fn the_tracking_waits_for_nothing_and_polls_nothing() {
    let sources = tracking_sources();
    assert!(
        sources.len() >= 4,
        "guardati {} sorgenti: la scansione non sta guardando dove crede",
        sources.len()
    );

    let mut ticking: Vec<String> = Vec::new();
    for path in &sources {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("leggere {}: {error}", path.display()));
        for problem in clocks_in(&text) {
            ticking.push(format!("{}: {problem}", path.display()));
        }
    }
    workspace::measured_against(
        sources.len(),
        "tracking sources read",
        SIGNS_OF_A_CLOCK.len(),
        "signs of a clock",
    );

    assert!(
        ticking.is_empty(),
        "il tracciamento ha un orologio dentro, e non deve:\n{}\n\n\
         Il censimento si chiama quando arriva un evento. Se serve sapere lo \
         stato più spesso, l'evento va mandato più spesso da chi lo sa — non \
         indovinato da un ciclo che si sveglia.",
        ticking.join("\n")
    );
}

/// Whoever measures gets measured: the detector must find a timer, and must let
/// through the reading of the hour that dates a fact.
#[test]
fn the_check_finds_a_timer_and_leaves_the_reading_of_the_hour_alone() {
    assert_eq!(
        clocks_in("    std::thread::sleep(Duration::from_secs(5));\n").len(),
        1,
        "il rilevatore non vede un'attesa"
    );
    assert!(
        clocks_in("    let at = now();\n").is_empty(),
        "datare un fatto non è aspettare, e questa prova non lo deve vietare"
    );
}
