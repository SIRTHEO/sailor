//! Il ponte fra il deposito su disco e i conti puri. `Ledger::open` crea la
//! cartella e i due file `.db` se mancano: aprirla solo per guardarla
//! lascerebbe una traccia che nessun flusso ha mai prodotto. Per questo si
//! controlla prima che il deposito esista già, e solo allora si apre.

use crate::parse::{parse_model_calls, parse_runs};
use crate::registry::FlowRegistry;
use flow::StepRecord;
use ledger::{Ledger, ModelCallRecord, RunRecord};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

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

/// Legge un grafo per ogni `*.json` nella cartella (nome del file = nome del
/// flusso). Non è un meccanismo imposto dal deposito: `flow::Graph` è già
/// serializzabile, ed è il modo più semplice per registrare "i flussi che
/// esistono" senza dipendere da nessun altro crate. Una cartella assente o
/// un file che non si legge come `Graph` viene saltato in silenzio: la
/// pagina non deve rompersi perché un file è a metà scritto.
pub fn load_flow_registry(dir: &Path) -> FlowRegistry {
    let mut registry = FlowRegistry::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return registry;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(graph) = serde_json::from_str::<flow::Graph>(&text) else {
            continue;
        };
        registry.insert(name.to_owned(), graph);
    }
    registry
}
