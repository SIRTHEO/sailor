//! L'innesco come passo di flusso.
//!
//! **COSA FA DAVVERO.** Legge quale sorgente il passo ha dichiarato, la cerca
//! nell'elenco dei descrittori, e — se quella sorgente porta il segnale con sé —
//! restituisce il segnale nella forma che i passi a valle sanno leggere. Non
//! esegue niente e non tocca il mondo.
//!
//! **DOVE SI FERMA, E PERCHÉ SI FERMA INVECE DI FINGERE.** Una sorgente da
//! terminale non viene ascoltata: il passo si rompe con un messaggio che dice
//! cosa manca. Le due cose che mancano non stanno in questa azione — non c'è
//! nessun processo di Sailor che resti in piedi ad aspettare un segnale, e non
//! c'è nessun lettore che tenga un cursore su una sessione — quindi qualunque
//! cosa questa azione restituisse sarebbe inventata. Un flusso verde direbbe
//! che qualcuno ha parlato quando non ha parlato nessuno, ed è il difetto
//! peggiore fra quelli possibili qui: costa chiamate vere a valle.

use crate::{default_sources, Catalog, Kind, Listen, Signal, Source, TriggerDescriptor};
use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use toolbox::Machine;

/// Il nome sotto cui l'azione si registra.
pub const TRIGGER_ACTION: &str = "trigger";

pub fn register_default(registry: &mut flow::ActionRegistry) {
    registry.register(TRIGGER_ACTION, TriggerAction);
}

#[derive(Debug, Deserialize)]
struct TriggerSpec {
    /// L'`id` del descrittore della sorgente. Obbligatorio: «da dove arriva il
    /// lavoro» non ha un valore predefinito ragionevole.
    source: String,
    /// Il testo della consegna, per una sorgente che lo porta con sé.
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    who: Option<String>,
    #[serde(default, rename = "where")]
    where_from: Option<String>,
    /// I file o le cartelle di descrittori da usare oltre a quelli abituali.
    #[serde(default)]
    descriptor_paths: Vec<String>,
    /// Se aggiungersi a quelli abituali o sostituirli.
    #[serde(default = "yes")]
    include_defaults: bool,
}

fn yes() -> bool {
    true
}

/// Il nodo di ingresso di un flusso: attende un segnale e lo mette a
/// disposizione dei passi a valle.
pub struct TriggerAction;

impl Action for TriggerAction {
    fn execute(
        &self,
        input: &Value,
        _shared: &SharedState,
    ) -> Result<ActionOutcome, ActionError> {
        let spec: TriggerSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let machine = Machine::current();
        let mut sources: Vec<Source> = if spec.include_defaults {
            default_sources(&machine)
        } else {
            Vec::new()
        };
        for raw in &spec.descriptor_paths {
            let path = PathBuf::from(machine.expand(raw));
            if path.is_dir() {
                sources.push(Source::Dir(path));
            } else {
                sources.push(Source::File(path));
            }
        }
        let catalog = Catalog::load(&sources);
        let Some(loaded) = catalog.find(&spec.source) else {
            // Un elenco che non dice cosa contiene costringe a cercare il file
            // per sapere come si scrive la riga giusta.
            let known = catalog.known();
            let known = if known.is_empty() {
                "nessuna sorgente è accesa".to_owned()
            } else {
                format!("le sorgenti accese sono: {}", known.join(", "))
            };
            let mut said = format!(
                "il passo chiede la sorgente di segnale «{}», che non è dichiarata da nessun descrittore; {known}",
                spec.source
            );
            for problem in &catalog.problems {
                said.push_str(&format!(
                    "\n(un descrittore non si è caricato: {} in {} — {})",
                    problem.about, problem.source, problem.reason
                ));
            }
            return Err(ActionError::new("unknown_trigger_source", said));
        };
        let descriptor = &loaded.descriptor;
        match descriptor.kind {
            Kind::Manual => {
                let text = spec.text.ok_or_else(|| {
                    ActionError::new(
                        "empty_signal",
                        format!(
                            "l'innesco «{}» porta il segnale con sé, ma il passo non gli ha dato nessun `text`: chi lancia deve mettere lì la consegna",
                            descriptor.id
                        ),
                    )
                })?;
                let signal = Signal {
                    text,
                    who: spec.who.unwrap_or_default(),
                    where_from: spec.where_from.unwrap_or_default(),
                    source: descriptor.id.clone(),
                    kind: "manual".to_owned(),
                };
                Ok(ActionOutcome::Went(serde_json::to_value(signal).expect(
                    "un segnale di soli testi si serializza sempre",
                )))
            }
            Kind::Terminal => Err(ActionError::new(
                "listening_not_built",
                not_listening_yet(descriptor),
            )),
        }
    }

