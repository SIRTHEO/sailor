//! The three nodes an agent **says it is here** with, and reads who else is.
//!
//! The meeting place is the ledger: one house per machine, answering the same
//! path from every worktree, opened directly by several processes with none of
//! them the server. An announcement **expires and is renewed** — the one
//! promise a process the system killed in its sleep cannot fake.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use ledger::{Ledger, StoreRecord};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

/// The name `WorkClaimAction` registers itself under.
pub const WORK_CLAIM_ACTION: &str = "work_claim";
/// The name `WorkReleaseAction` registers itself under.
pub const WORK_RELEASE_ACTION: &str = "work_release";
/// The name `WorkSurveyAction` registers itself under.
pub const WORK_SURVEY_ACTION: &str = "work_survey";

/// The ledger collection the claims live in.
pub const CLAIMS_COLLECTION: &str = "work-claims";

/// How long a claim that declares no duration of its own lasts.
pub const DEFAULT_LEASE_SECONDS: i64 = 900;

/// **THE SURVEY REGISTERS WITHOUT A STORE, THE TWO THAT WRITE DO NOT.** A
/// shipped flow names it, and `flow check` must be able to say the step names a
/// real action without opening anything. Without one it **refuses** rather than
/// answering «nobody»: unreadable is not empty, and seven agents read as zero
/// is the fault this module is written against.
pub fn register_presence(registry: &mut flow::ActionRegistry, ledger: Option<Ledger>) {
    registry.register(WORK_SURVEY_ACTION, WorkSurveyAction::new(ledger.clone()));
    if let Some(ledger) = ledger {
        registry.register(WORK_CLAIM_ACTION, WorkClaimAction::new(ledger.clone()));
        registry.register(WORK_RELEASE_ACTION, WorkReleaseAction::new(ledger));
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
struct ClaimSpec {
    agent: String,
    repository: String,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    doing: Option<String>,
    #[serde(default)]
    lease_seconds: Option<i64>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    at: Option<i64>,
    #[serde(default)]
    refuse_when_shared: bool,
}

#[derive(Debug, Deserialize)]
struct ReleaseSpec {
    agent: String,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SurveySpec {
    #[serde(default)]
    repository: Option<String>,
    #[serde(default)]
    at: Option<i64>,
}

/// Which announcement is which, for a step: an agent and the process holding it.
pub fn claim_key(agent: &str, holder: &str) -> String {
    format!("{agent}#{holder}")
}

/// **A TERMINAL HOLDS ONE ANNOUNCEMENT, whatever runs in it.** The name of the
/// command line is an attribute and not an identity: it changes when a graft
/// learns which line it is or a profile is switched, and keyed on it the old
/// name stays announced beside the new until its lease runs out. Nor is it the
/// pid of whoever writes: a hook is a new process at every keystroke.
pub fn terminal_claim_key(tty: &str) -> String {
    format!("terminal#{tty}")
}

/// What one holder announces.
///
/// **ONE COPY OF THE SHAPE.** The flow node and the terminal hook write the
/// same record; a second copy would drift the day one of them learns a field.
pub struct Claim {
    pub agent: String,
    /// Which announcement this is, from [`claim_key`] or [`terminal_claim_key`].
    pub key: String,
    pub repository: String,
    pub workdir: Option<String>,
    pub branch: Option<String>,
    pub paths: Vec<String>,
    pub doing: Option<String>,
    pub pid: u32,
    pub at: i64,
    pub lease_seconds: i64,
    /// The conversation this holder is in, where it has one.
    pub conversation: Option<String>,
    /// What it is doing, in the words A2A uses: `working`, `input_required`,
    /// `completed`, `failed`.
    pub state: String,
}

/// The announcement as a record. **The shared words travel in it**, under the
/// names OpenTelemetry and A2A already use, so an export costs nobody a
/// translation later.
pub fn claim_record(claim: &Claim) -> StoreRecord {
    StoreRecord {
        collection: CLAIMS_COLLECTION.to_owned(),
        key: claim.key.clone(),
        value: json!({
            "agent": claim.agent,
            "repository": claim.repository,
            "workdir": claim.workdir,
            "branch": claim.branch,
            "paths": claim.paths,
            "doing": claim.doing,
            "pid": claim.pid,
            "renewed_at": claim.at,
            "expires_at": claim.at + claim.lease_seconds,
            "released_at": Value::Null,
            "gen_ai.agent.name": claim.agent,
            "gen_ai.agent.id": claim.key,
            "gen_ai.conversation.id": claim.conversation,
            "state": claim.state,
        }),
        written_by: claim.agent.clone(),
        written_at: claim.at,
    }
}

/// Marks one announcement released, and says whether there was one to release.
pub fn release_claim(ledger: &Ledger, key: &str, at: i64) -> Result<bool, ledger::LedgerError> {
    let Some(mut record) = ledger.read_record(CLAIMS_COLLECTION, key)? else {
        return Ok(false);
    };
    record.value["released_at"] = json!(at);
    record.value["state"] = json!("completed");
    record.written_at = at;
    ledger.put_record(&record)?;
    Ok(true)
}

/// How far two claims overlap, from the narrowest to the widest.
///
/// **THREE AND NOT ONE, BECAUSE THE LIVED CASES ARE TWO OF DIFFERENT GRAVITY.**
/// Seven agents *always* share the repository: were `same_repository` worth as
/// much as the rest, every claim would be a collision, and an alarm that always
/// rings is one somebody switches off on day one. Whoever lost uncommitted work
/// lost it to somebody in their **own tree**, and that is the species that
/// stops the work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Overlap {
    /// Another worktree, same repository: worth knowing, not worth stopping for.
    Repository,
    /// Same worktree, declared and disjoint paths. It counts anyway: a
    /// `git commit` does not look at the paths somebody declared.
    Workdir,
    /// Same tree and paths that touch — or one of the two declared none, which
    /// means it took everything.
    Paths,
}

impl Overlap {
    fn named(self) -> &'static str {
        match self {
            Overlap::Repository => "same_repository",
            Overlap::Workdir => "same_workdir",
            Overlap::Paths => "same_paths",
        }
    }
}

/// One path contains the other, **whole segment by whole segment**.
///
/// Comparing as text would say `crates/act` contains `crates/actions`, and an
/// invented collision costs as much as a missed one: whoever gets it stops
/// believing the real ones.
fn one_path_contains_the_other(left: &str, right: &str) -> bool {
    let left: Vec<&str> = left
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let right: Vec<&str> = right
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let shared = left.len().min(right.len());
    shared > 0 && left[..shared] == right[..shared]
}

fn paths_meet(mine: &[String], theirs: &[String]) -> bool {
    // Declaring no paths means taking the whole tree: that is the default, and
    // the default has to be the cautious one.
    if mine.is_empty() || theirs.is_empty() {
        return true;
    }
    mine.iter()
        .any(|a| theirs.iter().any(|b| one_path_contains_the_other(a, b)))
}

fn text_at(value: &Value, field: &str) -> String {
    value[field].as_str().unwrap_or_default().to_owned()
}

fn paths_at(value: &Value) -> Vec<String> {
    value["paths"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|p| p.as_str().map(|s| s.to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

/// Why a claim holds nobody any more — or `None` while it still holds.
///
/// **The two reasons stay apart on purpose.** `released` is «somebody looked and
/// finished»; `expired` is «nobody showed up again», which on this machine
/// nearly always means a process killed by the system going to sleep. Merging
/// them into one «gone» would take from the reader the one fact that changes
/// what to do: in the first case the work is finished, in the second it is
/// half-done and nobody knows.
fn why_gone(claim: &Value, at: i64) -> Option<&'static str> {
    if claim["released_at"].is_i64() {
        return Some("released");
    }
    let expires_at = claim["expires_at"].as_i64().unwrap_or(i64::MIN);
    if at >= expires_at {
        return Some("expired");
    }
    None
}

/// How far somebody else's claim touches mine — `None` when it does not.
fn overlap_between(mine: &ClaimSpec, theirs: &Value) -> Option<Overlap> {
    if text_at(theirs, "repository") != mine.repository {
        return None;
    }
    let my_workdir = mine.workdir.clone().unwrap_or_default();
    if text_at(theirs, "workdir") != my_workdir {
        return Some(Overlap::Repository);
    }
    if paths_meet(&mine.paths, &paths_at(theirs)) {
        Some(Overlap::Paths)
    } else {
        Some(Overlap::Workdir)
    }
}

pub struct WorkClaimAction {
    ledger: Ledger,
}

impl WorkClaimAction {
    pub fn new(ledger: Ledger) -> Self {
        Self { ledger }
    }
}

impl Action for WorkClaimAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: ClaimSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let pid = spec.pid.unwrap_or_else(std::process::id);
        let at = spec.at.unwrap_or_else(now);
        let lease = spec.lease_seconds.unwrap_or(DEFAULT_LEASE_SECONDS);
        let expires_at = at + lease;
        let record = claim_record(&Claim {
            agent: spec.agent.clone(),
            key: claim_key(&spec.agent, &pid.to_string()),
            repository: spec.repository.clone(),
            workdir: spec.workdir.clone(),
            branch: spec.branch.clone(),
            paths: spec.paths.clone(),
            doing: spec.doing.clone(),
            pid,
            at,
            lease_seconds: lease,
            conversation: None,
            state: "working".to_owned(),
        });
        // **WRITE FIRST, LOOK AFTER.** The order matters: two agents starting in
        // the same instant must see each other *from at least one side*. Looking
        // before writing, both would read a ledger not yet holding the other and
        // conclude «I am alone» — exactly the race this node exists to make
        // visible. Writing first, whoever arrives second always sees the first,
        // and at worst — the same fraction of a second — they see each other,
        // which is the error on the right side.
        self.ledger
            .put_record(&record)
            .map_err(|error| ActionError::new("store_refused", error.to_string()))?;

        let others = self
            .ledger
            .records_in(CLAIMS_COLLECTION)
            .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
        let mut collisions: Vec<Value> = Vec::new();
        for other in others {
            if other.key == record.key {
                continue;
            }
            if why_gone(&other.value, at).is_some() {
                continue;
            }
            let Some(kind) = overlap_between(&spec, &other.value) else {
                continue;
            };
            collisions.push(json!({
                "kind": kind.named(),
                "agent": text_at(&other.value, "agent"),
                "key": other.key,
                "workdir": other.value["workdir"].clone(),
                "branch": other.value["branch"].clone(),
                "paths": other.value["paths"].clone(),
                "doing": other.value["doing"].clone(),
                "pid": other.value["pid"].clone(),
                "expires_at": other.value["expires_at"].clone(),
            }));
        }
        // Narrowest first: reading only the first line reads the worst one.
        collisions.sort_by(|a, b| b["kind"].as_str().cmp(&a["kind"].as_str()));

        // **THE BRAKE IS DECLARED, AND NEVER TRIPS ON `same_repository`.** Seven
        // agents always share the repository: a brake tripping there would stop
        // every claim of every day, and whoever suffers it switches it off —
        // after which it brakes nothing. Same shape as the Bazel model in
        // `docs/decisions.md`: you arrive as a warning and become a barrier
        // where somebody asked for one.
        if spec.refuse_when_shared {
            let holding: Vec<String> = collisions
                .iter()
                .filter(|c| c["kind"] != json!(Overlap::Repository.named()))
                .map(|c| {
                    format!(
                        "{} ({})",
                        c["agent"].as_str().unwrap_or("with no name"),
                        c["kind"].as_str().unwrap_or("?")
                    )
                })
                .collect();
            if !holding.is_empty() {
                return Err(ActionError::new(
                    "work_is_shared",
                    format!(
                        "this work tree is already claimed by: {}. \
                         The claim stays written: whoever takes the work up renews it.",
                        holding.join(", ")
                    ),
                ));
            }
        }

        Ok(ActionOutcome::Went(json!({
            "key": record.key,
            "expires_at": expires_at,
            "collisions": collisions,
        })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

pub struct WorkReleaseAction {
    ledger: Ledger,
}

impl WorkReleaseAction {
    pub fn new(ledger: Ledger) -> Self {
        Self { ledger }
    }
}

impl Action for WorkReleaseAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: ReleaseSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let pid = spec.pid.unwrap_or_else(std::process::id);
        let at = spec.at.unwrap_or_else(now);
        let key = claim_key(&spec.agent, &pid.to_string());
        let released = release_claim(&self.ledger, &key, at)
            .map_err(|error| ActionError::new("store_refused", error.to_string()))?;
        if !released {
            return Ok(ActionOutcome::Went(json!({ "released": false })));
        }
        Ok(ActionOutcome::Went(json!({ "released": true, "key": key })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

pub struct WorkSurveyAction {
    ledger: Option<Ledger>,
}

impl WorkSurveyAction {
    pub fn new(ledger: Option<Ledger>) -> Self {
        Self { ledger }
    }
}

impl Action for WorkSurveyAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: SurveySpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let at = spec.at.unwrap_or_else(now);
        let ledger = self.ledger.as_ref().ok_or_else(|| {
            ActionError::new(
                "no_store",
                "I cannot tell where the claims are, and an empty list here would say «nobody»"
                    .to_owned(),
            )
        })?;
        let records = ledger
            .records_in(CLAIMS_COLLECTION)
            .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
        let mut working: Vec<Value> = Vec::new();
        let mut gone: Vec<Value> = Vec::new();
        for record in records {
            if let Some(wanted) = &spec.repository {
                if record.value["repository"] != json!(wanted) {
                    continue;
                }
            }
            match why_gone(&record.value, at) {
                None => working.push(record.value),
                Some(why) => {
                    let mut entry = record.value;
                    entry["why"] = json!(why);
                    gone.push(entry);
                }
            }
        }
        Ok(ActionOutcome::Went(json!({
            "at": at,
            "working": working,
            "gone": gone,
        })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct TestStore(std::path::PathBuf);

    impl Drop for TestStore {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A counter in the name, not the clock alone: fault 21.
    fn store() -> (Ledger, TestStore) {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sailor-actions-presence-{}-{sequence}",
            std::process::id()
        ));
        let ledger = Ledger::open(&path).expect("open the ledger");
        (ledger, TestStore(path))
    }

    const NOON: i64 = 1_756_400_000;

    fn claim(agent: &str, pid: u32, workdir: &str, at: i64, paths: &[&str]) -> Value {
        json!({
            "agent": agent,
            "pid": pid,
            "repository": "/home/project/.git",
            "workdir": workdir,
            "branch": "main",
            "paths": paths,
            "doing": "something",
            "at": at,
            "lease_seconds": 900,
        })
    }

    fn went(outcome: ActionOutcome) -> Value {
        let ActionOutcome::Went(value) = outcome else {
            panic!("a node touching a local ledger waits for nobody");
        };
        value
    }

    /// **THE LIVED CASE.** Two agents in one worktree: the second has to learn
    /// about the first, by name.
    #[test]
    fn a_second_agent_in_the_same_workdir_learns_about_the_first() {
        let (ledger, _guard) = store();
        let shared = SharedState::new();
        let action = WorkClaimAction::new(ledger);

        action
            .execute(
                &claim("first", 101, "/home/project", NOON, &[]),
                &shared,
            )
            .expect("the first claim");
        let second = went(
            action
                .execute(
                    &claim("second", 102, "/home/project", NOON + 10, &[]),
                    &shared,
                )
                .expect("the second claim"),
        );

        let collisions = second["collisions"].as_array().expect("the collisions");
        assert_eq!(
            collisions.len(),
            1,
            "the second agent has to see the first"
        );
        assert_eq!(collisions[0]["agent"], json!("first"));
        assert_eq!(collisions[0]["kind"], json!("same_paths"));
    }

    /// **THE AGENT THAT DIED BADLY.** An expired claim is not a collision.
    #[test]
    fn an_expired_claim_is_not_a_collision() {
        let (ledger, _guard) = store();
        let shared = SharedState::new();
        let action = WorkClaimAction::new(ledger);

        action
            .execute(
                &claim("the-one-that-dies", 101, "/home/project", NOON, &[]),
                &shared,
            )
            .expect("the claim of the one that dies");
        let later = went(
            action
                .execute(
                    &claim("the-one-that-stays", 102, "/home/project", NOON + 901, &[]),
                    &shared,
                )
                .expect("the later claim"),
        );

        assert_eq!(
            later["collisions"].as_array().expect("the collisions").len(),
            0,
            "an expired claim holds nobody back"
        );
    }

    /// **TWO COMMAND LINES IN ONE TREE SEE EACH OTHER**, which is the half of
    /// the promise the flow node does not cover: a terminal's announcement is
    /// written by the hook and held by the terminal, not by the process, and
    /// the survey has to read it just the same.
    #[test]
    fn two_command_lines_in_one_tree_appear_in_the_same_survey() {
        let (ledger, _guard) = store();
        let terminal = |agent: &str, tty: &str| Claim {
            agent: agent.to_owned(),
            key: terminal_claim_key(tty),
            repository: "/home/project/.git".to_owned(),
            workdir: Some("/home/project".to_owned()),
            branch: Some("sorgenti".to_owned()),
            paths: Vec::new(),
            doing: None,
            pid: 101,
            at: NOON,
            lease_seconds: 900,
            conversation: Some(format!("conversation-of-{agent}")),
            state: "working".to_owned(),
        };
        // THE THIRD IS THE SAME COMMAND LINE IN ANOTHER TERMINAL, which is the
        // ordinary case on this machine and the one a key that forgets the
        // holder collapses: two sessions would become one row, and the survey
        // would say one agent where two are typing.
        for (agent, tty) in [
            ("anengine (this-machine)", "ttys004"),
            ("another-one (tests)", "ttys009"),
            ("anengine (this-machine)", "ttys010"),
        ] {
            ledger
                .put_record(&claim_record(&terminal(agent, tty)))
                .expect("the claim is written");
        }

        let survey = WorkSurveyAction::new(Some(ledger));
        let answer = went(
            survey
                .execute(&json!({"at": NOON + 60}), &SharedState::new())
                .expect("the survey"),
        );

        let working = answer["working"].as_array().expect("who is working");
        let names: Vec<&str> = working
            .iter()
            .filter_map(|entry| entry["agent"].as_str())
            .collect();
        assert_eq!(names.len(), 3, "{answer}");
        assert!(names.contains(&"anengine (this-machine)"), "{answer}");
        assert!(names.contains(&"another-one (tests)"), "{answer}");
        // AND EACH CARRIES ITS OWN CONVERSATION: without it the two rows say
        // that two agents are here and give no way to reach either.
        assert_ne!(
            working[0]["gen_ai.conversation.id"],
            working[1]["gen_ai.conversation.id"],
            "{answer}"
        );
    }

    /// **THE LESSON OF THE DOUBLE 27.** A renewal touches nobody else's row.
    #[test]
    fn a_renewal_never_erases_another_agents_claim() {
        let (ledger, _guard) = store();
        let shared = SharedState::new();
        let action = WorkClaimAction::new(ledger.clone());
        let survey = WorkSurveyAction::new(Some(ledger));

        action
            .execute(
                &claim("first", 101, "/home/project", NOON, &[]),
                &shared,
            )
            .expect("first");
        action
            .execute(
                &claim("second", 102, "/home/project", NOON, &[]),
                &shared,
            )
            .expect("second");
        action
            .execute(
                &claim("first", 101, "/home/project", NOON + 60, &[]),
                &shared,
            )
            .expect("the first one renewing");

        let seen = went(
            survey
                .execute(&json!({"at": NOON + 61}), &shared)
                .expect("the survey"),
        );
        let working = seen["working"].as_array().expect("who is working");
        assert_eq!(working.len(), 2, "one renewing does not erase the other");
    }

    /// **THE CASE OF THE SEVEN.** Different trees of one repository see each
    /// other, but the collision is of another species.
    #[test]
    fn different_worktrees_of_one_repository_see_each_other_as_a_lesser_kind() {
        let (ledger, _guard) = store();
        let shared = SharedState::new();
        let action = WorkClaimAction::new(ledger);

        action
            .execute(
                &claim("first", 101, "/home/project", NOON, &[]),
                &shared,
            )
            .expect("first");
        let second = went(
            action
                .execute(
                    &claim("second", 102, "/home/another-tree", NOON, &[]),
                    &shared,
                )
                .expect("second"),
        );

        let collisions = second["collisions"].as_array().expect("the collisions");
        assert_eq!(collisions.len(), 1);
        assert_eq!(collisions[0]["kind"], json!("same_repository"));
    }

    /// Declared and disjoint paths in one tree: they see each other, but not
    /// over the same files.
    #[test]
    fn disjoint_declared_paths_in_one_workdir_are_a_lesser_kind() {
        let (ledger, _guard) = store();
        let shared = SharedState::new();
        let action = WorkClaimAction::new(ledger);

        action
            .execute(
                &claim("first", 101, "/home/project", NOON, &["crates/ledger"]),
                &shared,
            )
            .expect("first");
        let second = went(
            action
                .execute(
                    &claim("second", 102, "/home/project", NOON, &["crates/actions"]),
                    &shared,
                )
                .expect("second"),
        );
        assert_eq!(second["collisions"][0]["kind"], json!("same_workdir"));

        let third = went(
            action
                .execute(
                    &claim(
                        "third",
                        103,
                        "/home/project",
                        NOON,
                        &["crates/actions/src/lib.rs"],
                    ),
                    &shared,
                )
                .expect("third"),
        );
        let kinds: Vec<&str> = third["collisions"]
            .as_array()
            .expect("the collisions")
            .iter()
            .map(|c| c["kind"].as_str().expect("the kind"))
            .collect();
        assert!(
            kinds.contains(&"same_paths"),
            "a file inside `crates/actions` touches whoever took `crates/actions`: {kinds:?}"
        );
    }

    /// **AN INVENTED COLLISION COSTS AS MUCH AS A MISSED ONE.** `crates/act` is
    /// not inside `crates/actions`: comparing as text would say it is, and
    /// whoever gets a false alarm stops believing the true ones too.
    #[test]
    fn a_text_prefix_that_is_not_a_path_prefix_is_not_a_collision() {
        let (ledger, _guard) = store();
        let shared = SharedState::new();
        let action = WorkClaimAction::new(ledger);

        action
            .execute(
                &claim("first", 101, "/home/project", NOON, &["crates/actions"]),
                &shared,
            )
            .expect("first");
        let second = went(
            action
                .execute(
                    &claim("second", 102, "/home/project", NOON, &["crates/act"]),
                    &shared,
                )
                .expect("second"),
        );
        assert_eq!(
            second["collisions"][0]["kind"],
            json!("same_workdir"),
            "`crates/act` and `crates/actions` are two different directories"
        );
    }

    /// A release stops holding **at once**, and stays distinguishable from an
    /// expiry: `released_at` written, not a claim gone missing.
    #[test]
    fn a_release_stops_holding_at_once_and_stays_distinguishable_from_an_expiry() {
        let (ledger, _guard) = store();
        let shared = SharedState::new();
        let action = WorkClaimAction::new(ledger.clone());
        let release = WorkReleaseAction::new(ledger.clone());
        let survey = WorkSurveyAction::new(Some(ledger));

        action
            .execute(
                &claim("first", 101, "/home/project", NOON, &[]),
                &shared,
            )
            .expect("first");
        release
            .execute(
                &json!({"agent": "first", "pid": 101, "at": NOON + 30}),
                &shared,
            )
            .expect("the release");

        let second = went(
            action
                .execute(
                    &claim("second", 102, "/home/project", NOON + 31, &[]),
                    &shared,
                )
                .expect("second"),
        );
        assert_eq!(
            second["collisions"]
                .as_array()
                .expect("the collisions")
                .len(),
            0,
            "whoever released holds nothing any more"
        );

        let seen = went(
            survey
                .execute(&json!({"at": NOON + 32}), &shared)
                .expect("the survey"),
        );
        let gone = seen["gone"].as_array().expect("who is gone");
        let released = gone
            .iter()
            .find(|entry| entry["agent"] == json!("first"))
            .expect("the first one is among those gone");
        assert_eq!(
            released["why"],
            json!("released"),
            "a declared release is never mistaken for an expiry"
        );
    }

    /// The brake **is declared**: whoever does not ask for it carries on informed.
    #[test]
    fn refuse_when_shared_stops_the_second_and_names_the_first() {
        let (ledger, _guard) = store();
        let shared = SharedState::new();
        let action = WorkClaimAction::new(ledger);

        action
            .execute(
                &claim("first", 101, "/home/project", NOON, &[]),
                &shared,
            )
            .expect("first");

        let mut wanted = claim("second", 102, "/home/project", NOON + 1, &[]);
        wanted["refuse_when_shared"] = json!(true);
        let error = action
            .execute(&wanted, &shared)
            .expect_err("the second stops because it asked to");
        assert_eq!(error.class, "work_is_shared");
        assert!(
            error.said.contains("first"),
            "the refusal names whoever was already there: {}",
            error.said
        );
    }

    /// **`same_repository` never stops anyone**, not even with the brake
    /// declared: seven agents always share the repository, and a brake that
    /// always trips is one somebody switches off on day one.
    #[test]
    fn refuse_when_shared_ignores_a_mere_shared_repository() {
        let (ledger, _guard) = store();
        let shared = SharedState::new();
        let action = WorkClaimAction::new(ledger);

        action
            .execute(
                &claim("first", 101, "/home/project", NOON, &[]),
                &shared,
            )
            .expect("first");

        let mut wanted = claim("second", 102, "/home/another-tree", NOON + 1, &[]);
        wanted["refuse_when_shared"] = json!(true);
        let outcome = went(
            action
                .execute(&wanted, &shared)
                .expect("another tree stops nobody"),
        );
        assert_eq!(outcome["collisions"][0]["kind"], json!("same_repository"));
    }

    /// The survey separates the working from the gone, and says **why**.
    #[test]
    fn a_survey_separates_the_living_from_the_gone_and_says_why() {
        let (ledger, _guard) = store();
        let shared = SharedState::new();
        let action = WorkClaimAction::new(ledger.clone());
        let survey = WorkSurveyAction::new(Some(ledger));

        action
            .execute(
                &claim("the-one-that-dies", 101, "/home/project", NOON, &[]),
                &shared,
            )
            .expect("the one that then dies");
        action
            .execute(
                &claim("the-one-that-stays", 102, "/home/project", NOON + 900, &[]),
                &shared,
            )
            .expect("the one that stays");

        let seen = went(
            survey
                .execute(&json!({"at": NOON + 901}), &shared)
                .expect("the survey"),
        );
        let working = seen["working"].as_array().expect("who is working");
        assert_eq!(working.len(), 1);
        assert_eq!(working[0]["agent"], json!("the-one-that-stays"));
        let gone = seen["gone"].as_array().expect("who is gone");
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0]["agent"], json!("the-one-that-dies"));
        assert_eq!(gone[0]["why"], json!("expired"));
    }

    // **FAULT 28: THE PROOF SITS WHERE THE RULE SITS.** References are resolved
    // in one place, `flow::step_input`, and that every action receives its input
    // resolved is proved by
    // `crates/flow/tests/a_reference_reaches_every_action.rs`.
}
