//! The last gate a candidate passes before it leaves this machine.
//!
//! **A FLOW STEP IS NOT A SHELL PROGRAM.** Integration and a release ask the
//! same questions before anything leaves, so they ask them here, once, and the
//! answer is a verdict a case can reach without a forge behind it.

use crate::process::{run_with_timeout, RunOutcome};
use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub const CANDIDATE_GATE_ACTION: &str = "candidate_gate";

const GATE_FIELDS: &[&str] = &[
    "repo",
    "remote",
    "base",
    "candidate",
    "scan",
    "scan_base",
    "policy_read",
    "trunk_was",
    "already",
    "branch",
    "reviewed",
    "in_service",
    "walked_on",
];

/// A remote that does not answer in this long is one nothing is decided on.
const A_REMOTE_ANSWERS_WITHIN: Duration = Duration::from_secs(120);

/// The whole step's allowance, given to the scan alone.
const THE_SCAN_ANSWERS_WITHIN: Duration = Duration::from_secs(300);

pub fn register_candidate_gate(registry: &mut flow::ActionRegistry) {
    registry.register(CANDIDATE_GATE_ACTION, CandidateGateAction);
}

#[derive(Debug, Deserialize)]
struct GateSpec {
    repo: PathBuf,
    remote: String,
    base: String,
    candidate: String,
    /// The scan the repository commits, relative to its top: the product
    /// names no program of its own here, the flow says which one judges.
    scan: String,
    #[serde(default)]
    scan_base: String,
    policy_read: Value,
    #[serde(default)]
    trunk_was: String,
    #[serde(default)]
    already: Value,
    #[serde(default)]
    branch: String,
    #[serde(default)]
    reviewed: String,
    #[serde(default)]
    in_service: Value,
    #[serde(default)]
    walked_on: String,
}

/// The four words of a policy an authorization rests on.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PolicyWords {
    pub merge: String,
    pub push: String,
    pub release: String,
    pub read_from: String,
}

impl From<&workspace::delivery::Policy> for PolicyWords {
    fn from(policy: &workspace::delivery::Policy) -> PolicyWords {
        PolicyWords {
            merge: policy.merge.clone(),
            push: policy.push.clone(),
            release: policy.release.clone(),
            read_from: policy.read_from.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TheScan {
    TheTrunks,
    Differs,
    NotOnTheTrunk,
}

/// The heads of the branch a review pinned, read locally and on the remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heads {
    pub reviewed: String,
    pub local: Option<String>,
    pub remote: Option<String>,
}

/// The binary a release put into service, and the digest the journey that
/// accepted the candidate was walked on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub commit: String,
    pub sha256: String,
    pub walked_on: String,
}

#[derive(Debug, Clone)]
pub struct Readings {
    pub candidate: String,
    pub scan: TheScan,
    pub policy_read: PolicyWords,
    pub policy_now: Result<PolicyWords, String>,
    pub trunk_was: String,
    pub trunk_now: Option<String>,
    pub already: bool,
    pub heads: Option<Heads>,
    pub service: Option<Service>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    ScanNotOnTheTrunk,
    ScanDiffers,
    NoTrustedPolicy(String),
    PolicyChanged,
    TrunkMoved(Option<String>),
    BranchMoved(Option<String>),
    RemoteBranchMoved(Option<String>),
    BuiltFromSomethingElse(String),
    ServiceChanged(String),
    LetTheScanJudge,
}

fn as_commit(said: &str) -> Option<String> {
    Some(said.to_owned()).filter(|said| !said.is_empty())
}

/// **ONE THAT MERGED ALREADY CARRIES NO HEAD TO COMPARE**: the trunk and the
/// remote branch are allowed to have moved on, because the pull request did.
/// The local branch is not, and neither is a trunk nobody could read.
pub fn what_the_gate_says(readings: &Readings) -> Verdict {
    match readings.scan {
        TheScan::TheTrunks => {}
        TheScan::Differs => return Verdict::ScanDiffers,
        TheScan::NotOnTheTrunk => return Verdict::ScanNotOnTheTrunk,
    }
    match &readings.policy_now {
        Err(why) => return Verdict::NoTrustedPolicy(why.clone()),
        Ok(now) if *now != readings.policy_read => return Verdict::PolicyChanged,
        Ok(_) => {}
    }
    if let Some(service) = &readings.service {
        if service.commit != readings.candidate {
            return Verdict::BuiltFromSomethingElse(service.commit.clone());
        }
        if service.sha256 != service.walked_on {
            return Verdict::ServiceChanged(service.sha256.clone());
        }
    }
    let moved = readings.trunk_now != as_commit(&readings.trunk_was);
    if moved && (!readings.already || readings.trunk_now.is_none()) {
        return Verdict::TrunkMoved(readings.trunk_now.clone());
    }
    if let Some(heads) = &readings.heads {
        if heads.local.as_deref() != Some(heads.reviewed.as_str()) {
            return Verdict::BranchMoved(heads.local.clone());
        }
        if !readings.already && heads.remote.as_deref() != Some(heads.reviewed.as_str()) {
            return Verdict::RemoteBranchMoved(heads.remote.clone());
        }
    }
    Verdict::LetTheScanJudge
}

/// Nothing leaves. The class is written here rather than held in a constant:
/// the scan that pairs every class with a sentence reads the literal at the
/// call, and a constant is a blind spot.
fn held(why: String) -> ActionError {
    ActionError::new("the_candidate_is_held", why)
}

fn git(repo: &Path, args: &[&str], within: Duration) -> Result<String, String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo).args(args);
    match run_with_timeout(command, within) {
        RunOutcome::Finished { status, stdout, .. } if status.success() => {
            Ok(String::from_utf8_lossy(&stdout).into_owned())
        }
        RunOutcome::Finished { status, stderr, .. } => Err(format!(
            "git {} exited {}: {}",
            args.join(" "),
            status.code().unwrap_or(-1),
            String::from_utf8_lossy(&stderr).trim()
        )),
        RunOutcome::TimedOut => Err(format!(
            "git {} did not answer within {}s",
            args.join(" "),
            within.as_secs()
        )),
        RunOutcome::SpawnFailed(why) => Err(format!("git did not start: {why}")),
    }
}

