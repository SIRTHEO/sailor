//! Il passo che esegue un altro flusso. Decisione «I flussi si compongono, non
//! si fondono» in `docs/decisioni.md`; gli invarianti stanno accanto a ciò che
//! li impone — [`system::sources`] per la precedenza, [`call_cycle`] e
//! [`CALL_CHAIN`] per la ricorsione, [`MAX_DEPTH`] per la profondità,
//! [`tightest`] e [`remaining_of`] per il tetto e per ciò che non promette.

use crate::system::{self, FlowSource};
use crate::{
    Action, ActionError, ActionOutcome, ActionRegistry, Clock, Execution, ExecutionRequest,
    Executor, FlowFile, Graph, InProcessExecutor, Outcome, RecordStore, SharedState, StepRecord,
    StepSpecies, SystemClock, CURRENT_CAP, CURRENT_RUN, CURRENT_STEP,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Il nome con cui un passo chiede di eseguire un altro flusso.
///
/// **È QUELLO CHE LA FINESTRA SCRIVE DA SEMPRE.** `desktop/src/flow.ts` mappa
/// `subflow` sulla famiglia di nodo omonima e lo offre nella cassetta dei passi:
/// cambiarlo qui farebbe smettere di funzionare i passi già disegnati.
pub const SUBFLOW_ACTION: &str = "subflow";

/// La chiave sotto cui viaggia la catena dei flussi già in pila.
///
/// **PORTA I NOMI, NON UN CONTATORE.** Un numero direbbe «troppo profondo» e
/// niente altro; la catena permette all'errore di **nominare** chi chiama chi,
/// che è la sola forma in cui una persona può togliere l'anello. Il prefisso
/// `flow.` è dell'esecutore, come per [`CURRENT_RUN`]: un flusso non ci scrive.
pub const CALL_CHAIN: &str = "flow.subflow.chain";

/// Quanti flussi possono stare impilati, contando il primo chiamato.
///
/// **NON È UN LIMITE DELLA MACCHINA.** La pila ne reggerebbe molti di più; a non
/// reggerne di più è chi guarda. Quattro è la profondità del ciclo che questa
/// casa compone davvero — ricerca, smistamento, sviluppo, interrogazione — e un
/// quinto livello, a oggi, è più probabilmente un errore di scrittura che un
/// disegno. Chi ne ha bisogno alza questa riga e dice perché.
pub const MAX_DEPTH: usize = 4;

/// I campi che il passo conosce. Serve a `flow check`, non all'esecuzione.
const KNOWN_FIELDS: &[&str] = &["flow", "inputs"];

/// Ciò che il passo dichiara: quale flusso, e con quali ingressi.
///
/// **NON HA `deny_unknown_fields`, E NON È UNA DIMENTICANZA.** A tempo di
/// esecuzione l'ingresso di un passo è l'uscita della sua dipendenza, dove i
/// campi estranei sono la norma. La severità sta dove serve — su ciò che una
/// persona scrive a mano — ed è [`SubflowAction::unknown_fields`], che
/// `flow check` interroga prima che la corsa parta.
///
/// **NON C'È UN TETTO DI SPESA QUI DENTRO**, per decisione: «il tetto di spesa è
/// del flusso» (31/08/2026). Un passo che potesse alzarlo per il flusso che
/// chiama sposterebbe la dichiarazione lontano da chi la deve leggere.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Call {
    /// Il nome del flusso, come si vedrebbe sul disco senza `.flow.json`.
    pub flow: String,
    /// I `root_inputs` che il passo impone al figlio, chiave per chiave.
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
}

/// Come è finita una corsa figlia, per chi la registra.
pub struct RunNote<'a> {
    pub flow: &'a FlowFile,
    pub run_id: &'a str,
    pub parent_run_id: &'a str,
    pub parent_step_id: &'a str,
    /// `running`, `complete`, `failed`, `waiting`, `stopped`, `cap_reached`.
    pub status: &'a str,
    pub started_at: i64,
    /// `None` finché la corsa figlia è aperta.
    pub ended_at: Option<i64>,
    pub error: Option<String>,
}

