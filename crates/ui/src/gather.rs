//! Il ponte fra il deposito su disco e i conti puri. `Ledger::open` crea la
//! cartella e i due file `.db` se mancano: aprirla solo per guardarla
//! lascerebbe una traccia che nessun flusso ha mai prodotto. Per questo si
//! controlla prima che il deposito esista già, e solo allora si apre.

use crate::parse::{parse_model_calls, parse_runs};
use crate::registry::{FlowFile, FlowRegistry};
use flow::StepRecord;
use ledger::{Ledger, ModelCallRecord, RunRecord};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct GatherError(String);

impl fmt::Display for GatherError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl std::error::Error for GatherError {}

pub struct GatheredData {
    pub runs: Vec<RunRecord>,
    pub steps_by_run: BTreeMap<String, Vec<StepRecord>>,
    pub calls_by_run: BTreeMap<String, Vec<ModelCallRecord>>,
}

/// Vero solo se `state.db` ed `events.db` esistono già: è il segno che
/// qualcosa è davvero girato, non solo che qualcuno ha guardato la pagina.
pub fn ledger_present(dir: &Path) -> bool {
    dir.join("state.db").exists() && dir.join("events.db").exists()
}

pub fn gather(dir: &Path) -> Result<Option<GatheredData>, GatherError> {
    if !ledger_present(dir) {
        return Ok(None);
    }
    let ledger = Ledger::open(dir).map_err(|error| GatherError(error.to_string()))?;
    let dump = ledger
        .projection_dump()
        .map_err(|error| GatherError(error.to_string()))?;
    let runs = parse_runs(&dump);
    let calls = parse_model_calls(&dump);

    let mut steps_by_run = BTreeMap::new();
    for run in &runs {
        let steps = ledger
            .steps(&run.run_id)
            .map_err(|error| GatherError(error.to_string()))?;
        steps_by_run.insert(run.run_id.clone(), steps);
    }

    let mut calls_by_run: BTreeMap<String, Vec<ModelCallRecord>> = BTreeMap::new();
    for call in calls {
        calls_by_run.entry(call.run_id.clone()).or_default().push(call);
    }

    Ok(Some(GatheredData {
        runs,
        steps_by_run,
        calls_by_run,
    }))
}

/// Legge i flussi dichiarativi nella cartella (formato `{ id, description, graph, inputs }`).
///
/// In precedenza i file non leggibili venivano saltati in silenzio con la motivazione
/// che "la pagina non deve rompersi perché un file è a metà scritto". Quella scelta era
/// sbagliata: un file a metà scritto è uno stato transitorio di pochi millisecondi,
/// mentre un file rotto è permanente, e trattarli allo stesso modo fa sparire il secondo
/// per sempre. Chi guarda la finestra vede un elenco corto senza sapere che è corto.
///
/// Ora ogni file `*.flow.json` o `*.json` viene incluso nel registro: se è valido viene
/// caricato come [`FlowFile`], se è illeggibile o malformato viene registrato con il
/// motivo del rifiuto, così la finestra può mostrarlo marcato.
pub fn load_flow_registry(dir: &Path) -> FlowRegistry {
    let mut registry = FlowRegistry::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return registry;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default();
        let is_flow_json = file_name.ends_with(".flow.json");
        let is_json = path.extension().and_then(|ext| ext.to_str()) == Some("json");
        if !is_flow_json && !is_json {
            continue;
        }
        let name = file_name
            .strip_suffix(".flow.json")
            .or_else(|| file_name.strip_suffix(".json"))
            .unwrap_or(&file_name)
            .to_owned();
        if name.is_empty() {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) => {
                registry.insert(
                    name,
                    Err(format!("non riesco a leggere {}: {error}", path.display())),
                );
                continue;
            }
        };
        match serde_json::from_str::<FlowFile>(&text) {
            Ok(flow) => {
                registry.insert(name, Ok(flow));
            }
            Err(error) => {
                registry.insert(
                    name,
                    Err(format!("{} non è un flusso valido: {error}", path.display())),
                );
            }
        }
    }
    registry
}

