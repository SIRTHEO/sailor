//! `sailor flow publish`: the home's flows go to a git repository of the
//! person's own, and a flow that carries a secret never leaves the machine.

use flow::system::{FlowSource, YOUR_ORIGIN};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A value that must not be published, and where it sits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Secret {
    pub step: String,
    pub key: String,
    pub why: String,
}

/// The shapes a key or token is known to take, wherever they sit.
const TOKEN_PREFIXES: &[&str] = &["sk-", "ghp_", "gho_", "github_pat_", "xoxb-", "xoxp-", "AKIA", "AIza", "HRKU-", "sk_live_", "rk_live_"];

/// The head of a private key in the armour every tool writes it in:
/// `-----BEGIN `, upper-case words each followed by one space, `PRIVATE KEY`,
/// an optional ` BLOCK` as PGP writes it, `-----`.
const PEM_OPENING: &str = "-----BEGIN ";
const PEM_PRIVATE_KEY_CLOSINGS: &[&str] = &["PRIVATE KEY-----", "PRIVATE KEY BLOCK-----"];

/// A whole header and no body, so a truncated key is still refused, while a
/// pattern that only names the armour is not.
fn holds_a_private_key_header(value: &str) -> bool {
    value.match_indices(PEM_OPENING).any(|(at, _)| {
        let mut rest = &value[at + PEM_OPENING.len()..];
        loop {
            if PEM_PRIVATE_KEY_CLOSINGS.iter().any(|closing| rest.starts_with(closing)) {
                return true;
            }
            let word = rest.bytes().take_while(u8::is_ascii_uppercase).count();
            if word == 0 || rest.as_bytes().get(word) != Some(&b' ') {
                return false;
            }
            rest = &rest[word + 1..];
        }
    })
}

/// How much must follow a prefix before a word is a key and not a word that
/// begins the same way. Without it `task-force` reads as an `sk-` token.
const HOW_LONG_A_TOKEN_RUNS: usize = 16;

/// The words in a key's name that say its value is a credential.
const CREDENTIAL_WORDS: &[&str] = &["key", "token", "secret", "password", "passwd", "credential"];

/// A token anywhere in the text, not only at its start: a key pasted into a
/// sentence a step sends is a key that has left the machine.
fn looks_like_a_token(value: &str) -> bool {
    if holds_a_private_key_header(value) {
        return true;
    }
    value
        .split(|letter: char| letter.is_whitespace() || "\"'=,;:()[]{}".contains(letter))
        .any(|word| {
            word.char_indices()
                .filter(|&(at, _)| {
                    word[..at].chars().next_back().is_none_or(|before| !before.is_ascii_alphanumeric())
                })
                .any(|(at, _)| {
                    let rest = &word[at..];
                    TOKEN_PREFIXES.iter().any(|prefix| {
                        rest.strip_prefix(prefix).is_some_and(|body| {
                            body.bytes()
                                .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'-')
                                .count()
                                >= HOW_LONG_A_TOKEN_RUNS
                        })
                    })
                })
        })
}

/// The top-level fields an action declares as a plain identifier, whatever
/// their name says: the name rule yields to them, the shape rule never does.
const IDENTIFIERS_NAMED_LIKE_A_CREDENTIAL: &[(&str, &str)] = &[
    (actions::store::STORE_WRITE_ACTION, "key"),
    (actions::store::STORE_WRITE_IF_ABSENT_ACTION, "key"),
    (actions::store::STORE_READ_ACTION, "key"),
];

fn names_a_credential(key: &str) -> bool {
    let lower = key.to_lowercase();
    CREDENTIAL_WORDS.iter().any(|word| lower.contains(word))
}

fn declared_an_identifier(action: Option<&str>, path: &str, key: &str) -> bool {
    let at_the_root = path == "with" || path == "inputs";
    at_the_root
        && action.is_some_and(|action| IDENTIFIERS_NAMED_LIKE_A_CREDENTIAL.contains(&(action, key)))
}