/// Ciò che il passo `subflow` non può sapere da sé.
///
/// **PERCHÉ UN TRATTO E NON TRE CAMPI.** Il passo deve far girare il flusso
/// figlio **con le stesse azioni del padre**, cioè con il registro in cui esso
/// stesso è registrato: un riferimento diretto sarebbe un anello che il
/// compilatore rifiuta di costruire. E deve scrivere nel deposito, che il crate
/// del flusso non conosce e non deve conoscere — `flow` non dipende da
/// `ledger`, ed è la direzione che tiene in piedi tutto il resto. Chi costruisce
/// il registro ha in mano tutte e due le cose: le passa di qui.
pub trait SubflowHost: Send + Sync {
    /// Dove si cercano i flussi, nell'ordine di [`crate::system::sources`].
    fn sources(&self) -> Vec<FlowSource>;

    /// Le azioni con cui gira il figlio: le stesse del padre.
    fn actions(&self) -> Result<Arc<ActionRegistry>, ActionError>;

    /// Dove si scrivono i passi della corsa figlia.
    fn store(&self) -> Result<Arc<dyn RecordStore>, ActionError>;

    /// Scrive — o aggiorna — l'intestazione della corsa figlia.
    fn note_run(&self, note: &RunNote<'_>) -> Result<(), ActionError>;

    /// La riga che spiega a una persona come è finita la corsa figlia.
    ///
    /// La compone chi mostra, non chi esegue: `SpendStop` porta i dati e la
    /// frase sta altrove, e ricopiarla qui ne farebbe una seconda copia da
    /// tenere allineata. Chi non ha una frase non ne inventa una.
    fn why(&self, _execution: &Execution) -> Option<String> {
        None
    }
}

/// Il passo che esegue un altro flusso.
pub struct SubflowAction {
    host: Arc<dyn SubflowHost>,
}

impl SubflowAction {
    pub fn new(host: Arc<dyn SubflowHost>) -> Self {
        Self { host }
    }
}

impl Action for SubflowAction {
    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let call: Call = serde_json::from_value(input.clone()).map_err(|error| {
            ActionError::new(
                "invalid_subflow_call",
                format!("il passo non dichiara quale flusso eseguire: {error}"),
            )
        })?;

        // Chi chiama: la corsa e il passo li scrive l'esecutore prima di ogni
        // azione. Senza, la corsa figlia non sarebbe risalibile, e una corsa
        // figlia che nessuno può ricollegare al passo che l'ha chiesta è il
        // guasto che la decisione 4 esiste per non avere.
        let parent_run = text(shared, CURRENT_RUN).ok_or_else(|| {
            ActionError::new(
                "no_parent_run",
                "nessuna corsa in corso: un sotto-flusso esiste solo dentro una corsa",
            )
        })?;
        let parent_step = text(shared, CURRENT_STEP).ok_or_else(|| {
            ActionError::new(
                "no_parent_step",
                "nessun passo in corso: non saprei a chi attribuire la corsa figlia",
            )
        })?;

        // IL DEPOSITO SI CHIEDE PRIMA DI LEGGERE I FILE. Non averlo è una
        // condizione del passo, non del flusso che nomina: scoprirlo dopo aver
        // percorso tutte le sorgenti farebbe dire «non trovo quel flusso» a chi
        // in realtà non poteva eseguirne nessuno.
        let store = self.host.store()?;

        let sources = self.host.sources();
        let found = system::load_all(&sources);
        let (_, origin, entry) = found
            .iter()
            .find(|(name, _, _)| name == &call.flow)
            .ok_or_else(|| {
                ActionError::new(
                    "unknown_subflow",
                    format!(
                        "nessun flusso «{}» fra quelli che vedo: {}",
                        call.flow,
                        places(&sources)
                    ),
                )
            })?;
        let child = entry
            .clone()
            .map_err(|why| ActionError::new("invalid_subflow", why))?;

        // PRIMA DI APRIRE, NON DOPO AVER SPESO. Le chiamate dichiarate nei
        // `with` si leggono senza eseguire niente: un anello fra file diversi
        // si scopre qui, al primo passo `subflow` della corsa più esterna.
        if let Some(cycle) = call_cycle(&call.flow, &known_flows(&found)) {
            return Err(cyclic(&cycle));
        }

        let chain = extend_chain(&chain_of(shared), &call.flow)?;

