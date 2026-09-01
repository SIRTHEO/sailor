//! `sailor faults`: the faults met, as data instead of as a document.
//!
//! A hand-written table made whoever wrote pick the next number by looking at
//! the last row, and two branches do not see each other. Here the store hands
//! out the number, and there is one store.

use faults::{Draft, Fault, Faults};
use std::path::PathBuf;

/// The forms of `sailor faults`, one per line.
pub const USAGE: &[&str] = &[
    "sailor faults list      [--open] [--json]   the faults on record",
    "sailor faults add       < fault.json        record a fault; the store gives it a number",
    "sailor faults status <n> <text>             change a fault's status",
    "sailor faults render    [--file <md>]       write the table out, for whoever reads it that way",
    "sailor faults import    <file.md>           bring in a hand-written table, once",
    "sailor faults check     <file.md>           the store against the table; non-zero if they differ",
];

const FORMS: &[&str] = &["list", "add", "status", "render", "import", "check"];

const WITHOUT_VALUE: &[&str] = &["open", "json"];

fn usage_text() -> String {
    format!("usage: {}", USAGE.join("\n       "))
}

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(said) => {
            if !said.is_empty() {
                println!("{said}");
            }
            0
        }
        Err(why) => {
            eprintln!("sailor faults: {why}");
            1
        }
    }
}

fn dispatch(args: &[String]) -> Result<String, String> {
    let Some(verb) = args.first().map(String::as_str) else {
        return Err(usage_text());
    };
    if !FORMS.contains(&verb) {
        return Err(format!(
            "«{verb}» is not a form of this command; there are {}\n{}",
            FORMS.join(", "),
            usage_text()
        ));
    }

    let mut options = std::collections::BTreeMap::new();
    let mut loose: Vec<String> = Vec::new();
    let mut rest = args[1..].iter();
    while let Some(word) = rest.next() {
        match word.strip_prefix("--") {
            Some(name) if WITHOUT_VALUE.contains(&name) => {
                options.insert(name.to_owned(), "true".to_owned());
            }
            Some(name) => {
                let value = rest
                    .next()
                    .ok_or_else(|| format!("«--{name}» wants a value after it"))?;
                options.insert(name.to_owned(), value.clone());
            }
            None => loose.push(word.clone()),
        }
    }

    let path = match options.get("store") {
        Some(declared) => PathBuf::from(declared),
        None => Faults::default_path().map_err(|error| error.to_string())?,
    };
    let store = Faults::open(&path).map_err(|error| format!("{}: {error}", path.display()))?;

    match verb {
        "list" => list(&store, &options),
        "add" => add(&store),
        "status" => set_status(&store, &loose),
        "render" => render(&store),
        "import" => import(&store, &loose),
        "check" => check(&store, &loose),
        other => Err(format!("«{other}» is not a form of this command")),
    }
}

fn list(
    store: &Faults,
    options: &std::collections::BTreeMap<String, String>,
) -> Result<String, String> {
    let all = store.all().map_err(|error| error.to_string())?;
    let shown: Vec<&Fault> = if options.contains_key("open") {
        all.iter().filter(|f| f.still_open()).collect()
    } else {
        all.iter().collect()
    };

    if options.contains_key("json") {
        return serde_json::to_string_pretty(&shown).map_err(|error| error.to_string());
    }

    let mut out = String::new();
    for fault in &shown {
        // The status leads, not the title: a list of what happened reads as a
        // story, a list of what stands reads as work left, which is the point.
        let standing = if fault.still_open() {
            "open  "
        } else {
            "closed"
        };
        let title: String = fault.what_happened.chars().take(96).collect();
        out.push_str(&format!("{:>3}  {standing}  {}\n", fault.number, title));
    }
    let open = store.still_open().map_err(|error| error.to_string())?;
    out.push_str(&format!(
        "\n{open} still open out of {}, counted now and not copied",
        all.len()
    ));
    Ok(out)
}

/// Records a fault read from standard input, without a number.
///
/// The number is not a field that can be sent: if it were, whoever writes
/// would go back to choosing it, and the collision would come back with them.
fn add(store: &Faults) -> Result<String, String> {
    let raw = std::io::read_to_string(std::io::stdin())
        .map_err(|error| format!("cannot read the fault: {error}"))?;
    let draft: Draft = serde_json::from_str(&raw).map_err(|error| {
        format!(
            "a fault is written as JSON with happened_on, what_happened, \
             how_it_showed, what_would_prevent and status: {error}"
        )
    })?;
    if draft.what_would_prevent.trim().is_empty() {
        // Without that column this is a diary, which is the one thing the
        // record exists not to be.
        return Err(
            "«what_would_prevent» is missing: a fault without the check that \
             would have stopped it is an anecdote, not work"
                .to_owned(),
        );
    }
    let recorded = store.record(&draft).map_err(|error| error.to_string())?;
    Ok(format!(
        "recorded fault {}: the store gave it the number",
        recorded.number
    ))
}

