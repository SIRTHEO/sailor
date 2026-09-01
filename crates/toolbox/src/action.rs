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

/// L'ingresso del passo.
///
/// **LO SCHEMA RESTA CHIUSO**, e un campo in più costa una riga qui: `familia`
/// al posto di `family` deve restare un errore detto a chi ha scritto il passo,
/// non un filtro che sparisce in silenzio. La prova
/// `the_flow_action_rejects_an_input_it_cannot_read` tiene ferma quella metà.
///
/// **PERÒ CHI COMPONE L'INGRESSO NON È SOLO CHI SCRIVE IL FLUSSO**: l'esecutore
/// aggiunge il `workdir` a ogni passo il cui schema dichiarato lo accetterebbe,
/// e `{"type": "any"}` accetta tutto. Guasto misurato il 01/09/2026 sul flusso
/// spedito `strumenti-di-questa-macchina`, che dichiara proprio quello: dentro
/// una cartella con `sailor.json` moriva sempre — `unknown field 'workdir'`,
/// `failure_class: invalid_input` — e fuori da un progetto girava, perché senza
/// radice non c'è niente da offrire. Un campo non dichiarato qui non è «un
/// campo che nessuno usa»: può essere l'esecutore stesso.
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
    /// La cartella da cui contare i `descriptor_paths` scritti relativi.
    ///
    /// **NON LA SCRIVE CHI FA IL FLUSSO: LA METTE L'ESECUTORE**, ed è la radice
    /// del progetto. Dichiararla qui la rende un dato invece che un campo
    /// tollerato e buttato via: un descrittore scritto `.sailor/tools.d/x.json`
    /// si legge dalla radice del progetto e non da dove sta il processo, che è
    /// il guasto 25. Assente — flusso lanciato fuori da un progetto — un
    /// percorso relativo resta relativo, com'era prima.
    #[serde(default)]
    workdir: Option<String>,
}

fn yes() -> bool {
    true
}

/// Un percorso di descrittore, contato dalla cartella giusta.
///
/// L'espansione di `~` e delle variabili viene prima: un `~/x` è assoluto anche
/// se non comincia per `/`, e attaccarlo a una radice ne farebbe un percorso
/// plausibile e sbagliato. Una variabile che non esiste resta scritta com'è
/// (vedi `Machine::expand`), e resta relativa: meglio un file che non si trova
/// col suo nome scritto in chiaro che uno trovato per caso altrove.
fn rooted(machine: &Machine, workdir: Option<&str>, raw: &str) -> PathBuf {
    let expanded = PathBuf::from(machine.expand(raw));
    if expanded.is_absolute() {
        return expanded;
    }
    match workdir {
        Some(root) => PathBuf::from(machine.expand(root)).join(expanded),
        None => expanded,
    }
}

/// Risponde a «cosa posso usare qui?» leggendo i descrittori e guardando la
/// macchina.
pub struct DetectToolsAction;

impl Action for DetectToolsAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
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
            let path = rooted(&machine, spec.workdir.as_deref(), raw);
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
