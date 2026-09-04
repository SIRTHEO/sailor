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

/// Il nome sotto cui `WorkClaimAction` si registra.
pub const WORK_CLAIM_ACTION: &str = "work_claim";
/// Il nome sotto cui `WorkReleaseAction` si registra.
pub const WORK_RELEASE_ACTION: &str = "work_release";
/// Il nome sotto cui `WorkSurveyAction` si registra.
pub const WORK_SURVEY_ACTION: &str = "work_survey";

/// La collezione del deposito dove vivono gli annunci.
pub const CLAIMS_COLLECTION: &str = "work-claims";

/// Quanto dura un annuncio che non dichiara una durata sua.
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

/// Quanto due annunci si sovrappongono, dal più stretto al più largo.
///
/// **SONO TRE E NON UNO PERCHÉ I CASI VISSUTI SONO DUE, DI GRAVITÀ DIVERSA.**
/// Sette agenti condividono *sempre* la repo: se `same_repository` valesse
/// quanto il resto, ogni annuncio sarebbe una collisione, e un allarme che
/// suona sempre è un allarme che qualcuno spegne il primo giorno. Chi ha perso
/// il lavoro non committato lo ha perso da uno che stava nel **suo stesso
/// albero**, ed è quella la specie che ferma.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Overlap {
    /// Altro albero di lavoro, stessa repo: da sapere, non da fermarsi.
    SameRepository,
    /// Stesso albero di lavoro, percorsi dichiarati e disgiunti. Conta lo
    /// stesso: un `git commit` non guarda i percorsi che qualcuno ha dichiarato.
    SameWorkdir,
    /// Stesso albero e percorsi che si toccano — o uno dei due non ne ha
    /// dichiarati, cioè ha preso tutto.
    SamePaths,
}

impl Overlap {
    fn named(self) -> &'static str {
        match self {
            Overlap::SameRepository => "same_repository",
            Overlap::SameWorkdir => "same_workdir",
            Overlap::SamePaths => "same_paths",
        }
    }
}

/// Un percorso contiene l'altro, **a segmenti interi**.
///
/// Il confronto per testo direbbe che `crates/act` contiene `crates/actions`, e
/// una collisione inventata costa quanto una mancata: chi la riceve smette di
/// credere alle vere.
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
    // Chi non dichiara percorsi ha preso l'albero intero: è il valore
    // predefinito, e deve essere quello prudente.
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

/// Perché un annuncio non trattiene più nessuno — o `None` se trattiene ancora.
///
/// **Le due ragioni restano separate apposta.** `released` è «qualcuno ha
/// guardato e ha finito»; `expired` è «nessuno si è più fatto vivo», che sulla
/// macchina di Theo vuol dire quasi sempre un processo ucciso dal sonno del
/// sistema. Fonderle in un unico «non c'è più» toglierebbe a chi legge l'unica
/// informazione che cambia cosa fare: nel primo caso il lavoro è finito, nel
/// secondo è a metà e nessuno lo sa.
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

