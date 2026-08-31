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

/// Legge i flussi di una sorgente.
///
/// **PASSA DA QUI ANCHE LA SORGENTE DI SISTEMA, che non è una cartella.** I
/// flussi spediti col prodotto stanno dentro il binario: chiedere il loro
/// elenco a `read_dir` darebbe zero, e chi mostra «dove ho guardato e cosa ho
/// trovato» scriverebbe «di sistema: 0 flussi» accanto a flussi di sistema che
/// stanno girando. Il riconoscimento sta qui e non in chi chiama perché i
/// chiamanti sono più di uno — la finestra conta le voci di ogni sorgente — e un
/// ramo dimenticato là fuori è invisibile.
///
/// Il resto è come è sempre stato, e la ragione sta in `flow::system`: un flusso
/// rotto entra nel registro col suo motivo invece di sparire.
pub fn load_flow_registry(dir: &Path) -> FlowRegistry {
    if flow::system::is_place(dir) {
        return flow::system::builtin_registry();
    }
    flow::system::load_registry(dir)
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

/// Da dove viene un flusso: il tipo vive nel crate del flusso, perché dal
/// 29/08/2026 non è più solo la finestra a chiedersi dove stanno i flussi — lo
/// chiede anche un passo, e due risposte alla stessa domanda sono il difetto che
/// `crates/flow/src/file.rs` racconta di aver già pagato sul formato del file.
pub use flow::system::FlowSource;

/// Tutti i posti in cui si cercano i flussi, nell'ordine in cui si guardano.
///
/// **PERCHÉ PIÙ DI UNO, E PERCHÉ È UN DIFETTO CHE FOSSE UNO SOLO.** Il 29/08/2026
/// la finestra mostrava «nessun flusso» mentre la riga di comando ne eseguiva
/// quattro: la prima guardava nella casa dell'utente, la seconda in `flows/`
/// sotto la cartella di lavoro. Nessuna delle due sbagliava da sola — sbagliava
/// il fatto che ce ne fosse una sola, perché **i due posti servono a due cose
/// diverse**: nella casa stanno i flussi di chi usa Sailor, che valgono ovunque
/// si trovi; nel progetto stanno i flussi di quel progetto, che vanno con lui e
/// non riguardano nessun altro.
///
/// **E LA TERZA È QUELLA CHE FA DI SAILOR UN PRODOTTO.** I flussi di sistema
/// sono spediti dentro il binario: chi installa Sailor su una macchina pulita
/// trova già dei flussi, senza che nessuno gli abbia copiato una cartella. Sono
/// i meno specifici — `di sistema` < `tuoi` < `del progetto` — quindi chi ne
/// vuole uno diverso ne scrive uno con lo stesso nome in casa propria o nel
/// proprio progetto, e vince il suo.
///
/// La regola che governa tutto è l'ordine, e il perché sta in `flow::system`.
pub fn flow_sources() -> Vec<FlowSource> {
    let declared = std::env::var_os("SAILOR_FLOWS").map(PathBuf::from);
    let working = std::env::current_dir().ok();
    flow::system::sources(
        &sailor_home().join("flows"),
        working.as_deref(),
        declared.as_deref().map(Path::new),
    )
}

/// I flussi di tutte le sorgenti, ciascuno con l'origine da cui viene.
///
/// A parità di nome vince l'ultima sorgente, cioè la più specifica, e l'origine
/// resta visibile su ogni riga: una sostituzione silenziosa fa credere di aver
/// modificato un flusso che non è quello che gira.
pub fn load_all_flows(
    sources: &[FlowSource],
) -> Vec<(String, &'static str, Result<flow::FlowFile, String>)> {
    flow::system::load_all(sources)
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
mod flow_sources_tests {
    use super::*;

    /// LA SORGENTE CHE FA DI SAILOR UN PRODOTTO, e la prova sta qui perché è
    /// qui che la finestra la chiede: su una macchina appena installata, senza
    /// che nessuno abbia copiato niente, dei flussi ci sono. Le regole di
    /// precedenza e di sovrascrittura si provano in `flow::system`, dove
    /// vivono.
    #[test]
    fn the_window_always_sees_the_shipped_flows_first() {
        let sources = flow_sources();
        assert_eq!(sources[0].origin, "di sistema");
        assert!(sources[0].is_builtin());
        assert!(
            !load_flow_registry(&sources[0].dir).is_empty(),
            "la sorgente di sistema non è una cartella e va letta dal binario"
        );
    }

    /// IL NUMERO CHE LA FINESTRA MOSTRA ACCANTO A OGNI SORGENTE passa da
    /// `load_flow_registry`. Se la sorgente di sistema rispondesse zero, chi
    /// guarda leggerebbe «di sistema: 0 flussi» accanto a flussi di sistema che
    /// stanno girando, e non avrebbe modo di capire che il conto è sbagliato e
    /// non l'elenco.
    #[test]
    fn counting_the_system_source_gives_the_shipped_flows() {
        assert_eq!(
            load_flow_registry(&FlowSource::builtin().dir).len(),
            flow::system::FLOWS.len()
        );
    }
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

        let truncated = registry.get("flusso-tronco").expect("flusso tronco presente");
        assert!(truncated.is_err(), "il file tronco deve essere marcato come errore");
        let reason_truncated = truncated.as_ref().unwrap_err();
        assert!(
            reason_truncated.contains("non è un flusso valido"),
            "motivo: {reason_truncated}"
        );

        let cyclic = registry.get("flusso-ciclico").expect("flusso ciclico presente");
        assert!(cyclic.is_err(), "il flusso con ciclo deve essere marcato come errore");
        let reason_cyclic = cyclic.as_ref().unwrap_err();
        assert!(
            reason_cyclic.contains("backward dependency") || reason_cyclic.contains("non è un flusso valido"),
            "motivo: {reason_cyclic}"
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