/// `~/.claude/state/flussi`, o questa macchina se `HOME` non è impostata.
pub fn default_ledger_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/someone".to_owned());
    PathBuf::from(home).join(".claude/state/flussi")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn temp_test_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sailor-ui-gather-test-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("creazione cartella temporanea");
        dir
    }

    #[test]
    fn load_flow_registry_loads_valid_flow_file_with_declarative_schema() {
        let dir = temp_test_dir("valid-flow");
        let flow_content = json!({
            "id": "mio-flusso",
            "description": "Flusso valido di prova",
            "graph": {
                "steps": [{
                    "id": "passo-uno",
                    "deps": [],
                    "action": "shell_check",
                    "max_attempts": 1,
                    "when": null,
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"}
                }],
                "skippable_dependencies": []
            },
            "inputs": {
                "passo-uno": {"command": "echo ok"}
            }
        });
        fs::write(
            dir.join("mio-flusso.flow.json"),
            serde_json::to_string(&flow_content).unwrap(),
        )
        .expect("scrittura file");

        let registry = load_flow_registry(&dir);
        assert_eq!(registry.len(), 1);
        let entry = registry.get("mio-flusso").expect("voce presente");
        let flow = entry.as_ref().expect("flusso valido");
        assert_eq!(flow.id, "mio-flusso");
        assert_eq!(flow.description, "Flusso valido di prova");
        assert_eq!(flow.graph.steps().len(), 1);
        assert_eq!(flow.graph.steps()[0].id, "passo-uno");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_flow_registry_records_broken_flow_with_reason_instead_of_silently_skipping() {
        let dir = temp_test_dir("broken-flow");
        // File JSON non valido (sintassi tronca)
        fs::write(
            dir.join("flusso-tronco.flow.json"),
            r#"{"id": "flusso-tronco", "description": "#,
        )
        .expect("scrittura file tronco");

        // File con ciclo nel grafo
        let cyclic_flow = json!({
            "id": "flusso-ciclico",
            "description": "Flusso con dipendenza circolare",
            "graph": {
                "steps": [
                    {
                        "id": "a",
                        "deps": ["b"],
                        "action": "test",
                        "max_attempts": 1,
                        "when": null,
                        "input_schema": {"type": "any"},
                        "output_schema": {"type": "any"}
                    },
                    {
                        "id": "b",
                        "deps": ["a"],
                        "action": "test",
                        "max_attempts": 1,
                        "when": null,
                        "input_schema": {"type": "any"},
                        "output_schema": {"type": "any"}
                    }
                ],
                "skippable_dependencies": []
            },
            "inputs": {}
        });
        fs::write(
            dir.join("flusso-ciclico.flow.json"),
            serde_json::to_string(&cyclic_flow).unwrap(),
        )
        .expect("scrittura file ciclico");

        let registry = load_flow_registry(&dir);
        // Prima della modifica entrambi venivano ignorati in silenzio e registry.len() era 0
        assert_eq!(registry.len(), 2, "entrambi i flussi rotti devono essere nel registro");

        let tronco = registry.get("flusso-tronco").expect("flusso tronco presente");
        assert!(tronco.is_err(), "il file tronco deve essere marcato come errore");
        let reason_tronco = tronco.as_ref().unwrap_err();
        assert!(
            reason_tronco.contains("non è un flusso valido"),
            "motivo: {reason_tronco}"
        );

        let ciclico = registry.get("flusso-ciclico").expect("flusso ciclico presente");
        assert!(ciclico.is_err(), "il flusso con ciclo deve essere marcato come errore");
        let reason_ciclico = ciclico.as_ref().unwrap_err();
        assert!(
            reason_ciclico.contains("backward dependency") || reason_ciclico.contains("non è un flusso valido"),
            "motivo: {reason_ciclico}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_flow_registry_rejects_naked_graph_format_with_reason() {
        let dir = temp_test_dir("naked-graph");
        let naked = json!({
            "steps": [{
                "id": "nudo",
                "deps": [],
                "action": "test",
                "max_attempts": 1,
                "when": null,
                "input_schema": {"type": "any"},
                "output_schema": {"type": "any"}
            }]
        });
        fs::write(
            dir.join("vecchio-grafo.json"),
            serde_json::to_string(&naked).unwrap(),
        )
        .expect("scrittura file");

        let registry = load_flow_registry(&dir);
        assert_eq!(registry.len(), 1);
        let entry = registry.get("vecchio-grafo").expect("voce presente");
        assert!(
            entry.is_err(),
            "il vecchio formato grafo nudo senza {{ id, description, graph, inputs }} deve essere rifiutato con motivo"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_flow_registry_ignores_non_json_files() {
        let dir = temp_test_dir("non-json");
        fs::write(dir.join("README.md"), "Documentazione").expect("scrittura file");
        fs::write(dir.join(".DS_Store"), "binary data").expect("scrittura file");

        let registry = load_flow_registry(&dir);
        assert!(registry.is_empty(), "i file non JSON non devono entrare nel registro");

        let _ = fs::remove_dir_all(&dir);
    }
}