/// Quanto l'annuncio di un altro tocca il mio — `None` se non lo tocca.
fn overlap_between(mine: &ClaimSpec, theirs: &Value) -> Option<Overlap> {
    if text_at(theirs, "repository") != mine.repository {
        return None;
    }
    let my_workdir = mine.workdir.clone().unwrap_or_default();
    if text_at(theirs, "workdir") != my_workdir {
        return Some(Overlap::SameRepository);
    }
    if paths_meet(&mine.paths, &paths_at(theirs)) {
        Some(Overlap::SamePaths)
    } else {
        Some(Overlap::SameWorkdir)
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
        // **PRIMA SI SCRIVE, POI SI GUARDA.** L'ordine non è indifferente: due
        // agenti che partono nello stesso istante devono vedersi *almeno da un
        // lato*. Guardando prima di scrivere, entrambi leggerebbero un deposito
        // che non contiene ancora l'altro e concluderebbero «sono solo» —
        // esattamente la corsa critica che questo nodo esiste per rendere
        // visibile. Scrivendo prima, chi arriva secondo vede sempre il primo, e
        // nel caso peggiore — la stessa frazione di secondo — si vedono tutti e
        // due, che è l'errore dalla parte giusta.
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
        // Il più stretto per primo: chi legge solo la prima riga legge la peggiore.
        collisions.sort_by(|a, b| b["kind"].as_str().cmp(&a["kind"].as_str()));

        // **IL FRENO SI DICHIARA, E NON SCATTA SU `same_repository`.** Sette
        // agenti condividono sempre la repo: un freno che scattasse lì
        // fermerebbe ogni annuncio di ogni giorno, e chi lo subisce lo spegne —
        // dopodiché non frena più niente. È la stessa forma del modello Bazel
        // in `docs/decisioni.md`: si entra come avviso, si diventa barriera solo
        // dove qualcuno l'ha chiesto.
        if spec.refuse_when_shared {
            let holding: Vec<String> = collisions
                .iter()
                .filter(|c| c["kind"] != json!(Overlap::SameRepository.named()))
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
                        "l'albero di lavoro è già annunciato da: {}. \
                         L'annuncio resta scritto: chi riprende lo rinnova.",
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

    /// Un contatore nel nome, non solo l'orologio: guasto 21.
    fn store() -> (Ledger, TestStore) {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sailor-actions-presence-{}-{sequence}",
            std::process::id()
        ));
        let ledger = Ledger::open(&path).expect("aprire il deposito");
        (ledger, TestStore(path))
    }

    const NOON: i64 = 1_756_400_000;

    fn claim(agent: &str, pid: u32, workdir: &str, at: i64, paths: &[&str]) -> Value {
        json!({
            "agent": agent,
            "pid": pid,
            "repository": "/casa/progetto/.git",
            "workdir": workdir,
            "branch": "sorgenti",
            "paths": paths,
            "doing": "qualcosa",
            "at": at,
            "lease_seconds": 900,
        })
    }

    fn went(outcome: ActionOutcome) -> Value {
        let ActionOutcome::Went(value) = outcome else {
            panic!("un nodo che tocca un deposito locale non aspetta nessuno");
        };
        value
    }

    /// **IL CASO VISSUTO.** Due agenti nello stesso albero di lavoro: il secondo
    /// deve sapere del primo, per nome.
    #[test]
    fn a_second_agent_in_the_same_workdir_learns_about_the_first() {
        let (ledger, _guard) = store();
        let mut shared = SharedState::new();
        let action = WorkClaimAction::new(ledger);

        action
            .execute(
                &claim("prima", 101, "/casa/progetto", NOON, &[]),
                &mut shared,
            )
            .expect("il primo annuncio");
        let second = went(
            action
                .execute(
                    &claim("seconda", 102, "/casa/progetto", NOON + 10, &[]),
                    &mut shared,
                )
                .expect("il secondo annuncio"),
        );

        let collisions = second["collisions"].as_array().expect("le collisioni");
        assert_eq!(
            collisions.len(),
            1,
            "il secondo agente deve vedere il primo"
        );
        assert_eq!(collisions[0]["agent"], json!("prima"));
        assert_eq!(collisions[0]["kind"], json!("same_paths"));
    }

    /// **L'AGENTE MORTO MALE.** Un annuncio scaduto non è una collisione.
    #[test]
    fn an_expired_claim_is_not_a_collision() {
        let (ledger, _guard) = store();
        let mut shared = SharedState::new();
        let action = WorkClaimAction::new(ledger);

        action
            .execute(
                &claim("morta", 101, "/casa/progetto", NOON, &[]),
                &mut shared,
            )
            .expect("l'annuncio di chi poi muore");
        let later = went(
            action
                .execute(
                    &claim("viva", 102, "/casa/progetto", NOON + 901, &[]),
                    &mut shared,
                )
                .expect("l'annuncio di dopo"),
        );

        assert_eq!(
            later["collisions"].as_array().expect("le collisioni").len(),
            0,
            "un annuncio scaduto non trattiene nessuno"
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
            repository: "/casa/progetto/.git".to_owned(),
            workdir: Some("/casa/progetto".to_owned()),
            branch: Some("sorgenti".to_owned()),
            paths: Vec::new(),
            doing: None,
            pid: 101,
            at: NOON,
            lease_seconds: 900,
            conversation: Some(format!("conversazione-di-{agent}")),
            state: "working".to_owned(),
        };
        // THE THIRD IS THE SAME COMMAND LINE IN ANOTHER TERMINAL, which is the
        // ordinary case on this machine and the one a key that forgets the
        // holder collapses: two sessions would become one row, and the survey
        // would say one agent where two are typing.
        for (agent, tty) in [
            ("unmotore (questa-macchina)", "ttys004"),
            ("un-altro (prove)", "ttys009"),
            ("unmotore (questa-macchina)", "ttys010"),
        ] {
            ledger
                .put_record(&claim_record(&terminal(agent, tty)))
                .expect("l'annuncio si scrive");
        }

        let survey = WorkSurveyAction::new(Some(ledger));
        let answer = went(
            survey
                .execute(&json!({"at": NOON + 60}), &mut SharedState::new())
                .expect("il censimento"),
        );

        let working = answer["working"].as_array().expect("chi lavora");
        let names: Vec<&str> = working
            .iter()
            .filter_map(|entry| entry["agent"].as_str())
            .collect();
        assert_eq!(names.len(), 3, "{answer}");
        assert!(names.contains(&"unmotore (questa-macchina)"), "{answer}");
        assert!(names.contains(&"un-altro (prove)"), "{answer}");
        // AND EACH CARRIES ITS OWN CONVERSATION: without it the two rows say
        // that two agents are here and give no way to reach either.
        assert_ne!(
            working[0]["gen_ai.conversation.id"],
            working[1]["gen_ai.conversation.id"],
            "{answer}"
        );
    }

    /// **LA LEZIONE DEL DOPPIO 27.** Chi rinnova non tocca la riga di nessun altro.
    #[test]
    fn a_renewal_never_erases_another_agents_claim() {
        let (ledger, _guard) = store();
        let mut shared = SharedState::new();
        let action = WorkClaimAction::new(ledger.clone());
        let survey = WorkSurveyAction::new(Some(ledger));

        action
            .execute(
                &claim("prima", 101, "/casa/progetto", NOON, &[]),
                &mut shared,
            )
            .expect("prima");
        action
            .execute(
                &claim("seconda", 102, "/casa/progetto", NOON, &[]),
                &mut shared,
            )
            .expect("seconda");
        action
            .execute(
                &claim("prima", 101, "/casa/progetto", NOON + 60, &[]),
                &mut shared,
            )
            .expect("il rinnovo della prima");

        let seen = went(
            survey
                .execute(&json!({"at": NOON + 61}), &mut shared)
                .expect("il censimento"),
        );
        let working = seen["working"].as_array().expect("chi lavora");
        assert_eq!(working.len(), 2, "il rinnovo di una non cancella l'altra");
    }

    /// **IL CASO DEI SETTE.** Alberi diversi della stessa repo si vedono, ma la
    /// collisione è di un'altra specie.
    #[test]
    fn different_worktrees_of_one_repository_see_each_other_as_a_lesser_kind() {
        let (ledger, _guard) = store();
        let mut shared = SharedState::new();
        let action = WorkClaimAction::new(ledger);

        action
            .execute(
                &claim("prima", 101, "/casa/progetto", NOON, &[]),
                &mut shared,
            )
            .expect("prima");
        let second = went(
            action
                .execute(
                    &claim("seconda", 102, "/casa/altro-albero", NOON, &[]),
                    &mut shared,
                )
                .expect("seconda"),
        );

        let collisions = second["collisions"].as_array().expect("le collisioni");
        assert_eq!(collisions.len(), 1);
        assert_eq!(collisions[0]["kind"], json!("same_repository"));
    }

    /// Percorsi dichiarati e disgiunti nello stesso albero: si vedono, ma non
    /// sugli stessi file.
    #[test]
    fn disjoint_declared_paths_in_one_workdir_are_a_lesser_kind() {
        let (ledger, _guard) = store();
        let mut shared = SharedState::new();
        let action = WorkClaimAction::new(ledger);

        action
            .execute(
                &claim("prima", 101, "/casa/progetto", NOON, &["crates/ledger"]),
                &mut shared,
            )
            .expect("prima");
        let second = went(
            action
                .execute(
                    &claim("seconda", 102, "/casa/progetto", NOON, &["crates/actions"]),
                    &mut shared,
                )
                .expect("seconda"),
        );
        assert_eq!(second["collisions"][0]["kind"], json!("same_workdir"));

        let third = went(
            action
                .execute(
                    &claim(
                        "terza",
                        103,
                        "/casa/progetto",
                        NOON,
                        &["crates/actions/src/lib.rs"],
                    ),
                    &mut shared,
                )
                .expect("terza"),
        );
        let kinds: Vec<&str> = third["collisions"]
            .as_array()
            .expect("le collisioni")
            .iter()
            .map(|c| c["kind"].as_str().expect("la specie"))
            .collect();
        assert!(
            kinds.contains(&"same_paths"),
            "un file dentro `crates/actions` tocca chi ha preso `crates/actions`: {kinds:?}"
        );
    }

    /// **UNA COLLISIONE INVENTATA COSTA QUANTO UNA MANCATA.** `crates/act` non è
    /// dentro `crates/actions`: chi confronta per testo lo direbbe, e chi riceve
    /// un allarme falso smette di credere anche ai veri.
    #[test]
    fn a_text_prefix_that_is_not_a_path_prefix_is_not_a_collision() {
        let (ledger, _guard) = store();
        let mut shared = SharedState::new();
        let action = WorkClaimAction::new(ledger);

        action
            .execute(
                &claim("prima", 101, "/casa/progetto", NOON, &["crates/actions"]),
                &mut shared,
            )
            .expect("prima");
        let second = went(
            action
                .execute(
                    &claim("seconda", 102, "/casa/progetto", NOON, &["crates/act"]),
                    &mut shared,
                )
                .expect("seconda"),
        );
        assert_eq!(
            second["collisions"][0]["kind"],
            json!("same_workdir"),
            "`crates/act` e `crates/actions` sono due cartelle diverse"
        );
    }

    /// Un rilascio smette di trattenere **subito**, e resta distinguibile da una
    /// scadenza: `released_at` scritto, non un annuncio sparito.
    #[test]
    fn a_release_stops_holding_at_once_and_stays_distinguishable_from_an_expiry() {
        let (ledger, _guard) = store();
        let mut shared = SharedState::new();
        let action = WorkClaimAction::new(ledger.clone());
        let release = WorkReleaseAction::new(ledger.clone());
        let survey = WorkSurveyAction::new(Some(ledger));

        action
            .execute(
                &claim("prima", 101, "/casa/progetto", NOON, &[]),
                &mut shared,
            )
            .expect("prima");
        release
            .execute(
                &json!({"agent": "prima", "pid": 101, "at": NOON + 30}),
                &mut shared,
            )
            .expect("il rilascio");

        let second = went(
            action
                .execute(
                    &claim("seconda", 102, "/casa/progetto", NOON + 31, &[]),
                    &mut shared,
                )
                .expect("seconda"),
        );
        assert_eq!(
            second["collisions"]
                .as_array()
                .expect("le collisioni")
                .len(),
            0,
            "chi ha rilasciato non trattiene più"
        );

        let seen = went(
            survey
                .execute(&json!({"at": NOON + 32}), &mut shared)
                .expect("il censimento"),
        );
        let gone = seen["gone"].as_array().expect("chi non c'è più");
        let released = gone
            .iter()
            .find(|entry| entry["agent"] == json!("prima"))
            .expect("la prima sta fra chi non c'è più");
        assert_eq!(
            released["why"],
            json!("released"),
            "un rilascio dichiarato non si confonde con una scadenza"
        );
    }

    /// Il freno **si dichiara**: chi non lo chiede prosegue informato.
    #[test]
    fn refuse_when_shared_stops_the_second_and_names_the_first() {
        let (ledger, _guard) = store();
        let mut shared = SharedState::new();
        let action = WorkClaimAction::new(ledger);

        action
            .execute(
                &claim("prima", 101, "/casa/progetto", NOON, &[]),
                &mut shared,
            )
            .expect("prima");

        let mut wanted = claim("seconda", 102, "/casa/progetto", NOON + 1, &[]);
        wanted["refuse_when_shared"] = json!(true);
        let error = action
            .execute(&wanted, &mut shared)
            .expect_err("il secondo si ferma perché l'ha chiesto");
        assert_eq!(error.class, "work_is_shared");
        assert!(
            error.said.contains("prima"),
            "il rifiuto nomina chi c'era: {}",
            error.said
        );
    }

    /// **`same_repository` non ferma mai**, nemmeno a freno dichiarato: sette
    /// agenti condividono sempre la repo, e un freno che scatta sempre è un
    /// freno che qualcuno spegne il primo giorno.
    #[test]
    fn refuse_when_shared_ignores_a_mere_shared_repository() {
        let (ledger, _guard) = store();
        let mut shared = SharedState::new();
        let action = WorkClaimAction::new(ledger);

        action
            .execute(
                &claim("prima", 101, "/casa/progetto", NOON, &[]),
                &mut shared,
            )
            .expect("prima");

        let mut wanted = claim("seconda", 102, "/casa/altro-albero", NOON + 1, &[]);
        wanted["refuse_when_shared"] = json!(true);
        let outcome = went(
            action
                .execute(&wanted, &mut shared)
                .expect("un altro albero non ferma nessuno"),
        );
        assert_eq!(outcome["collisions"][0]["kind"], json!("same_repository"));
    }

    /// Il censimento separa chi lavora da chi non c'è più, e dice **perché**.
    #[test]
    fn a_survey_separates_the_living_from_the_gone_and_says_why() {
        let (ledger, _guard) = store();
        let mut shared = SharedState::new();
        let action = WorkClaimAction::new(ledger.clone());
        let survey = WorkSurveyAction::new(Some(ledger));

        action
            .execute(
                &claim("morta", 101, "/casa/progetto", NOON, &[]),
                &mut shared,
            )
            .expect("chi poi muore");
        action
            .execute(
                &claim("viva", 102, "/casa/progetto", NOON + 900, &[]),
                &mut shared,
            )
            .expect("chi resta");

        let seen = went(
            survey
                .execute(&json!({"at": NOON + 901}), &mut shared)
                .expect("il censimento"),
        );
        let working = seen["working"].as_array().expect("chi lavora");
        assert_eq!(working.len(), 1);
        assert_eq!(working[0]["agent"], json!("viva"));
        let gone = seen["gone"].as_array().expect("chi non c'è più");
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0]["agent"], json!("morta"));
        assert_eq!(gone[0]["why"], json!("expired"));
    }

    // **GUASTO 28: LA PROVA SI È SPOSTATA DOVE STA LA REGOLA.** Questa
    // chiamava `execute` con `{"$from": "/root"}` dentro, e reggeva perché
    // queste tre azioni risolvevano i rinvii ciascuna per conto proprio — una
    // delle dodici copie della stessa riga. Adesso c'è un posto solo,
    // `flow::step_input`, e la prova che ogni azione riceva l'ingresso sciolto
    // sta lì: `crates/flow/tests/a_reference_reaches_every_action.rs`.
}
