//! `sailor flow`: carica i file dichiarativi da `flows/`, mostra anche quelli
//! guasti, controlla che le azioni nominate esistano ed esegue il grafo nel
//! deposito durevole comune di Sailor.

// Il formato del file vive nel crate del flusso: qui si importa, non si
// ridichiara. Averlo scritto due volte, il 28/08/2026, li ha fatti coincidere
// per fortuna e non per costruzione.
use flow::{
    ActionRegistry, Decision, Execution, ExecutionRequest, Executor, FlowFile, Graph,
    InProcessExecutor, RecordStore, SharedState, SystemClock,
};
use ledger::{Ledger, RunRecord};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const FLOW_SUFFIX: &str = ".flow.json";

pub fn run(args: &[String]) -> i32 {
    let flows_dir = match std::env::current_dir() {
        Ok(directory) => directory.join("flows"),
        Err(error) => {
            eprintln!("sailor flow: non riesco a leggere la cartella corrente: {error}");
            return 1;
        }
    };
    match dispatch(args, &flows_dir) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(message) => {
            eprintln!("sailor flow: {message}");
            1
        }
    }
}

fn dispatch(args: &[String], flows_dir: &Path) -> Result<String, String> {
    match args {
        [command] if command == "list" => list_flows(flows_dir),
        [command] if command == "due" => due_flows(flows_dir),
        [command, name] if command == "check" => check_flow(flows_dir, name),
        [command, name] if command == "run" => run_flow(flows_dir, name),
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "uso: sailor flow <list|due|check <nome>|run <nome>>".to_owned()
}

/// Quali flussi sono dovuti adesso, e quando ciascuno è girato l'ultima volta.
///
/// PERCHÉ QUESTO COMANDO ESISTE PRIMA DI UNO SCHEDULATORE. Finché nessuno sa
/// dire *che cosa dovrebbe girare adesso*, un cron non si può convertire in
/// flusso: si convertirebbe che cosa fa, perdendo quando lo fa. Qui la domanda
/// riceve una risposta che una persona può leggere e smentire — che è il
/// gradino prima di lasciarla eseguire a una macchina.
///
/// L'ora si legge **una volta sola** e si passa a tutti: due flussi giudicati su
/// due istanti diversi non sono confrontabili, e la differenza si vede solo nei
/// casi rari, cioè quando fa più danno.
fn due_flows(flows_dir: &Path) -> Result<String, String> {
    let paths = flow_paths(flows_dir)?;
    if paths.is_empty() {
        return Ok("nessun flusso trovato in flows/".to_owned());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // Un deposito che non c'è ancora non è un errore: nessun flusso è mai
    // girato, quindi sono tutti dovuti — ed è la risposta giusta.
    let last = default_ledger_dir()
        .ok()
        .filter(|dir| dir.join("state.db").exists())
        .and_then(|dir| Ledger::open(&dir).ok())
        .and_then(|ledger| ledger.last_started_at().ok())
        .unwrap_or_default();

    let mut report = String::new();
    let mut due = 0usize;
    let mut unplanned = 0usize;
    for path in paths {
        let name = flow_name(&path);
        let Ok(flow) = load_flow(&path) else {
            let _ = writeln!(report, "{name}\tnon caricabile");
            continue;
        };
        let Some(schedule) = flow.schedule.as_ref() else {
            unplanned += 1;
            continue;
        };
        let last_run = last.get(&flow.id).copied();
        let verdict = if flow::is_due(schedule, last_run, now) {
            due += 1;
            "DOVUTO"
        } else {
            "non ancora"
        };
        let when = match last_run {
            Some(seconds) => format!("ultima corsa {} minuti fa", (now - seconds) / 60),
            None => "mai girato".to_owned(),
        };
        let _ = writeln!(report, "{}\t{verdict}\t{when}", flow.id);
    }
    let _ = write!(
        report,
        "{due} dovuti adesso; {unplanned} senza pianificazione, che partono solo a mano"
    );
    Ok(report)
}

fn list_flows(flows_dir: &Path) -> Result<String, String> {
    let paths = flow_paths(flows_dir)?;
    if paths.is_empty() {
        return Ok("nessun flusso trovato in flows/".to_owned());
    }
    let mut report = String::new();
    for path in paths {
        let name = flow_name(&path);
        match load_flow(&path) {
            Ok(flow) => {
                let _ = writeln!(
                    report,
                    "{}\t{} passi\t{}",
                    flow.id,
                    flow.graph.steps().len(),
                    flow.description
                );
            }
            Err(error) => {
                let _ = writeln!(report, "{name}\tnon caricabile: {error}");
            }
        }
    }
    report.pop();
    Ok(report)
}

fn check_flow(flows_dir: &Path, name: &str) -> Result<String, String> {
    let path = flow_path(flows_dir, name)?;
    let flow = load_flow(&path)?;
    Ok(check_report(&flow, &default_registry()))
}

fn check_report(flow: &FlowFile, registry: &ActionRegistry) -> String {
    let dependency_count: usize = flow.graph.steps().iter().map(|step| step.deps.len()).sum();
    let missing = missing_actions(&flow.graph, registry);
    let mut report = format!(
        "flusso: {}\ndescrizione: {}\npassi: {}\ncicli: nessuno\ndipendenze: {}",
        flow.id,
        flow.description,
        flow.graph.steps().len(),
        dependency_count
    );
    for step in flow.graph.steps() {
        let dependencies = if step.deps.is_empty() {
            "nessuna".to_owned()
        } else {
            step.deps.join(", ")
        };
        let _ = write!(report, "\n  {} <- {}", step.id, dependencies);
    }
    if missing.is_empty() {
        report.push_str("\nazioni mancanti: nessuna");
    } else {
        let _ = write!(
            report,
            "\nazioni mancanti: {}",
            missing.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    report
}

fn run_flow(flows_dir: &Path, name: &str) -> Result<String, String> {
    let path = flow_path(flows_dir, name)?;
    let flow = load_flow(&path)?;
    let registry = default_registry();
    let missing = missing_actions(&flow.graph, &registry);
    if !missing.is_empty() {
        return Err(format!(
            "il flusso {} nomina azioni non registrate: {}",
            flow.id,
            missing.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }

    let ledger_dir = default_ledger_dir()?;
    let ledger = Ledger::open(&ledger_dir).map_err(|error| {
        format!(
            "non riesco ad aprire il deposito {}: {error}",
            ledger_dir.display()
        )
    })?;
    let run_id = new_run_id(&flow.id)?;
    let started_at = now_secs()?;
    record_run(&ledger, &flow, &run_id, "running", started_at, None, None)?;

    let mut store = ledger.clone();
    let result = execute_flow(
        &flow,
        &run_id,
        &mut store,
        &registry,
        &mut SystemClock,
    );
    match result {
        Ok(execution) => {
            let (status, exit_ok) = execution_status(&execution);
            record_run(
                &ledger,
                &flow,
                &run_id,
                status,
                started_at,
                Some(now_secs()?),
                None,
            )?;
            if exit_ok {
                Ok(format!("flusso {} completato; corsa {run_id}", flow.id))
            } else {
                Err(format!(
                    "flusso {} terminato con stato {status}; corsa {run_id}",
                    flow.id
                ))
            }
        }
        Err(error) => {
            let said = error.to_string();
            record_run(
                &ledger,
                &flow,
                &run_id,
                "failed",
                started_at,
                Some(now_secs()?),
                Some(said.clone()),
            )?;
            Err(format!(
                "esecuzione del flusso {} fallita: {said}; corsa {run_id}",
                flow.id
            ))
        }
    }
}

fn execute_flow(
    flow: &FlowFile,
    run_id: &str,
    store: &mut dyn RecordStore,
    registry: &ActionRegistry,
    clock: &mut dyn flow::Clock,
) -> Result<Execution, flow::FlowError> {
    InProcessExecutor.execute(
        &flow.graph,
        execution_request(flow, run_id),
        store,
        registry,
        clock,
    )
}

fn execution_request(flow: &FlowFile, run_id: &str) -> ExecutionRequest {
    ExecutionRequest {
        run_id: run_id.to_owned(),
        root_inputs: flow.inputs.clone(),
        gates: Vec::new(),
        shared: SharedState::new(),
    }
}

fn execution_status(execution: &Execution) -> (&'static str, bool) {
    match execution.decisions.last() {
        Some(Decision::Complete) => ("complete", true),
        Some(Decision::Waiting(_)) => ("waiting", false),
        Some(Decision::Stopped(_)) => ("stopped", false),
        Some(Decision::Failed(_)) => ("failed", false),
        Some(Decision::Ready(_)) | Some(Decision::Running(_)) | None => ("incomplete", false),
    }
}

fn record_run(
    ledger: &Ledger,
    flow: &FlowFile,
    run_id: &str,
    status: &str,
    started_at: i64,
    ended_at: Option<i64>,
    error: Option<String>,
) -> Result<(), String> {
    ledger
        .record_run(&RunRecord {
            run_id: run_id.to_owned(),
            kind: "flow".to_owned(),
            entity: flow.id.clone(),
            parent_run_id: None,
            started_by: "sailor flow".to_owned(),
            status: status.to_owned(),
            total_cost_micros: 0,
            error,
            started_at,
            ended_at,
        })
        .map_err(|error| format!("non riesco a registrare la corsa {run_id}: {error}"))
}

fn default_registry() -> ActionRegistry {
    let mut registry = ActionRegistry::default();
    actions::register_default(&mut registry);
    registry
}

fn missing_actions(graph: &Graph, registry: &ActionRegistry) -> BTreeSet<String> {
    graph
        .steps()
        .iter()
        .filter(|step| registry.get(&step.action).is_none())
        .map(|step| step.action.clone())
        .collect()
}

fn load_flow(path: &Path) -> Result<FlowFile, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("non riesco a leggere {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("{} non è un flusso valido: {error}", path.display()))
}

fn flow_paths(flows_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = std::fs::read_dir(flows_dir)
        .map_err(|error| format!("non riesco a leggere {}: {error}", flows_dir.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!("non riesco a leggere una voce in {}: {error}", flows_dir.display())
        })?;
        let path = entry.path();
        // Anche una voce illeggibile deve arrivare a `load_flow`: chiedere qui
        // i metadati con `is_file` trasformerebbe il suo errore in un falso e
        // la farebbe sparire dall'elenco senza motivo.
        if path.to_string_lossy().ends_with(FLOW_SUFFIX) {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn flow_path(flows_dir: &Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
    {
        return Err(format!("nome di flusso non valido: {name}"));
    }
    Ok(flows_dir.join(format!("{name}{FLOW_SUFFIX}")))
}

fn flow_name(path: &Path) -> String {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| path.as_os_str().to_string_lossy());
    file_name
        .strip_suffix(FLOW_SUFFIX)
        .unwrap_or(&file_name)
        .to_owned()
}

fn default_ledger_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "HOME non è definita: non so dove aprire il deposito".to_owned())?;
    Ok(PathBuf::from(home).join(".claude/state/flussi"))
}

fn now_secs() -> Result<i64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .map_err(|error| format!("l'orologio di sistema precede Unix epoch: {error}"))
}

fn new_run_id(flow_id: &str) -> Result<String, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| format!("{flow_id}-{}", duration.as_nanos()))
        .map_err(|error| format!("l'orologio di sistema precede Unix epoch: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow::{Clock, InMemoryRecordStore};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "sailor-flow-test-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("creare la cartella di prova");
            Self(path)
        }

        fn write(&self, name: &str, contents: &str) {
            fs::write(self.0.join(name), contents).expect("scrivere il flusso di prova");
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct Tick(i64);

    impl Clock for Tick {
        fn now(&mut self) -> Result<i64, flow::FlowError> {
            self.0 += 1;
            Ok(self.0)
        }
    }

    fn flow_json(action: &str, dependencies: &str, inputs: &str) -> String {
        format!(
            r#"{{
                "id": "prova",
                "description": "flusso di prova",
                "graph": {{
                    "steps": [{{
                        "id": "root",
                        "deps": {dependencies},
                        "action": "{action}",
                        "max_attempts": 1,
                        "when": null,
                        "input_schema": {{"type": "any"}},
                        "output_schema": {{"type": "any"}}
                    }}],
                    "skippable_dependencies": []
                }},
                "inputs": {inputs}
            }}"#
        )
    }

    #[test]
    fn list_keeps_an_unloadable_flow_visible_with_its_reason() {
        let directory = TestDirectory::new();
        directory.write("buono.flow.json", &flow_json("shell_check", "[]", "{}"));
        directory.write("rotto.flow.json", "{ non-json");

        let report = list_flows(&directory.0).expect("elencare i flussi");

        assert!(report.contains("prova\t1 passi"), "{report}");
        assert!(report.contains("rotto\tnon caricabile:"), "{report}");
        assert!(report.contains("rotto.flow.json"), "{report}");
    }

    #[test]
    fn a_cycle_is_rejected_while_loading_the_file() {
        let json = r#"{
            "id": "ciclo",
            "description": "non deve caricarsi",
            "graph": {
                "steps": [
                    {"id":"a","deps":["b"],"action":"shell_check","max_attempts":1,"when":null,"input_schema":{"type":"any"},"output_schema":{"type":"any"}},
                    {"id":"b","deps":["a"],"action":"shell_check","max_attempts":1,"when":null,"input_schema":{"type":"any"},"output_schema":{"type":"any"}}
                ]
            },
            "inputs": {}
        }"#;

        let error = serde_json::from_str::<FlowFile>(json)
            .expect_err("il ciclo deve essere rifiutato");

        assert!(error.to_string().contains("backward dependency"), "{error}");
    }

    #[test]
    fn check_reports_steps_dependencies_and_every_missing_action() {
        let json = flow_json("azione_assente", "[]", "{}");
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");

        let report = check_report(&flow, &default_registry());

        assert!(report.contains("passi: 1"), "{report}");
        assert!(report.contains("cicli: nessuno"), "{report}");
        assert!(report.contains("dipendenze: 0"), "{report}");
        assert!(report.contains("root <- nessuna"), "{report}");
        assert!(report.contains("azioni mancanti: azione_assente"), "{report}");
    }

    #[test]
    fn check_names_each_dependency_not_only_the_total() {
        let json = r#"{
            "id": "dipendenze",
            "description": "rende visibili gli archi",
            "graph": {
                "steps": [
                    {"id":"root","deps":[],"action":"shell_check","max_attempts":1,"when":null,"input_schema":{"type":"any"},"output_schema":{"type":"any"}},
                    {"id":"child","deps":["root"],"action":"shell_check","max_attempts":1,"when":null,"input_schema":{"type":"any"},"output_schema":{"type":"any"}}
                ]
            },
            "inputs": {}
        }"#;
        let flow: FlowFile = serde_json::from_str(json).expect("caricare il flusso");

        let report = check_report(&flow, &default_registry());

        assert!(report.contains("dipendenze: 1"), "{report}");
        assert!(report.contains("child <- root"), "{report}");
    }

    #[test]
    fn both_default_actions_are_known_to_check() {
        let registry = default_registry();
        assert!(registry.get("external_engine").is_some());
        assert!(registry.get("shell_check").is_some());
    }

    #[test]
    fn inputs_become_root_inputs_without_being_changed() {
        let inputs = r#"{"root":{"command":"true","env":{},"timeout_secs":1}}"#;
        let json = flow_json("shell_check", "[]", inputs);
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");

        let request = execution_request(&flow, "corsa-1");

        assert_eq!(request.root_inputs, flow.inputs);
        assert_eq!(request.run_id, "corsa-1");
    }

    #[test]
    fn prima_corsa_uses_the_decided_file_shape_and_only_registered_actions() {
        let flow: FlowFile = serde_json::from_str(include_str!(
            "../../../flows/prima-corsa.flow.json"
        ))
        .expect("caricare il primo flusso reale");

        assert_eq!(flow.id, "prima-corsa");
        assert_eq!(flow.graph.steps().len(), 1);
        assert!(missing_actions(&flow.graph, &default_registry()).is_empty());
        assert!(flow.inputs.contains_key("working-tree-is-clean"));
    }

    #[test]
    fn run_executes_the_registered_action_with_the_declared_input() {
        let inputs = r#"{"root":{"command":"true","env":{},"timeout_secs":1}}"#;
        let json = flow_json("shell_check", "[]", inputs);
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");
        let mut store = InMemoryRecordStore::default();

        let execution = execute_flow(
            &flow,
            "corsa-1",
            &mut store,
            &default_registry(),
            &mut Tick(0),
        )
        .expect("eseguire il flusso");

        assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
        assert_eq!(store.all().len(), 1);
        assert_eq!(store.all()[0].input, flow.inputs["root"]);
    }

    #[test]
    fn a_name_cannot_escape_the_flows_directory() {
        assert!(flow_path(Path::new("/tmp/flows"), "../segreto").is_err());
        assert!(flow_path(Path::new("/tmp/flows"), "cartella/segreto").is_err());
    }
}
