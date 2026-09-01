//! **IL CENSIMENTO È INNESCATO, NON A OROLOGIO.**
//!
//! Theo ha escluso esplicitamente i timeout: niente timer, niente cicli di
//! attesa, niente sorveglianza in sottofondo. Il censimento è una funzione che
//! si chiama quando arriva un evento, e in nessun altro momento.
//!
//! **PERCHÉ UNA PROVA E NON UNA RIGA DI DISEGNO.** Un poller è la cosa più
//! naturale da aggiungere al mondo: chi arriva dopo vuole «lo stato aggiornato»
//! e mette un ciclo da cinque secondi, e da quel momento Sailor consuma la
//! macchina che dice di osservare — su una macchina che dorme circa settanta
//! volte al giorno, per giunta, dove un ciclo si sveglia e trova un mondo
//! diverso da quello che aveva lasciato. Un vincolo scritto solo in un
//! documento non diventa rosso mai.
//!
//! Questa prova guarda la **forma** del codice, non il suo comportamento, e lo
//! dichiara: è un controllo grossolano che può lasciar passare un timer scritto
//! in un modo che non conosce. Non può però accusare a torto, e il costo di
//! aggiungere una parola al suo elenco è una riga.

use std::path::{Path, PathBuf};

/// I segni di un orologio dentro il codice.
///
/// **`now()` NON C'È E NON CI DEVE STARE.** Leggere che ore sono per datare un
/// fatto è il contrario di un timer: è ciò che rende un fatto ricostruibile.
/// Quello che è vietato è **aspettare**, non **guardare l'ora**.
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

    assert!(
        ticking.is_empty(),
        "il tracciamento ha un orologio dentro, e non deve:\n{}\n\n\
         Il censimento si chiama quando arriva un evento. Se serve sapere lo \
         stato più spesso, l'evento va mandato più spesso da chi lo sa — non \
         indovinato da un ciclo che si sveglia.",
        ticking.join("\n")
    );
}

/// Chi misura va misurato: il rilevatore deve trovare un timer, e deve lasciar
/// passare la lettura dell'ora che data i fatti.
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
