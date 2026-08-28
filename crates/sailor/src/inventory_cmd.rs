//! `sailor inventory`: che cosa è installato su questa macchina, da dove viene,
//! e se è raggiungibile.
//!
//! Il giudizio sta nella libreria `inventory`; qui c'è solo l'interpretazione
//! degli argomenti e la stampa. Due uscite: leggibile da una persona, e `--json`
//! per la pagina — la stessa fonte, così l'elenco che si legge da terminale e
//! quello che si vede nella finestra non possono divergere.

use inventory::{collect, default_roots, Entry, Inventory, Kind, Reach};

pub fn run(args: &[String]) -> i32 {
    let mut json = false;
    let mut only: Option<Kind> = None;
    let mut hidden = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json = true,
            "--unreachable" => hidden = true,
            "--kind" => {
                i += 1;
                let Some(raw) = args.get(i) else {
                    eprintln!("--kind richiede un valore");
                    return 2;
                };
                match parse_kind(raw) {
                    Some(k) => only = Some(k),
                    None => {
                        eprintln!("--kind sconosciuto: {raw}");
                        print_usage();
                        return 2;
                    }
                }
            }
            other => {
                eprintln!("opzione sconosciuta: {other}");
                print_usage();
                return 2;
            }
        }
        i += 1;
    }

    let found = collect(&default_roots());
    if json {
        match serde_json::to_string_pretty(&found) {
            Ok(text) => {
                println!("{text}");
                0
            }
            Err(error) => {
                eprintln!("l'inventario non si lascia scrivere: {error}");
                1
            }
        }
    } else {
        print_human(&found, only, hidden);
        0
    }
}

fn print_usage() {
    eprintln!("uso:");
    eprintln!("  sailor inventory [--kind skill|agent|command|rule|hook] [--unreachable] [--json]");
}

fn parse_kind(raw: &str) -> Option<Kind> {
    match raw {
        "skill" | "competenza" => Some(Kind::Skill),
        "agent" | "agente" => Some(Kind::Agent),
        "command" | "comando" => Some(Kind::Command),
        "rule" | "regola" => Some(Kind::Rule),
        "hook" | "gancio" => Some(Kind::Hook),
        _ => None,
    }
}

fn print_human(found: &Inventory, only: Option<Kind>, unreachable_only: bool) {
    println!("radici guardate:");
    for root in &found.roots {
        println!("  {root}");
    }
    println!();

    let kinds = [
        Kind::Skill,
        Kind::Agent,
        Kind::Command,
        Kind::Rule,
        Kind::Hook,
    ];
    for kind in kinds {
        if only.is_some_and(|k| k != kind) {
            continue;
        }
        let entries: Vec<&Entry> = found
            .of(kind)
            .into_iter()
            .filter(|e| !unreachable_only || !matches!(e.reach, Reach::Active))
            .collect();
        let blocked = found
            .of(kind)
            .iter()
            .filter(|e| matches!(e.reach, Reach::Inactive(_)))
            .count();
        println!(
            "{} — {} in tutto{}",
            kind.label(),
            found.count(kind),
            if blocked > 0 {
                format!(", di cui {blocked} irraggiungibili")
            } else {
                String::new()
            }
        );
        for entry in entries {
            let mark = match &entry.reach {
                Reach::Active => String::new(),
                Reach::Inactive(reason) => format!("  ✗ {reason}"),
                Reach::Unknown(reason) => format!("  ? {reason}"),
            };
            println!("  {:<34} {:<18}{}", entry.name, entry.origin, mark);
        }
        println!();
    }
}
