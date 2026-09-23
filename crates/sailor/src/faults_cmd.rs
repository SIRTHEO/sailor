//! `sailor faults`: the faults met, as data instead of as a document.
//!
//! A hand-written table made whoever wrote pick the next number by looking at
//! the last row, and two branches do not see each other. Here the store hands
//! out the number, and there is one store.

use crate::Form;
use faults::{Draft, Fault, Faults};
use std::collections::BTreeMap;
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
        form: "sailor faults summary <n> <text>",
        says_key: "cli.faults.form.summary",
    },
    Form {
        form: "sailor faults reword <n> < fault.json",
        says_key: "cli.faults.form.reword",
    },
    Form {
        form: "sailor faults render [--open] [--file <md>]",
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
    Form {
        form: "sailor faults link <n> <issue-url>",
        says_key: "cli.faults.form.link",
    },
    Form {
        form: "sailor faults unlink <n>",
        says_key: "cli.faults.form.unlink",
    },
];

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
    if !crate::is_a_form(USAGE, verb) {
        return Err(format!(
            "{}\n{}",
            catalogue::say(
                "cli.not_a_form_of_this_command",
                &[("verb", verb), ("forms", &crate::verbs_of(USAGE).join(", "))],
            ),
            usage_text()
        ));
    }

    let mut options: BTreeMap<String, String> = BTreeMap::new();
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
    // Three of these verbs write and three only read, and the reading ones are
    // what an agent runs from a sandbox that grants no writes.
    // A page stamped with a moment is rendered from a store that keeps its
    // history, or no later check could tell what stood then.
    let stamping = verb == "render" && options.contains_key("open") && options.contains_key("file");
    let opened = match verb {
        "render" if stamping => Faults::open(&path),
        "list" | "render" | "check" => Faults::open_for_reading(&path),
        _ => Faults::open(&path),
    };
    let store = opened.map_err(|error| format!("{}: {error}", path.display()))?;

    match verb {
        "list" => list(&store, &options),
        "add" => add(&store),
        "status" => set_status(&store, &loose),
        "summary" => summarised(&store, &loose, &what_a_public_summary_cannot_carry),
        "reword" => reword(&store, &loose),
        "render" => render(&store, &options),
        "import" => import(&store, &loose),
        "check" => check(&store, &loose),
        "link" => link(&store, &loose),
        "unlink" => unlink(&store, &loose),
        other => Err(catalogue::say("cli.no_such_form", &[("verb", other)])),
    }
}

/// Reads `<owner>/<repo>#<n>` or `.../issues/<n>` out of a URL; a bare number
/// is taken as-is. Never opens a network connection — this only parses text
/// somebody already has, whether from a browser tab or a `gh` command.
fn issue_number_in(url: &str) -> Result<i64, String> {
    if let Ok(bare) = url.parse::<i64>() {
        return Ok(bare);
    }
    let digits: String = url.rsplit('/').next().unwrap_or("").chars().filter(char::is_ascii_digit).collect();
    digits
        .parse()
        .map_err(|_| catalogue::say("cli.faults.not_an_issue_url", &[("url", url)]))
}

fn link(store: &Faults, loose: &[String]) -> Result<String, String> {
    let [number, url] = loose else {
        return Err(catalogue::say("cli.faults.usage_link", &[]));
    };
    let number: i64 = number
        .parse()
        .map_err(|_| catalogue::say("cli.faults.not_a_number", &[("number", number)]))?;
    let issue_number = issue_number_in(url)?;
    let linked = store.link(number, issue_number, url).map_err(|error| error.to_string())?;
    Ok(catalogue::say(
        "cli.faults.linked",
        &[("number", &linked.number.to_string()), ("issue", &issue_number.to_string())],
    ))
}

