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
const TOKEN_PREFIXES: &[&str] = &["sk-", "ghp_", "gho_", "github_pat_", "xoxb-", "xoxp-", "AKIA", "AIza"];

/// The head of a private key in the armour every tool writes it in.
const PEM_HEADER: &str = "-----BEGIN";

/// How much must follow a prefix before a word is a key and not a word that
/// begins the same way. Without it `task-force` reads as an `sk-` token.
const HOW_LONG_A_TOKEN_RUNS: usize = 16;

/// The words in a key's name that say its value is a credential.
const CREDENTIAL_WORDS: &[&str] = &["key", "token", "secret", "password", "passwd", "credential"];

/// A token anywhere in the text, not only at its start: a key pasted into a
/// sentence a step sends is a key that has left the machine.
fn looks_like_a_token(value: &str) -> bool {
    if value.contains(PEM_HEADER) {
        return true;
    }
    value
        .split(|letter: char| letter.is_whitespace() || "\"'=,;:()[]{}".contains(letter))
        .any(|word| {
            TOKEN_PREFIXES.iter().any(|prefix| {
                word.starts_with(prefix) && word.len() >= prefix.len() + HOW_LONG_A_TOKEN_RUNS
            })
        })
}

fn names_a_credential(key: &str) -> bool {
    let lower = key.to_lowercase();
    CREDENTIAL_WORDS.iter().any(|word| lower.contains(word))
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
    for step in &steps {
        let id = step.get("id").and_then(Value::as_str).unwrap_or("?").to_owned();
        if let Some(with) = step.get("with") {
            walk(with, &id, "with", &mut found);
        }
    }
    if let Some(Value::Object(inputs)) = flow.get("inputs") {
        for (step, input) in inputs {
            walk(input, step, "inputs", &mut found);
        }
    }
    found
}

/// Why this string must not be published, if it must not: one reason and not
/// three, because a value reported once per rule reads as three secrets.
fn why_a_literal_stays_home(key: &str, text: &str, in_env: bool) -> Option<String> {
    if text.is_empty() || key.starts_with('$') {
        return None;
    }
    if in_env {
        Some(catalogue::say("cli.flow.publish_why_a_literal_in_env", &[]))
    } else if names_a_credential(key) {
        Some(catalogue::say("cli.flow.publish_why_the_name_says_credential", &[]))
    } else if looks_like_a_token(text) {
        Some(catalogue::say("cli.flow.publish_why_the_shape_is_a_token", &[]))
    } else {
        None
    }
}

fn walk(value: &Value, step: &str, path: &str, found: &mut Vec<Secret>) {
    match value {
        Value::Object(fields) => {
            let in_env = path == "with.env" || path.ends_with(".env");
            for (key, inner) in fields {
                let here = format!("{path}.{key}");
                if let Value::String(text) = inner {
                    if let Some(why) = why_a_literal_stays_home(key, text, in_env) {
                        found.push(Secret { step: step.to_owned(), key: here, why });
                    }
                    // The string is judged; walking into it would judge it a
                    // second time under the other rule.
                    continue;
                }
                walk(inner, step, &here, found);
            }
        }
        Value::Array(items) => {
            for (index, inner) in items.iter().enumerate() {
                walk(inner, step, &format!("{path}[{index}]"), found);
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

/// Publishes the flows under `dir`: refuses when any carries a secret,
/// otherwise initialises the repository if none, commits what changed, and
/// pushes to `remote` when one is given. Creating the remote, private, is the
/// person's gesture; nothing here talks to a forge.
pub fn publish(dir: &Path, remote: Option<&str>) -> Result<Published, String> {
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
    let remembered = git(dir, &["remote", "get-url", "origin"]).ok();
    let pushed_to = match (remote, remembered) {
        (Some(remote), None) => {
            git(dir, &["remote", "add", "origin", remote])?;
            git(dir, &["push", "-q", "-u", "origin", "HEAD"])?;
            Some(remote.to_owned())
        }
        (Some(remote), Some(remembered)) if remote != remembered => {
            return Err(catalogue::say(
                "cli.flow.publish_remote_already_named",
                &[("remembered", &remembered), ("asked", remote)],
            ));
        }
        (_, Some(remembered)) => {
            git(dir, &["push", "-q", "-u", "origin", "HEAD"])?;
            Some(remembered)
        }
        (None, None) => None,
    };
    Ok(Published { flows, committed, pushed_to })
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
    }

    #[test]
    fn publishing_refuses_a_directory_with_a_secret_and_commits_a_clean_one() {
        let dir = std::env::temp_dir().join(format!("sailor-publish-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        std::fs::write(
            dir.join("leaky.flow.json"),
            flow_with_env(json!({"OPENROUTER_API_KEY": "sk-secret"})).to_string(),
        )
        .expect("write");
        let refused = publish(&dir, None).expect_err("a secret is refused");
        assert!(refused.contains("leaky.flow.json") && refused.contains("«ask»") && refused.contains("OPENROUTER_API_KEY"), "{refused}");
        assert!(!dir.join(".git").exists(), "nothing was initialised on a refusal");

        std::fs::write(
            dir.join("leaky.flow.json"),
            flow_with_env(json!({"OPENROUTER_API_KEY": {"$env": "OPENROUTER_API_KEY"}})).to_string(),
        )
        .expect("rewrite");
        let done = publish(&dir, None).expect("a clean directory publishes");
        assert_eq!(done, Published { flows: 1, committed: true, pushed_to: None });
        let again = publish(&dir, None).expect("nothing new is fine");
        assert!(!again.committed, "nothing changed, nothing committed");

        let sources = [FlowSource { origin: YOUR_ORIGIN, dir: dir.clone() }];
        // Beside the flows and not under them: a repository inside the
        // directory being published would be added to it.
        let elsewhere = dir.with_extension("elsewhere.git");
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
        let pushed = publish(&dir, Some(&named)).expect("a remote named once is pushed to");
        assert_eq!(pushed.pushed_to.as_deref(), Some(named.as_str()));
        let again = publish(&dir, None).expect("the remote is remembered, not asked for twice");
        assert_eq!(again.pushed_to.as_deref(), Some(named.as_str()), "no argument, same place");
        let other = publish(&dir, Some("/somewhere/else.git")).expect_err("a second place is refused");
        assert!(other.contains(&named), "the refusal says where they already go: {other}");


        let said = publish_flows(&sources, None).expect("the source that is yours publishes");
        assert!(said.contains("nothing") && said.contains(&dir.display().to_string()), "{said}");
        let builtin_only = [FlowSource::builtin()];
        let refused = publish_flows(&builtin_only, None).expect_err("built in flows are not yours");
        assert!(refused.contains("built in"), "{refused}");
    }
}
