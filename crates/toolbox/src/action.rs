//! Il rilevamento come passo di flusso.
//!
//! STESSA FORMA DELLE AZIONI GIÀ REGISTRATE: un nome stabile, un ingresso letto
//! da JSON, un'uscita che è un dato. Non tocca `crates/actions` — un
//! `flow::ActionRegistry` accetta chiunque implementi il tratto, e chi compone
//! il registro decide che cosa metterci. L'aggancio è una riga sola nel punto in
//! cui il registro si costruisce.
//!
//! COSA NON È UN ERRORE DELL'AZIONE. Uno strumento assente, un binario che non
//! risponde, un descrittore sbagliato: sono tutti dati del mondo, e finiscono
//! nell'uscita. L'azione fallisce solo se il suo ingresso non si legge — cioè
//! se il guasto è di chi ha scritto il passo.

use crate::{default_sources, detect, Catalog, Machine, Source};
use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

/// Il nome sotto cui l'azione si registra.
pub const DETECT_TOOLS_ACTION: &str = "detect_tools";

/// Registra l'azione sotto il suo nome stabile.
pub fn register_default(registry: &mut flow::ActionRegistry) {
    registry.register(DETECT_TOOLS_ACTION, DetectToolsAction);
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DetectSpec {
    /// I file o le cartelle di descrittori da usare al posto delle sorgenti
    /// abituali. Vuoto significa: quelli spediti più quelli dell'utente.
    #[serde(default)]
    descriptor_paths: Vec<String>,
    /// Se dichiarata, si aggiungono alle sorgenti abituali invece di
    /// sostituirle. Un passo che vuole solo i propri descrittori mette
    /// `include_defaults: false`.
    #[serde(default = "yes")]
    include_defaults: bool,
    /// Quali cataloghi spediti col prodotto usare, per nome: oggi `tools` e
    /// `automations`. Vuoto significa nessuno *in più* — quello degli strumenti
    /// arriva già da `include_defaults`. Un nome che nessun catalogo porta
    /// diventa una segnalazione nell'uscita, non un elenco vuoto.
    #[serde(default)]
    builtin_catalogs: Vec<String>,
    /// Solo una famiglia: `ai_cli`, `mcp_server`, `tool`, o qualunque altra
    /// parola scritta in un descrittore.
    #[serde(default)]
    family: Option<String>,
    /// Se si può eseguire un binario per chiedergli la versione. Un flusso che
    /// gira dove eseguire è caro lo spegne, e ogni versione diventa «non
    /// chiesta» invece di diventare falsa.
    #[serde(default = "yes")]
    version_probes: bool,
}

fn yes() -> bool {
    true
}

/// Risponde a «cosa posso usare qui?» leggendo i descrittori e guardando la
/// macchina.
pub struct DetectToolsAction;

impl Action for DetectToolsAction {
    fn execute(&self, input: &Value, _shared: &mut SharedState) -> Result<ActionOutcome, ActionError> {
        // Un ingresso assente vale come uno vuoto: il caso più comune — «dimmi
        // cosa c'è» — non deve costringere a scrivere un oggetto di opzioni.
        let spec: DetectSpec = if input.is_null() {
            DetectSpec {
                include_defaults: true,
                version_probes: true,
                ..DetectSpec::default()
            }
        } else {
            serde_json::from_value(input.clone())
                .map_err(|error| ActionError::new("invalid_input", error.to_string()))?
        };
        let mut machine = Machine::current();
        machine.version_probes = spec.version_probes;
        let mut sources: Vec<Source> = if spec.include_defaults {
            default_sources(&machine)
        } else {
            Vec::new()
        };
        for name in &spec.builtin_catalogs {
            sources.push(Source::BuiltinNamed(name.clone()));
        }
        for raw in &spec.descriptor_paths {
            let path = PathBuf::from(machine.expand(raw));
            if path.is_dir() {
                sources.push(Source::Dir(path));
            } else {
                sources.push(Source::File(path));
            }
        }
        let catalog = Catalog::load(&sources);
        let report = detect(&catalog, &machine);
        let findings = match &spec.family {
            Some(family) => report
                .of_family(family)
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            None => report.findings.clone(),
        };
        let present = findings.iter().filter(|f| f.presence.is_present()).count();
        Ok(ActionOutcome::Went(json!({
            "findings": findings,
            "problems": report.problems,
            "looked_in": report.looked_in,
            "present": present,
            "total": findings.len(),
        })))
    }

    /// Rifare un rilevamento è sicuro: legge il mondo e dice com'è. L'unica cosa
    /// che esegue è il comando di versione dichiarato in un descrittore, e il
    /// contratto di quel campo è che sia una domanda, non un gesto — chi ci
    /// mette dentro un comando che cambia qualcosa ha già rotto il contratto,
    /// e lo aveva rotto anche senza nessuna interruzione di mezzo.
    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}
