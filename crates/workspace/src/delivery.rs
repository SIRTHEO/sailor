//! **A PROPOSED CHANGE MUST NEVER AUTHORIZE ITSELF.** The branch under review
//! holds a `.sailor/delivery-policy.json` whoever opened it can edit, so the
//! file is taken out of the commit the declared trunk points at (ADR-020).
//! One copy: `sailor policy` and `delivery_policy` must not disagree.

use serde::Serialize;
use std::path::Path;

pub const POLICY_FILE: &str = ".sailor/delivery-policy.json";

pub const GOVERNED: &[&str] = &["merge", "push", "release"];

pub const DECLARED: &[&str] = &["forge", "remote"];

const AUTO: &str = "auto";
const ASK: &str = "ask";

/// **A REFUSAL IS A CASE, NOT A SENTENCE.** The two callers word these
/// differently, and telling them apart by matching words in a message is how a
/// reworded sentence silently becomes the wrong refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NoTrunkDeclared,
    UnknownTrunk(String),
    NoPolicyOnTrunk,
    NotAutoNorAsk { field: String, found: String },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::NoTrunkDeclared => write!(out, "this repository declares no trunk"),
            Refusal::UnknownTrunk(trunk) => {
                write!(out, "the trunk {trunk} does not exist in this tree")
            }
            Refusal::NoPolicyOnTrunk => write!(out, "the trunk carries no readable {POLICY_FILE}"),
            Refusal::NotAutoNorAsk { field, found } => {
                write!(
                    out,
                    "the delivery policy says {found} for {field}, not {AUTO} or {ASK}"
                )
            }
        }
    }
}

/// The `asks_*` fields are not in the file: combining the settings at four call
/// sites is how four flows come to disagree. Integrating and releasing both
/// push, so both ask when the push asks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Policy {
    pub merge: String,
    pub push: String,
    pub release: String,
    pub forge: String,
    pub remote: String,
    pub read_from: String,
    pub asks_publication: String,
    pub asks_integration: String,
    pub asks_release: String,
}

impl Policy {
    fn of(
        merge: String,
        push: String,
        release: String,
        declared: [String; 2],
        read_from: String,
    ) -> Policy {
        let [forge, remote] = declared;
        let either = |one: &str, other: &str| {
            if one == ASK || other == ASK {
                ASK.to_owned()
            } else {
                AUTO.to_owned()
            }
        };
        Policy {
            asks_publication: push.clone(),
            asks_integration: either(&merge, &push),
            asks_release: either(&release, &push),
            merge,
            push,
            release,
            forge,
            remote,
            read_from,
        }
    }
}

pub fn policy_on_the_trunk(repo: &Path) -> Result<Policy, Refusal> {
    let trunk = crate::declared_trunk(repo).map_err(|_| Refusal::NoTrunkDeclared)?;
    let commit = crate::git(repo, &["rev-parse", "--verify", &format!("{trunk}^{{commit}}")])
        .map_err(|_| Refusal::UnknownTrunk(trunk))?
        .trim()
        .to_owned();
    let text = crate::git(repo, &["show", &format!("{commit}:{POLICY_FILE}")])
        .map_err(|_| Refusal::NoPolicyOnTrunk)?;
    read_policy(&text, commit)
}

pub fn read_policy(text: &str, read_from: String) -> Result<Policy, Refusal> {
    let file: serde_json::Value =
        serde_json::from_str(text).map_err(|_| Refusal::NoPolicyOnTrunk)?;
    if !file.is_object() {
        return Err(Refusal::NoPolicyOnTrunk);
    }
    let mut said = Vec::with_capacity(GOVERNED.len());
    for field in GOVERNED {
        match file.get(field).and_then(serde_json::Value::as_str) {
            Some(word) if word == AUTO || word == ASK => said.push(word.to_owned()),
            found => {
                return Err(Refusal::NotAutoNorAsk {
                    field: (*field).to_owned(),
                    found: found.map_or_else(
                        || file.get(field).map(ToString::to_string).unwrap_or_default(),
                        str::to_owned,
                    ),
                })
            }
        }
    }
    let mut declared = DECLARED.iter().map(|field| {
        file.get(field)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    });
    let declared = [
        declared.next().unwrap_or_default(),
        declared.next().unwrap_or_default(),
    ];
    let mut said = said.into_iter();
    let (merge, push, release) = (
        said.next().unwrap_or_default(),
        said.next().unwrap_or_default(),
        said.next().unwrap_or_default(),
    );
    Ok(Policy::of(merge, push, release, declared, read_from))
}
