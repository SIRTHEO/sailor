//! «Cosa chiedono i flussi che stanno su questa macchina, e che qui non c'è.»
//!
//! **PERCHÉ NON BASTA IL RILEVAMENTO.** `detect_tools` risponde a «cosa c'è
//! qui»: è un elenco, e un elenco non dice a nessuno cosa farne. La domanda che
//! una persona ha davvero è l'altra — *questi flussi, su questa macchina,
//! girano?* — e per rispondere servono due cose che nessuna azione sola ha:
//! quello che la macchina offre, e quello che i flussi chiedono. Questa azione
//! porta il secondo pezzo e li mette insieme.
//!
//! **NON GUARDA LA MACCHINA, E NON È UNA MANCANZA.** Riceve il rilevamento dal
//! passo che la precede e non ne fa uno suo. Così il flusso dichiara la catena
//! invece di nasconderla, il rilevamento si paga una volta sola, e se un giorno
//! qualcuno vorrà incrociare i flussi con il rilevamento di *un'altra* macchina
//! — un elenco arrivato da fuori — questa azione funziona già, perché non ha mai
//! saputo da dove venisse.
//!
//! **CHE POTERE HA.** Uno solo: leggere i file dei flussi nei posti in cui
//! Sailor li cerca. Il confronto che fa dopo non è un potere, è composizione, e
//! per questo sta tutto qui dentro e non in un interprete dentro il flusso.

use crate::Finding;
use flow::system::{self, FlowSource};
use flow::{Action, ActionError, ActionOutcome, FlowFile, SharedState, StepSpecies};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Il nome sotto cui l'azione si registra.
pub const TOOL_NEEDS_ACTION: &str = "tool_needs";

/// Registra l'azione sotto il suo nome stabile.
pub fn register_needs(registry: &mut flow::ActionRegistry) {
    registry.register(TOOL_NEEDS_ACTION, ToolNeedsAction);
}

/// L'ingresso del passo.
///
/// **NIENTE `deny_unknown_fields`, E VA SPIEGATO.** L'ingresso di un passo con
/// una dipendenza *è* l'uscita di quella dipendenza, col `with` sovrapposto:
/// arriva quindi tutto quello che il rilevamento ha prodotto — `problems`,
/// `looked_in`, `present`, `total` — e rifiutare i campi sconosciuti vorrebbe
/// dire rifiutare l'unico modo in cui questa azione può essere invocata.
#[derive(Debug, Deserialize)]
struct NeedsSpec {
    /// Quello che il passo di rilevamento ha trovato su questa macchina.
    findings: Vec<Finding>,
    /// Cartelle di flussi da guardare al posto di quelle abituali.
    #[serde(default)]
    flows_dirs: Vec<String>,
    /// Se guardare anche dove Sailor cerca sempre: i flussi spediti, quelli
    /// della casa, quelli del progetto.
    #[serde(default = "yes")]
    include_default_sources: bool,
}

fn yes() -> bool {
    true
}

/// Uno strumento chiesto da almeno un passo, e come sta messo qui.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Need {
    /// L'identificativo dello strumento, come lo scrive il passo.
    pub tool: String,
    /// Quali passi lo chiedono, scritti `flusso/passo`: senza questa riga chi
    /// legge sa che manca qualcosa e non sa cosa smetterà di funzionare.
    pub asked_by: Vec<String>,
    /// Dove sta l'eseguibile, se c'è.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    /// Perché non c'è, quando non c'è.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
    /// La nota del descrittore: è il posto in cui sta scritto da dove si
    /// installa, ed è tutto quello che chi legge avrà per rimediare.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// Risponde a «questi flussi girano qui?».
pub struct ToolNeedsAction;

