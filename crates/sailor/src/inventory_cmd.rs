//! `sailor inventory`: what is installed on this machine, where it comes from,
//! and whether it can be reached.
//!
//! The judgement lives in the `inventory` library; only argument parsing and
//! printing live here. Two outputs: readable by a person, and `--json` for the
//! page — one source, so the list read from a terminal and the one seen in the
//! window cannot diverge.

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
                match Kind::from_label(raw) {
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

    // THE HOUSE IS ASKED OF WHOEVER OWNS IT. The work roots are declared —
    // `SAILOR_WORK_ROOTS`, or the `work-roots` file — and the house that file
    // sits in is known to `ledger::sailor_home()`, the one place that rule lives.
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

/// The shapes of `sailor inventory`, one per line. See `flow_cmd::USAGE` for
/// why this is a public constant instead of lines inside the printing.
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

/// Deposits the scan, so the next one can say what changed.
///
/// WHY A COMMAND THAT ONLY COUNTS IS NOT ENOUGH. A list recomputed every time
/// can say what is there; it cannot say what is **no longer** there, and every
/// deletion hangs on that question. Without it, «gone yesterday» and «never
/// existed» read alike — and whoever deletes off such a list deletes blind.
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

/// **ONE PLACE KNOWS WHERE THE LEDGER LIVES**, and it is not this one.
///
/// Recomposing `HOME/.claude/state/flussi` here is no inert copy but a
/// **different** one: `ledger::default_directory()` also reads `SAILOR_LEDGER`
/// and recognises an earlier install's house, so with that variable set the
/// census went to one ledger while every other command read another — no error,
/// two ledgers, and the one being looked at empty. That is fault 12 recurring.
/// The guard `only_the_ledger_knows_where_the_ledger_lives` reads the sources
/// rather than comparing two functions, since two copies wrong together confirm
/// each other and the anchor must sit outside both.
fn open_ledger() -> Result<Ledger, String> {
    let directory =
        ledger::default_directory().ok_or_else(|| catalogue::say("cli.no_home", &[]))?;
    Ledger::open(&directory).map_err(|error| error.to_string())
}

/// What appeared and what vanished, according to the ledger.
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

fn print_human(found: &Inventory, only: Option<Kind>, unreachable_only: bool) {
    println!("{}", catalogue::say("cli.inventory.roots_looked_at", &[]));
    for root in &found.roots {
        println!("  {root}");
    }
    // WHERE NOBODY COULD LOOK SITS BESIDE WHERE THEY DID, not at the bottom:
    // whoever reads a count must have in view how much of the machine stayed
    // out, or reads a number believing it is the total.
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

    for kind in Kind::ALL {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_option_knows_every_family_and_nothing_else() {
        for kind in Kind::ALL {
            assert_eq!(Kind::from_label(kind.label()), Some(kind), "{}", kind.label());
        }
        assert_eq!(Kind::from_label("competenza"), None);
    }
}
