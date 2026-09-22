//! `sailor instructions`: what the person asks of every session they open,
//! kept once under Sailor's home and handed to each session as it starts.
//! Only the sessions a person talks to: a flow's steps answer in the shape
//! their step declares, and a standing request about tone is not theirs.

use std::io::Read;
use std::path::{Path, PathBuf};

pub const USAGE: &[crate::Form] = &[
    crate::Form {
        form: "sailor instructions",
        says_key: "cli.instructions.says",
    },
    crate::Form {
        form: "sailor instructions set <file>|-",
        says_key: "cli.instructions.form.set",
    },
    crate::Form {
        form: "sailor instructions clear",
        says_key: "cli.instructions.form.clear",
    },
];

/// Past this a session's start carries more of the person's standing words
/// than of the machine it arrives on, and an engine may cut what it is handed.
pub const THE_MOST_A_SESSION_IS_HANDED: usize = 6000;

const WHERE_THEY_ARE_KEPT: &str = "instructions/sessions.md";

/// What a session is handed, as the file holds it now.
#[derive(Debug, PartialEq, Eq)]
pub enum Standing {
    None,
    Words(String),
    TooLong { chars: usize, path: PathBuf },
}

pub fn run(args: &[String]) -> i32 {
    let Some(home) = ledger::sailor_home() else {
        eprintln!(
            "sailor instructions: {}",
            catalogue::say("cli.no_home", &[])
        );
        return 2;
    };
    let answered = match args {
        [] => Ok(reading(&home)),
        [verb, from] if verb == "set" => words_from(from).and_then(|words| set(&home, &words)),
        [verb] if verb == "clear" => clear(&home),
        [verb, ..] if verb == "set" || verb == "clear" => {
            Err(crate::forms_as_lines(USAGE).join("\n"))
        }
        [other, ..] => Err(catalogue::say("cli.no_such_form", &[("verb", other)])),
    };
    match answered {
        Ok(said) => {
            println!("{said}");
            0
        }
        Err(why) => {
            eprintln!("sailor instructions: {why}");
            2
        }
    }
}

pub fn kept_under(home: &Path) -> PathBuf {
    home.join(WHERE_THEY_ARE_KEPT)
}

pub fn standing_under(home: &Path) -> Standing {
    let path = kept_under(home);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Standing::None;
    };
    let words = text.trim();
    let chars = words.chars().count();
    if chars == 0 {
        Standing::None
    } else if chars > THE_MOST_A_SESSION_IS_HANDED {
        Standing::TooLong { chars, path }
    } else {
        Standing::Words(words.to_owned())
    }
}

fn reading(home: &Path) -> String {
    let path = kept_under(home).display().to_string();
    match standing_under(home) {
        Standing::None => catalogue::say("cli.instructions.none", &[("path", &path)]),
        Standing::Words(words) => format!(
            "{}\n\n{words}",
            catalogue::say(
                "cli.instructions.held",
                &[
                    ("path", &path),
                    ("chars", &words.chars().count().to_string())
                ],
            )
        ),
        Standing::TooLong { chars, .. } => too_long(chars, &path),
    }
}

fn words_from(from: &str) -> Result<String, String> {
    if from == "-" {
        let mut words = String::new();
        std::io::stdin()
            .read_to_string(&mut words)
            .map_err(|error| error.to_string())?;
        return Ok(words);
    }
    std::fs::read_to_string(from).map_err(|error| format!("{from}: {error}"))
}

fn set(home: &Path, words: &str) -> Result<String, String> {
    let words = words.trim();
    let path = kept_under(home);
    let shown = path.display().to_string();
    let chars = words.chars().count();
    if chars == 0 {
        return Err(catalogue::say("cli.instructions.empty", &[]));
    }
    if chars > THE_MOST_A_SESSION_IS_HANDED {
        return Err(too_long(chars, &shown));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("{shown}: {error}"))?;
    }
    let partial = path.with_extension("partial");
    std::fs::write(&partial, format!("{words}\n")).map_err(|error| format!("{shown}: {error}"))?;
    std::fs::rename(&partial, &path).map_err(|error| format!("{shown}: {error}"))?;
    Ok(catalogue::say(
        "cli.instructions.set",
        &[("path", &shown), ("chars", &chars.to_string())],
    ))
}

fn clear(home: &Path) -> Result<String, String> {
    let path = kept_under(home);
    let shown = path.display().to_string();
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(catalogue::say(
            "cli.instructions.cleared",
            &[("path", &shown)],
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(catalogue::say("cli.instructions.none", &[("path", &shown)]))
        }
        Err(error) => Err(format!("{shown}: {error}")),
    }
}

fn too_long(chars: usize, path: &str) -> String {
    catalogue::say(
        "cli.instructions.too_long",
        &[
            ("chars", &chars.to_string()),
            ("most", &THE_MOST_A_SESSION_IS_HANDED.to_string()),
            ("path", path),
        ],
    )
}