/// The secrets a flow carries: an `env` block with a literal value, a key
/// named like a credential holding a literal, or a token shape anywhere. A
/// reference (`{"$env": ...}`, `{"$from": ...}`) is never a secret.
pub fn secrets_in(flow: &Value) -> Vec<Secret> {
    let mut found = Vec::new();
    let steps = flow
        .pointer("/graph/steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let action_of = |id: &str| {
        steps
            .iter()
            .find(|step| step.get("id").and_then(Value::as_str) == Some(id))
            .and_then(|step| step.get("action").and_then(Value::as_str))
    };
    for step in &steps {
        let id = step.get("id").and_then(Value::as_str).unwrap_or("?");
        if let Some(with) = step.get("with") {
            walk(with, id, action_of(id), "with", &mut found);
        }
    }
    if let Some(Value::Object(inputs)) = flow.get("inputs") {
        for (step, input) in inputs {
            walk(input, step, action_of(step), "inputs", &mut found);
        }
    }
    found
}

/// Why this string must not be published, if it must not: one reason and not
/// three, because a value reported once per rule reads as three secrets.
fn why_a_literal_stays_home(key: &str, text: &str, in_env: bool, identifier: bool) -> Option<String> {
    if text.is_empty() || key.starts_with('$') {
        return None;
    }
    if in_env {
        Some(catalogue::say("cli.flow.publish_why_a_literal_in_env", &[]))
    } else if names_a_credential(key) && !identifier {
        Some(catalogue::say("cli.flow.publish_why_the_name_says_credential", &[]))
    } else if looks_like_a_token(text) || (identifier && looks_opaque(text)) {
        Some(catalogue::say("cli.flow.publish_why_the_shape_is_a_token", &[]))
    } else {
        None
    }
}

/// How long a piece of an identifier must run before its randomness makes it
/// a credential rather than a word.
const HOW_LONG_AN_OPAQUE_PIECE_RUNS: usize = 16;

/// Bits per character above which a long piece reads as random, not written.
const HOW_RANDOM_AN_OPAQUE_PIECE_IS: f64 = 3.5;

/// A credential with no known prefix: a long alphanumeric run that is not all
/// digits and is as random as a key. Judged only where the name rule yielded,
/// so ordinary text never meets it.
fn looks_opaque(value: &str) -> bool {
    if reads_as_a_row_name(value) {
        return false;
    }
    let an_opaque_piece = |piece: &str| {
        piece.len() >= HOW_LONG_AN_OPAQUE_PIECE_RUNS
            && !piece.bytes().all(|byte| byte.is_ascii_digit())
            && (bits_per_character(piece) >= HOW_RANDOM_AN_OPAQUE_PIECE_IS
                || piece.bytes().all(|byte| byte.is_ascii_hexdigit()))
    };
    looks_like_base64(value) || value.split(|letter: char| !letter.is_ascii_alphanumeric()).any(an_opaque_piece)
}

/// The longest lowercase word a row name carries; a longer run is a token.
const HOW_LONG_A_ROW_NAME_WORD_RUNS: usize = 20;

/// Lowercase words of one to 20 letters, and numeric run ids,
/// joined by single `/`, `-` or `_`. A word that is all hex from 16 letters is
/// a key, not a word; a digit-only piece stays a run id at any length.
fn reads_as_a_row_name(value: &str) -> bool {
    value.split(['/', '-', '_']).all(|piece| {
        let a_run_id = !piece.is_empty() && piece.bytes().all(|byte| byte.is_ascii_digit());
        let a_word = !piece.is_empty()
            && piece.len() <= HOW_LONG_A_ROW_NAME_WORD_RUNS
            && piece.bytes().all(|byte| byte.is_ascii_lowercase())
            && !(piece.len() >= HOW_LONG_AN_OPAQUE_PIECE_RUNS && piece.bytes().all(|byte| byte.is_ascii_hexdigit()));
        a_run_id || a_word
    })
}

/// How long a whole value in the base64 alphabet must run to be judged whole.
const HOW_LONG_A_BASE64_RUNS: usize = 24;

/// Bits per character above which a whole base64-alphabet value reads as random.
const HOW_RANDOM_A_BASE64_VALUE_IS: f64 = 4.0;

