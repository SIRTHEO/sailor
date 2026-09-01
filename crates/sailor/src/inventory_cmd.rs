//! `sailor inventory`: che cosa è installato su questa macchina, da dove viene,
//! e se è raggiungibile.
//!
//! Il giudizio sta nella libreria `inventory`; qui c'è solo l'interpretazione
//! degli argomenti e la stampa. Due uscite: leggibile da una persona, e `--json`
//! per la pagina — la stessa fonte, così l'elenco che si legge da terminale e
//! quello che si vede nella finestra non possono divergere.

use inventory::{collect_survey, default_roots, Entry, Inventory, Kind, Reach};
use ledger::{InventoryItem, InventoryScan, Ledger};

pub fn run(args: &[String]) -> i32 {
    let mut json = false;
    let mut only: Option<Kind> = None;
    let mut hidden = false;
    let mut record = false;
    let mut changes = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json = true,
            "--unreachable" => hidden = true,
            "--record" => record = true,
            "--changes" => changes = true,
            "--kind" => {
                i += 1;
                let Some(raw) = args.get(i) else {
                    eprintln!("--kind needs a value");
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

    // LA CASA LA CHIEDE A CHI LA POSSIEDE. Le basi di lavoro sono dichiarate —
    // `SAILOR_WORK_ROOTS`, o il file `work-roots` — e la casa dove sta quel file
    // la sa `ledger::sailor_home()`, che è l'unico posto dove quella regola vive.
    let survey = default_roots(ledger::sailor_home().as_deref());
    if !survey.bases_declared {
        eprintln!(
            "nessuna base di lavoro dichiarata: guardo solo la casa. \
             Dichiarale in `work-roots` dentro la casa di Sailor, una per riga, \
             oppure in `SAILOR_WORK_ROOTS` separate da due punti."
        );
    }
    for missing in &survey.unreadable {
        eprintln!(
            "could not look in {}: {}",
            missing.path.display(),
            missing.reason
        );
    }
    let found = collect_survey(&survey);

    if record {
        match deposit(&found) {
            Ok(message) => println!("{message}"),
            Err(error) => {
                eprintln!("the inventory was not stored: {error}");
                return 1;
            }
        }
    }
    if changes {
        match print_changes() {
            Ok(()) => {}
            Err(error) => {
                eprintln!("the store does not answer: {error}");
                return 1;
            }
        }
        return 0;
    }

    if json {
        match serde_json::to_string_pretty(&found) {
            Ok(text) => {
                println!("{text}");
                0
            }
            Err(error) => {
                eprintln!("the inventory will not be written: {error}");
                1
            }
        }
    } else {
        print_human(&found, only, hidden);
        0
    }
}

/// Le forme di `sailor inventory`, una per riga. Vedi `flow_cmd::USAGE` per il
/// motivo per cui è una costante pubblica invece di righe dentro la stampa.
pub const USAGE: &[&str] = &[
    "sailor inventory [--kind skill|agent|command|rule|hook] [--unreachable] [--json]",
    "sailor inventory --record        stores this scan",
    "sailor inventory --changes       what has appeared and what has gone",
];

fn print_usage() {
    eprintln!("usage:");
    for line in USAGE {
        eprintln!("  {line}");
    }
}

/// Deposita la scansione, così la prossima potrà dire che cosa è cambiato.
///
/// PERCHÉ UN COMANDO CHE CONTA NON BASTA. Un elenco ricalcolato ogni volta sa
/// dire che cosa c'è; non sa dire che cosa **non c'è più**, e quella è la
/// domanda da cui dipende ogni cancellazione. Senza, «sparito ieri» e «non è
/// mai esistito» si leggono uguali — e chi cancella leggendo un elenco così
/// cancella alla cieca.
fn deposit(found: &Inventory) -> Result<String, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|error| format!("l'orologio è indietro rispetto all'epoca: {error}"))?;
    let items = found
        .entries
        .iter()
        .map(|entry| InventoryItem {
            kind: entry.kind.label().to_string(),
            name: entry.name.clone(),
            origin: entry.origin.clone(),
            path: entry.path.clone(),
            reach: match &entry.reach {
                Reach::Active => "active".to_string(),
                Reach::Inactive(_) => "inactive".to_string(),
                Reach::Unknown(_) => "unknown".to_string(),
            },
            reason: match &entry.reach {
                Reach::Active => None,
                Reach::Inactive(reason) | Reach::Unknown(reason) => Some(reason.clone()),
            },
        })
        .collect();
    let ledger = open_ledger()?;
    ledger
        .record_inventory(&InventoryScan {
            taken_at: now,
            items,
        })
        .map_err(|error| error.to_string())?;
    Ok(format!(
        "scansione depositata: {} voci",
        found.entries.len()
    ))
}

/// **DOVE STA IL DEPOSITO LO SA UN POSTO SOLO**, e non è questo.
///
/// Fino al 01/09/2026 questa funzione ricomponeva `HOME/.claude/state/flussi` da
/// sé. Non era una copia inerte: era una copia **diversa**, perché
/// `ledger::default_directory()` guarda anche `SAILOR_LEDGER` e riconosce la
/// casa di un'installazione precedente. Con quella variabile impostata — come fa
/// chiunque provi qualcosa senza toccare il deposito vero — `sailor inventory`
/// scriveva il censimento in un deposito mentre ogni altro comando lo leggeva da
/// un altro: nessun errore, due depositi, e quello che si guardava risultava
/// vuoto. È la forma in cui il guasto 12 continua a ripresentarsi, un elenco
/// vuoto che ha l'aria di una risposta.
///
/// La sorveglianza è in `only_the_ledger_knows_where_the_ledger_lives`, che
/// guarda i sorgenti invece di confrontare due funzioni: due copie che sbagliano
/// insieme si confermano a vicenda, quindi l'ancora deve stare fuori da tutte e
/// due.
fn open_ledger() -> Result<Ledger, String> {
    let directory = ledger::default_directory()
        .ok_or_else(|| "HOME is not set: there is no telling where to open the store".to_owned())?;
    Ledger::open(&directory).map_err(|error| error.to_string())
}

/// Che cosa è comparso e che cosa è sparito, secondo il deposito.
fn print_changes() -> Result<(), String> {
    let ledger = open_ledger()?;
    let gone = ledger.inventory_gone().map_err(|e| e.to_string())?;
    let present = ledger.inventory_present().map_err(|e| e.to_string())?;

    if present.is_empty() && gone.is_empty() {
        println!("the store holds no scan yet: run one with --record");
        return Ok(());
    }

    println!("presenti: {}", present.len());
    let blocked = present.iter().filter(|item| item.reach != "active").count();
    if blocked > 0 {
        println!("di cui irraggiungibili: {blocked}");
    }
    println!();
    if gone.is_empty() {
        println!("sparite: nessuna");
    } else {
        println!("sparite: {}", gone.len());
        for item in &gone {
            println!(
                "  {:<10} {:<34} {}",
                item.kind,
                item.name,
                item.origin
            );
        }
    }
    Ok(())
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
    // DOVE NON SI È POTUTO GUARDARE STA ACCANTO A DOVE SI È GUARDATO, non in
    // fondo: chi legge un conteggio deve avere sott'occhio quanto di macchina è
    // rimasto fuori, o legge un numero credendolo il totale.
    if !found.unseen.is_empty() {
        println!("not looked at:");
        for missing in &found.unseen {
            println!("  {missing}");
        }
    }
    if !found.bases_declared {
        println!("no working base declared: this count is the home's alone");
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
