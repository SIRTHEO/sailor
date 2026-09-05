//! `sailor remember <type> <label> <value…>`: a person writes a memory by hand,
//! through the same door a flow uses.

use actions::memory::Memory;
use flow::ActionError;
use ledger::Ledger;
use std::path::Path;

pub const USAGE: &[crate::Form] = &[crate::Form {
    form: "sailor remember <user|feedback|project|reference> <label> <value...>",
    says_key: "cli.remember.says",
}];

pub fn run(args: &[String]) -> i32 {
    let [kind, label, value @ ..] = args else {
        eprintln!("sailor remember: {}", catalogue::say("cli.remember.wants_three", &[("usage", USAGE[0].form)]));
        return 2;
    };
    if value.is_empty() {
        eprintln!("sailor remember: {}", catalogue::say("cli.remember.wants_three", &[("usage", USAGE[0].form)]));
        return 2;
    }
    let ledger = match ledger::Ledger::open(ui::gather::default_ledger_dir()) {
        Ok(ledger) => ledger,
        Err(error) => {
            eprintln!("sailor remember: {error}");
            return 1;
        }
    };
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    let memory = Memory {
        kind: kind.clone(),
        label: label.clone(),
        value: value.join(" "),
        provenance: "a person, by hand".to_owned(),
        modified: at,
        valid_from: at,
        valid_until: None,
    };
    match kept_by_hand(&ledger, ledger::sailor_home().as_deref(), memory) {
        Ok(kept) => {
            println!(
                "{}",
                catalogue::say(
                    "cli.remember.kept",
                    &[("label", &kept.label), ("type", &kept.kind)],
                )
            );
            0
        }
        Err(error) => {
            eprintln!(
                "sailor remember: {}",
                catalogue::say(&format!("run.failure.{}", error.class), &[])
            );
            eprintln!("   {}", error.said);
            1
        }
    }
}

/// The write, and the page every command line reads refreshed behind it: a
/// memory kept in the ledger alone is one no command line is handed.
fn kept_by_hand(ledger: &Ledger, home: Option<&Path>, memory: Memory) -> Result<Memory, Box<ActionError>> {
    let kept = actions::memory::remember(ledger, memory).map_err(Box::new)?;
    if let Some(home) = home {
        actions::memory::write_page(ledger, home).map_err(Box::new)?;
    }
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a person writes by hand is on the page before the command returns.
    #[test]
    fn a_memory_written_by_hand_is_on_the_page() {
        let dir = std::env::temp_dir().join(format!("sailor-remember-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ledger = Ledger::open(dir.join("ledger")).expect("a ledger");
        let home = dir.join("home");
        let memory = Memory {
            kind: "project".to_owned(),
            label: "the trunk".to_owned(),
            value: "sorgenti".to_owned(),
            provenance: "test".to_owned(),
            modified: 10,
            valid_from: 10,
            valid_until: None,
        };

        let kept = kept_by_hand(&ledger, Some(&home), memory).expect("kept");
        let page = std::fs::read_to_string(actions::memory::page_path(&home)).expect("a page");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(kept.label, "the trunk");
        assert_eq!(page, "- **the trunk** (project): sorgenti");
    }
}