/// Judged whole before any split, because `+ / - _` would cut a key, standard
/// or URL-safe, into pieces too short to look random.
fn looks_like_base64(value: &str) -> bool {
    let body = value.trim_end_matches('=');
    let in_the_alphabet = body
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"+/-_".contains(&byte));
    value.len() - body.len() <= 2
        && body.len() >= HOW_LONG_A_BASE64_RUNS
        && in_the_alphabet
        && bits_per_character(body) >= HOW_RANDOM_A_BASE64_VALUE_IS
}

fn bits_per_character(piece: &str) -> f64 {
    let mut counts = std::collections::HashMap::new();
    for letter in piece.chars() {
        *counts.entry(letter).or_insert(0usize) += 1;
    }
    let length = piece.chars().count() as f64;
    counts
        .values()
        .map(|&count| {
            let share = count as f64 / length;
            -share * share.log2()
        })
        .sum()
}

fn walk(value: &Value, step: &str, action: Option<&str>, path: &str, found: &mut Vec<Secret>) {
    match value {
        Value::Object(fields) => {
            let in_env = path == "with.env" || path.ends_with(".env");
            for (key, inner) in fields {
                let here = format!("{path}.{key}");
                if let Value::String(text) = inner {
                    let identifier = declared_an_identifier(action, path, key);
                    if let Some(why) = why_a_literal_stays_home(key, text, in_env, identifier) {
                        found.push(Secret { step: step.to_owned(), key: here, why });
                    }
                    // The string is judged; walking into it would judge it a
                    // second time under the other rule.
                    continue;
                }
                walk(inner, step, action, &here, found);
            }
        }
        Value::Array(items) => {
            for (index, inner) in items.iter().enumerate() {
                walk(inner, step, action, &format!("{path}[{index}]"), found);
            }
        }
        Value::String(text) if looks_like_a_token(text) => found.push(Secret {
            step: step.to_owned(),
            key: path.to_owned(),
            why: catalogue::say("cli.flow.publish_why_the_shape_is_a_token", &[]),
        }),
        _ => {}
    }
}

fn flow_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() != ".git" {
                found.extend(flow_files(&path));
            }
        } else if path.to_string_lossy().ends_with(".flow.json") {
            found.push(path);
        }
    }
    found.sort();
    found
}

/// Every secret in every flow under `dir`, with the file it sits in.
pub fn secrets_under(dir: &Path) -> Result<Vec<(PathBuf, Secret)>, String> {
    let mut found = Vec::new();
    for path in flow_files(dir) {
        let text = std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let flow: Value = serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
        found.extend(secrets_in(&flow).into_iter().map(|secret| (path.clone(), secret)));
    }
    Ok(found)
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|error| {
            catalogue::say(
                "cli.flow.publish_git_did_not_run",
                &[("command", &args.join(" ")), ("error", &error.to_string())],
            )
        })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(catalogue::say(
            "cli.flow.publish_git_refused",
            &[
                ("command", &args.join(" ")),
                ("said", String::from_utf8_lossy(&output.stderr).trim()),
            ],
        ))
    }
}

/// What a publication did, for the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    pub flows: usize,
    pub committed: bool,
    pub pushed_to: Option<String>,
}

#[derive(Clone)]
struct FlowPrivacy {
    names: Vec<String>,
    home: String,
}

/// Publishes the flows under `dir`: refuses when any carries a secret,
/// otherwise initialises the repository if none, commits what changed, and
/// pushes to `remote` when one is given. Creating the remote, private, is the
/// person's gesture; nothing here talks to a forge.
pub fn publish(dir: &Path, remote: Option<&str>) -> Result<Published, String> {
    publish_with_privacy(dir, remote, None)
}