fn head_of(repo: &Path, reference: &str) -> Option<String> {
    git(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{reference}^{{commit}}"),
        ],
        A_REMOTE_ANSWERS_WITHIN,
    )
    .ok()
    .and_then(|said| as_commit(said.trim()))
}

/// `ls-remote`'s own exit is read before its answer: a lost transport is not
/// an empty ref, and nothing is decided on an empty answer.
fn remote_head_of(repo: &Path, remote: &str, reference: &str) -> Result<Option<String>, String> {
    let listed = git(
        repo,
        &["ls-remote", remote, reference],
        A_REMOTE_ANSWERS_WITHIN,
    )?;
    Ok(listed.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        let commit = words.next()?;
        (words.next()? == reference).then(|| commit.to_owned())
    }))
}

/// The scan in this tree, held against the one the trusted trunk commits: a
/// branch may not bring the judge that will judge it.
fn the_scan(repo: &Path, scan: &str, trunk_commit: &str) -> TheScan {
    let Ok(on_the_trunk) = git(
        repo,
        &["show", &format!("{trunk_commit}:{scan}")],
        A_REMOTE_ANSWERS_WITHIN,
    ) else {
        return TheScan::NotOnTheTrunk;
    };
    match std::fs::read_to_string(repo.join(scan)) {
        Ok(here) if here == on_the_trunk => TheScan::TheTrunks,
        _ => TheScan::Differs,
    }
}

/// A reference hands on a value or its JSON, and the flows spell it both ways.
fn as_answer(said: &Value) -> Value {
    match said.as_str() {
        Some(text) => serde_json::from_str(text).unwrap_or(Value::Null),
        None => said.clone(),
    }
}

fn is_yes(said: &Value) -> bool {
    matches!(as_answer(said), Value::Bool(true))
}

fn service_of(spec: &GateSpec) -> Result<Option<Service>, ActionError> {
    let said = as_answer(&spec.in_service);
    if said.is_null() {
        return Ok(None);
    }
    let word = |key: &str| said.get(key).and_then(Value::as_str).unwrap_or_default();
    if word("commit").is_empty() || word("sha256").is_empty() {
        return Err(held(
            "the binary in service was named without its commit and its sha256".to_owned(),
        ));
    }
    Ok(Some(Service {
        commit: word("commit").to_owned(),
        sha256: word("sha256").to_owned(),
        walked_on: spec.walked_on.clone(),
    }))
}

fn refusal(verdict: Verdict, spec: &GateSpec) -> ActionError {
    let shown = |said: Option<String>| said.unwrap_or_else(|| "nothing".to_owned());
    held(match verdict {
        Verdict::ScanNotOnTheTrunk => format!(
            "the trusted trunk commits no {}: there is nothing to judge the candidate with",
            spec.scan
        ),
        Verdict::ScanDiffers => format!(
            "{} in this tree differs from the one the trusted trunk commits: the gate refuses to \
             judge with it",
            spec.scan
        ),
        Verdict::NoTrustedPolicy(why) => format!("no trusted delivery policy: {why}"),
        Verdict::PolicyChanged => "the delivery policy changed since it was read: the \
                                   authorization in flight no longer holds; run the flow again"
            .to_owned(),
        Verdict::TrunkMoved(now) => format!(
            "{}/{} moved from {} to {} since the candidate was built on it: run the flow again",
            spec.remote,
            spec.base,
            shown(as_commit(&spec.trunk_was)),
            shown(now)
        ),
        Verdict::BranchMoved(now) => format!(
            "the branch {} stands at {}, not at the reviewed commit {}",
            spec.branch,
            shown(now),
            spec.reviewed
        ),
        Verdict::RemoteBranchMoved(now) => format!(
            "the remote branch head {} differs from the reviewed commit {}: the pull request no \
             longer shows what was reviewed",
            shown(now),
            spec.reviewed
        ),
        Verdict::BuiltFromSomethingElse(stamp) => format!(
            "the binary in service was built from {stamp}, not from the candidate {}",
            spec.candidate
        ),
        Verdict::ServiceChanged(digest) => format!(
            "the binary in service has sha256 {digest}, and the journey was walked on {}: it \
             changed since",
            spec.walked_on
        ),
        Verdict::LetTheScanJudge => "the gate refused nothing".to_owned(),
    })
}