    /// Rifare un innesco manuale è sicuro: rimette in forma ciò che gli è stato
    /// dato, e non tocca niente. Il giorno in cui un innesco *consumerà* un
    /// segnale — togliendolo da una coda — quel giorno la specie cambia, perché
    /// rifarlo salterebbe una consegna.
    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

/// Il confine, scritto per intero dentro il messaggio: chi lo legge deve sapere
/// cosa costruire, non solo che manca qualcosa.
fn not_listening_yet(descriptor: &TriggerDescriptor) -> String {
    let where_it_would_look = match &descriptor.listen {
        Some(Listen::AppendedLines { files, .. }) => {
            format!("guarderebbe le righe nuove di {}", files.join(", "))
        }
        Some(Listen::CursorCommand {
            tool,
            args,
            cursor_argument,
        }) => format!(
            "chiamerebbe lo strumento «{tool}» con {} e il cursore in {cursor_argument}",
            args.join(" ")
        ),
        // Il caricamento lo impedisce; se ci si arriva, il guasto è lì.
        None => "non dichiara dove guardare".to_owned(),
    };
    let missing_reader = match &descriptor.listen {
        Some(Listen::AppendedLines { files, .. }) => format!(
            "un lettore che tenga un cursore su {} e riconosca una riga nuova senza perdere ciò che è comparso mentre nessuno guardava",
            files.join(", ")
        ),
        Some(Listen::CursorCommand { tool, .. }) => format!(
            "un lettore che invochi «{tool}» e conservi fra una corsa e l'altra il punto già letto",
        ),
        None => "un lettore".to_owned(),
    };
    let mut said = format!(
        "l'innesco «{}» ascolta un terminale, e Sailor non sa ancora ascoltare: {where_it_would_look}. \
         Perché diventi vero mancano due cose, e nessuna delle due sta in questo passo: \
         (1) un processo che resti in piedi — `sailor flow run` esegue il grafo una volta e finisce, \
         quindi non c'è nessuno che aspetti un segnale e faccia partire una corsa quando arriva; \
         (2) {missing_reader}. \
         Fino ad allora l'unica sorgente che funziona è quella manuale, che porta il segnale con sé.",
        descriptor.id
    );
    if !descriptor.note.is_empty() {
        said.push_str(&format!(" Nota del descrittore: {}", descriptor.note));
    }
    said
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fire(input: Value) -> Result<Value, ActionError> {
        match TriggerAction.execute(&input, &mut SharedState::new())? {
            ActionOutcome::Went(output) => Ok(output),
            ActionOutcome::Waiting(reason) => panic!("nessun innesco resta in attesa: {reason}"),
        }
    }

    /// **IL NODO DI INGRESSO È VERO.** Il testo che il segnale portava esce nel
    /// campo che i passi a valle leggono, insieme a chi l'ha mandato e da dove.
    #[test]
    fn a_manual_signal_hands_down_what_it_carried() {
        let output = fire(json!({
            "source": "manual",
            "text": "trova i residui di configurazione",
            "who": "theo",
            "where": "la finestra"
        }))
        .expect("l'innesco manuale è spedito col prodotto");

        assert_eq!(output["text"], "trova i residui di configurazione");
        assert_eq!(output["who"], "theo");
        assert_eq!(output["where"], "la finestra");
        assert_eq!(output["source"], "manual");
        assert_eq!(output["kind"], "manual");
    }

    /// I campi che la sorgente non sa restano testi vuoti, mai assenti: il
    /// passo dopo unisce testo con `$join`, e un valore assente romperebbe lui
    /// invece dell'innesco.
    #[test]
    fn a_signal_that_does_not_know_who_sent_it_still_answers_with_texts() {
        let output = fire(json!({"source": "manual", "text": "vai"})).expect("basta il testo");

        assert_eq!(output["who"], "");
        assert_eq!(output["where"], "");
        assert!(output["who"].is_string() && output["where"].is_string());
    }

    #[test]
    fn a_manual_trigger_without_a_text_says_who_should_have_given_it() {
        let error = fire(json!({"source": "manual"})).expect_err("un segnale vuoto non è un segnale");
        assert_eq!(error.class, "empty_signal");
        assert!(error.said.contains("text"), "{}", error.said);
    }

    /// **IL CONFINE, PROVATO.** Una sorgente da terminale non risponde con un
    /// segnale finto: rompe il passo e dice cosa manca. Il mutante che fa
    /// cadere questa prova è far tornare un segnale vuoto invece dell'errore —
    /// ed è esattamente il difetto da cui la prova difende, perché un segnale
    /// vuoto fa partire i motori a valle e costa chiamate vere.
    #[test]
    fn a_terminal_source_refuses_to_pretend_it_listened() {
        let error = fire(json!({"source": "sailor-terminal"}))
            .expect_err("nessuno ascolta un terminale, oggi");

        assert_eq!(error.class, "listening_not_built");
        assert!(error.said.contains("resti in piedi"), "{}", error.said);
        assert!(error.said.contains("cursore"), "{}", error.said);
    }

    /// Le due voci di terminale di questa macchina sono un elenco, non un ramo
    /// di codice: si comportano tutte e due allo stesso modo, e ognuna porta il
    /// proprio posto dentro il messaggio.
    #[test]
    fn every_shipped_terminal_source_stops_at_the_same_border() {
        let catalog = Catalog::load(&[Source::Builtin]);
        let terminals: Vec<String> = catalog
            .live()
            .into_iter()
            .filter(|loaded| loaded.descriptor.kind == Kind::Terminal)
            .map(|loaded| loaded.descriptor.id.clone())
            .collect();
        assert_eq!(terminals.len(), 2, "le due voci misurate: {terminals:?}");
        for id in terminals {
            let error = fire(json!({"source": id})).expect_err("nessuna delle due ascolta");
            assert_eq!(error.class, "listening_not_built");
            assert!(error.said.contains(&id), "{}", error.said);
        }
    }

    #[test]
    fn an_unknown_source_lists_the_ones_that_exist() {
        let error = fire(json!({"source": "il-citofono", "text": "x"}))
            .expect_err("nessun descrittore la dichiara");

        assert_eq!(error.class, "unknown_trigger_source");
        assert!(error.said.contains("il-citofono"), "{}", error.said);
        assert!(error.said.contains("manual"), "{}", error.said);
    }

    /// **CHI USA SAILOR RIEMPIE L'ELENCO DIVERSAMENTE**, e non deve ricompilare
    /// niente: un file di descrittori suo, e la sua sorgente esiste.
    #[test]
    fn a_source_declared_by_the_user_works_without_recompiling() {
        let dir = std::env::temp_dir().join(format!("sailor-trigger-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("creare la cartella di prova");
        let file = dir.join("miei.json");
        std::fs::write(
            &file,
            r#"[{"id": "il-citofono", "kind": "manual", "label": "Il citofono"}]"#,
        )
        .expect("scrivere i descrittori di prova");

        let output = fire(json!({
            "source": "il-citofono",
            "text": "aprimi",
            "descriptor_paths": [file.to_string_lossy()],
            "include_defaults": false
        }))
        .expect("la sorgente dichiarata dall'utente esiste");

        assert_eq!(output["source"], "il-citofono");
        assert_eq!(output["text"], "aprimi");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