fn set_status(store: &Faults, loose: &[String]) -> Result<String, String> {
    let [number, status] = loose else {
        return Err("usage: sailor faults status <number> <status text>".to_owned());
    };
    let number: i64 = number
        .parse()
        .map_err(|_| format!("«{number}» is not a fault number"))?;
    let changed = store
        .set_status(number, status)
        .map_err(|error| error.to_string())?;
    Ok(format!("fault {}: {}", changed.number, changed.status))
}

fn render(store: &Faults) -> Result<String, String> {
    let all = store.all().map_err(|error| error.to_string())?;
    Ok(faults::render(&all).trim_end().to_owned())
}

/// The store against the table, on a machine that has both.
///
/// **NOT A TEST, AND THAT IS THE POINT.** The store sits outside the repository,
/// beside the ledger: a test reading it would be red on any other machine at
/// unchanged code, which is fault 5. The table is the register, so this reports
/// the drift and repairs neither side.
fn check(store: &Faults, loose: &[String]) -> Result<String, String> {
    let [file] = loose else {
        return Err("usage: sailor faults check <file.md>".to_owned());
    };
    let text = std::fs::read_to_string(file).map_err(|error| format!("{file}: {error}"))?;
    let written: std::collections::BTreeMap<i64, Fault> = faults::parse(&text)
        .into_iter()
        .map(|fault| (fault.number, fault))
        .collect();
    if written.is_empty() {
        return Err(format!(
            "{file}: no row with six columns in it. Reporting «they agree» \
             after reading nothing is the failure this command exists to \
             prevent"
        ));
    }
    let kept: std::collections::BTreeMap<i64, Fault> = store
        .all()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|fault| (fault.number, fault))
        .collect();

    let only_in_store: Vec<i64> = kept
        .keys()
        .filter(|n| !written.contains_key(n))
        .copied()
        .collect();
    let only_in_table: Vec<i64> = written
        .keys()
        .filter(|n| !kept.contains_key(n))
        .copied()
        .collect();
    let differing: Vec<i64> = written
        .iter()
        .filter(|(number, fault)| kept.get(number).is_some_and(|held| held != *fault))
        .map(|(number, _)| *number)
        .collect();

    if only_in_store.is_empty() && only_in_table.is_empty() && differing.is_empty() {
        return Ok(format!(
            "the store and {file} say the same thing: {} faults",
            written.len()
        ));
    }

    let mut said = format!(
        "the store and {file} have drifted apart. The table is the register, \
         so the store is what has to move:\n"
    );
    if !only_in_store.is_empty() {
        said.push_str(&format!(
            "  {:?} are in the store and not in the table - unpublished, and \
             invisible to anyone without this machine\n",
            only_in_store
        ));
    }
    if !only_in_table.is_empty() {
        said.push_str(&format!(
            "  {:?} are in the table and not in the store - «sailor faults \
             import {file}» brings them in\n",
            only_in_table
        ));
    }
    if !differing.is_empty() {
        said.push_str(&format!(
            "  {:?} have the same number and different text; the table's is \
             the one that counts\n",
            differing
        ));
    }
    Err(said.trim_end().to_owned())
}

/// Brings in a hand-written table. Once, and it says so.
fn import(store: &Faults, loose: &[String]) -> Result<String, String> {
    let [file] = loose else {
        return Err("usage: sailor faults import <file.md>".to_owned());
    };
    let text = std::fs::read_to_string(file).map_err(|error| format!("{file}: {error}"))?;
    let read = faults::parse(&text);
    if read.is_empty() {
        return Err(format!(
            "{file}: no row with six columns in it. Better to stop than to \
             import nothing and call it done"
        ));
    }
    for fault in &read {
        store.restore(fault).map_err(|error| error.to_string())?;
    }
    let now = store.all().map_err(|error| error.to_string())?;
    Ok(format!(
        "brought in {} faults from {file}; the store now holds {}",
        read.len(),
        now.len()
    ))
}