        let cap = tightest(
            child.spend_cap_micros,
            remaining_of(shared, &store, &parent_run)?,
        );

        let run_id = child_run_id(&parent_run, &parent_step)?;
        let started_at = SystemClock.now().map_err(clock_broke)?;
        let mut note = RunNote {
            flow: &child,
            run_id: &run_id,
            parent_run_id: &parent_run,
            parent_step_id: &parent_step,
            status: "running",
            started_at,
            ended_at: None,
            error: None,
        };
        self.host.note_run(&note)?;

        // GLI INGRESSI DEL FIGLIO SONO I SUOI, SOVRASCRITTI DAL PASSO. Non
        // quelli del padre: quello che il figlio riceve è scritto in un posto
        // solo, e si legge senza sapere niente di chi lo chiama.
        let mut root_inputs = child.inputs.clone();
        root_inputs.extend(call.inputs.clone());

        let mut child_shared = SharedState::new();
        child_shared.insert(CALL_CHAIN.to_owned(), chain_value(&chain));

        let actions = self.host.actions()?;
        let outcome = InProcessExecutor.execute(
            &child.graph,
            ExecutionRequest {
                run_id: run_id.clone(),
                root_inputs,
                gates: Vec::new(),
                shared: child_shared,
                spend_cap_micros: cap,
            },
            store.as_ref(),
            actions.as_ref(),
            &SystemClock,
        );

        let ended_at = SystemClock.now().map_err(clock_broke)?;
        note.ended_at = Some(ended_at);

        let execution = match outcome {
            Ok(execution) => execution,
            Err(error) => {
                let said = error.to_string();
                note.status = "failed";
                note.error = Some(said.clone());
                self.host.note_run(&note)?;
                return Err(ActionError::new(
                    "subflow_broke",
                    format!("la corsa {run_id} del flusso {} non è partita: {said}", call.flow),
                ));
            }
        };

        let (status, went_well) = crate::run_status(&execution);
        let why = self.host.why(&execution);
        note.status = status;
        note.error = why.clone();
        self.host.note_run(&note)?;

        if !went_well {
            // ASPETTARE NON È ROMPERSI. Un figlio fermo su un passo che aspetta
            // fa aspettare il padre: la corsa del padre resta ripartibile
            // invece di risultare guasta, che è come si comporta qualunque
            // altro passo che non sa ancora il proprio esito.
            if status == "waiting" {
                return Ok(ActionOutcome::Waiting(format!(
                    "la corsa {run_id} del flusso {} sta aspettando",
                    call.flow
                )));
            }
            return Err(ActionError::new(
                format!("subflow_{status}"),
                why.unwrap_or_else(|| {
                    format!(
                        "la corsa {run_id} del flusso {} è finita in stato {status}",
                        call.flow
                    )
                }),
            ));
        }