fn unlink(store: &Faults, loose: &[String]) -> Result<String, String> {
    let [number] = loose else {
        return Err(catalogue::say("cli.faults.usage_unlink", &[]));
    };
    let number: i64 = number
        .parse()
        .map_err(|_| catalogue::say("cli.faults.not_a_number", &[("number", number)]))?;
    let unlinked = store.unlink(number).map_err(|error| error.to_string())?;
    Ok(catalogue::say("cli.faults.unlinked", &[("number", &unlinked.number.to_string())]))
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
    // Unclassified rows are neither open nor closed, and silence about them is
    // the subtraction that reassures.
    let unknown = all
        .iter()
        .filter(|f| f.standing == faults::Standing::Unknown)
        .count();
    if unknown > 0 {
        out.push('\n');
        out.push_str(&catalogue::say(
            "cli.faults.standing_unknown",
            &[("count", &unknown.to_string())],
        ));
    }
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
    what_the_repository_could_not_publish(&raw)?;
    let recorded = store.record(&draft).map_err(|error| error.to_string())?;
    Ok(catalogue::say(
        "cli.faults.recorded",
        &[("number", &recorded.number.to_string())],
    ))
}

/// Refuses text this machine declares it cannot publish.
///
/// **THE GATE USED TO RUN ONLY AT COMMIT**, which means somebody else, another
/// day, rendering the table and finding a home path and a private name inside
/// it. The rule is read from one place, `toolbox::privacy`, so the store and
/// the gate cannot drift apart — and the refusal says where, never what.
fn what_the_repository_could_not_publish(text: &str) -> Result<(), String> {
    let names = toolbox::privacy::declared_here(&|path| std::fs::read_to_string(path).ok());
    let home = std::env::var("HOME").ok();
    refusal_for(&toolbox::privacy::what_cannot_be_published(text, &names, home.as_deref()))
}

/// What a public summary may not carry: every shape the release scan refuses in
/// a public text. A list of names that cannot be read refuses too.
fn what_a_public_summary_cannot_carry(text: &str) -> Result<(), String> {
    public_text_refusal(
        text,
        toolbox::privacy::required_input(
            std::env::var("SAILOR_PRIVATE_NAMES").ok(),
            std::env::var("HOME").ok(),
        ),
    )
}

/// The same decision with the inputs handed in, so a test declares them.
fn public_text_refusal(
    text: &str,
    inputs: Result<(Vec<String>, String), toolbox::privacy::PublicationPreflight>,
) -> Result<(), String> {
    let Ok((names, home)) = inputs else {
        return Err(catalogue::say("cli.faults.cannot_prove_privacy", &[]));
    };
    refusal_for(&toolbox::privacy::what_a_public_text_cannot_carry(text, &names, Some(&home)))
}

/// The first reason found, said by where it starts and never by what it is.
fn refusal_for(found: &[toolbox::privacy::Reason]) -> Result<(), String> {
    use toolbox::privacy::Reason;
    let Some(first) = found.first() else {
        return Ok(());
    };
    let (key, at) = match first {
        Reason::APrivateName { at } => ("cli.faults.a_private_name", at),
        Reason::APathOfThisMachine { at } => ("cli.faults.a_path_of_this_machine", at),
        Reason::AShapeOfAMachine { at } => ("cli.faults.a_shape_of_a_machine", at),
        Reason::APassageAboutTheMachine { at } => ("cli.faults.a_passage_about_the_machine", at),
    };
    Err(catalogue::say(
        key,
        &[
            ("at", &at.to_string()),
            ("count", &found.len().to_string()),
        ],
    ))
}

/// The three prose columns of a fault, and nothing else. Unknown fields are
/// refused: handed the shape `add` takes, a date and a status would be dropped
/// in silence and whoever edited them would believe they had landed.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Rewording {
    what_happened: String,
    how_it_showed: String,
    what_would_prevent: String,
}

/// Rewrites the prose of a fault already on record, keeping its number, its
/// date and its standing: what happened does not change because the words did.
fn reword(store: &Faults, loose: &[String]) -> Result<String, String> {
    let raw = std::io::read_to_string(std::io::stdin()).map_err(|error| {
        catalogue::say(
            "cli.faults.cannot_read_the_fault",
            &[("error", &error.to_string())],
        )
    })?;
    reworded(store, loose, &raw)
}