/// What this machine answers alone, with the remote taken as unmoved: a gate
/// that would refuse on these refuses before a transport can hide why.
fn read_here(spec: &GateSpec) -> Result<Readings, ActionError> {
    let policy_read: PolicyWords = serde_json::from_value(as_answer(&spec.policy_read))
        .map_err(|why| held(format!("the policy the flow read is not a policy: {why}")))?;
    let policy_now = workspace::delivery::policy_on_the_trunk(&spec.repo)
        .map(|policy| PolicyWords::from(&policy))
        .map_err(|why| why.to_string());
    let scan = match &policy_now {
        Ok(now) => the_scan(&spec.repo, &spec.scan, &now.read_from),
        Err(_) => the_scan(&spec.repo, &spec.scan, &policy_read.read_from),
    };
    Ok(Readings {
        candidate: spec.candidate.clone(),
        scan,
        policy_read,
        policy_now,
        trunk_was: spec.trunk_was.clone(),
        trunk_now: as_commit(&spec.trunk_was),
        already: is_yes(&spec.already),
        heads: None,
        service: service_of(spec)?,
    })
}

fn read_the_remote(spec: &GateSpec, here: Readings) -> Result<Readings, ActionError> {
    git(
        &spec.repo,
        &["fetch", "--quiet", "--no-tags", &spec.remote],
        A_REMOTE_ANSWERS_WITHIN,
    )
    .map_err(|why| held(format!("cannot read {}: {why}", spec.remote)))?;
    let trunk_now = head_of(
        &spec.repo,
        &format!("refs/remotes/{}/{}", spec.remote, spec.base),
    );
    let heads = if spec.branch.is_empty() {
        None
    } else {
        let branch = format!("refs/heads/{}", spec.branch);
        Some(Heads {
            reviewed: spec.reviewed.clone(),
            local: head_of(&spec.repo, &branch),
            remote: remote_head_of(&spec.repo, &spec.remote, &branch)
                .map_err(|why| held(format!("cannot read {branch} on {}: {why}", spec.remote)))?,
        })
    };
    Ok(Readings {
        trunk_now,
        heads,
        ..here
    })
}

fn let_the_scan_judge(verdict: Verdict, spec: &GateSpec) -> Result<(), ActionError> {
    match verdict {
        Verdict::LetTheScanJudge => Ok(()),
        refused => Err(refusal(refused, spec)),
    }
}

fn run_the_scan(spec: &GateSpec) -> Result<(), ActionError> {
    let mut command = Command::new(spec.repo.join(&spec.scan));
    command.current_dir(&spec.repo).arg(&spec.candidate);
    if !spec.scan_base.is_empty() {
        command.arg(&spec.scan_base);
    }
    let (code, said) = match run_with_timeout(command, THE_SCAN_ANSWERS_WITHIN) {
        RunOutcome::Finished {
            status,
            stdout,
            stderr,
        } => (
            status.code(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr)
            ),
        ),
        RunOutcome::TimedOut => (
            None,
            format!(
                "it did not answer within {}s",
                THE_SCAN_ANSWERS_WITHIN.as_secs()
            ),
        ),
        RunOutcome::SpawnFailed(why) => (None, format!("it did not start: {why}")),
    };
    if code == Some(0) {
        return Ok(());
    }
    Err(ActionError::new(
        "publication_refused",
        format!(
            "{} said no to {} ({}); nothing leaves:\n{}",
            spec.scan,
            spec.candidate,
            code.map_or_else(|| "no exit".to_owned(), |code| format!("exit {code}")),
            said.trim()
        ),
    ))
}

struct CandidateGateAction;

impl Action for CandidateGateAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: GateSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let here = read_here(&spec)?;
        let_the_scan_judge(what_the_gate_says(&here), &spec)?;
        let_the_scan_judge(what_the_gate_says(&read_the_remote(&spec, here)?), &spec)?;
        run_the_scan(&spec)?;
        Ok(ActionOutcome::Went(
            json!({ "ref": spec.candidate, "privacy_exit": 0 }),
        ))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match declared.as_object() {
            Some(fields) => fields
                .keys()
                .filter(|name| !GATE_FIELDS.contains(&name.as_str()))
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    /// It fetches, which moves nothing anybody reads as the delivery's own.
    fn redo_evidence(&self, _record: &flow::StepRecord) -> flow::RedoEvidence {
        flow::RedoEvidence::TouchesNothing
    }
}