fn publish_with_privacy(
    dir: &Path,
    remote: Option<&str>,
    supplied_privacy: Option<FlowPrivacy>,
) -> Result<Published, String> {
    let secrets = secrets_under(dir)?;
    if let Some((path, secret)) = secrets.first() {
        return Err(catalogue::say(
            "cli.flow.publish_refused_a_secret",
            &[
                ("file", &path.display().to_string()),
                ("step", &secret.step),
                ("key", &secret.key),
                ("why", &secret.why),
            ],
        ));
    }
    let privacy = match supplied_privacy {
        Some(privacy) => privacy,
        None => flow_privacy_input()?,
    };
    if flow_content_is_private(dir, &privacy)? {
        return Err(catalogue::say("cli.flow.publish_refused_private_history", &[]));
    }
    let remembered = git(dir, &["remote", "get-url", "origin"]).ok();
    match (remote, remembered.as_deref()) {
        (Some(remote), Some(remembered)) if remote != remembered => {
            return Err(catalogue::say("cli.flow.publish_destination_conflict", &[]));
        }
        (Some(_), None) => {
            return Err(catalogue::say("cli.flow.publish_cannot_prove_privacy", &[]));
        }
        (_, Some(_)) => {
            let message = catalogue::say("cli.flow.publish_commit_message", &[]);
            flow_publication_preflight(dir, &privacy, &[message])?;
        }
        (None, None) => {}
    }
    let flows = flow_files(dir).len();
    if !dir.join(".git").exists() {
        git(dir, &["init", "-q"])?;
    }
    git(dir, &["add", "-A"])?;
    let staged = git(dir, &["diff", "--cached", "--name-only"])?;
    let committed = if staged.is_empty() {
        false
    } else {
        git(dir, &["commit", "-q", "-m", &catalogue::say("cli.flow.publish_commit_message", &[])])?;
        true
    };
    // A remote named once is remembered by git itself, so a later publication
    // needs no argument and no second place to declare it.
    let pushed_to = match (remote, remembered) {
        (_, Some(remembered)) => {
            let plan = flow_publication_preflight(dir, &privacy, &[])?;
            push_flows(dir, &plan)?;
            Some(remembered)
        }
        (None, None) => None,
        // Proven unreachable above: `(Some(_), None)` already returned. An
        // error and not a panic anyway — the proof is in this function, not
        // in the type, and a future edit that loosens it must not crash.
        (Some(_), None) => return Err(catalogue::say("cli.flow.publish_cannot_prove_privacy", &[])),
    };
    Ok(Published { flows, committed, pushed_to })
}

fn flow_privacy_input() -> Result<FlowPrivacy, String> {
    let (names, home) = toolbox::privacy::required_input(
        std::env::var("SAILOR_PRIVATE_NAMES").ok(),
        std::env::var("HOME").ok(),
    )
    .map_err(|_| catalogue::say("cli.flow.publish_cannot_prove_privacy", &[]))?;
    Ok(FlowPrivacy { names, home })
}

fn flow_publication_preflight(
    dir: &Path,
    privacy: &FlowPrivacy,
    planned_messages: &[String],
) -> Result<toolbox::privacy::PushPlan, String> {
    let plan = toolbox::privacy::push_plan(dir)
        .map_err(|_| catalogue::say("cli.flow.publish_cannot_prove_privacy", &[]))?;
    let private_metadata = toolbox::privacy::outgoing_metadata_is_private(
        dir,
        &plan,
        &privacy.names,
        &privacy.home,
        planned_messages,
    )
    .map_err(|_| catalogue::say("cli.flow.publish_cannot_prove_privacy", &[]))?;
    let private_content = flow_content_is_private(dir, privacy)?;
    if private_metadata || private_content {
        return Err(catalogue::say("cli.flow.publish_refused_private_history", &[]));
    }
    Ok(plan)
}

fn flow_content_is_private(dir: &Path, privacy: &FlowPrivacy) -> Result<bool, String> {
    for path in flow_files(dir) {
        let text = std::fs::read_to_string(path)
            .map_err(|_| catalogue::say("cli.flow.publish_cannot_prove_privacy", &[]))?;
        if !toolbox::privacy::what_cannot_be_published(&text, &privacy.names, Some(&privacy.home))
            .is_empty()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn push_flows(dir: &Path, plan: &toolbox::privacy::PushPlan) -> Result<(), String> {
    let refspec = format!("{}:{}", plan.head(), plan.destination());
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["push", "-q", "-u", plan.remote(), &refspec])
        .output()
        .map_err(|_| catalogue::say("cli.flow.publish_push_refused", &[]))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(catalogue::say("cli.flow.publish_push_refused", &[]))
    }
}