/// The decision, with the text already in hand. **IT EXISTS SO IT CAN BE
/// TESTED**: `reword` reads standard input, which a test cannot give it.
fn reworded(store: &Faults, loose: &[String], raw: &str) -> Result<String, String> {
    let [number] = loose else {
        return Err(catalogue::say("cli.faults.usage_reword", &[]));
    };
    let number: i64 = number
        .parse()
        .map_err(|_| catalogue::say("cli.faults.not_a_number", &[("number", number)]))?;
    let said: Rewording = serde_json::from_str(raw).map_err(|error| {
        catalogue::say(
            "cli.faults.shape_of_a_fault",
            &[("error", &error.to_string())],
        )
    })?;
    if said.what_would_prevent.trim().is_empty() {
        return Err(catalogue::say("cli.faults.no_prevention", &[]));
    }
    what_the_repository_could_not_publish(raw)?;
    let already = store.get(number).map_err(|error| error.to_string())?;
    store
        .restore(&Fault {
            number,
            happened_on: already.happened_on,
            happened: already.happened,
            what_happened: said.what_happened,
            how_it_showed: said.how_it_showed,
            what_would_prevent: said.what_would_prevent,
            status: already.status,
            standing: already.standing,
            public_summary: already.public_summary,
            github_issue: already.github_issue,
        })
        .map_err(|error| error.to_string())?;
    Ok(catalogue::say(
        "cli.faults.reworded",
        &[("number", &number.to_string())],
    ))
}

fn set_status(store: &Faults, loose: &[String]) -> Result<String, String> {
    let [number, status] = loose else {
        return Err(catalogue::say("cli.faults.usage_status", &[]));
    };
    let number: i64 = number
        .parse()
        .map_err(|_| catalogue::say("cli.faults.not_a_number", &[("number", number)]))?;
    what_the_repository_could_not_publish(status)?;
    let changed = store
        .set_status(number, status)
        .map_err(|error| error.to_string())?;
    Ok(format!("fault {}: {}", changed.number, changed.status))
}

/// The sentence a user reads for a fault on the public page. It passes the
/// check every other prose write into the register passes, handed in so a
/// test can declare the names.
fn summarised(
    store: &Faults,
    loose: &[String],
    publishable: &dyn Fn(&str) -> Result<(), String>,
) -> Result<String, String> {
    let [number, summary] = loose else {
        return Err(catalogue::say("cli.faults.usage_summary", &[]));
    };
    let number: i64 = number
        .parse()
        .map_err(|_| catalogue::say("cli.faults.not_a_number", &[("number", number)]))?;
    publishable(summary)?;
    store
        .set_public_summary(number, summary)
        .map_err(|error| error.to_string())?;
    Ok(catalogue::say(
        "cli.faults.summarised",
        &[("number", &number.to_string())],
    ))
}

