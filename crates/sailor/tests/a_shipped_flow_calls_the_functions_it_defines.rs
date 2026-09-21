//! A shell function defined in a step and never called there.
//!
//! **A STEP IS ITS OWN SHELL**, so nothing it defines reaches the next one.
//! Four copies of a 763-character `forge()` stood in `the_account`, declaring
//! how to call the forge — and the step only ever printed a name.

use std::path::{Path, PathBuf};

const WHERE_FLOWS_LIVE: &[&str] = &["flows", "crates/flow/system"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_path_buf()
}

fn flow_files(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = WHERE_FLOWS_LIVE
        .iter()
        .flat_map(|under| std::fs::read_dir(root.join(under)).into_iter().flatten())
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".flow.json"))
        .collect();
    found.sort();
    found
}

/// **THE WORD HAS TO BE A COMMAND, NOT A WORD.** The first reading of this
/// counted `forge` inside `echo "no forge token for …"` as a call and found
/// nothing dead anywhere. What a shell quotes is text, so the quoted runs come
/// out before anything is looked for.
fn unquoted(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut inside: Option<char> = None;
    for letter in line.chars() {
        match inside {
            Some(quote) if letter == quote => inside = None,
            Some(_) => {}
            None if letter == '"' || letter == '\'' => inside = Some(letter),
            None => out.push(letter),
        }
    }
    out
}

/// `name() {` at the head of a line, which is how every one of these is
/// written and all a shell needs.
fn defined_in(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once('(')?;
            let name = name.trim_end();
            let plain = !name.is_empty()
                && name
                    .chars()
                    .all(|letter| letter.is_ascii_alphanumeric() || letter == '_');
            let opens = rest.trim_start().starts_with(')');
            (plain && opens && line.trim_end().ends_with('{')).then(|| name.to_owned())
        })
        .collect()
}

/// A word stands where a command stands when nothing but a separator precedes
/// it: the head of a line, a pipe, a semicolon, an `&&`, a substitution.
fn called_in(body: &str, name: &str) -> bool {
    body.lines()
        .filter(|line| !line.trim_start().starts_with(&format!("{name}(")))
        .map(unquoted)
        .any(|line| {
            line.match_indices(name).any(|(at, _)| {
                let before = line[..at].trim_end();
                let after = &line[at + name.len()..];
                let starts = before.is_empty()
                    || before.ends_with(['|', ';', '&', '(', '`', '{', '!'])
                    || before.ends_with("$(")
                    || before.ends_with("then")
                    || before.ends_with("else")
                    || before.ends_with("do");
                let ends = after.is_empty() || after.starts_with([' ', ')', ';', '|', '&']);
                starts && ends
            })
        })
}

#[test]
fn no_shipped_flow_defines_a_shell_function_the_step_never_calls() {
    let mut dead = Vec::new();
    let mut read = 0;
    for path in flow_files(&root()) {
        let text = std::fs::read_to_string(&path).expect("a flow file reads");
        let flow: serde_json::Value = serde_json::from_str(&text).expect("a flow file is JSON");
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().replace(".flow.json", ""))
            .unwrap_or_default();
        for step in flow["graph"]["steps"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            let Some(body) = step["with"]["command"].as_str() else {
                continue;
            };
            read += 1;
            let id = step["id"].as_str().unwrap_or("?");
            for defined in defined_in(body) {
                if !called_in(body, &defined) {
                    dead.push(format!("{name} · {id} · {defined}()"));
                }
            }
        }
    }
    workspace::measured(read, "shell commands read in the shipped flows");
    assert!(
        dead.is_empty(),
        "a shell function is defined in a step that never calls it: {}. \
         A step is its own shell and nothing it defines reaches the next one, \
         so this is a helper for a gesture the step does not make. Delete it, \
         or make the step use it.",
        dead.join(", ")
    );
}

/// **A CHECK THAT CANNOT SAY NO IS NOT A CHECK.** These two hold the reading
/// itself, because the thing it reads is a shell and the first attempt was
/// fooled by a word inside a message.
#[test]
fn a_function_the_step_calls_is_not_called_dead() {
    let body = "greet() {\n  echo hello\n}\nout=$(greet)\n";

    assert_eq!(defined_in(body), vec!["greet".to_owned()]);
    assert!(called_in(body, "greet"), "it is called in a substitution");
}

#[test]
fn a_name_that_only_stands_inside_a_message_is_not_a_call() {
    let body = "forge() {\n  echo one\n}\necho \"no forge token for this account\" >&2\n";

    assert!(
        !called_in(body, "forge"),
        "a word a shell quotes is text, not a command"
    );
}