        let records = store
            .records(&run_id)
            .map_err(|error| ActionError::new("subflow_unreadable", error.to_string()))?;
        Ok(ActionOutcome::Went(json!({
            "flow": call.flow,
            "origin": origin,
            "run_id": run_id,
            "status": status,
            "outputs": last_outputs(&child.graph, &records),
        })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        let Some(object) = declared.as_object() else {
            return Vec::new();
        };
        object
            .keys()
            .filter(|name| !KNOWN_FIELDS.contains(&name.as_str()))
            .cloned()
            .collect()
    }

    /// **SI CONSEGNA A UNA PERSONA, E NON È PRUDENZA GENERICA.** Rifare questo
    /// passo vuol dire rifare un flusso intero, con dentro tutto quello che
    /// quel flusso tocca: motori a pagamento, file scritti, pannelli aperti. La
    /// specie del figlio non si può dedurre da qui — è la somma delle specie
    /// dei suoi passi, e basta che uno solo sia da consegnare a una persona
    /// perché lo sia anche la chiamata.
    fn species(&self) -> StepSpecies {
        StepSpecies::HandToHuman
    }
}

/// La catena dei flussi già in pila, letta dallo stato condiviso.
pub fn chain_of(shared: &SharedState) -> Vec<String> {
    shared
        .get(CALL_CHAIN)
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// La catena con `next` in fondo, o l'errore che nomina il perché non ci sta.
///
/// **DUE GUASTI DIVERSI, DUE PAROLE DIVERSE.** Un anello è un errore di disegno
/// e non si toglie alzando un numero; una pila troppo alta può essere legittima
/// e la si alza. Dirli tutti e due «troppo profondo» manderebbe chi legge a
/// cercare la cosa sbagliata.
pub fn extend_chain(chain: &[String], next: &str) -> Result<Vec<String>, ActionError> {
    if let Some(from) = chain.iter().position(|seen| seen == next) {
        let mut cycle: Vec<String> = chain[from..].to_vec();
        cycle.push(next.to_owned());
        return Err(cyclic(&cycle));
    }
    if chain.len() >= MAX_DEPTH {
        let mut deep: Vec<String> = chain.to_vec();
        deep.push(next.to_owned());
        return Err(ActionError::new(
            "subflow_too_deep",
            format!(
                "più di {MAX_DEPTH} flussi impilati uno dentro l'altro: {}",
                deep.join(" → ")
            ),
        ));
    }
    let mut extended = chain.to_vec();
    extended.push(next.to_owned());
    Ok(extended)
}

/// I flussi nominati dai passi `subflow` di questo flusso.
///
/// **LEGGE IL `with`, CIOÈ CIÒ CHE È DICHIARATO.** Un passo che ricava il nome
/// del flusso dall'uscita di una dipendenza non compare qui, e non può
/// comparirci: a tempo di controllo quel nome non esiste ancora. È il limite
/// dichiarato del controllo statico, e la ragione per cui la catena viaggia
/// anche a tempo di esecuzione.
pub fn calls_of(flow: &FlowFile) -> Vec<String> {
    flow.graph
        .steps()
        .iter()
        .filter(|step| step.action == SUBFLOW_ACTION)
        .filter_map(|step| {
            step.with
                .as_ref()
                .and_then(|with| with.get("flow"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

/// I flussi validi fra quelli caricati, per nome.
///
/// Quelli rotti restano fuori: un file che non si legge non dichiara chiamate,
/// e dire che ne ha zero sarebbe un'affermazione che nessuno ha verificato.
pub fn known_flows(
    found: &[(String, &'static str, Result<FlowFile, String>)],
) -> BTreeMap<String, FlowFile> {
    found
        .iter()
        .filter_map(|(name, _, entry)| {
            entry
                .as_ref()
                .ok()
                .map(|flow| (name.clone(), flow.clone()))
        })
        .collect()
}

/// La catena di chiamate che torna su se stessa partendo da `entry`, se c'è.
///
/// **È IL CONTROLLO CHE IL GRAFO NON PUÒ FARE.** `Graph::validate` rifiuta i
/// cicli, ma guarda dentro un file solo: con `subflow` un anello attraversa più
/// file, e nessuno dei due grafi da solo ha niente di storto. Qui si percorrono
/// le chiamate dichiarate, e la catena restituita si legge come si legge:
/// `ricerca → sviluppo → ricerca`.
pub fn call_cycle(entry: &str, known: &BTreeMap<String, FlowFile>) -> Option<Vec<String>> {
    let mut chain = Vec::new();
    walk(entry, known, &mut chain)
}

fn walk(
    name: &str,
    known: &BTreeMap<String, FlowFile>,
    chain: &mut Vec<String>,
) -> Option<Vec<String>> {
    if let Some(from) = chain.iter().position(|seen| seen == name) {
        let mut cycle: Vec<String> = chain[from..].to_vec();
        cycle.push(name.to_owned());
        return Some(cycle);
    }
    let flow = known.get(name)?;
    chain.push(name.to_owned());
    for next in calls_of(flow) {
        if let Some(cycle) = walk(&next, known, chain) {
            return Some(cycle);
        }
    }
    chain.pop();
    None
}

/// Il tetto che vale per il figlio: il più stretto fra i due dichiarati.
///
/// **`None` NON È ZERO NEMMENO QUI.** Chi non dichiara niente non impone
/// niente: il tetto che resta è quello dell'altro. Sono tutti e due assenti solo
/// quando nessuno ha messo un limite, ed è l'unico caso in cui il figlio gira
/// senza.
pub fn tightest(declared: Option<i64>, remaining: Option<i64>) -> Option<i64> {
    match (declared, remaining) {
        (Some(one), Some(other)) => Some(one.min(other)),
        (Some(one), None) => Some(one),
        (None, other) => other,
    }
}

/// Quanto resta al padre sotto il proprio tetto, se un tetto ce l'ha.
///
/// **LIMITE NOTO.** Il deposito somma per corsa e la spesa del figlio sta sotto
/// il suo `run_id`: questo residuo non cala per ciò che i figli hanno speso, e
/// il caso peggiore è il tetto del padre per il numero dei suoi passi
/// `subflow`. Si chiude facendo risalire `parent_run_id` nella somma.
fn remaining_of(
    shared: &SharedState,
    store: &Arc<dyn RecordStore>,
    parent_run: &str,
) -> Result<Option<i64>, ActionError> {
    let Some(cap) = shared.get(CURRENT_CAP).and_then(Value::as_i64) else {
        return Ok(None);
    };
    let spent = store
        .spent(parent_run)
        .map_err(|error| ActionError::new("subflow_unreadable", error.to_string()))?;
    Ok(Some((cap - spent.micros).max(0)))
}

/// L'identificativo della corsa figlia.
///
/// **PORTA IL PADRE NEL PROPRIO NOME, E IL PERCHÉ È LA DECISIONE 4.** Il legame
/// vero sta nella colonna `parent_run_id` del deposito; questo prefisso lo
/// raddoppia dove non c'è un deposito da interrogare — un elenco di file, una
/// riga di registro, un rapporto. Le nanosecondi in fondo rendono unico ogni
/// tentativo: un passo `subflow` ritentato apre una corsa nuova invece di
/// riaprire quella rotta, che è come si comporta ogni altro tentativo.
fn child_run_id(parent_run: &str, parent_step: &str) -> Result<String, ActionError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| ActionError::new("clock_before_epoch", error.to_string()))?;
    Ok(format!("{parent_run}::{parent_step}::{}", now.as_nanos()))
}

/// Le uscite dei passi terminali della corsa figlia.
///
/// **TERMINALE VUOL DIRE «NESSUNO DIPENDE DA LUI».** Un flusso non dichiara
/// quale sia la sua uscita, e inventare una convenzione — «l'ultimo passo del
/// file» — legherebbe il risultato all'ordine in cui qualcuno ha scritto le
/// righe. I passi da cui non pende nessuno sono ciò che quel flusso ha
/// prodotto, e sono una risposta che non cambia riordinando il file.
fn last_outputs(graph: &Graph, records: &[StepRecord]) -> Value {
    let depended: BTreeSet<&str> = graph
        .steps()
        .iter()
        .flat_map(|step| step.deps.iter().map(String::as_str))
        .collect();
    let mut outputs = Map::new();
    for step in graph.steps() {
        if depended.contains(step.id.as_str()) {
            continue;
        }
        let last = records
            .iter()
            .filter(|record| record.step_id == step.id && record.outcome == Some(Outcome::Went))
            .max_by_key(|record| (record.attempt, record.epoch));
        if let Some(output) = last.and_then(|record| record.output.clone()) {
            outputs.insert(step.id.clone(), output);
        }
    }
    Value::Object(outputs)
}

fn cyclic(cycle: &[String]) -> ActionError {
    ActionError::new(
        "subflow_cycle",
        format!(
            "un flusso non può richiamare se stesso, nemmeno passando per altri: {}",
            cycle.join(" → ")
        ),
    )
}

fn chain_value(chain: &[String]) -> Value {
    Value::Array(chain.iter().cloned().map(Value::String).collect())
}

fn text(shared: &SharedState, key: &str) -> Option<String> {
    shared
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn places(sources: &[FlowSource]) -> String {
    sources
        .iter()
        .map(|source| format!("{} ({})", source.origin, source.dir.display()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn clock_broke(error: crate::FlowError) -> ActionError {
    ActionError::new("clock_broke", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Step, ValueSchema};

    fn calling(id: &str, called: &[&str]) -> FlowFile {
        let steps: Vec<Step> = called
            .iter()
            .enumerate()
            .map(|(nth, target)| Step {
                id: format!("chiama-{nth}"),
                deps: Vec::new(),
                input_schema: ValueSchema::Any,
                output_schema: ValueSchema::Any,
                with: Some(json!({ "flow": target })),
                when: None,
                action: SUBFLOW_ACTION.to_owned(),
                max_attempts: 1,
            })
            .collect();
        FlowFile {
            id: id.to_owned(),
            description: "un flusso di prova".to_owned(),
            graph: Graph::new(steps).expect("grafo valido"),
            inputs: BTreeMap::new(),
            schedule: None,
            spend_cap_micros: None,
        }
    }

    fn map(flows: Vec<FlowFile>) -> BTreeMap<String, FlowFile> {
        flows.into_iter().map(|flow| (flow.id.clone(), flow)).collect()
    }

    #[test]
    fn two_flows_that_call_each_other_are_a_named_chain() {
        let known = map(vec![calling("ricerca", &["sviluppo"]), calling("sviluppo", &["ricerca"])]);

        let cycle = call_cycle("ricerca", &known).expect("l'anello c'è");

        assert_eq!(cycle, vec!["ricerca", "sviluppo", "ricerca"]);
    }

    #[test]
    fn a_flow_that_calls_itself_is_a_chain_of_two() {
        let known = map(vec![calling("solitario", &["solitario"])]);

        assert_eq!(
            call_cycle("solitario", &known).expect("l'anello c'è"),
            vec!["solitario", "solitario"]
        );
    }

    /// Senza questa, «trova sempre un anello» resterebbe verde su tutte le
    /// altre: un albero di chiamate che converge sullo stesso flusso da due
    /// rami non è un ciclo.
    #[test]
    fn a_diamond_of_calls_is_not_a_cycle() {
        let known = map(vec![
            calling("cima", &["sinistra", "destra"]),
            calling("sinistra", &["fondo"]),
            calling("destra", &["fondo"]),
            calling("fondo", &[]),
        ]);

        assert_eq!(call_cycle("cima", &known), None);
    }

    /// Un nome che nessuna sorgente conosce non è un ciclo: è un flusso che
    /// manca, e lo dice un altro errore con un'altra parola.
    #[test]
    fn a_call_to_a_flow_nobody_has_is_not_a_cycle() {
        let known = map(vec![calling("cima", &["mai-scritto"])]);

        assert_eq!(call_cycle("cima", &known), None);
    }

    #[test]
    fn the_tighter_cap_is_the_one_that_holds() {
        assert_eq!(tightest(Some(500), Some(200)), Some(200));
        assert_eq!(tightest(Some(100), Some(900)), Some(100));
        assert_eq!(tightest(Some(100), None), Some(100));
        assert_eq!(tightest(None, Some(900)), Some(900));
        assert_eq!(tightest(None, None), None);
    }

    /// Zero è un tetto, non un'assenza: un padre che ha finito i soldi non
    /// lascia partire un figlio «senza limiti».
    #[test]
    fn a_remaining_of_zero_still_caps_the_child() {
        assert_eq!(tightest(Some(1_000_000), Some(0)), Some(0));
    }

    #[test]
    fn the_chain_grows_until_the_declared_depth() {
        let mut chain = Vec::new();
        for nth in 0..MAX_DEPTH {
            chain = extend_chain(&chain, &format!("f{nth}")).expect("ci sta");
        }
        assert_eq!(chain.len(), MAX_DEPTH);

        let error = extend_chain(&chain, "uno-di-troppo").expect_err("il tetto scatta");

        assert_eq!(error.class, "subflow_too_deep");
        assert!(
            error.said.contains("f0 → f1") && error.said.contains("uno-di-troppo"),
            "l'errore deve nominare la catena: {}",
            error.said
        );
    }

    #[test]
    fn a_repeated_flow_in_the_chain_is_named_as_a_cycle() {
        let chain = vec!["ricerca".to_owned(), "sviluppo".to_owned()];

        let error = extend_chain(&chain, "ricerca").expect_err("è un anello");

        assert_eq!(error.class, "subflow_cycle");
        assert!(
            error.said.contains("ricerca → sviluppo → ricerca"),
            "l'errore deve nominare la catena: {}",
            error.said
        );
    }
}