impl Action for ToolNeedsAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: NeedsSpec = serde_json::from_value(input.clone()).map_err(|error| {
            ActionError::new(
                "invalid_input",
                format!(
                    "{error}. Questo passo va dopo un rilevamento: riceve i suoi `findings` \
                     come ingresso, e da solo non guarda la macchina"
                ),
            )
        })?;

        let mut sources: Vec<FlowSource> = Vec::new();
        if spec.include_default_sources {
            sources.extend(default_flow_sources());
        }
        for raw in &spec.flows_dirs {
            sources.push(FlowSource {
                origin: "dichiarati nel passo",
                dir: PathBuf::from(raw),
            });
        }

        let mut asked: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut named_binaries: Vec<String> = Vec::new();
        let mut flows_seen = 0usize;
        let mut flows_broken: Vec<String> = Vec::new();
        for (name, _, entry) in system::load_all(&sources) {
            match entry {
                Ok(flow) => {
                    flows_seen += 1;
                    for (tool, step) in tools_named_by(&flow) {
                        asked.entry(tool).or_default().push(format!("{name}/{step}"));
                    }
                    for step in binaries_named_by(&flow) {
                        named_binaries.push(format!("{name}/{step}"));
                    }
                }
                // UN FLUSSO ROTTO NON È UN FLUSSO CHE NON CHIEDE NIENTE. Contarlo
                // come zero direbbe «ti serve solo questo» a chi ha metà elenco.
                Err(_) => flows_broken.push(name),
            }
        }

        let found: BTreeMap<&str, &Finding> = spec
            .findings
            .iter()
            .map(|finding| (finding.name.as_str(), finding))
            .collect();

        let mut present: Vec<Need> = Vec::new();
        let mut missing: Vec<Need> = Vec::new();
        let mut unknown: Vec<Need> = Vec::new();
        for (tool, asked_by) in asked {
            match found.get(tool.as_str()) {
                Some(finding) if finding.presence.is_present() => present.push(Need {
                    tool,
                    asked_by,
                    executable: finding.executable.clone(),
                    reason: String::new(),
                    note: finding.note.clone(),
                }),
                Some(finding) => missing.push(Need {
                    tool,
                    asked_by,
                    executable: None,
                    reason: presence_reason(finding),
                    note: finding.note.clone(),
                }),
                None => unknown.push(Need {
                    tool,
                    asked_by,
                    executable: None,
                    reason: "nessun descrittore lo dichiara: non è che manchi su questa macchina, \
                             è che Sailor non sa cosa sia. Si aggiunge scrivendo un file JSON in \
                             ~/.config/sailor/tools.d/, senza ricompilare niente"
                        .to_string(),
                    note: String::new(),
                }),
            }
        }

        let looked_in: Vec<String> = sources
            .iter()
            .map(|source| format!("{}: {}", source.origin, source.dir.display()))
            .collect();
        let report = report_of(
            flows_seen,
            &flows_broken,
            &looked_in,
            &present,
            &missing,
            &unknown,
            &named_binaries,
        );

        Ok(ActionOutcome::Went(json!({
            "flows_seen": flows_seen,
            "flows_broken": flows_broken,
            "looked_in": looked_in,
            "present": present,
            "missing": missing,
            "unknown": unknown,
            "steps_naming_a_binary": named_binaries,
            "report": report,
        })))
    }

    /// Legge dei file e confronta due elenchi: rifarlo non cambia niente.
    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

/// I posti in cui Sailor cerca i flussi su questa macchina.
///
/// **LA CASA LA CHIEDE AL DEPOSITO**, come fa la finestra: due idee di dove sta
/// la casa non danno un errore, danno un elenco che parla di flussi che nessuno
/// esegue.
fn default_flow_sources() -> Vec<FlowSource> {
    let home = ledger::sailor_home().unwrap_or_else(|| PathBuf::from("."));
    let working = std::env::current_dir().ok();
    let declared = std::env::var_os("SAILOR_FLOWS").map(PathBuf::from);
    system::sources(
        &home.join("flows"),
        working.as_deref(),
        declared.as_deref().map(Path::new),
    )
}

