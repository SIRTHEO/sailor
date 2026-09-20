//! The delivery policy, read where it can be trusted: as the trunk commits it.
//!
//! **A PROPOSED CHANGE MUST NEVER AUTHORIZE ITSELF.** The working tree and the
//! branch under review both hold a `.sailor/delivery-policy.json` that whoever
//! opened the branch can edit, so neither is read here; the file is taken out
//! of the commit the declared trunk points at. Which branch that is, the
//! repository says — the product assumes no name of its own (ADR-020).
//!
//! It lives in `workspace` because two callers need the same answer and a
//! second copy of this rule would diverge from the first: `sailor policy` for
//! a person at a terminal, and the `delivery_policy` action for the four flows
//! that deliver. Until this module existed the flows carried it as 1.966
//! characters of shell repeated four times over.

use serde::Serialize;
use std::path::Path;

/// Where the policy is committed, relative to the root of the repository.
pub const POLICY_FILE: &str = ".sailor/delivery-policy.json";

/// The three settings the file governs, in the order they are printed.
pub const GOVERNED: &[&str] = &["merge", "push", "release"];

/// What the repository declares about itself in the same file. The product
/// keeps no list of the forges or the remotes that exist, so there is nothing
/// to check these against — only whether they were declared (ADR-020). An
/// empty string is the absence, written down rather than guessed at.
pub const DECLARED: &[&str] = &["forge", "remote"];

/// The two words a setting may hold. Anything else is a refusal rather than a
/// default: a policy nobody can read must not resolve to the lenient side.
const AUTO: &str = "auto";
const ASK: &str = "ask";

/// Why the policy could not be read.
///
/// **A REFUSAL IS A CASE, NOT A SENTENCE.** Both callers say this differently —
/// one to a person at a terminal out of the catalogue, one to a step as the
/// reason a run stopped — and telling the four apart by matching words in a
/// message is how a reworded sentence silently becomes the wrong refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The repository declared no trunk. Nothing is assumed (ADR-020).
    NoTrunkDeclared,
    /// It declared one that is not in this tree.
    UnknownTrunk(String),
    /// The trunk carries no policy, or none that reads as an object.
    NoPolicyOnTrunk,
    /// A governed field holds a word that is neither `auto` nor `ask`.
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

/// What the trunk says, and where it was read.
///
/// The three `asks_*` fields are not in the file: they are what each gesture
/// costs once the settings are combined, and combining them at each of the
/// four call sites is how four flows come to disagree. Publishing asks when
/// pushing asks; integrating asks when merging *or* pushing does, because an
/// integration pushes; releasing likewise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Policy {
    pub merge: String,
    pub push: String,
    pub release: String,
    /// Empty when the repository declared none.
    pub forge: String,
    pub remote: String,
    /// The commit the file was read out of, so a run can be audited against
    /// the exact text that authorized it.
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

/// The policy as the declared trunk commits it.
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

/// The half that touches nothing, so a case can hand it a text and read the
/// refusal rather than build a repository to get one.
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
