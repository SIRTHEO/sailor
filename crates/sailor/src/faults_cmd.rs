//! `sailor faults`: the faults met, as data instead of as a document.
//!
//! A hand-written table made whoever wrote pick the next number by looking at
//! the last row, and two branches do not see each other. Here the store hands
//! out the number, and there is one store.

use crate::Form;
use faults::{Draft, Fault, Faults};
use std::path::PathBuf;

/// The forms of `sailor faults`, one per line.
pub const USAGE: &[Form] = &[
    Form {
        form: "sailor faults list [--open] [--json]",
        says_key: "cli.faults.form.list",
    },
    Form {
        form: "sailor faults add < fault.json",
        says_key: "cli.faults.form.add",
    },
    Form {
        form: "sailor faults status <n> <text>",
        says_key: "cli.faults.form.status",
    },
    Form {
        form: "sailor faults render [--file <md>]",
        says_key: "cli.faults.form.render",
    },
    Form {
        form: "sailor faults import <file.md>",
        says_key: "cli.faults.form.import",
    },
    Form {
        form: "sailor faults check <file.md>",
        says_key: "cli.faults.form.check",
    },
];

const FORMS: &[&str] = &["list", "add", "status", "render", "import", "check"];

const WITHOUT_VALUE: &[&str] = &["open", "json"];

fn usage_text() -> String {
    format!(
        "{} {}",
        catalogue::say("cli.usage_heading", &[]),
        crate::forms_as_lines(USAGE).join("\n       ")
    )
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
            "{}\n{}",
            catalogue::say(
                "cli.not_a_form_of_this_command",
                &[("verb", verb), ("forms", &FORMS.join(", "))],
            ),
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
                let value = rest.next().ok_or_else(|| {
                    catalogue::say(
                        "cli.option_wants_a_value",
                        &[("option", &format!("--{name}"))],
                    )
                })?;
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
        other => Err(catalogue::say("cli.no_such_form", &[("verb", other)])),
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
            catalogue::say("cli.faults.open", &[])
        } else {
            catalogue::say("cli.faults.closed", &[])
        };
        let title: String = fault.what_happened.chars().take(96).collect();
        out.push_str(&format!("{:>3}  {standing:<6}  {}\n", fault.number, title));
    }
    let open = store.still_open().map_err(|error| error.to_string())?;
    out.push('\n');
    out.push_str(&catalogue::say(
        "cli.faults.still_open_out_of",
        &[
            ("open", &open.to_string()),
            ("total", &all.len().to_string()),
        ],
    ));
    Ok(out)
}

/// Records a fault read from standard input, without a number.
///
/// The number is not a field that can be sent: if it were, whoever writes
/// would go back to choosing it, and the collision would come back with them.
fn add(store: &Faults) -> Result<String, String> {
    let raw = std::io::read_to_string(std::io::stdin()).map_err(|error| {
        catalogue::say(
            "cli.faults.cannot_read_the_fault",
            &[("error", &error.to_string())],
        )
    })?;
    let draft: Draft = serde_json::from_str(&raw).map_err(|error| {
        catalogue::say(
            "cli.faults.shape_of_a_fault",
            &[("error", &error.to_string())],
        )
    })?;
    if draft.what_would_prevent.trim().is_empty() {
        // Without that column this is a diary, which is the one thing the
        // record exists not to be.
        return Err(catalogue::say("cli.faults.no_prevention", &[]));
    }
    let recorded = store.record(&draft).map_err(|error| error.to_string())?;
    Ok(catalogue::say(
        "cli.faults.recorded",
        &[("number", &recorded.number.to_string())],
    ))
}