/// The table, on the screen or into the file `--file` names.
///
/// **THE OPTION WAS IN THE USAGE LINE AND IN NO CODE.** Whoever typed it got
/// the table on standard output, read «done», and left a file on disk that had
/// not changed — a command claiming more than it did, which is the family of
/// defect this very register is kept for.
fn render(store: &Faults, options: &BTreeMap<String, String>) -> Result<String, String> {
    let all = store.all().map_err(|error| error.to_string())?;
    let open_only = options.contains_key("open");
    let Some(file) = options.get("file") else {
        let rows = if open_only {
            faults::render_open(&all)
        } else {
            faults::render(&all)
        };
        return Ok(rows.trim_end().to_owned());
    };
    // **THE FILE IS READ BEFORE IT IS WRITTEN.** A document is not its table:
    // the rows are replaced where they stand and the prose around them stays.
    let document = std::fs::read_to_string(file).map_err(|error| format!("{file}: {error}"))?;
    if open_only {
        let stood = store.stood_now().map_err(|error| error.to_string())?;
        let then = match store
            .as_it_stood(&stood)
            .map_err(|error| error.to_string())?
        {
            Some(then) => then,
            None => all,
        };
        let on_the_page = faults::on_the_public_page(&then).len();
        let open = then.iter().filter(|fault| fault.still_open()).count();
        let page = faults::the_page_from(&document, &then, &stood);
        std::fs::write(file, page).map_err(|error| format!("{file}: {error}"))?;
        return Ok(catalogue::say(
            "cli.faults.written_open",
            &[
                ("file", file),
                ("count", &on_the_page.to_string()),
                ("others", &(open - on_the_page).to_string()),
            ],
        ));
    }
    std::fs::write(file, faults::render_into(&document, &all))
        .map_err(|error| format!("{file}: {error}"))?;
    Ok(catalogue::say(
        "cli.faults.written",
        &[("file", file), ("count", &all.len().to_string())],
    ))
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
    let a_public_page = text.lines().any(|line| {
        faults::is_a_data_row(line)
            || line.trim() == faults::PUBLIC_HEADER
            || faults::is_the_count_sentence(line)
    });
    if written.is_empty() && a_public_page {
        return match faults::stood_in(&text) {
            Some(stood) => the_page_against_the_store(store, file, &text, &stood),
            None => Err(catalogue::say(
                "cli.faults.check_public_page",
                &[("file", file)],
            )),
        };
    }
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

/// The public page against a fresh render of the store as it stood when the
/// page was counted: what opened or closed since is the next render's.
fn the_page_against_the_store(
    store: &Faults,
    file: &str,
    text: &str,
    stood: &faults::Stood,
) -> Result<String, String> {
    let held = store
        .hold_the_page(text, stood)
        .map_err(|error| error.to_string())?;
    let through = stood.through.to_string();
    let mut said = vec![
        ("file", file),
        ("through", through.as_str()),
        ("at", stood.at.as_str()),
    ];
    match held {
        faults::Held::Agrees { open_then } => {
            let open_then = open_then.to_string();
            let open_now = store
                .still_open()
                .map_err(|error| error.to_string())?
                .to_string();
            said.extend([("open", open_then.as_str()), ("now", open_now.as_str())]);
            Ok(catalogue::say("cli.faults.page_agrees", &said))
        }
        faults::Held::Differs(difference) => {
            let line = difference.line.to_string();
            said.extend([
                ("line", line.as_str()),
                ("page", difference.page.as_str()),
                ("store", difference.store.as_str()),
            ]);
            Err(catalogue::say("cli.faults.page_differs", &said))
        }
        faults::Held::CannotTell { kept_since, rows } => {
            let since = match kept_since {
                Some(at) => catalogue::say("cli.faults.history_kept_since", &[("at", &at)]),
                None => catalogue::say("cli.faults.history_never_kept", &[]),
            };
            let rows = format!("{rows:?}");
            said.extend([("since", since.as_str()), ("rows", rows.as_str())]);
            Err(catalogue::say("cli.faults.page_cannot_be_told", &said))
        }
    }
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

    fn a_store_holding(label: &str, draft: &Draft) -> (PathBuf, Faults, i64) {
        let dir = scratch(label);
        let store = Faults::open(dir.join("faults.db")).expect("the store");
        let recorded = store.record(draft).expect("record");
        (dir, store, recorded.number)
    }

    fn a_fault() -> Draft {
        Draft {
            happened_on: "07/09".to_owned(),
            what_happened: "a thing broke inside a worktree an outside app manages".to_owned(),
            how_it_showed: "the attach refused".to_owned(),
            what_would_prevent: "an identity the caller can supply".to_owned(),
            status: "**open**".to_owned(),
            standing: None,
        }
    }

    /// **A FAULT'S PROSE WAS WRITTEN ONCE AND COULD NOT BE CORRECTED.** Only the
    /// standing had a verb, so text that turned a judge red — a product name in
    /// a public document — had no cure but a hand inside the store.
    #[test]
    fn rewording_a_fault_keeps_its_number_its_date_and_its_standing() {
        let (dir, store, number) = a_store_holding("reword", &a_fault());
        let said = reworded(
            &store,
            &[number.to_string()],
            r#"{"what_happened":"a thing broke where no tty is reachable",
                "how_it_showed":"the attach refused",
                "what_would_prevent":"an identity the caller can supply"}"#,
        )
        .expect("the words are rewritten");
        assert!(said.contains(&number.to_string()), "{said}");

        let after = store.get(number).expect("the fault is still there");
        assert_eq!(after.what_happened, "a thing broke where no tty is reachable");
        assert_eq!(after.happened_on, "07/09", "the date moved with the words");
        assert_eq!(after.status, "**open**", "the standing moved with the words");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A DATE OR A STANDING SENT HERE WOULD BE DROPPED IN SILENCE**, and
    /// whoever edited a file of the shape `add` takes would believe both had
    /// landed. Refused instead, by name.
    #[test]
    fn a_rewording_carrying_more_than_the_prose_is_refused() {
        let (dir, store, number) = a_store_holding("reword-extra", &a_fault());
        let refused = reworded(
            &store,
            &[number.to_string()],
            r#"{"happened_on":"01/01","what_happened":"a","how_it_showed":"b",
                "what_would_prevent":"c","status":"**closed**"}"#,
        );
        assert!(refused.is_err(), "a date and a standing were accepted and dropped");
        let after = store.get(number).expect("the fault is still there");
        assert_eq!(after.status, "**open**", "the refused text changed the store anyway");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Without that column the record is a diary, which is the one thing it
    /// exists not to be — and rewording must not be the way round it.
    #[test]
    fn a_rewording_that_drops_the_prevention_is_refused() {
        let (dir, store, number) = a_store_holding("reword-diary", &a_fault());
        let refused = reworded(
            &store,
            &[number.to_string()],
            r#"{"what_happened":"a","how_it_showed":"b","what_would_prevent":"  "}"#,
        );
        assert!(refused.is_err(), "a fault with no prevention was written");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A DOCUMENTED OPTION THAT NO CODE READS IS A COMMAND THAT LIES.** The
    /// usage line offered `--file` and `render` took no options at all: the
    /// table went to the screen and the file on disk stayed as it was.
    #[test]
    fn rendering_into_a_file_writes_the_file() {
        let dir = scratch("rendered");
        let store = Faults::open(dir.join("faults.db")).expect("opening");
        store
            .record(&Draft {
                happened_on: "03/09".to_owned(),
                what_happened: "what the store holds".to_owned(),
                how_it_showed: "reading it".to_owned(),
                what_would_prevent: "a check that reads it back".to_owned(),
                status: "**open**".to_owned(),
                standing: None,
            })
            .expect("recording");
        let written = dir.join("table.md");
        std::fs::write(
            &written,
            "# The faults\n\nWhy this file is not a diary.\n\n| # | date |\n|---|---|\n",
        )
        .expect("a document to write into");
        let options: BTreeMap<String, String> = [(
            "file".to_owned(),
            written.display().to_string(),
        )]
        .into_iter()
        .collect();

        let said = render(&store, &options).expect("rendering");

        let text = std::fs::read_to_string(&written).expect("the file was written");
        assert!(
            text.contains("what the store holds"),
            "the table is not in the file: {text}"
        );
        assert!(
            said.contains(&written.display().to_string()),
            "the answer does not name the file it wrote: {said}"
        );
        assert!(
            text.contains("Why this file is not a diary."),
            "the prose around the table was overwritten: {text}"
        );
        assert!(
            !said.contains("what the store holds"),
            "the table went to the screen as well, so nobody can tell whether the file was written: {said}"
        );
    }

    /// The public page carries the open faults given a summary for users, and
    /// none of the register's own prose.
    #[test]
    fn rendering_the_public_page_writes_only_the_summarised_open_faults() {
        let dir = scratch("rendered-open");
        let store = Faults::open(dir.join("faults.db")).expect("opening");
        for (what, status) in [
            ("a workshop story of the first fault", "**open** — nobody took it. Later"),
            ("a workshop story of the second fault", "**open**"),
            ("a fault already repaired", "**closed** on 04/09"),
        ] {
            store
                .record(&Draft {
                    happened_on: "03/09".to_owned(),
                    what_happened: what.to_owned(),
                    how_it_showed: "reading it".to_owned(),
                    what_would_prevent: "a check that reads it back".to_owned(),
                    status: status.to_owned(),
                    standing: None,
                })
                .expect("recording");
        }
        store
            .set_public_summary(1, "The window forgets a flow you renamed.")
            .expect("a summary");
        store
            .set_public_summary(3, "A repaired defect.")
            .expect("a summary");
        let written = dir.join("page.md");
        std::fs::write(
            &written,
            "# Open faults\n\n| # | since | what goes wrong | status |\n|---|---|---|---|\n\nWhat the page is not.\n",
        )
        .expect("a document to write into");
        let options: BTreeMap<String, String> = [
            ("file".to_owned(), written.display().to_string()),
            ("open".to_owned(), "true".to_owned()),
        ]
        .into_iter()
        .collect();

        let said = render(&store, &options).expect("rendering");

        let text = std::fs::read_to_string(&written).expect("the file was written");
        assert!(
            text.contains("| 1 | 03/09 | The window forgets a flow you renamed. | **open** |"),
            "the summarised row is not in the public shape: {text}"
        );
        assert!(!text.contains("nobody took it"), "the register's status prose reached the page: {text}");
        assert!(!text.contains("workshop story"), "the register's prose reached the page: {text}");
        assert!(!text.contains("| 2 |"), "an open fault with no summary reached the page: {text}");
        assert!(!text.contains("A repaired defect."), "a closed fault reached the page: {text}");
        assert!(
            text.contains("**One open fault is described on this page; one more is kept only in the fault store.**"),
            "the count sentence does not count the page and the store apart: {text}"
        );
        assert!(
            said.contains(&written.display().to_string()),
            "the answer does not name the file it wrote: {said}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **THE SUMMARY IS PROSE THAT LEAVES THE MACHINE**, so it is held to the
    /// release scan's public-text shapes. The names here are declared by the test.
    #[test]
    fn a_summary_carrying_anything_the_public_scan_refuses_is_refused() {
        let (dir, store, number) = a_store_holding("summary-private", &a_fault());
        let names = vec!["quillfeather".to_owned()];
        let publishable = |text: &str| {
            public_text_refusal(text, Ok((names.clone(), "/home/pilot".to_owned())))
        };

        for refused in [
            "Quillfeather's window forgets a flow.",
            "The relay stops pid 91964 and forgets it.",
            "A terminal on ttys008 is never released.",
            "The server on 127.0.0.1 is never asked.",
            "On this machine the token lives in the keychain.",
            "A flow kept in /home/anybody/flows is forgotten.",
        ] {
            assert!(
                summarised(&store, &[number.to_string(), refused.to_owned()], &publishable).is_err(),
                "a summary that cannot be published was accepted: {refused}"
            );
        }
        assert_eq!(
            store.get(number).expect("the fault").public_summary,
            None,
            "a refused summary landed in the store anyway"
        );

        summarised(
            &store,
            &[number.to_string(), "The window forgets a flow.".to_owned()],
            &publishable,
        )
        .expect("a publishable summary is written");
        assert_eq!(
            store.get(number).expect("the fault").public_summary.as_deref(),
            Some("The window forgets a flow.")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **«NO ROWS TO CHECK» WAS TRUE AND USELESS** on the one page the
    /// repository ships: the command must say what the page is and how it is kept.
    #[test]
    fn checking_the_public_page_says_it_is_not_the_register() {
        let dir = scratch("check-public");
        let store = Faults::open(dir.join("faults.db")).expect("opening");
        let page = dir.join("page.md");
        std::fs::write(
            &page,
            "| # | since | what goes wrong | status |\n|---|---|---|---|\n| 4 | 01/09 | The window forgets a flow. | **open** |\n",
        )
        .expect("a public page");
        let file = page.display().to_string();

        let said = check(&store, std::slice::from_ref(&file)).expect_err("a public page is not a register");

        assert!(said.contains("render --open"), "the refusal does not say how the page is kept: {said}");
        assert_ne!(
            said,
            catalogue::say("cli.faults.no_rows_to_check", &[("file", &file)]),
            "the public page got the generic answer"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A GUARD THAT CANNOT READ WHAT IT GUARDS CANNOT CALL A TEXT CLEAN.**
    #[test]
    fn a_summary_is_refused_when_the_list_of_names_cannot_be_read() {
        let (dir, store, number) = a_store_holding("summary-no-list", &a_fault());
        let unreadable =
            |text: &str| public_text_refusal(text, Err(toolbox::privacy::PublicationPreflight::CannotProve));

        let refused = summarised(
            &store,
            &[number.to_string(), "The window forgets a flow.".to_owned()],
            &unreadable,
        );

        assert!(refused.is_err(), "an unreadable list of private names let a summary through");
        assert_eq!(store.get(number).expect("the fault").public_summary, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The page the repository ships may have no row at all, and it is still
    /// the public page and not an empty register.
    #[test]
    fn checking_a_public_page_with_no_rows_still_says_what_it_is() {
        let dir = scratch("check-public-empty");
        let store = Faults::open(dir.join("faults.db")).expect("opening");
        let page = dir.join("page.md");
        std::fs::write(
            &page,
            format!(
                "# Faults still open\n\n{}\n|---|---|---|---|\n\n{}\n",
                faults::PUBLIC_HEADER,
                faults::count_sentence(0, 3)
            ),
        )
        .expect("an empty public page");
        let file = page.display().to_string();

        let said = check(&store, std::slice::from_ref(&file)).expect_err("an empty public page is not a register");

        assert!(said.contains("render --open"), "the empty public page got the generic answer: {said}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The page is held to a render of the store as it stood when it was
    /// counted: a close written since by plain SQL, as an older binary
    /// writes it, leaves it agreeing, and a line the store never gave does not.
    #[test]
    fn checking_the_public_page_holds_it_to_the_store_as_it_stood() {
        let (dir, store, number) = a_store_holding("check-stood", &a_fault());
        store
            .set_public_summary(number, "The window forgets a flow.")
            .expect("a summary");
        let page = dir.join("page.md");
        let header = format!(
            "# Faults still open\n\n{}\n|---|---|---|---|\n",
            faults::PUBLIC_HEADER
        );
        std::fs::write(&page, header).expect("a public page");
        let file = page.display().to_string();
        let options: BTreeMap<String, String> = [
            ("file".to_owned(), file.clone()),
            ("open".to_owned(), "true".to_owned()),
        ]
        .into_iter()
        .collect();
        render(&store, &options).expect("rendering");
        store
            .record(&a_fault())
            .expect("a fault opened after the count");
        rusqlite::Connection::open(dir.join("faults.db"))
            .expect("an older binary")
            .execute(
                "UPDATE faults SET standing = 'closed' WHERE number = ?1",
                [number],
            )
            .expect("closed past this crate");

        let agreed = check(&store, std::slice::from_ref(&file))
            .expect("the page agrees with the store as it stood");
        assert!(
            agreed.contains("1 open then") && agreed.contains("1 open now"),
            "{agreed}"
        );

        let text = std::fs::read_to_string(&page).expect("the page");
        let forged = text.replace("The window forgets a flow.", "Invented text nobody wrote.");
        std::fs::write(&page, forged).expect("a row nobody wrote");
        let refused = check(&store, std::slice::from_ref(&file)).expect_err("invented text");
        assert!(refused.contains("Invented text nobody wrote."), "{refused}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Rendering the page opens the store to write, so the history begins
    /// before the stamp even where only older binaries have written. A stamp
    /// the history does not reach is answered as unknown, never as agreeing.
    #[test]
    fn a_page_is_stamped_from_a_store_that_keeps_its_history() {
        let (dir, store, number) = a_store_holding("stamp-keeps", &a_fault());
        store
            .set_public_summary(number, "The window forgets a flow.")
            .expect("a summary");
        drop(store);
        let db = dir.join("faults.db");
        rusqlite::Connection::open(&db)
            .expect("an older binary")
            .execute_batch("DROP TRIGGER a_fault_is_written; DROP TABLE store_history;")
            .expect("a store no binary with the history has opened");
        let page = dir.join("page.md");
        let header = format!(
            "# Faults still open\n\n{}\n|---|---|---|---|\n",
            faults::PUBLIC_HEADER
        );
        std::fs::write(&page, header).expect("a public page");
        let (file, db) = (page.display().to_string(), db.display().to_string());
        let run = |words: &[&str]| {
            let mut args: Vec<String> = words.iter().map(|word| word.to_string()).collect();
            args.extend(["--store".to_owned(), db.clone()]);
            dispatch(&args)
        };

        run(&["render", "--open", "--file", &file]).expect("rendering");
        run(&["check", &file]).expect("a page stamped where the history is kept");

        let text = std::fs::read_to_string(&page).expect("the page");
        let stood = faults::stood_in(&text).expect("the stamp");
        let older = faults::stood_written_into(
            &text,
            &faults::Stood {
                change: None,
                ..stood
            },
        );
        std::fs::write(&page, older).expect("a stamp with no change of the history");
        let unknown = run(&["check", &file]).expect_err("a moment the history does not reach");
        assert!(unknown.contains("cannot tell"), "{unknown}");
        let _ = std::fs::remove_dir_all(&dir);
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
                status: "**open**".to_owned(),
                standing: None,
            })
            .expect("recording");

        let older = dir.join("older.md");
        std::fs::write(
            &older,
            "| 1 | 01/09 | what an older checkout says | by running it | \
             this test | **open** |\n",
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
             **open** |\n",
        )
        .expect("writing the table");

        let said = import(&store, &[table.display().to_string()]).expect("importing");

        assert!(
            !said.contains("replaced"),
            "nothing was overwritten and the import said it was: {said}"
        );
    }
}
