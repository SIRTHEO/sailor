//! `sailor inventory`: che cosa è installato su questa macchina, da dove viene,
//! e se è raggiungibile.
//!
//! Il giudizio sta nella libreria `inventory`; qui c'è solo l'interpretazione
//! degli argomenti e la stampa. Due uscite: leggibile da una persona, e `--json`
//! per la pagina — la stessa fonte, così l'elenco che si legge da terminale e
//! quello che si vede nella finestra non possono divergere.

use crate::Form;
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
                    eprintln!(
                        "{}",
                        catalogue::say("cli.option_wants_a_value", &[("option", "--kind")])
                    );
                    return 2;
                };
                match parse_kind(raw) {
                    Some(k) => only = Some(k),
                    None => {
                        eprintln!(
                            "{}",
                            catalogue::say("cli.inventory.unknown_kind", &[("raw", raw)])
                        );
                        print_usage();
                        return 2;
                    }
                }
            }
            other => {
                eprintln!(
                    "{}",
                    catalogue::say("cli.unknown_option", &[("option", other)])
                );
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
        eprintln!("{}", catalogue::say("cli.inventory.no_bases_declared", &[]));
    }
    for missing in &survey.unreadable {
        eprintln!(
            "{}",
            catalogue::say(
                "cli.inventory.could_not_look",
                &[
                    ("path", &missing.path.display().to_string()),
                    ("reason", &missing.reason)
                ],
            )
        );
    }
    let found = collect_survey(&survey);

    if record {
        match deposit(&found) {
            Ok(message) => println!("{message}"),
            Err(error) => {
                eprintln!(
                    "{}",
                    catalogue::say("cli.inventory.not_stored", &[("error", &error)])
                );
                return 1;
            }
        }
    }
    if changes {
        match print_changes() {
            Ok(()) => {}
            Err(error) => {
                eprintln!(
                    "{}",
                    catalogue::say("cli.inventory.store_silent", &[("error", &error)])
                );
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
                eprintln!(
                    "{}",
                    catalogue::say(
                        "cli.inventory.will_not_be_written",
                        &[("error", &error.to_string())]
                    )
                );
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
pub const USAGE: &[Form] = &[
    Form {
        form: "sailor inventory [--kind skill|agent|command|rule|hook] [--unreachable] [--json]",
        says_key: "",
    },
    Form {
        form: "sailor inventory --record",
        says_key: "cli.inventory.form.record",
    },
    Form {
        form: "sailor inventory --changes",
        says_key: "cli.inventory.form.changes",
    },
];

fn print_usage() {
    eprintln!("{}", catalogue::say("cli.usage_heading", &[]));
    for line in crate::forms_as_lines(USAGE) {
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
        .map_err(|error| {
            catalogue::say(
                "cli.inventory.clock_behind_epoch",
                &[("error", &error.to_string())],
            )
        })?;
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
    Ok(catalogue::say(
        "cli.inventory.scan_stored",
        &[("count", &found.entries.len().to_string())],
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
    let directory =
        ledger::default_directory().ok_or_else(|| catalogue::say("cli.no_home", &[]))?;
    Ledger::open(&directory).map_err(|error| error.to_string())
}

/// Che cosa è comparso e che cosa è sparito, secondo il deposito.
fn print_changes() -> Result<(), String> {
    let ledger = open_ledger()?;
    let gone = ledger.inventory_gone().map_err(|e| e.to_string())?;
    let present = ledger.inventory_present().map_err(|e| e.to_string())?;

    if present.is_empty() && gone.is_empty() {
        println!("{}", catalogue::say("cli.inventory.no_scan_yet", &[]));
        return Ok(());
    }

    println!(
        "{}",
        catalogue::say(
            "cli.inventory.present",
            &[("count", &present.len().to_string())]
        )
    );
    let blocked = present.iter().filter(|item| item.reach != "active").count();
    if blocked > 0 {
        println!(
            "{}",
            catalogue::say(
                "cli.inventory.of_which_unreachable",
                &[("count", &blocked.to_string())]
            )
        );
    }
    println!();
    if gone.is_empty() {
        println!("{}", catalogue::say("cli.inventory.none_gone", &[]));
    } else {
        println!(
            "{}",
            catalogue::say("cli.inventory.gone", &[("count", &gone.len().to_string())])
        );
        for item in &gone {
            println!("  {:<10} {:<34} {}", item.kind, item.name, item.origin);
        }
    }
    // **WHICH COMMAND LINES WERE LOOKED AT, ALWAYS.** An inventory that names
    // only what it found reads as "you have nothing" on a machine holding a
    // command line nobody declared — and that is the answer this crate was
    // rewritten to stop giving.
    println!(
        "{}",
        catalogue::say(
            "cli.inventory.command_lines_looked_at",
            &[(
                "count",
                &inventory::extensions::how_many_declared(ledger::sailor_home().as_deref())
                    .to_string()
            )]
        )
    );
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
    println!("{}", catalogue::say("cli.inventory.roots_looked_at", &[]));
    for root in &found.roots {
        println!("  {root}");
    }
    // DOVE NON SI È POTUTO GUARDARE STA ACCANTO A DOVE SI È GUARDATO, non in
    // fondo: chi legge un conteggio deve avere sott'occhio quanto di macchina è
    // rimasto fuori, o legge un numero credendolo il totale.
    if !found.unseen.is_empty() {
        println!("{}", catalogue::say("cli.inventory.not_looked_at", &[]));
        for missing in &found.unseen {
            println!("  {missing}");
        }
    }
    if !found.bases_declared {
        println!("{}", catalogue::say("cli.inventory.no_working_base", &[]));
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
        let of_which = if blocked > 0 {
            catalogue::say(
                "cli.inventory.of_which_blocked",
                &[("count", &blocked.to_string())],
            )
        } else {
            String::new()
        };
        println!(
            "{}",
            catalogue::say(
                "cli.inventory.kind_in_all",
                &[
                    ("kind", kind.label()),
                    ("count", &found.count(kind).to_string()),
                    ("of_which", &of_which)
                ],
            )
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