fn set_status(store: &Faults, loose: &[String]) -> Result<String, String> {
    let [number, status] = loose else {
        return Err(catalogue::say("cli.faults.usage_status", &[]));
    };
    let number: i64 = number
        .parse()
        .map_err(|_| catalogue::say("cli.faults.not_a_number", &[("number", number)]))?;
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
        return Err(catalogue::say("cli.faults.usage_check", &[]));
    };
    let text = std::fs::read_to_string(file).map_err(|error| format!("{file}: {error}"))?;
    let written: std::collections::BTreeMap<i64, Fault> = faults::parse(&text)
        .into_iter()
        .map(|fault| (fault.number, fault))
        .collect();
    if written.is_empty() {
        return Err(catalogue::say(
            "cli.faults.no_rows_to_check",
            &[("file", file)],
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
        return Ok(catalogue::say(
            "cli.faults.they_agree",
            &[("file", file), ("count", &written.len().to_string())],
        ));
    }

    // **THE STORE IS ONE PER MACHINE, THE TABLE IS ONE PER BRANCH.** So a
    // difference has two readings and the command must not pick for you: work
    // not yet published, or a checkout older than the store. Saying only the
    // first sends whoever reads it to import an older register over a newer one.
    let mut said = catalogue::say("cli.faults.they_differ", &[("file", file)]);
    said.push('\n');
    if !only_in_store.is_empty() {
        said.push_str("  ");
        said.push_str(&catalogue::say(
            "cli.faults.only_in_the_store",
            &[("numbers", &format!("{only_in_store:?}"))],
        ));
        said.push('\n');
    }
    if !only_in_table.is_empty() {
        said.push_str("  ");
        said.push_str(&catalogue::say(
            "cli.faults.only_in_the_table",
            &[("numbers", &format!("{only_in_table:?}")), ("file", file)],
        ));
        said.push('\n');
    }
    if !differing.is_empty() {
        said.push_str("  ");
        said.push_str(&catalogue::say(
            "cli.faults.same_number_other_text",
            &[("numbers", &format!("{differing:?}"))],
        ));
        said.push('\n');
    }
    Err(said.trim_end().to_owned())
}

/// Brings in a hand-written table. Once, and it says so.
fn import(store: &Faults, loose: &[String]) -> Result<String, String> {
    let [file] = loose else {
        return Err(catalogue::say("cli.faults.usage_import", &[]));
    };
    let text = std::fs::read_to_string(file).map_err(|error| format!("{file}: {error}"))?;
    let read = faults::parse(&text);
    if read.is_empty() {
        return Err(catalogue::say(
            "cli.faults.no_rows_to_import",
            &[("file", file)],
        ));
    }
    // **WHAT IT OVERWROTE, BY NUMBER.** A count of what came in reads the same
    // whether every row was new or half of them replaced text written later
    // somewhere else - and importing an older checkout is exactly how that
    // happens. The numbers are collected before the write, because afterwards
    // the old text is gone and nobody can tell what changed.
    let before: std::collections::BTreeMap<i64, Fault> = store
        .all()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|fault| (fault.number, fault))
        .collect();
    let replaced: Vec<i64> = read
        .iter()
        .filter(|fault| before.get(&fault.number).is_some_and(|held| held != *fault))
        .map(|fault| fault.number)
        .collect();

    for fault in &read {
        store.restore(fault).map_err(|error| error.to_string())?;
    }
    let now = store.all().map_err(|error| error.to_string())?;
    let mut said = catalogue::say(
        "cli.faults.brought_in",
        &[
            ("count", &read.len().to_string()),
            ("file", file),
            ("total", &now.len().to_string()),
        ],
    );
    if !replaced.is_empty() {
        said.push('\n');
        said.push_str(&catalogue::say(
            "cli.faults.text_replaced",
            &[("numbers", &format!("{replaced:?}"))],
        ));
    }
    Ok(said)
}

#[cfg(test)]
mod tests {
    use super::*;
    use faults::Draft;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "faults-cmd-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("the scratch directory");
        dir
    }

    /// **A COUNT OF WHAT CAME IN HIDES WHAT WENT OUT.** «Brought in 62» reads
    /// the same whether every row was new or half of them replaced text written
    /// later on another branch — and the store is one per machine while the
    /// table is one per checkout, so importing an older tree is not an exotic
    /// mistake, it is Tuesday.
    #[test]
    fn importing_says_which_rows_it_overwrote() {
        let dir = scratch("overwrote");
        let store = Faults::open(dir.join("faults.db")).expect("opening");
        store
            .record(&Draft {
                happened_on: "01/09".to_owned(),
                what_happened: "what the store holds now".to_owned(),
                how_it_showed: "by running it".to_owned(),
                what_would_prevent: "this test".to_owned(),
                status: "**aperto**".to_owned(),
            })
            .expect("recording");

        let older = dir.join("older.md");
        std::fs::write(
            &older,
            "| 1 | 01/09 | what an older checkout says | by running it | \
             this test | **aperto** |\n",
        )
        .expect("writing the older table");

        let said = import(&store, &[older.display().to_string()]).expect("importing");

        assert!(
            said.contains("[1]") && said.contains("replaced"),
            "the import must name the row whose text it overwrote, or a step \
             back reads exactly like a step forward: {said}"
        );
    }

    /// The same import, when nothing was there before, says nothing about
    /// replacements — or the warning becomes noise and stops being read.
    #[test]
    fn importing_into_an_empty_store_reports_no_overwrite() {
        let dir = scratch("fresh");
        let store = Faults::open(dir.join("faults.db")).expect("opening");
        let table = dir.join("table.md");
        std::fs::write(
            &table,
            "| 1 | 01/09 | a first fault | by running it | this test | \
             **aperto** |\n",
        )
        .expect("writing the table");

        let said = import(&store, &[table.display().to_string()]).expect("importing");

        assert!(
            !said.contains("replaced"),
            "nothing was overwritten and the import said it was: {said}"
        );
    }
}
