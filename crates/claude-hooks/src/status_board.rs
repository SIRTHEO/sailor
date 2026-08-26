//! Raccoglie dal disco i fatti che il cruscotto mostra. Il giudizio — cosa
//! merita una riga e cosa no, e come si scrive — sta tutto in
//! `guards::status_board`, che è puro e si prova senza toccare il mondo. Qui
//! non c'è nessuna decisione da ricopiare: solo le letture.
//!
//! NON È UN GANCIO: nessun evento lo invoca, lo si chiama a mano
//! (`claude-hooks stato`) quando si vuole sapere cosa sta facendo il sistema.
//! Per questo fallisce parlando invece di tacere: un gancio nel dubbio si
//! zittisce, uno strumento invocato apposta deve dire cosa non ha potuto
//! leggere — se tace, chi guarda conclude «non fa niente», che è esattamente
//! l'errore da cui nasce.

use guards::status_board::{Board, Drift, Produced, Service, Waiting};
use std::path::PathBuf;
use std::process::Command;

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn claude_dir() -> PathBuf {
    PathBuf::from(format!("{}/.claude", home()))
}

/// I servizi residenti di casa. L'elenco è corto e dichiarato: un cruscotto che
/// scopre da sé cosa sorvegliare mostra anche il rumore di sistema.
const SERVICES: &[(&str, &str)] = &[
    ("ciclo di riparazione", "com.theo.notte"),
    ("presidio della coda", "work.gyver.queue-watch"),
];

fn service_running(label: &str) -> bool {
    Command::new("launchctl")
        .args(["print", &format!("gui/501/{label}")])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("state = running"))
        .unwrap_or(false)
}

/// L'ultima decisione del ciclo, dalla coda del suo registro.
fn last_loop_decision() -> String {
    let path = claude_dir().join("state/notte/notte.log");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return String::new();
    };
    let Some(line) = text.lines().filter(|l| !l.trim().is_empty()).next_back() else {
        return String::new();
    };
    // La riga porta data, misure e poi «→ <decisione>»: si tiene la coda.
    match line.split_once('→') {
        Some((_, tail)) => tail.trim().to_string(),
        None => String::new(),
    }
}

fn today() -> String {
    Command::new("date")
        .arg("+%Y-%m-%d")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// I compiti chiusi oggi e quanti erano verdi.
fn tasks_today(day: &str) -> (usize, usize) {
    let dir = claude_dir().join("state/plancia/coda-notte/fatti");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return (0, 0);
    };
    let mut done = 0;
    let mut green = 0;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.contains(day) {
            continue;
        }
        done += 1;
        if std::fs::read_to_string(e.path())
            .map(|t| t.contains("notte-status: green"))
            .unwrap_or(false)
        {
            green += 1;
        }
    }
    (done, green)
}

fn commits_today(day: &str) -> usize {
    // `--since` con la data nuda vale «da adesso» quando la data è oggi: l'ora
    // va scritta, o un commit di stamattina sparisce da una lettura di adesso.
    Command::new("git")
        .args([
            "-C",
            &claude_dir().to_string_lossy(),
            "log",
            "--oneline",
            &format!("--since={day} 00:00:00"),
        ])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0)
}

/// Le voci di coda aperte, con da quante ore aspettano e per chi.
fn waiting_entries(day: &str) -> (Vec<Waiting>, usize) {
    let dir = claude_dir().join("state/plancia/segnalazioni");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return (Vec::new(), 0);
    };
    let mut open = Vec::new();
    let mut closed_today = 0;
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().map(|x| x != "md").unwrap_or(true) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let head: String = text.lines().take(12).collect::<Vec<_>>().join("\n");
        if head.contains("stato: chiusa") {
            if text.contains(day) {
                closed_today += 1;
            }
            continue;
        }
        if !head.contains("stato: aperta") {
            continue;
        }
        let for_whom = head
            .lines()
            .find_map(|l| l.strip_prefix("per: "))
            .unwrap_or("nessuno")
            .split(&['—', '-'][..])
            .next()
            .unwrap_or("nessuno")
            .trim()
            .to_string();
        let hours = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .map(|d| (d.as_secs() / 3600) as u32)
            .unwrap_or(0);
        let what = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        open.push(Waiting { what, for_whom, hours_open: hours });
    }
    // Prima le più vecchie: è l'ordine in cui si consumano.
    open.sort_by(|a, b| b.hours_open.cmp(&a.hours_open));
    (open, closed_today)
}

/// Gli scarti fra ciò che gira e ciò che dovrebbe girare.
fn drifts() -> Vec<Drift> {
    let mut out = Vec::new();
    let dir = claude_dir();

    let stamped = std::fs::read_to_string(dir.join("state/hooks-binary-commit"))
        .unwrap_or_default()
        .trim()
        .chars()
        .take(7)
        .collect::<String>();
    let head = Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if !stamped.is_empty() && !head.is_empty() && stamped != head {
        out.push(Drift { what: "binario dei ganci".into(), live: stamped, expected: head });
    }

    let seen = std::fs::read_to_string(dir.join("state/ronda-versione-vista"))
        .unwrap_or_default()
        .trim()
        .to_string();
    let installed = std::fs::read_dir(format!("{}/.local/share/claude/versions", home()))
        .ok()
        .map(|d| {
            let mut v: Vec<String> =
                d.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
            v.sort();
            v.pop().unwrap_or_default()
        })
        .unwrap_or_default();
    if !seen.is_empty() && !installed.is_empty() && seen != installed {
        out.push(Drift {
            what: "versione vista dalla ronda".into(),
            live: seen,
            expected: installed,
        });
    }

    out
}

pub fn run() -> i32 {
    let day = today();
    let (done, green) = tasks_today(&day);
    let (waiting, closed) = waiting_entries(&day);

    let services = SERVICES
        .iter()
        .map(|(name, label)| Service {
            name: (*name).to_string(),
            running: service_running(label),
            last_decision: if *label == "com.theo.notte" {
                last_loop_decision()
            } else {
                String::new()
            },
        })
        .collect();

    let board = Board {
        services,
        produced: Produced {
            tasks_done: done,
            tasks_green: green,
            commits: commits_today(&day),
            entries_closed: closed,
        },
        waiting,
        drifts: drifts(),
    };

    print!("{}", guards::status_board::render(&board));
    0
}
