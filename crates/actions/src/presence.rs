//! I tre nodi con cui un agente **dice di esserci** e **scopre chi altro c'è**.
//!
//! **PERCHÉ ESISTONO.** Il 31/08/2026 sette agenti lavoravano su sette alberi
//! di lavoro della stessa repo e nessuno sapeva dell'esistenza degli altri.
//! Nello stesso giorno, due cose che discendono da lì: una sessione ha
//! committato su `sorgenti` cancellando il lavoro non committato di un'altra —
//! e nessuna delle due se n'è accorta; e due sessioni hanno numerato in modo
//! indipendente una voce nuova dello stesso documento, producendo due `27` e
//! due `28`. Non è una svista due volte: è la stessa mancanza, cioè che il
//! sistema non ha nessun posto dove uno dica «ci sono» e un altro lo legga.
//!
//! **DOVE STA IL PUNTO D'INCONTRO, VISTO CHE NON C'È UN CENTRO.** Il deposito
//! è già una casa sola per macchina — `ledger::default_directory()` risponde
//! lo stesso percorso da qualunque albero di lavoro — ed è SQLite in modalità
//! WAL con un tempo di attesa dichiarato: più processi lo aprono direttamente,
//! senza che nessuno di loro sia «il server». Non serve un demone, non serve
//! un servizio, non serve una porta: chi non c'è non tiene niente. Questi tre
//! nodi non aggiungono infrastruttura, compongono i tre nodi del deposito che
//! esistevano già.
//!
//! **UN ANNUNCIO SCADE, E NON SI RILASCIA SOLTANTO.** Su questa macchina i
//! processi vengono uccisi dal sonno del sistema, quindi un rilascio esplicito
//! è una promessa che il morto non può mantenere: sarebbe l'annuncio appeso
//! per sempre, che è il difetto principale di questa famiglia di sistemi. Qui
//! l'annuncio dura un tempo dichiarato e va **rinnovato** — l'unica promessa
//! che un processo morto non può fingere di mantenere. Il rilascio esiste
//! comunque, perché chi finisce alle 10:00 non deve trattenere fino alle
//! 10:15, e resta **distinto** da una scadenza: `released` dice «qualcuno ha
//! guardato e ha finito», `expired` dice «nessuno si è più fatto vivo».
//!
//! **LA CHIAVE PORTA IL PROCESSO, ED È IL PUNTO DI TUTTO.** Un annuncio sta
//! sotto `<agente>#<pid>`: nessun agente scrive mai la riga di un altro, quindi
//! nessun rinnovo può cancellare il lavoro di nessuno. È la stessa lezione del
//! doppio `27` letta al contrario — due che scrivono nello stesso posto si
//! sovrascrivono, due che scrivono ciascuno nel proprio si sommano — e senza
//! il `#<pid>` due agenti che scegliessero lo stesso nome ricadrebbero
//! esattamente nel difetto contro cui questo modulo è scritto.
//!
//! **NON CHIEDE AL SISTEMA OPERATIVO SE UN PROCESSO È VIVO.** Il `pid` viene
//! registrato perché una persona possa controllare, e non è l'oracolo: il
//! guasto 12 di `docs/guasti-incontrati.md` dice che dentro il perimetro
//! `pgrep` non vede i processi e **risponde vuoto senza errore**, e la cura
//! scritta accanto è «chiedere lo stato al deposito, non al sistema
//! operativo». La regola di vita è la scadenza, e solo quella.
//!
//! **QUELLO CHE LA SCADENZA NON SA, E VA LETTO PRIMA DI FIDARSENE.** Durante il
//! sonno di sistema un processo è **congelato ma vivo**, e l'orologio a muro
//! avanza lo stesso: al risveglio la sua scadenza è passata mentre lui non è
//! morto affatto. Per questo il censimento non dice mai «morto»: dice
//! `expired`, che significa «nessuno si è più fatto vivo» — e chi legge ha
//! `renewed_at` accanto per sapere se il silenzio dura trenta secondi o otto
//! ore. La misura autorevole esiste ed è `flock(2)`: il kernel rilascia il lock
//! anche dopo un `SIGKILL`, ed è l'unica primitiva in cui la scadenza non
//! dipende da codice nostro. **Non è qui**, e la ragione è dichiarata: vuole o
//! una dipendenza nuova nel workspace — che il `Cargo.toml` di casa tiene al
//! minimo per scelta scritta — o del codice `unsafe` in un crate che non ne ha.
//! Va aggiunta come **secondo strato autorevole** sopra questo, non al suo
//! posto: `flock` sa se un processo esiste, non se sta lavorando.

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