/// `sailor flow publish [remote]`: the flows of the source that is yours.
pub fn publish_flows(sources: &[FlowSource], remote: Option<&str>) -> Result<String, String> {
    let yours = sources
        .iter()
        .find(|source| source.origin == YOUR_ORIGIN)
        .ok_or_else(|| {
            let origins: Vec<&str> = sources.iter().map(|source| source.origin).collect();
            catalogue::say("cli.flow.no_flows_of_yours_to_publish", &[("origins", &origins.join(", "))])
        })?;
    let done = publish(&yours.dir, remote)?;
    let dir = yours.dir.display().to_string();
    let count = done.flows.to_string();
    let mut said = if done.committed {
        catalogue::say("cli.flow.published", &[("count", &count), ("dir", &dir)])
    } else {
        catalogue::say("cli.flow.published_nothing_new", &[("count", &count), ("dir", &dir)])
    };
    if let Some(remote) = done.pushed_to {
        said.push_str(&catalogue::say("cli.flow.published_and_pushed", &[("remote", &remote)]));
    }
    Ok(said)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn flow_with_env(env: Value) -> Value {
        json!({
            "id": "x",
            "graph": {"steps": [{"id": "ask", "action": "external_engine", "with": {"tool": "t", "env": env}}]},
            "inputs": {}
        })
    }

    #[test]
    fn a_literal_key_is_refused_and_a_reference_to_it_is_not() {
        let literal = flow_with_env(json!({"OPENROUTER_API_KEY": "sk-or-v1-0123456789abcdef"}));
        let found = secrets_in(&literal);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!((found[0].step.as_str(), found[0].key.as_str()), ("ask", "with.env.OPENROUTER_API_KEY"));

        let referenced = flow_with_env(json!({"OPENROUTER_API_KEY": {"$env": "OPENROUTER_API_KEY"}}));
        assert!(secrets_in(&referenced).is_empty(), "a reference carries nothing");
    }

    #[test]
    fn a_token_shape_anywhere_and_a_credential_name_with_a_literal_are_refused() {
        let in_stdin = json!({"id": "x", "graph": {"steps": [{"id": "s", "with": {"stdin": "use ghp_abcdefghijklmnop"}}]}});
        assert_eq!(secrets_in(&in_stdin)[0].key, "with.stdin");
        let named = json!({"id": "x", "graph": {"steps": [{"id": "s", "with": {"api_token": "plain-looking"}}]}});
        assert_eq!(secrets_in(&named)[0].key, "with.api_token");
        let harmless = json!({"id": "x", "graph": {"steps": [{"id": "s", "with": {"stdin": "count the keys of the map"}}]}});
        assert!(secrets_in(&harmless).is_empty());
        let patterns = json!({"id": "x", "graph": {"steps": [{"id": "s", "with": {
            "command": "grep -E 'sk-ant-[A-Za-z0-9_-]{24,}|github_pat_[A-Za-z0-9_]{30,}|github_pat_\\w+|ghp_\\w+|sk-ant-\\S+' flows"
        }}]}});
        assert!(secrets_in(&patterns).is_empty(), "{:?}", secrets_in(&patterns));
        let in_stdin = |text: &str| json!({"id": "x", "graph": {"steps": [{"id": "s", "with": {"stdin": text}}]}});
        // Split because the publication scan refuses a whole header outside test paths.
        let refused = [
            concat!("-----BEGIN RSA PRIVATE", " KEY-----"),
            concat!("-----BEGIN PRIVATE", " KEY-----"),
            concat!("-----BEGIN OPENSSH PRIVATE", " KEY-----"),
            concat!("-----BEGIN PGP PRIVATE", " KEY BLOCK-----"),
        ];
        let accepted = [
            "-----BEGIN CERTIFICATE-----",
            "grep -E '-----BEGIN (RSA |EC )?PRIVATE KEY|-----BEGIN [A-Z ]+' flows",
        ];
        let misjudged: Vec<&str> = refused
            .into_iter()
            .filter(|text| secrets_in(&in_stdin(text)).len() != 1)
            .chain(accepted.into_iter().filter(|text| !secrets_in(&in_stdin(text)).is_empty()))
            .collect();
        assert!(misjudged.is_empty(), "misjudged: {misjudged:?}");
    }

    fn one_step(action: &str, with: Value) -> Value {
        json!({"id": "x", "graph": {"steps": [{"id": "s", "action": action, "with": with}]}, "inputs": {}})
    }

    #[test]
    fn a_store_row_named_in_key_is_not_a_secret() {
        let written = one_step(
            "store_write",
            json!({"collection": "c", "key": "fault-card-from-a-note", "value": 1, "written_by": "w"}),
        );
        assert!(secrets_in(&written).is_empty(), "{:?}", secrets_in(&written));

        let with_a_run_id = one_step(
            "store_write",
            json!({"collection": "c", "key": "take-the-next-work-1789118850025316000", "value": 1, "written_by": "w"}),
        );
        assert!(secrets_in(&with_a_run_id).is_empty(), "{:?}", secrets_in(&with_a_run_id));

        let with_a_path = one_step(
            "store_read",
            json!({"collection": "c", "key": "cards/Surface/warm/palette/print/back"}),
        );
        assert!(secrets_in(&with_a_path).is_empty(), "{:?}", secrets_in(&with_a_path));

        let with_long_words = one_step(
            "store_read",
            json!({"collection": "c", "key": "abcdefghijklmnopqrst/uvwxyzabcdefghijklmn"}),
        );
        assert!(secrets_in(&with_long_words).is_empty(), "{:?}", secrets_in(&with_long_words));

        let through_inputs = json!({
            "id": "x",
            "graph": {"steps": [{"id": "read", "action": "store_read"}]},
            "inputs": {"read": {"collection": "c", "key": "fault-card-from-a-note"}}
        });
        assert!(secrets_in(&through_inputs).is_empty(), "{:?}", secrets_in(&through_inputs));
    }

    #[test]
    fn a_credential_name_holding_a_token_shape_is_refused() {
        let flow = one_step("external_engine", json!({"api_key": "sk-abcdefghijklmnopqrstuv"}));
        let found = secrets_in(&flow);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].key, "with.api_key");
        assert_eq!(found[0].why, catalogue::say("cli.flow.publish_why_the_name_says_credential", &[]));
    }

    #[test]
    fn an_opaque_credential_under_a_store_key_is_refused() {
        let flow = one_step(
            "store_write",
            json!({"collection": "c", "key": "Q7vX2mK9pL4wR8tN1cZ6yB3hJ5dF0gS2aE7uW9qT", "value": 1, "written_by": "w"}),
        );
        let found = secrets_in(&flow);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].key, "with.key");

        let in_base64 = one_step(
            "store_write",
            json!({"collection": "c", "key": "Q7vX2mK9pL4w+R8tN1cZ6yB3h/J5dF0gS2aE7uW9/qT6rH1kP8vL3cN=", "value": 1, "written_by": "w"}),
        );
        let found = secrets_in(&in_base64);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].key, "with.key");

        let in_url_safe_base64 = one_step(
            "store_write",
            json!({"collection": "c", "key": "Q7vX2mK9pL4w-R8tN1cZ6yB3h_J5dF0gS2aE7uW9_qT6rH1kP8vL3cN=", "value": 1, "written_by": "w"}),
        );
        let found = secrets_in(&in_url_safe_base64);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].key, "with.key");

        let shaped_like_a_row_name = [
            "deadbeefcafebabefacedeadbeefcafebabefade",
            "-ghp_abcdefghijklmnop",
            "notes-ghp_abcdefghijklmnop",
            "-abcdefghijklmnopqrst",
            "qzvtkmxwrplbjhdnfgcsyaeoiuwmzxkq",
            "HRKU-4594b794-0c94-416b-a374-bb33a025411f",
            "sk_live_zzzzyyyyxxxxwwww",
            "rk_live_zzzzyyyyxxxxwwww",
        ];
        let published: Vec<&str> = shaped_like_a_row_name
            .into_iter()
            .filter(|key| {
                secrets_in(&one_step(
                    "store_write",
                    json!({"collection": "c", "key": key, "value": 1, "written_by": "w"}),
                ))
                .len()
                    != 1
            })
            .collect();
        assert!(published.is_empty(), "published: {published:?}");
    }

    #[test]
    fn a_token_shape_under_a_store_key_is_still_refused() {
        let flow = one_step(
            "store_write",
            json!({"collection": "c", "key": "ghp_abcdefghijklmnopqrst", "value": 1, "written_by": "w"}),
        );
        let found = secrets_in(&flow);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].key, "with.key");
        assert_eq!(found[0].why, catalogue::say("cli.flow.publish_why_the_shape_is_a_token", &[]));
    }

    #[test]
    fn a_key_outside_the_store_actions_is_judged_by_its_name() {
        let flow = one_step("external_engine", json!({"key": "some-literal"}));
        let found = secrets_in(&flow);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].key, "with.key");
        assert_eq!(found[0].why, catalogue::say("cli.flow.publish_why_the_name_says_credential", &[]));
    }

    #[test]
    fn local_and_remote_flow_publication_refuse_private_material_before_persisting() {
        // A scratch named after the process alone comes back on the next run
        // with the same number, and a leftover from before decides the verdict.
        let dir = std::env::temp_dir().join(format!(
            "sailor-publish-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).expect("scratch");
        let privacy = FlowPrivacy {
            names: vec!["mylberry".to_owned()],
            home: "/home/tester".to_owned(),
        };
        std::fs::write(
            dir.join("leaky.flow.json"),
            flow_with_env(json!({"OPENROUTER_API_KEY": "sk-secret"})).to_string(),
        )
        .expect("write");
        let refused = publish(&dir, None).expect_err("a secret is refused");
        assert!(refused.contains("leaky.flow.json") && refused.contains("«ask»") && refused.contains("OPENROUTER_API_KEY"), "{refused}");
        assert!(!dir.join(".git").exists(), "nothing was initialised on a refusal");

        let private_flow = json!({
            "id": "x",
            "graph": {"steps": []},
            "inputs": {"note": "mylberry"}
        });
        std::fs::write(dir.join("leaky.flow.json"), private_flow.to_string()).expect("private flow");
        let refused = publish_with_privacy(&dir, None, Some(privacy.clone()))
            .expect_err("private content is refused before a local commit");
        assert!(
            !refused.contains("mylberry"),
            "the refusal carries private content: {refused}"
        );
        assert!(
            !dir.join(".git").exists(),
            "the refusal initialised a local repository"
        );

        std::fs::write(
            dir.join("leaky.flow.json"),
            flow_with_env(json!({"OPENROUTER_API_KEY": {"$env": "OPENROUTER_API_KEY"}})).to_string(),
        )
        .expect("rewrite");
        with_an_author(&dir);
        let done = publish_with_privacy(&dir, None, Some(privacy.clone()))
            .expect("a clean directory publishes");
        assert_eq!(done, Published { flows: 1, committed: true, pushed_to: None });
        let again = publish_with_privacy(&dir, None, Some(privacy.clone()))
            .expect("nothing new is fine");
        assert!(!again.committed, "nothing changed, nothing committed");

        // Beside the flows and not under them: a repository inside the
        // directory being published would be added to it.
        let elsewhere = dir.with_extension("elsewhere.git");
        let _ = std::fs::remove_dir_all(&elsewhere);
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q", "--bare"])
                .arg(&elsewhere)
                .status()
                .expect("git runs")
                .success(),
            "the bare repository this pushes into is the proof's own, not the network's"
        );
        let named = elsewhere.display().to_string();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .expect("git runs");
            assert!(output.status.success(), "git {args:?}");
        };
        git(&["remote", "add", "origin", &named]);
        git(&["push", "--quiet", "-u", "origin", "HEAD"]);
        let harmless_flow = json!({
            "id": "x",
            "graph": {"steps": []},
            "inputs": {"note": "ordinary"}
        });
        std::fs::write(dir.join("leaky.flow.json"), harmless_flow.to_string()).expect("harmless change");
        let pushed = publish_with_privacy(&dir, Some(&named), Some(privacy.clone()))
            .expect("a configured local destination is published to");
        assert!(pushed.committed && pushed.pushed_to.as_deref() == Some(named.as_str()));
        let conflict = publish_with_privacy(&dir, Some("another-destination"), Some(privacy.clone()))
            .expect_err("a conflicting destination is refused");
        assert!(
            !conflict.contains(&named) && !conflict.contains("another-destination"),
            "the conflict names configured destinations: {conflict}"
        );

        let head = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git runs")
            .stdout;
        let private_flow = json!({
            "id": "x",
            "graph": {"steps": []},
            "inputs": {"note": "mylberry"}
        });
        std::fs::write(dir.join("leaky.flow.json"), private_flow.to_string()).expect("private flow");
        let refused = publish_with_privacy(&dir, Some(&named), Some(privacy.clone()))
            .expect_err("proposed private flow is refused before a commit");
        assert!(
            !refused.contains("mylberry"),
            "the refusal carries private content: {refused}"
        );
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(["rev-parse", "HEAD"])
                .output()
                .expect("git runs")
                .stdout
                == head,
            "the refusal committed the proposed private flow"
        );

        let metadata_flow = json!({
            "id": "x",
            "graph": {"steps": []},
            "inputs": {"note": "second"}
        });
        std::fs::write(dir.join("leaky.flow.json"), metadata_flow.to_string()).expect("clean flow");
        git(&["add", "leaky.flow.json"]);
        git(&["commit", "--quiet", "-m", "mylberry"]);
        let refused = match flow_publication_preflight(&dir, &privacy, &[]) {
            Err(refused) => refused,
            Ok(_) => panic!("a newly created private commit is refused before push"),
        };
        assert!(
            !refused.contains("mylberry"),
            "the metadata refusal carries private content: {refused}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&elsewhere);
    }

    /// A repository whose commits have an author: a runner has none to guess.
    fn with_an_author(dir: &Path) {
        for args in [&["init", "-q"][..], &["config", "user.name", "a"], &["config", "user.email", "a@b"]] {
            git(dir, args).expect("preparing the scratch repository");
        }
    }

    #[test]
    fn publish_flows_reads_the_source_that_is_yours() {
        let dir = std::env::temp_dir().join(format!(
            "sailor-publish-flows-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).expect("scratch");
        std::fs::write(
            dir.join("clean.flow.json"),
            flow_with_env(json!({"OPENROUTER_API_KEY": {"$env": "OPENROUTER_API_KEY"}})).to_string(),
        )
        .expect("write");
        let privacy = FlowPrivacy {
            names: Vec::new(),
            home: "/home/tester".to_owned(),
        };
        with_an_author(&dir);
        publish_with_privacy(&dir, None, Some(privacy)).expect("a clean directory publishes");

        // `publish_flows` reads its own privacy input from the environment, so
        // this brings a declared list with fictitious names rather than lean
        // on whatever home the test happens to run in.
        let declared = dir.with_file_name(format!(
            "{}-declared-private-names",
            dir.file_name().and_then(|name| name.to_str()).unwrap_or("scratch")
        ));
        std::fs::write(&declared, "example-private-name\n").expect("the fictitious list is written");
        let previous_declared = std::env::var("SAILOR_PRIVATE_NAMES").ok();
        std::env::set_var("SAILOR_PRIVATE_NAMES", &declared);

        let sources = [FlowSource { origin: YOUR_ORIGIN, dir: dir.clone() }];
        let said = publish_flows(&sources, None).expect("the source that is yours publishes");
        assert!(said.contains("nothing") && said.contains(&dir.display().to_string()), "{said}");
        let builtin_only = [FlowSource::builtin()];
        let refused = publish_flows(&builtin_only, None).expect_err("built in flows are not yours");
        assert!(refused.contains("built in"), "{refused}");

        match previous_declared {
            Some(value) => std::env::set_var("SAILOR_PRIVATE_NAMES", value),
            None => std::env::remove_var("SAILOR_PRIVATE_NAMES"),
        }
        let _ = std::fs::remove_file(&declared);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