/// Gli strumenti che un flusso chiede, passo per passo.
///
/// **SI GUARDA DOVE LI LEGGE CHI ESEGUE**, cioè al primo livello di `with` e dei
/// valori dichiarati per un passo senza dipendenze: è lì che `external_engine`
/// cerca `tool`. Cercare più a fondo troverebbe la parola `tool` dentro un
/// prompt o dentro uno schema di risposta e la conterebbe come una richiesta.
fn tools_named_by(flow: &FlowFile) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for step in flow.graph.steps() {
        for place in [step.with.as_ref(), flow.inputs.get(&step.id)] {
            if let Some(tool) = place.and_then(|value| value.get("tool")).and_then(Value::as_str) {
                out.push((tool.to_owned(), step.id.clone()));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// I passi che nominano un binario invece di uno strumento.
///
/// **NON È UN ERRORE, È UNA MISURA CHE SERVE.** Un passo che scrive `bin` gira
/// solo dove quel nome è nel percorso di chi esegue, e nessun elenco di
/// strumenti mancanti lo vedrà mai: è il modo silenzioso in cui un flusso smette
/// di essere portabile. Dirlo qui costa una riga.
fn binaries_named_by(flow: &FlowFile) -> Vec<String> {
    let mut out = Vec::new();
    for step in flow.graph.steps() {
        for place in [step.with.as_ref(), flow.inputs.get(&step.id)] {
            if place.and_then(|value| value.get("bin")).is_some() {
                out.push(step.id.clone());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn presence_reason(finding: &Finding) -> String {
    match &finding.presence {
        crate::Presence::Present(reason)
        | crate::Presence::Absent(reason)
        | crate::Presence::Undetermined(reason) => reason.clone(),
    }
}

/// La risposta scritta perché una persona la legga.
///
/// Sta accanto ai dati e non al posto loro: chi guarda la finestra legge questa,
/// chi compone un altro passo prende gli elenchi.
/// «1 passi nominano» è una frase che fa sembrare rotto chi la scrive, e chi
/// legge un rapporto sbagliato nella forma smette di fidarsi anche dei numeri.
fn count(quantity: usize, one: &str, many: &str) -> String {
    if quantity == 1 {
        format!("{quantity} {one}")
    } else {
        format!("{quantity} {many}")
    }
}

fn report_of(
    flows_seen: usize,
    flows_broken: &[String],
    looked_in: &[String],
    present: &[Need],
    missing: &[Need],
    unknown: &[Need],
    named_binaries: &[String],
) -> String {
    let mut text = String::new();
    let _ = write!(
        text,
        "{} letti in {}; chiedono {}.",
        count(flows_seen, "flusso", "flussi"),
        count(looked_in.len(), "posto", "posti"),
        count(
            present.len() + missing.len() + unknown.len(),
            "strumento",
            "strumenti"
        )
    );
    if !flows_broken.is_empty() {
        let _ = write!(
            text,
            "\n\nATTENZIONE: {} non si sono potuti leggere, quindi questo elenco è parziale: {}.",
            count(flows_broken.len(), "flusso", "flussi"),
            flows_broken.join(", ")
        );
    }
    if !present.is_empty() {
        let _ = write!(text, "\n\nCi sono, e questi flussi qui girano:");
        for need in present {
            let _ = write!(
                text,
                "\n  {} — {} (chiesto da {})",
                need.tool,
                need.executable.as_deref().unwrap_or("trovato"),
                need.asked_by.join(", ")
            );
        }
    }
    if missing.is_empty() && unknown.is_empty() {
        let _ = write!(
            text,
            "\n\nNon manca niente: ogni strumento chiesto da un flusso è su questa macchina."
        );
    }
    if !missing.is_empty() {
        let _ = write!(
            text,
            "\n\nMANCANO QUI, e senza di loro questi passi si fermano:"
        );
        for need in missing {
            let _ = write!(
                text,
                "\n  {} — {}\n    si ferma: {}",
                need.tool,
                need.reason,
                need.asked_by.join(", ")
            );
            if !need.note.is_empty() {
                let _ = write!(text, "\n    da dove si prende: {}", need.note);
            }
        }
    }
    if !unknown.is_empty() {
        let _ = write!(
            text,
            "\n\nCHIESTI DA UN FLUSSO E SCONOSCIUTI A SAILOR — questi non si riparano installando \
             qualcosa, si riparano scrivendo un descrittore:"
        );
        for need in unknown {
            let _ = write!(
                text,
                "\n  {} — chiesto da {}",
                need.tool,
                need.asked_by.join(", ")
            );
        }
    }
    if !named_binaries.is_empty() {
        let _ = write!(
            text,
            "\n\n{} un binario invece di uno strumento ({}): girano solo dove quel nome è nel \
             percorso di chi esegue, e nessun elenco come questo può accorgersene.",
            count(named_binaries.len(), "passo nomina", "passi nominano"),
            named_binaries.join(", ")
        );
    }
    let _ = write!(text, "\n\nGuardato in:\n  {}", looked_in.join("\n  "));
    text
}