pub fn register_presence(registry: &mut flow::ActionRegistry, ledger: Ledger) {
    registry.register(WORK_CLAIM_ACTION, WorkClaimAction::new(ledger.clone()));
    registry.register(WORK_RELEASE_ACTION, WorkReleaseAction::new(ledger.clone()));
    registry.register(WORK_SURVEY_ACTION, WorkSurveyAction::new(ledger));
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

fn claim_key(agent: &str, pid: u32) -> String {
    format!("{agent}#{pid}")
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
    let left: Vec<&str> = left.trim_matches('/').split('/').filter(|s| !s.is_empty()).collect();
    let right: Vec<&str> = right.trim_matches('/').split('/').filter(|s| !s.is_empty()).collect();
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
        let expires_at = at + spec.lease_seconds.unwrap_or(DEFAULT_LEASE_SECONDS);
        let record = StoreRecord {
            collection: CLAIMS_COLLECTION.to_owned(),
            key: claim_key(&spec.agent, pid),
            value: json!({
                "agent": spec.agent,
                "repository": spec.repository,
                "workdir": spec.workdir,
                "branch": spec.branch,
                "paths": spec.paths,
                "doing": spec.doing,
                "pid": pid,
                "renewed_at": at,
                "expires_at": expires_at,
                "released_at": Value::Null,
            }),
            written_by: spec.agent.clone(),
            written_at: at,
        };
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
                        c["agent"].as_str().unwrap_or("senza nome"),
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
        let key = claim_key(&spec.agent, pid);
        let found = self
            .ledger
            .read_record(CLAIMS_COLLECTION, &key)
            .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
        let Some(mut record) = found else {
            return Ok(ActionOutcome::Went(json!({ "released": false })));
        };
        record.value["released_at"] = json!(at);
        record.written_at = at;
        self.ledger
            .put_record(&record)
            .map_err(|error| ActionError::new("store_refused", error.to_string()))?;
        Ok(ActionOutcome::Went(json!({ "released": true, "key": key })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

pub struct WorkSurveyAction {
    ledger: Ledger,
}

impl WorkSurveyAction {
    pub fn new(ledger: Ledger) -> Self {
        Self { ledger }
    }
}

impl Action for WorkSurveyAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: SurveySpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let at = spec.at.unwrap_or_else(now);
        let records = self
            .ledger
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
            .execute(&claim("prima", 101, "/casa/progetto", NOON, &[]), &mut shared)
            .expect("il primo annuncio");
        let second = went(
            action
                .execute(&claim("seconda", 102, "/casa/progetto", NOON + 10, &[]), &mut shared)
                .expect("il secondo annuncio"),
        );

        let collisions = second["collisions"].as_array().expect("le collisioni");
        assert_eq!(collisions.len(), 1, "il secondo agente deve vedere il primo");
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
            .execute(&claim("morta", 101, "/casa/progetto", NOON, &[]), &mut shared)
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

    /// **LA LEZIONE DEL DOPPIO 27.** Chi rinnova non tocca la riga di nessun altro.
    #[test]
    fn a_renewal_never_erases_another_agents_claim() {
        let (ledger, _guard) = store();
        let mut shared = SharedState::new();
        let action = WorkClaimAction::new(ledger.clone());
        let survey = WorkSurveyAction::new(ledger);

        action
            .execute(&claim("prima", 101, "/casa/progetto", NOON, &[]), &mut shared)
            .expect("prima");
        action
            .execute(&claim("seconda", 102, "/casa/progetto", NOON, &[]), &mut shared)
            .expect("seconda");
        action
            .execute(&claim("prima", 101, "/casa/progetto", NOON + 60, &[]), &mut shared)
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
            .execute(&claim("prima", 101, "/casa/progetto", NOON, &[]), &mut shared)
            .expect("prima");
        let second = went(
            action
                .execute(&claim("seconda", 102, "/casa/altro-albero", NOON, &[]), &mut shared)
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
                    &claim("terza", 103, "/casa/progetto", NOON, &["crates/actions/src/lib.rs"]),
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
        let survey = WorkSurveyAction::new(ledger);

        action
            .execute(&claim("prima", 101, "/casa/progetto", NOON, &[]), &mut shared)
            .expect("prima");
        release
            .execute(&json!({"agent": "prima", "pid": 101, "at": NOON + 30}), &mut shared)
            .expect("il rilascio");

        let second = went(
            action
                .execute(&claim("seconda", 102, "/casa/progetto", NOON + 31, &[]), &mut shared)
                .expect("seconda"),
        );
        assert_eq!(
            second["collisions"].as_array().expect("le collisioni").len(),
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
            .execute(&claim("prima", 101, "/casa/progetto", NOON, &[]), &mut shared)
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
            .execute(&claim("prima", 101, "/casa/progetto", NOON, &[]), &mut shared)
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
        let survey = WorkSurveyAction::new(ledger);

        action
            .execute(&claim("morta", 101, "/casa/progetto", NOON, &[]), &mut shared)
            .expect("chi poi muore");
        action
            .execute(&claim("viva", 102, "/casa/progetto", NOON + 900, &[]), &mut shared)
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
