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

/// La casa di Sailor: dove vivono i flussi, il deposito e la configurazione.
///
/// NIENTE PERCORSI DI UNA PERSONA SOLA. Fino al 28/08/2026 le due funzioni qui
/// sotto nominavano le cartelle di chi sviluppa Sailor — `~/.claude/state`,
/// `~/personal/sailor` — e ripiegavano sul suo nome utente. Chi avesse
/// installato il prodotto si sarebbe portato dietro la macchina di un altro:
/// **un prodotto che conosce una casa sola non è un prodotto**.
///
/// La casa si scopre come la scopre qualunque programma su questo sistema, e
/// questa macchina torna a essere quello che è — **un caso configurato**, che
/// dichiara `SAILOR_HOME` nel comando che apre la finestra, non un caso scritto
/// nel codice.
///
/// I gradini, dal più esplicito al più generale. L'ultimo è la cartella
/// corrente e non un percorso inventato: se `HOME` non c'è, il posto meno
/// sbagliato è dove il programma è stato avviato, e chi guarda se ne accorge
/// subito — mentre un percorso plausibile ma altrui fa credere che i dati siano
/// spariti.
/// **La scoperta vive in `ledger`, non qui**, ed è la correzione di un difetto
/// che questa funzione stava per introdurre: il deposito lo apre chi esegue i
/// flussi, e se la finestra si costruisse la propria idea di dove sta la casa,
/// i due guarderebbero posti diversi senza che nessuno dei due dica di
/// sbagliare. Qui resta solo il ripiego per quando l'ambiente non dichiara
/// nemmeno la cartella dell'utente: si resta dove il programma è stato avviato,
/// che si vede subito, invece di inventare un percorso plausibile.
pub fn sailor_home() -> PathBuf {
    ledger::sailor_home().unwrap_or_else(|| PathBuf::from("."))
}

/// Dove vive il deposito: gli eventi e la proiezione delle corse.
///
/// `SAILOR_LEDGER` lo sposta da solo, per chi tiene lo stato altrove — un disco
/// diverso, una cartella sincronizzata, un deposito condiviso fra due macchine.
pub fn default_ledger_dir() -> PathBuf {
    ledger::default_directory().unwrap_or_else(|| sailor_home().join("ledger"))
}

/// Dove stanno i flussi dichiarati.
///
/// IL DIFETTO CHE QUESTA FUNZIONE CHIUDE, misurato il 28/08/2026: la pagina
/// rispondeva `"flows": []` e nessuno sapeva perché. Cercava i flussi **accanto
/// al deposito** (`<ledger_dir>/flows`, cioè `~/.claude/state/flussi/flows`),
/// una cartella che non è mai esistita; i quattordici flussi veri stanno nei
/// sorgenti, in `~/personal/sailor/flows/`. L'elenco vuoto non era un errore da
/// leggere: era la risposta esatta a una domanda posta nel posto sbagliato.
///
/// **Il deposito è stato, i flussi sono sorgenti.** Tenerli sotto la stessa
/// radice li faceva sembrare la stessa cosa, ed è la ragione dello scambio.
///
/// I flussi stanno nella casa di Sailor, in `flows/`. `SAILOR_FLOWS` li sposta
/// da solo: è il gradino che serve a chi sviluppa Sailor e tiene i flussi
/// nell'albero dei sorgenti mentre la casa è altrove.
pub fn default_flows_dir() -> PathBuf {
    flows_dir_from(
        std::env::var_os("SAILOR_FLOWS").map(PathBuf::from),
        sailor_home(),
    )
}

/// La scelta, senza l'ambiente: si prova questa, non quella sopra.
///
/// Le variabili d'ambiente sono globali al processo e le prove girano in
/// parallelo nello stesso: una prova che le scrivesse rovinerebbe le altre a
/// caso, e chi vede il rosso guarderebbe il modulo sbagliato. Una stringa vuota
/// vale come «non impostata» — è quello che lascia dietro uno script che
/// esporta una variabile senza valore, e trattarla come un percorso manderebbe
/// a cercare i flussi nella radice.
fn flows_dir_from(explicit: Option<PathBuf>, home: PathBuf) -> PathBuf {
    explicit
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| home.join("flows"))
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

    /// IL GUASTO CHE QUESTA PROVA ESISTE PER PRENDERE, misurato il 28/08/2026:
    /// la pagina cercava i flussi accanto al deposito e rispondeva `"flows": []`
    /// senza errore. I flussi stanno nella casa di Sailor, il deposito è un'altra
    /// cosa e sta accanto a loro, non sopra.
    #[test]
    fn the_flows_live_in_their_own_folder_not_inside_the_ledger() {
        let chosen = flows_dir_from(None, PathBuf::from("/casa/sailor"));
        assert_eq!(chosen, PathBuf::from("/casa/sailor/flows"));
        assert!(
            !chosen.starts_with("/casa/sailor/ledger"),
            "la cartella dei flussi non sta dentro il deposito: {}",
            chosen.display()
        );
    }

    /// Il gradino esplicito vince: chi nomina la cartella dei flussi non vuole
    /// che la si deduca dalla casa. Serve a chi sviluppa Sailor e tiene i flussi
    /// nell'albero dei sorgenti mentre la casa sta altrove.
    #[test]
    fn the_explicit_folder_wins_over_the_home() {
        assert_eq!(
            flows_dir_from(Some(PathBuf::from("/qui/i/flussi")), PathBuf::from("/casa/sailor")),
            PathBuf::from("/qui/i/flussi")
        );
    }

    /// Una variabile esportata senza valore non è un percorso: presa alla
    /// lettera manderebbe a cercare i flussi nella radice del disco, e
    /// `read_dir` su `/flows` fallisce in silenzio dando di nuovo un elenco
    /// vuoto — lo stesso guasto da cui si è partiti, con un'altra causa.
    #[test]
    fn an_empty_variable_counts_as_unset() {
        assert_eq!(
            flows_dir_from(Some(PathBuf::new()), PathBuf::from("/casa/sailor")),
            PathBuf::from("/casa/sailor/flows")
        );
    }

    /// CHI GUARDA IL DEPOSITO DEVE GUARDARE DOVE SCRIVE CHI LO RIEMPIE. La
    /// finestra e chi esegue i flussi devono chiedere la casa alla stessa
    /// funzione: due idee di dove sta il deposito non danno un errore, danno una
    /// finestra che dice «nessuna corsa» mentre le corse ci sono.
    #[test]
    fn the_window_asks_the_ledger_where_the_ledger_lives() {
        assert_eq!(
            default_ledger_dir(),
            ledger::default_directory().expect("questa macchina dichiara HOME")
        );
    }
}
