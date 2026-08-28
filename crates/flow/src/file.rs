//! Il formato di un flusso su disco.
//!
//! PERCHÉ STA QUI. Il 28/08/2026 questo tipo è nato due volte nella stessa
//! notte — in `ui::registry` e in `sailor::flow_cmd` — perché due motori
//! esterni lavoravano in parallelo su perimetri separati e nessuno dei due
//! poteva vedere l'altro. I campi coincidevano per fortuna, non per costruzione:
//! bastava che uno aggiungesse un campo perché la finestra e la riga di comando
//! leggessero due formati diversi chiamandoli con lo stesso nome. Il formato del
//! flusso appartiene al crate del flusso, e chi lo legge lo importa.
//!
//! Ne resta una terza copia che non si può togliere: `desktop/src/flow.ts`, la
//! finestra, che è in un altro linguaggio. Chi cambia questo tipo cambia anche
//! quel file — non c'è compilatore che leghi i due, solo questa riga.
//!
//! Il grafo da solo dichiara le forme e non dice mai *quale* comando o *quale*
//! motore: per questo il file porta anche i valori.

use crate::{Graph, Schedule};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Un flusso dichiarato: il grafo più i valori con cui parte.
///
/// `graph` passa dalla validazione di `Graph` al caricamento — cicli,
/// dipendenze mancanti, tetti a zero e fusioni distruttive vengono rifiutati
/// lì, non a metà esecuzione. `inputs` diventa i `root_inputs` della richiesta:
/// una voce per ogni passo senza dipendenze.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowFile {
    pub id: String,
    pub description: String,
    pub graph: Graph,
    pub inputs: BTreeMap<String, Value>,
    /// Quando il flusso è dovuto, quanto pesa, dove può scrivere.
    ///
    /// FACOLTATIVO PERCHÉ ESISTONO TUTTI E DUE I CASI, e non è un ripiego: un
    /// flusso lanciato a mano non ha una ricorrenza, e dargliene una per forza
    /// vorrebbe dire che qualcosa, prima o poi, lo fa partire da solo. `None`
    /// significa «gira quando qualcuno lo chiede», che è un fatto, non un vuoto
    /// da riempire.
    #[serde(default)]
    pub schedule: Option<Schedule>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Il formato che la finestra e la riga di comando devono leggere allo
    /// stesso modo. Se questa prova cade, le due si sono separate di nuovo.
    #[test]
    fn a_declared_flow_carries_its_graph_and_its_values() {
        let text = r#"{
            "id": "prima-corsa",
            "description": "una verifica sola",
            "graph": {
                "steps": [{
                    "id": "clean",
                    "deps": [],
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"},
                    "when": null,
                    "action": "shell_check",
                    "max_attempts": 1
                }]
            },
            "inputs": { "clean": { "command": "true", "timeout_secs": 5 } }
        }"#;

        let file: FlowFile = serde_json::from_str(text).expect("flusso valido");

        assert_eq!(file.id, "prima-corsa");
        assert_eq!(file.graph.steps().len(), 1);
        assert_eq!(file.inputs["clean"]["command"], "true");
    }

    /// Un grafo nudo non è un flusso: senza `inputs` nessuno sa con che valori
    /// parte, e senza `id` non ha un nome per essere invocato.
    #[test]
    fn a_naked_graph_is_not_a_flow_file() {
        let text = r#"{"steps": []}"#;

        assert!(serde_json::from_str::<FlowFile>(text).is_err());
    }
}
