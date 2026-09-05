//! `sailor notes`: the written documents Sailor keeps, so a working note does
//! not have to live in a repository strangers read.
//!
//! Same shape as the fault register: the text is data in the store, and
//! `render` writes it back out. **Nothing imported is trapped** — what goes in
//! comes back byte for byte, or the round trip is a one-way door.

use actions::notes::{self, Note};
use ledger::Ledger;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const USAGE: &[crate::Form] = &[
    crate::Form {
        form: "sailor notes import <file.md>",
        says_key: "cli.notes.form.import",
    },
    crate::Form {
        form: "sailor notes list [--json]",
        says_key: "cli.notes.form.list",
    },
    crate::Form {
        form: "sailor notes show <slug>",
        says_key: "cli.notes.form.show",
    },
    crate::Form {
        form: "sailor notes render <slug> [--file <path>]",
        says_key: "cli.notes.form.render",
    },
    crate::Form {
        form: "sailor notes remove <slug>",
        says_key: "cli.notes.form.remove",
    },
];

const WITHOUT_VALUE: &[&str] = &["json"];

/// What a form leaves behind: a line for whoever typed it, or the document
/// itself. **The two are not printed the same way.** A note handed to a pipe
/// carries its own last newline, and a `println!` would add a second.
enum Written {
    Said(String),
    Verbatim(String),
}

fn usage_text() -> String {
    format!(
        "{} {}",
        catalogue::say("cli.usage_heading", &[]),
        crate::forms_as_lines(USAGE).join("\n       ")
    )
}

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(Written::Said(said)) => {
            if !said.is_empty() {
                println!("{said}");
            }
            0
        }
        Ok(Written::Verbatim(text)) => match handed_over(&text) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("sailor notes: {error}");
                1
            }
        },
        Err(why) => {
            eprintln!("sailor notes: {why}");
            1
        }
    }
}

fn handed_over(text: &str) -> std::io::Result<()> {
    let mut out = std::io::stdout();
    out.write_all(text.as_bytes())?;
    out.flush()
}

fn dispatch(args: &[String]) -> Result<Written, String> {
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

    let directory = match options.get("store") {
        Some(declared) => PathBuf::from(declared),
        None => ui::gather::default_ledger_dir(),
    };
    let ledger =
        Ledger::open(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;

    match verb {
        "import" => import(&ledger, &loose),
        "list" => list(&ledger, &options),
        "show" => show(&ledger, &loose),
        "render" => render(&ledger, &loose, &options),
        "remove" => remove(&ledger, &loose),
        other => Err(catalogue::say("cli.no_such_form", &[("verb", other)])),
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

/// The tree the note is being taken in from, when there is one to name.
fn tree_here() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|here| actions::memory::tree_of(&here))
}

/// Takes a markdown file in. A second import under the same slug replaces the
/// first and says so, rather than filing a copy nobody can tell from it.
fn import(ledger: &Ledger, loose: &[String]) -> Result<Written, String> {
    let [file] = loose else {
        return Err(catalogue::say("cli.notes.usage_import", &[]));
    };
    let path = Path::new(file);
    let raw = std::fs::read(path).map_err(|error| format!("{file}: {error}"))?;
    let text = String::from_utf8(raw)
        .map_err(|_| catalogue::say("cli.notes.not_text", &[("file", file)]))?;
    let slug = notes::slug_of(path);
    if slug.is_empty() {
        return Err(catalogue::say("cli.notes.no_slug", &[("file", file)]));
    }
    let title = notes::title_of(&text, &slug);
    let done = notes::import(
        ledger,
        Note {
            slug,
            title,
            text,
            tree: tree_here(),
            imported_at: now(),
            removed_at: None,
        },
    )
    .map_err(|error| error.to_string())?;
    let key = if done.replaced {
        "cli.notes.replaced"
    } else {
        "cli.notes.imported"
    };
    Ok(Written::Said(catalogue::say(
        key,
        &[
            ("slug", &done.note.slug),
            ("title", &done.note.title),
            ("bytes", &done.note.bytes().to_string()),
        ],
    )))
}

fn list(ledger: &Ledger, options: &BTreeMap<String, String>) -> Result<Written, String> {
    let held = notes::all(ledger).map_err(|error| error.to_string())?;
    if options.contains_key("json") {
        return serde_json::to_string_pretty(&held)
            .map(Written::Said)
            .map_err(|error| error.to_string());
    }
    let width = held.iter().map(|note| note.slug.len()).max().unwrap_or(0);
    let mut out = String::new();
    for note in &held {
        out.push_str(&format!(
            "{:width$}  {:>8}  {}  {}\n",
            note.slug,
            note.bytes(),
            notes::instant(note.imported_at),
            note.title,
            width = width
        ));
    }
    out.push_str(&catalogue::say(
        "cli.notes.held",
        &[("count", &held.len().to_string())],
    ));
    Ok(Written::Said(out))
}

fn show(ledger: &Ledger, loose: &[String]) -> Result<Written, String> {
    let [slug] = loose else {
        return Err(catalogue::say("cli.notes.usage_show", &[]));
    };
    Ok(Written::Verbatim(held(ledger, slug)?.text))
}

/// Writes a note back to a file, so nothing taken in is ever trapped. The slug
/// with a markdown suffix when `--file` names nowhere else.
fn render(
    ledger: &Ledger,
    loose: &[String],
    options: &BTreeMap<String, String>,
) -> Result<Written, String> {
    let [slug] = loose else {
        return Err(catalogue::say("cli.notes.usage_render", &[]));
    };
    let note = held(ledger, slug)?;
    let file = options
        .get("file")
        .cloned()
        .unwrap_or_else(|| format!("{}.md", note.slug));
    let bytes = note.bytes().to_string();
    std::fs::write(&file, note.text.as_bytes()).map_err(|error| format!("{file}: {error}"))?;
    Ok(Written::Said(catalogue::say(
        "cli.notes.written",
        &[("file", &file), ("slug", &note.slug), ("bytes", &bytes)],
    )))
}

fn remove(ledger: &Ledger, loose: &[String]) -> Result<Written, String> {
    let [slug] = loose else {
        return Err(catalogue::say("cli.notes.usage_remove", &[]));
    };
    if !notes::remove(ledger, slug, now()).map_err(|error| error.to_string())? {
        return Err(catalogue::say("cli.notes.no_such_note", &[("slug", slug)]));
    }
    Ok(Written::Said(catalogue::say(
        "cli.notes.removed",
        &[("slug", slug)],
    )))
}

/// The note under a slug, or the refusal that names the slug nobody wrote.
fn held(ledger: &Ledger, slug: &str) -> Result<Note, String> {
    notes::read(ledger, slug)
        .map_err(|error| error.to_string())?
        .filter(Note::kept)
        .ok_or_else(|| catalogue::say("cli.notes.no_such_note", &[("slug", slug)]))
}
