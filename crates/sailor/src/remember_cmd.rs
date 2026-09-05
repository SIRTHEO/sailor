//! `sailor remember [--global] <type> <label> <value…>`: a person writes a
//! memory by hand, through the same door a flow uses, for the tree they are
//! typing in — or for every tree.

use actions::memory::Memory;
use flow::ActionError;
use ledger::Ledger;
use std::path::Path;

pub const USAGE: &[crate::Form] = &[crate::Form {
    form: "sailor remember [--global] <user|feedback|project|reference> <label> <value...>",
    says_key: "cli.remember.says",
}];

/// The word that makes a memory hold in every tree instead of this one.
const GLOBAL: &str = "--global";

pub fn run(args: &[String]) -> i32 {
    let here = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let Some(memory) = memory_typed(args, &here, now()) else {
        eprintln!("sailor remember: {}", catalogue::say("cli.remember.wants_three", &[("usage", USAGE[0].form)]));
        return 2;
    };
    let ledger = match ledger::Ledger::open(ui::gather::default_ledger_dir()) {
        Ok(ledger) => ledger,
        Err(error) => {
            eprintln!("sailor remember: {error}");
            return 1;
        }
    };
    match kept_by_hand(&ledger, ledger::sailor_home().as_deref(), memory) {
        Ok(kept) => {
            println!("{}", said_kept(&kept));
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

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// The memory the words describe, for the checkout around `here` — or for
/// every tree, when `--global` is among them or no checkout is. `None` when
/// the words are not a type, a label and a value.
fn memory_typed(args: &[String], here: &Path, at: i64) -> Option<Memory> {
    let global = args.iter().any(|word| word == GLOBAL);
    let words: Vec<&str> = args.iter().map(String::as_str).filter(|word| *word != GLOBAL).collect();
    let [kind, label, value @ ..] = words.as_slice() else {
        return None;
    };
    if value.is_empty() {
        return None;
    }
    let tree = if global { None } else { workspace::tree_around(here).map(|tree| tree.display().to_string()) };
    Some(Memory {
        kind: (*kind).to_owned(),
        label: (*label).to_owned(),
        value: value.join(" "),
        provenance: "a person, by hand".to_owned(),
        modified: at,
        valid_from: at,
        valid_until: None,
        tree,
    })
}

/// What is said back: the label, the type, and where the memory holds.
fn said_kept(kept: &Memory) -> String {
    match &kept.tree {
        Some(tree) => catalogue::say(
            "cli.remember.kept_in_tree",
            &[("label", &kept.label), ("type", &kept.kind), ("tree", tree)],
        ),
        None => catalogue::say(
            "cli.remember.kept_everywhere",
            &[("label", &kept.label), ("type", &kept.kind)],
        ),
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
            tree: None,
        };

        let kept = kept_by_hand(&ledger, Some(&home), memory).expect("kept");
        let page = std::fs::read_to_string(actions::memory::page_path(&home)).expect("a page");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(kept.label, "the trunk");
        assert_eq!(page, format!("## {}\n- **the trunk** (project): sorgenti", actions::memory::EVERYWHERE));
    }

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    /// **A MEMORY TYPED IN A CHECKOUT IS THAT CHECKOUT'S**, in its real path
    /// and from any directory inside it. `--global`, wherever it stands among
    /// the words, makes it hold in every tree; outside any checkout it does too.
    #[test]
    fn a_memory_typed_in_a_checkout_is_written_for_that_tree_unless_global() {
        let dir = std::env::temp_dir().join(format!("sailor-remember-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let checkout = dir.join("a-checkout");
        let deep = checkout.join("deep");
        let outside = dir.join("nowhere");
        std::fs::create_dir_all(&deep).expect("scratch");
        std::fs::create_dir_all(&outside).expect("scratch");
        let init = std::process::Command::new("git")
            .arg("-C")
            .arg(&checkout)
            .args(["init", "--quiet"])
            .status()
            .expect("git");
        assert!(init.success());
        let real = checkout.canonicalize().expect("real").display().to_string();

        let own = memory_typed(&words(&["project", "the trunk", "sorgenti,", "pushed"]), &deep, 10).expect("a memory");
        let global = memory_typed(&words(&["project", "--global", "the trunk", "sorgenti"]), &deep, 10).expect("a memory");
        let nowhere = memory_typed(&words(&["project", "the trunk", "sorgenti"]), &outside, 10).expect("a memory");
        let short = memory_typed(&words(&["project", "the trunk"]), &deep, 10);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(own.tree, Some(real));
        assert_eq!(own.value, "sorgenti, pushed");
        assert_eq!(own.modified, 10);
        assert_eq!(global.tree, None);
        assert_eq!(global.label, "the trunk", "the flag was read as a word");
        assert_eq!(nowhere.tree, None);
        assert!(short.is_none(), "two words are not a memory: {short:?}");
    }

    /// What is said back names the tree, or says the memory holds in every one.
    #[test]
    fn the_answer_says_where_the_memory_holds() {
        let in_a_tree = Memory {
            kind: "project".to_owned(),
            label: "the trunk".to_owned(),
            value: "sorgenti".to_owned(),
            provenance: "test".to_owned(),
            modified: 10,
            valid_from: 10,
            valid_until: None,
            tree: Some("/trees/a".to_owned()),
        };
        let everywhere = Memory { tree: None, ..in_a_tree.clone() };
        let named = [("label", "the trunk"), ("type", "project")];

        assert_eq!(
            said_kept(&in_a_tree),
            catalogue::say("cli.remember.kept_in_tree", &[named[0], named[1], ("tree", "/trees/a")])
        );
        assert_eq!(said_kept(&everywhere), catalogue::say("cli.remember.kept_everywhere", &named));
        assert_ne!(said_kept(&in_a_tree), said_kept(&everywhere), "the two answers read the same");
    }
}
