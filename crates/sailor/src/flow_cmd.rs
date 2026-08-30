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
use serde_json::Value;
use ui::gather::FlowSource;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::Write as IoWrite;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn run(args: &[String]) -> i32 {
    match dispatch(args, &ui::gather::flow_sources()) {
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

fn dispatch(args: &[String], sources: &[FlowSource]) -> Result<String, String> {
    match args {
        [command] if command == "list" => list_flows(sources),
        [command] if command == "due" => due_flows(sources),
        [command, name] if command == "check" => check_flow(sources, name),
        [command, name] if command == "run" => run_flow(sources, name),
        _ => Err(usage()),
    }
}

/// I flussi che questa macchina vede, con l'origine di ciascuno.
///
/// **LA RIGA DI COMANDO E LA FINESTRA DEVONO GUARDARE NEGLI STESSI POSTI.** Fino
/// al 29/08/2026 questo comando leggeva `flows/` sotto la cartella corrente e
/// nient'altro: su una macchina appena installata rispondeva «nessun flusso
/// trovato in flows/» mentre la finestra, dallo stesso binario, ne mostrava due
/// spediti dentro di esso. Due risposte alla stessa domanda non danno un errore
/// da leggere — danno due persone che si dicono cose diverse guardando lo stesso
/// prodotto.
///
/// **UN FLUSSO SI NOMINA, NON SI PERCORRE.** Prima il nome diventava un percorso
/// e serviva un controllo perché non uscisse dalla cartella. Adesso il nome si
/// cerca in un elenco già costruito: un nome che quell'elenco non contiene non
/// apre niente, e non c'è nessun posto da cui scappare.
fn known_flows(sources: &[FlowSource]) -> Vec<(String, &'static str, Result<FlowFile, String>)> {
    ui::gather::load_all_flows(sources)
}

/// Il flusso che si chiama così, con l'origine da cui viene.
fn one_flow(sources: &[FlowSource], name: &str) -> Result<(FlowFile, &'static str), String> {
    let known = known_flows(sources);
    match known.iter().find(|(known, _, _)| known == name) {
        Some((_, origin, Ok(flow))) => Ok((flow.clone(), origin)),
        Some((_, origin, Err(reason))) => Err(format!("il flusso {name} ({origin}) non si carica: {reason}")),
        None => {
            let names: Vec<&str> = known.iter().map(|(name, _, _)| name.as_str()).collect();
            Err(format!(
                "nessun flusso si chiama {name}; quelli che vedo sono: {}",
                if names.is_empty() { "nessuno".to_owned() } else { names.join(", ") }
            ))
        }
    }
}

/// Dove si è guardato, sempre in coda a un elenco vuoto: una lista vuota che non
/// dice dove ha cercato è indistinguibile da un guasto.
fn nothing_found(sources: &[FlowSource]) -> String {
    format!(
        "nessun flusso trovato. Guardato in:\n  {}",
        sources
            .iter()
            .map(|source| format!("{}: {}", source.origin, source.dir.display()))
            .collect::<Vec<_>>()
            .join("\n  ")
    )
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
fn due_flows(sources: &[FlowSource]) -> Result<String, String> {
    let known = known_flows(sources);
    if known.is_empty() {
        return Ok(nothing_found(sources));
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
    for (name, _, entry) in known {
        let Ok(flow) = entry else {
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

fn list_flows(sources: &[FlowSource]) -> Result<String, String> {
    let known = known_flows(sources);
    if known.is_empty() {
        return Ok(nothing_found(sources));
    }
    let mut report = String::new();
    // L'ORIGINE STA NELL'ELENCO, e non è ornamento: due flussi con lo stesso
    // nome in due posti sono uno solo qui dentro — vince il piu' specifico — e
    // chi non vede da dove viene quello che gira modifica l'altro.
    for (name, origin, entry) in known {
        match entry {
            Ok(flow) => {
                let _ = writeln!(
                    report,
                    "{}\t{} passi\t{origin}\t{}",
                    flow.id,
                    flow.graph.steps().len(),
                    flow.description
                );
            }
            Err(error) => {
                let _ = writeln!(report, "{name}\t{origin}\tnon caricabile: {error}");
            }
        }
    }
    report.pop();
    Ok(report)
}

fn check_flow(sources: &[FlowSource], name: &str) -> Result<String, String> {
    let (flow, _) = one_flow(sources, name)?;
    let tools = toolbox::Tools::current();
    let (report, unknown) = check_report(
        &flow,
        &default_registry(open_default_ledger(), None),
        Some(&tools),
    );
    if unknown.is_empty() {
        return Ok(report);
    }
    // IL RAPPORTO SI VEDE ANCHE QUANDO IL FLUSSO È ROTTO. Chi controlla un
    // flusso lo fa per capirlo: rispondere con la sola riga dell'errore
    // costringerebbe a rilanciare il comando per vedere il resto.
    println!("{report}");
    Err(format!(
        "il flusso {} chiede strumenti che nessun descrittore dichiara: {}. \
         Non è «manca su questa macchina»: quei nomi non esistono in nessun catalogo, \
         quindi il flusso non gira da nessuna parte finché non si corregge il nome o \
         non si aggiunge un descrittore in ~/.config/sailor/tools.d/",
        flow.id,
        unknown.join(", ")
    ))
}

/// Il rapporto, e i nomi di strumento che nessun descrittore dichiara.
///
/// **PERCHÉ DUE ESITI E NON UNO.** Un flusso può essere sbagliato in due modi
/// che si somigliano e non lo sono: chiedere uno strumento che qui non è
/// installato — e allora il flusso è sano, gira altrove, e installarlo lo fa
/// girare anche qui — oppure chiedere un nome che nessun catalogo dichiara, e
/// allora è rotto su qualunque macchina e non c'è niente da installare. Prima
/// del 28/08/2026 il controllo non vedeva né l'uno né l'altro: `flow check`
/// chiudeva a zero dicendo «azioni mancanti: nessuna», e il difetto si scopriva
/// solo eseguendo. Solo il secondo caso è un errore; il primo è un avviso,
/// perché un prodotto che gira su macchine diverse non può chiamare rotto un
/// flusso che non è il suo.
fn check_report(
    flow: &FlowFile,
    registry: &ActionRegistry,
    tools: Option<&toolbox::Tools>,
) -> (String, Vec<String>) {
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
    // **CHI CONTROLLA UN FLUSSO DEVE VEDERE COSA PUÒ SCRIVERCI DENTRO.** Il
    // rapporto nominava solo le azioni mancanti, cioè rispondeva a «questo
    // flusso gira?» e non a «cosa posso mettere nel prossimo passo». L'elenco
    // arriva dal registro, non da una copia scritta qui accanto.
    let _ = write!(report, "\nazioni disponibili: {}", registry.names().join(", "));
    if missing.is_empty() {
        report.push_str("\nazioni mancanti: nessuna");
    } else {
        let _ = write!(
            report,
            "\nazioni mancanti: {}",
            missing.into_iter().collect::<Vec<_>>().join(", ")
        );
    }

    let wanted = tools_wanted(&flow.graph);
    let mut unknown = Vec::new();
    match tools {
        // Senza rilevatore non si dichiara niente: un rapporto che tace è
        // meglio di uno che chiama sconosciuto ogni strumento perché non ha
        // avuto modo di guardare.
        None => {}
        Some(tools) => {
            let (declared, undeclared): (Vec<String>, Vec<String>) =
                wanted.into_iter().partition(|id| tools.declares(id));
            unknown = undeclared;
            if !declared.is_empty() {
                let _ = write!(report, "\nstrumenti chiesti: {}", declared.join(", "));
            }
            if !unknown.is_empty() {
                let _ = write!(
                    report,
                    "\nstrumenti che nessun descrittore dichiara: {}",
                    unknown.join(", ")
                );
            }
        }
    }
    (report, unknown)
}

/// Gli strumenti che un flusso chiede per identificativo.
///
/// Legge il campo `tool` di ogni passo, qualunque azione sia: è il nome del
/// campo a dire che quello è un identificativo di strumento, non l'azione che
/// lo porta. Un'azione futura che ne chiedesse uno sarebbe controllata senza
/// che nessuno tocchi questa funzione.
///
/// **CONTA ANCHE I MOTORI DENTRO UNA CATENA.** Dal 29/08/2026 un passo può
/// scrivere `"tool": ["claude-code", "agy"]` invece di un nome solo. Chi legge
/// solo la stringa vede quei passi come se non chiedessero niente, e il
/// controllo chiuderebbe in verde senza aver guardato metà dei motori del
/// flusso: sarebbe il guasto 3 rifatto da capo, con la stessa forma.
fn tools_wanted(graph: &Graph) -> BTreeSet<String> {
    let mut wanted = BTreeSet::new();
    for tool in graph
        .steps()
        .iter()
        .filter_map(|step| step.with.as_ref())
        .filter_map(|with| with.get("tool"))
    {
        match tool {
            Value::String(id) => {
                wanted.insert(id.clone());
            }
            Value::Array(chain) => {
                wanted.extend(chain.iter().filter_map(Value::as_str).map(str::to_owned));
            }
            _ => {}
        }
    }
    wanted
}

// ── il testo di un passo mentre il passo gira ──────────────────────────

/// Da che pipe veniva la riga ancora aperta, se ce n'è una.
///
/// `None` vuol dire «siamo a inizio riga», cioè il prossimo byte vuole un
/// marcatore davanti.
#[derive(Default)]
struct LineState {
    open: Option<actions::Pipe>,
}

/// Riversa i byte così come sono arrivati, anteponendo `[passo · out]` o
/// `[passo · err]` a ogni riga.
///
/// **NON DECODIFICA NIENTE.** I byte del testo escono di peso, nell'ordine in
/// cui sono entrati: una sequenza UTF-8 spezzata fra due letture si ricompone da
/// sé sul terminale, e non c'è nessun punto in cui possa diventare un carattere
/// di sostituzione o far panicare qualcuno. L'unica cosa che questa funzione
/// aggiunge sono i marcatori, che sono ASCII e stanno a inizio riga.
///
/// **UNA RIGA APPARTIENE A UNA PIPE SOLA.** Se stdout ha lasciato una riga
/// aperta e arriva stderr, la riga si chiude prima: altrimenti due testi diversi
/// finirebbero sotto lo stesso marcatore, e chi guarda leggerebbe un errore
/// attribuito all'uscita normale — che è peggio di non vederlo.
fn marked(
    out: &mut impl IoWrite,
    state: &mut LineState,
    step: &str,
    pipe: actions::Pipe,
    bytes: &[u8],
) -> std::io::Result<()> {
    let mut rest = bytes;
    while !rest.is_empty() {
        if state.open.is_some_and(|open| open != pipe) {
            out.write_all(b"\n")?;
            state.open = None;
        }
        if state.open.is_none() {
            write!(out, "[{step} · {}] ", pipe.name())?;
            state.open = Some(pipe);
        }
        match rest.iter().position(|byte| *byte == b'\n') {
            Some(end) => {
                out.write_all(&rest[..=end])?;
                state.open = None;
                rest = &rest[end + 1..];
            }
            None => {
                out.write_all(rest)?;
                rest = &[];
            }
        }
    }
    Ok(())
}

/// Dove finisce il testo dei passi.
///
/// **UNA FABBRICA E NON UNO SCRITTORE SOLO**, perché la presa sul terminale si
/// prende e si lascia a ogni pezzo: tenerla aperta fra una consegna e l'altra
/// bloccherebbe chiunque altro scriva, e i due fili che drenano le pipe
/// consegnano insieme.
type Screenward = Arc<dyn Fn() -> Box<dyn IoWrite> + Send + Sync>;

/// Il destinatario di un passo: scrive sul terminale ciò che il passo dice,
/// mentre lo dice.
struct StepEcho {
    step: String,
    /// Un lucchetto solo per passo: i due fili che drenano stdout e stderr
    /// chiamano insieme, e senza serializzazione le righe si intreccerebbero a
    /// metà — compreso il marcatore.
    state: Mutex<LineState>,
    out: Screenward,
}

impl actions::LiveSink for StepEcho {
    fn chunk(&self, pipe: actions::Pipe, bytes: &[u8]) {
        // Un lucchetto avvelenato non è una ragione per far cadere il passo:
        // qui si sta solo mostrando del testo, e il lavoro vero è altrove.
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        // **SU STDERR, NON SU STDOUT.** Il rapporto finale esce da stdout e non
        // cambia forma: chi lo redirige in un file non deve trovarci dentro il
        // testo dei passi. Quale descrittore sia lo dice la fabbrica, decisa da
        // chi ha costruito il destinatario.
        let mut out = (self.out)();
        let _ = marked(&mut out, &mut state, &self.step, pipe, bytes);
        // Il flush a ogni pezzo è il punto del lavoro: senza, il testo
        // resterebbe fermo in un buffer fino alla fine, cioè il difetto di prima
        // spostato di un metro.
        let _ = out.flush();
    }
}

/// Chi mostra sul terminale il testo dei passi di una corsa.
///
/// Sta qui e non in `actions` perché è una decisione di presentazione: dove il
/// testo va a finire lo sceglie chi compone il programma. Un secondo
/// consumatore — un file, la finestra, il deposito — sarebbe un'altra
/// implementazione di `StepSinks`, non una modifica del crate che esegue.
struct TerminalWatcher {
    out: Screenward,
}

impl TerminalWatcher {
    /// Il terminale vero: stderr.
    fn new() -> Self {
        Self {
            out: Arc::new(|| Box::new(std::io::stderr().lock())),
        }
    }

    /// La stessa catena verso un'altra destinazione.
    ///
    /// **ESISTE PERCHÉ L'ARRIVO DEL TESTO SI POSSA CRONOMETRARE.** Con stderr
    /// cablato dentro `chunk` l'unica verifica possibile era rileggere il codice
    /// e trovarlo convincente — che è esattamente il modo in cui il difetto di
    /// prima è passato: consegnava tutto alla fine e sembrava giusto.
    #[cfg(test)]
    fn writing_to(out: Screenward) -> Self {
        Self { out }
    }
}

impl actions::StepSinks for TerminalWatcher {
    fn sink_for(&self, step: &str) -> Arc<dyn actions::LiveSink> {
        Arc::new(StepEcho {
            step: step.to_owned(),
            state: Mutex::new(LineState::default()),
            out: Arc::clone(&self.out),
        })
    }
}

fn run_flow(sources: &[FlowSource], name: &str) -> Result<String, String> {
    let (flow, _) = one_flow(sources, name)?;
    // IL DEPOSITO PRIMA DEL REGISTRO, e non è un dettaglio d'ordine: i nodi
    // `store_write`/`store_read` lo possiedono, quindi un registro costruito
    // prima non li avrebbe e dichiarerebbe mancanti due azioni che esistono.
    let ledger_dir = default_ledger_dir()?;
    let ledger = Ledger::open(&ledger_dir).map_err(|error| {
        format!(
            "non riesco ad aprire il deposito {}: {error}",
            ledger_dir.display()
        )
    })?;
    // CHI GUARDA È IL TERMINALE, e solo qui: `flow check` non esegue niente e
    // non ha testo da mostrare.
    let registry = default_registry(
        Some(ledger.clone()),
        Some(Arc::new(TerminalWatcher::new()) as Arc<dyn actions::StepSinks>),
    );
    let missing = missing_actions(&flow.graph, &registry);
    if !missing.is_empty() {
        return Err(format!(
            "il flusso {} nomina azioni non registrate: {}",
            flow.id,
            missing.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }

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
    store: &dyn RecordStore,
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

/// Il registro delle azioni sta in `crates/registry`, e ci sta per una ragione
/// misurata: questa lista era scritta anche nel guscio della finestra, le due
/// copie si sono disallineate tre volte, e l'ultima — il 30/08/2026 — ha fatto
/// girare lo stesso flusso in due modi diversi a seconda di chi lo lanciava.
use registry::default_registry;
/// Il deposito predefinito se si apre, `None` se non c'è o non si apre.
///
/// Non riporta l'errore: chi la chiama sta facendo un controllo statico, e un
/// deposito assente non è un guasto del flusso che sta guardando. Chi invece
/// deve *eseguire* apre il deposito da sé e pretende che riesca.
fn open_default_ledger() -> Option<Ledger> {
    let dir = default_ledger_dir().ok()?;
    if !dir.exists() {
        return None;
    }
    Ledger::open(&dir).ok()
}

fn missing_actions(graph: &Graph, registry: &ActionRegistry) -> BTreeSet<String> {
    graph
        .steps()
        .iter()
        .filter(|step| registry.get(&step.action).is_none())
        .map(|step| step.action.clone())
        .collect()
}

fn default_ledger_dir() -> Result<PathBuf, String> {
    ledger::default_directory()
        .ok_or_else(|| "HOME non è definita: non so dove aprire il deposito".to_owned())
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
    use std::time::{Duration, Instant};

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

    /// Un orologio finto che avanza di uno a ogni domanda. Il contatore è
    /// atomico perché l'orologio ora è condiviso fra i fili di un fronte: un
    /// `i64` mutabile qui non compilerebbe, ed è la stessa ragione per cui il
    /// tratto chiede `&self`.
    struct Tick(std::sync::atomic::AtomicI64);

    impl Tick {
        fn new(start: i64) -> Self {
            Tick(std::sync::atomic::AtomicI64::new(start))
        }
    }

    impl Clock for Tick {
        fn now(&self) -> Result<i64, flow::FlowError> {
            Ok(self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1)
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

    // ── il marcatore del testo in diretta ────────────────────────────

    /// Il testo prodotto da `marked`, senza toccare nessun terminale.
    fn marking(step: &str, chunks: &[(actions::Pipe, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut state = LineState::default();
        for (pipe, bytes) in chunks {
            marked(&mut out, &mut state, step, *pipe, bytes).expect("un Vec non fallisce");
        }
        out
    }

    #[test]
    fn every_line_says_which_step_and_which_pipe_it_came_from() {
        let out = marking(
            "prova-le-cose",
            &[
                (actions::Pipe::Stdout, b"prima\nseconda\n"),
                (actions::Pipe::Stderr, b"guasto\n"),
            ],
        );
        assert_eq!(
            String::from_utf8(out).expect("ASCII puro"),
            "[prova-le-cose · out] prima\n\
             [prova-le-cose · out] seconda\n\
             [prova-le-cose · err] guasto\n"
        );
    }

    /// UN PEZZO NON È UNA RIGA: una lettura si ferma dove capita, e il marcatore
    /// va messo a inizio riga, non a inizio pezzo — altrimenti una riga spezzata
    /// in tre ne stamperebbe tre.
    #[test]
    fn a_line_split_across_chunks_gets_one_marker_only() {
        let out = marking(
            "passo",
            &[
                (actions::Pipe::Stdout, b"una riga "),
                (actions::Pipe::Stdout, b"spezzata "),
                (actions::Pipe::Stdout, b"in tre\n"),
            ],
        );
        assert_eq!(
            String::from_utf8(out).expect("ASCII puro"),
            "[passo · out] una riga spezzata in tre\n"
        );
    }

    /// NIENTE TESTO CORROTTO E NIENTE PANICO su una sequenza UTF-8 tagliata a
    /// metà fra due letture: i byte non vengono decodificati mai, escono di peso
    /// nell'ordine in cui sono entrati, e il carattere si ricompone da sé.
    #[test]
    fn a_multibyte_character_split_between_chunks_comes_out_intact() {
        // «però» in UTF-8: la `ò` sono due byte, e qui il taglio cade in mezzo.
        let text = "però".as_bytes();
        let cut = text.len() - 1;
        let out = marking(
            "passo",
            &[
                (actions::Pipe::Stdout, &text[..cut]),
                (actions::Pipe::Stdout, &text[cut..]),
                (actions::Pipe::Stdout, b"\n"),
            ],
        );
        assert_eq!(
            String::from_utf8(out).expect("il testo si ricompone"),
            "[passo · out] però\n"
        );
    }

    /// Una riga appartiene a una pipe sola: se stderr interrompe una riga di
    /// stdout ancora aperta, quella si chiude prima — altrimenti un errore
    /// finirebbe sotto il marcatore dell'uscita normale.
    #[test]
    fn stderr_never_lands_inside_an_open_stdout_line() {
        let out = marking(
            "passo",
            &[
                (actions::Pipe::Stdout, b"a meta"),
                (actions::Pipe::Stderr, b"allarme\n"),
                (actions::Pipe::Stdout, b" e poi\n"),
            ],
        );
        assert_eq!(
            String::from_utf8(out).expect("ASCII puro"),
            "[passo · out] a meta\n\
             [passo · err] allarme\n\
             [passo · out]  e poi\n"
        );
    }

    // ── il testo arriva allo schermo mentre il passo gira ────────────

    /// Uno schermo finto che si comporta come un terminale con buffer: ciò che
    /// gli si scrive resta invisibile finché non gli si chiede il flush.
    ///
    /// **REGISTRA L'ISTANTE SUL FLUSH E NON SULLA `write`**, ed è la scelta che
    /// rende questa prova capace di venire diversa: un registratore che segna
    /// l'ora a ogni scrittura resterebbe verde anche togliendo il flush dal
    /// codice vero, cioè proverebbe la metà che non è in discussione. «Scritto»
    /// e «visibile» sono due fatti distinti, e qui si misura il secondo.
    struct Screen {
        start: Instant,
        pending: Mutex<Vec<u8>>,
        shown: Mutex<Vec<(Duration, Vec<u8>)>>,
    }

    impl Screen {
        fn new(start: Instant) -> Arc<Self> {
            Arc::new(Self {
                start,
                pending: Mutex::new(Vec::new()),
                shown: Mutex::new(Vec::new()),
            })
        }

        fn shown(&self) -> Vec<(Duration, Vec<u8>)> {
            self.shown.lock().expect("nessuno panica qui").clone()
        }

        /// Tutto ciò che è diventato visibile, nell'ordine.
        fn visible_text(&self) -> String {
            let joined: Vec<u8> = self
                .shown()
                .into_iter()
                .flat_map(|(_, bytes)| bytes)
                .collect();
            String::from_utf8_lossy(&joined).into_owned()
        }
    }

    /// La presa che la fabbrica consegna a ogni pezzo: scrive nello schermo
    /// condiviso, così le prese successive continuano lo stesso testo.
    struct ScreenHandle(Arc<Screen>);

    impl IoWrite for ScreenHandle {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .pending
                .lock()
                .expect("nessuno panica qui")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            let mut pending = self.0.pending.lock().expect("nessuno panica qui");
            if pending.is_empty() {
                return Ok(());
            }
            let bytes = std::mem::take(&mut *pending);
            self.0
                .shown
                .lock()
                .expect("nessuno panica qui")
                .push((self.0.start.elapsed(), bytes));
            Ok(())
        }
    }

    /// LA CATENA INTERA, CRONOMETRATA: pipe del figlio → `drain` →
    /// `StepEcho::chunk` → `marked` → scrittura → flush → schermo. Il comando
    /// stampa, dorme quattro secondi, stampa ancora, e non si guarda che alla
    /// fine il testo ci sia — sarebbe verde anche mostrando tutto sulla morte
    /// del figlio — si guarda **quando** la prima riga è diventata visibile.
    ///
    /// Margini larghi come nella prova gemella dentro `actions`: quattro secondi
    /// di sonno contro una soglia di due, perché su una macchina carica non
    /// diventi rossa a caso.
    #[test]
    fn the_terminal_shows_a_step_talking_while_the_step_is_still_running() {
        let start = Instant::now();
        let screen = Screen::new(start);
        let watcher = TerminalWatcher::writing_to({
            let screen = Arc::clone(&screen);
            Arc::new(move || Box::new(ScreenHandle(Arc::clone(&screen))) as Box<dyn IoWrite>)
        });
        let sink = actions::StepSinks::sink_for(&watcher, "un-passo-che-parla");

        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg("echo primo; sleep 4; echo secondo");
        let outcome =
            actions::run_with_timeout_watched(cmd, Duration::from_secs(30), Some(sink.as_ref()));
        let whole = start.elapsed();
        assert!(
            whole >= Duration::from_secs(4),
            "il comando doveva davvero durare quattro secondi, altrimenti la \
             misura non distingue niente: {whole:?}"
        );

        let shown = screen.shown();
        let (when, bytes) = shown
            .first()
            .cloned()
            .expect("qualcosa doveva diventare visibile sullo schermo");
        // IL TEMPO PRIMA DEL CONTENUTO: è l'istante la cosa che questa prova
        // misura, e leggerlo per ultimo nasconderebbe il motivo vero di un rosso.
        assert!(
            when < Duration::from_secs(2),
            "il primo pezzo è diventato visibile dopo {when:?}, cioè con la fine \
             del comando e non mentre girava (durata totale {whole:?})"
        );
        let first = String::from_utf8_lossy(&bytes).into_owned();
        assert!(
            first.contains("[un-passo-che-parla · out] primo"),
            "chi guarda deve sapere passo e pipe già dalla prima riga: {first:?}"
        );
        let all = screen.visible_text();
        assert!(
            all.contains("[un-passo-che-parla · out] secondo"),
            "anche ciò che il passo dice dopo deve arrivare: {all:?}"
        );
        assert!(
            matches!(outcome, actions::RunOutcome::Finished { .. }),
            "doveva finire in tempo"
        );
    }

    #[test]
    fn list_keeps_an_unloadable_flow_visible_with_its_reason() {
        let directory = TestDirectory::new();
        directory.write("buono.flow.json", &flow_json("shell_check", "[]", "{}"));
        directory.write("rotto.flow.json", "{ non-json");

        let report = list_flows(&[FlowSource {
            origin: "di prova",
            dir: directory.0.clone(),
        }])
        .expect("elencare i flussi");

        assert!(report.contains("prova\t1 passi\tdi prova"), "{report}");
        assert!(report.contains("rotto\tdi prova\tnon caricabile:"), "{report}");
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

    /// Un catalogo deciso dalla prova, così l'esito non dipende da cosa è
    /// installato su chi la esegue.
    fn tools_declaring(ids: &[&str]) -> toolbox::Tools {
        let voci: Vec<String> = ids
            .iter()
            .map(|id| {
                format!(
                    r#"{{"id":"{id}","family":"tool","label":"{id}","detect":{{"command":"{id}"}}}}"#
                )
            })
            .collect();
        let file = std::env::temp_dir().join(format!("prova-strumenti-{}.json", ids.join("-")));
        std::fs::write(&file, format!(r#"{{"tools":[{}]}}"#, voci.join(","))).expect("scrivere");
        let catalog = toolbox::Catalog::load(&[toolbox::Source::File(file)]);
        toolbox::Tools::new(catalog, toolbox::Machine::current())
    }

    fn flow_wanting_tool(tool: &str) -> FlowFile {
        let json = format!(
            r#"{{
                "id": "prova",
                "description": "flusso di prova",
                "graph": {{
                    "steps": [{{
                        "id": "root",
                        "deps": [],
                        "action": "external_engine",
                        "max_attempts": 1,
                        "when": null,
                        "with": {{"tool": "{tool}", "timeout_secs": 10}},
                        "input_schema": {{"type": "any"}},
                        "output_schema": {{"type": "any"}}
                    }}],
                    "skippable_dependencies": []
                }},
                "inputs": {{}}
            }}"#
        );
        serde_json::from_str(&json).expect("caricare il flusso")
    }

    /// Il difetto misurato il 28/08/2026: `flow check` chiudeva a zero dicendo
    /// «azioni mancanti: nessuna» su un flusso che nominava uno strumento
    /// inesistente, e il guasto si scopriva solo eseguendo.
    #[test]
    fn a_tool_no_catalogue_declares_is_named_by_the_check() {
        let flow = flow_wanting_tool("questo-non-esiste-in-nessun-catalogo");
        let tools = tools_declaring(&["git"]);

        let (report, unknown) = check_report(&flow, &default_registry(None, None), Some(&tools));

        assert_eq!(unknown, vec!["questo-non-esiste-in-nessun-catalogo"]);
        assert!(
            report.contains("strumenti che nessun descrittore dichiara: questo-non-esiste-in-nessun-catalogo"),
            "{report}"
        );
    }

    /// L'altra metà, ed è quella che rende il prodotto adottabile: uno
    /// strumento **dichiarato** non è un difetto, nemmeno quando su questa
    /// macchina non è installato. Un flusso scritto altrove non è un flusso
    /// rotto, e chiamarlo tale renderebbe inutilizzabile ogni flusso condiviso.
    #[test]
    fn a_declared_tool_is_reported_but_never_an_error() {
        let flow = flow_wanting_tool("strumento-dichiarato-mai-installato");
        let tools = tools_declaring(&["strumento-dichiarato-mai-installato"]);

        let (report, unknown) = check_report(&flow, &default_registry(None, None), Some(&tools));

        assert!(unknown.is_empty(), "non è un errore: {unknown:?}");
        assert!(
            report.contains("strumenti chiesti: strumento-dichiarato-mai-installato"),
            "{report}"
        );
    }

    /// Senza rilevatore il rapporto tace sugli strumenti invece di chiamarli
    /// tutti sconosciuti: non aver potuto guardare non è aver visto che manca.
    #[test]
    fn without_a_detector_the_check_says_nothing_about_tools() {
        let flow = flow_wanting_tool("qualunque");

        let (report, unknown) = check_report(&flow, &default_registry(None, None), None);

        assert!(unknown.is_empty());
        assert!(!report.contains("strument"), "{report}");
    }

    #[test]
    fn check_reports_steps_dependencies_and_every_missing_action() {
        let json = flow_json("azione_assente", "[]", "{}");
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");

        let (report, _) = check_report(&flow, &default_registry(None, None), None);

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

        let (report, _) = check_report(&flow, &default_registry(None, None), None);

        assert!(report.contains("dipendenze: 1"), "{report}");
        assert!(report.contains("child <- root"), "{report}");
    }

    #[test]
    fn both_default_actions_are_known_to_check() {
        let registry = default_registry(None, None);
        assert!(registry.get("external_engine").is_some());
        assert!(registry.get("shell_check").is_some());
    }

    /// **CHI INTERROGA LO STORICO C'È ANCHE SENZA DEPOSITO.**
    ///
    /// Il mutante che la fa cadere è spostare `register_history` dentro il ramo
    /// `if let Some(ledger)` insieme ai nodi di `store` — l'errore più facile da
    /// fare in quel punto, perché le due registrazioni si somigliano. `flow
    /// check` direbbe «azione mancante» di un'azione che esiste, e lo direbbe
    /// esattamente sulla macchina appena installata.
    #[test]
    fn the_history_question_is_registered_even_without_a_deposit() {
        let registry = default_registry(None, None);
        assert!(registry.get("history_ask").is_some());
        assert!(
            registry.get("store_write").is_none(),
            "chi scrive resta fuori: senza deposito non ha dove scrivere"
        );
    }

    /// Il rapporto nomina le azioni **disponibili**, non solo quelle mancanti.
    ///
    /// Cade se l'elenco sparisce o se smette di venire dal registro: chi apre
    /// un flusso per capire cosa può metterci dentro leggerebbe una riga
    /// vecchia, o nessuna riga.
    #[test]
    fn the_check_names_the_actions_a_flow_can_use() {
        let json = flow_json("shell_check", "[]", "{}");
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");

        let (report, _) = check_report(&flow, &default_registry(None, None), None);

        assert!(report.contains("azioni disponibili: "), "{report}");
        assert!(report.contains("history_ask"), "{report}");
        assert!(report.contains("external_engine"), "{report}");
    }

    /// Il nodo di ingresso e il rilevatore sono azioni come le altre: un flusso
    /// che li nomina si controlla senza che nessuno le registri a mano.
    #[test]
    fn the_trigger_and_the_detector_are_known_to_check() {
        let registry = default_registry(None, None);
        assert!(registry.get("trigger").is_some());
        assert!(registry.get("detect_tools").is_some());
    }

    /// **IL MOTORE REGISTRATO QUI SA RISOLVERE UNO STRUMENTO.** Il mutante che
    /// fa cadere questa prova è togliere la riga che lo sostituisce: il passo
    /// tornerebbe a rispondere «questo motore non ha un modo per risolverlo», e
    /// un flusso che nomina strumenti invece di binari non partirebbe più.
    /// L'identificativo cercato non esiste apposta: la risposta che conta è
    /// *chi* si lamenta, non che lo strumento ci sia.
    #[test]
    fn the_registered_engine_knows_how_to_resolve_a_tool_id() {
        let registry = default_registry(None, None);
        let engine = registry.get("external_engine").expect("il motore è registrato");
        let input = serde_json::json!({
            "tool": "nessuno-strumento-si-chiama-cosi",
            "timeout_secs": 1
        });

        let error = engine
            .execute(&input, &mut SharedState::new())
            .expect_err("quell'identificativo non esiste");

        assert_eq!(error.class, "tool_unavailable", "{}", error.said);
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

    /// I FLUSSI DI CHI USA SAILOR NON SONO UNA FIXTURE. Fino al 28/08/2026
    /// questa prova includeva `flows/prima-corsa.flow.json` a tempo di
    /// compilazione: il giorno in cui la cartella dei flussi è stata svuotata —
    /// un gesto legittimo di chi usa il programma — **il crate ha smesso di
    /// compilare**. Una batteria non può dipendere dai dati dell'utente.
    ///
    /// Quello che la prova voleva dire resta, e vale per tutti: ogni flusso
    /// presente si carica nella forma decisa e non nomina azioni che il motore
    /// non sa eseguire. Una cartella vuota non è un fallimento — non c'è niente
    /// da verificare — ma non si spaccia per una verifica riuscita: il
    /// conteggio si stampa, così chi legge il verde sa su quanti file è passato.
    #[test]
    fn every_flow_on_disk_loads_and_names_only_registered_actions() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../flows");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            println!("nessuna cartella dei flussi: niente da verificare");
            return;
        };
        let mut checked = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.to_string_lossy().ends_with(".flow.json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("leggere il flusso");
            let flow: FlowFile = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("{} non si carica: {e}", path.display()));
            let unknown = missing_actions(&flow.graph, &default_registry(None, None));
            assert!(
                unknown.is_empty(),
                "{} nomina azioni che il motore non conosce: {unknown:?}",
                path.display()
            );
            checked += 1;
        }
        println!("flussi verificati: {checked}");
    }

    /// La forma decisa del file, su una fixture nostra: qui la prova deve
    /// fallire se cambia il formato, non se qualcuno cancella un file suo.
    #[test]
    fn the_decided_file_shape_still_loads() {
        let inputs = r#"{"solo":{"command":"true","env":{},"timeout_secs":1}}"#;
        let json = flow_json("shell_check", "[]", inputs);
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare la forma decisa");
        assert_eq!(flow.graph.steps().len(), 1);
        assert!(missing_actions(&flow.graph, &default_registry(None, None)).is_empty());
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
            &default_registry(None, None),
            &mut Tick::new(0),
        )
        .expect("eseguire il flusso");

        assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
        assert_eq!(store.all().len(), 1);
        assert_eq!(store.all()[0].input, flow.inputs["root"]);
    }

    /// UN NOME NON DIVENTA PIÙ UN PERCORSO, e la protezione cambia di natura:
    /// prima `../segreto` veniva unito alla cartella e serviva un controllo che
    /// lo rifiutasse; adesso il nome si cerca in un elenco già costruito, quindi
    /// non apre niente perché non c'è niente che si chiami così. La prova resta
    /// perché la garanzia deve restare: nessun nome deve poter far leggere un
    /// file che non è un flusso di questa macchina.
    #[test]
    fn a_name_that_is_not_a_known_flow_opens_nothing() {
        let directory = TestDirectory::new();
        directory.write("buono.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = [FlowSource {
            origin: "di prova",
            dir: directory.0.clone(),
        }];

        for name in ["../segreto", "cartella/segreto", "", "..", "buono.flow.json"] {
            let refused = one_flow(&sources, name).expect_err(&format!(
                "«{name}» non è un flusso di questa macchina e non deve aprirsi"
            ));
            assert!(
                refused.contains("nessun flusso si chiama"),
                "«{name}»: {refused}"
            );
        }
        assert!(one_flow(&sources, "buono").is_ok(), "il flusso vero si apre");
    }

    /// I FLUSSI SPEDITI SI VEDONO ANCHE DALLA RIGA DI COMANDO. Il difetto che
    /// questa prova esiste per prendere: `sailor flow list` rispondeva «nessun
    /// flusso» su una macchina appena installata mentre la finestra, dallo
    /// stesso binario, ne mostrava due.
    #[test]
    fn the_command_line_sees_the_shipped_flows_too() {
        let report = list_flows(&[FlowSource::builtin()]).expect("elencare i flussi");
        for (name, _) in flow::system::FLOWS {
            assert!(report.contains(name), "manca «{name}» in:\n{report}");
        }
        assert!(report.contains("di sistema"), "{report}");
    }
}
