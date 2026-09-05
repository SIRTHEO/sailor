//! `sailor flow run` and `sailor flow resume`: one run, its handoffs, and the
//! text of each step while it is running.

use flow::{
    ActionRegistry, Execution, Executor, FlowFile, InProcessExecutor, RecordStore, SystemClock,
};
use ledger::Ledger;
use serde_json::Value;
use std::fmt::Write as _;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use ui::gather::FlowSource;

use super::{default_ledger_dir, missing_actions, new_run_id, now_secs, one_flow};

/// Mette il mandato nell'ingresso del passo di innesco.
///
/// Il passo si riconosce dall'**azione** che nomina, non dal suo identificativo:
/// un flusso può chiamare il proprio innesco come vuole, e cercare un passo di
/// nome «trigger» funzionerebbe solo su quelli scritti finora.
fn put_mandate(flow: &mut FlowFile, text: &str) -> Result<(), String> {
    let trigger = flow
        .graph
        .steps()
        .iter()
        .find(|step| step.action == "trigger")
        .map(|step| step.id.clone())
        .ok_or_else(|| catalogue::say("cli.flow.no_trigger_step", &[("flow", &flow.id)]))?;
    let entry = flow
        .inputs
        .entry(trigger)
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    match entry {
        Value::Object(fields) => {
            fields.insert("text".to_owned(), Value::String(text.to_owned()));
            Ok(())
        }
        other => Err(catalogue::say(
            "cli.flow.trigger_input_not_an_object",
            &[("flow", &flow.id), ("other", &other.to_string())],
        )),
    }
}

/// Chi tiene un passo consegnato: **una scadenza scritta nel record**, non un
/// processo.
///
/// **NON CHIEDE NIENTE AL SISTEMA OPERATIVO, E IL DIVIETO HA UN NUMERO.** È il
/// guasto 12: dentro il perimetro `pgrep` risponde vuoto *senza errore*, e una
/// sorveglianza ha dichiarato «nessun flusso in esecuzione» mentre due giravano.
/// Un agente in un terminale non è comunque figlio di questo processo, e il
/// kernel non lo distingue da nessun altro: la domanda giusta non è «vive
/// ancora?» ma «il tempo che si era dato è passato?».
///
/// **UN RECORD CON UN PID SI DICHIARA TENUTO, SEMPRE.** Quel record l'ha aperto
/// l'esecutore in processo, non una consegna: questa sonda non ha modo di
/// guardare quel processo, e *non so vedere* non è *è morto*. Dichiararlo morto
/// chiuderebbe sotto i piedi di chi lavora un passo che sta girando davvero.
struct HandoffLease {
    now: i64,
}

impl flow::ProcessProbe for HandoffLease {
    fn is_running(&self, record: &flow::StepRecord) -> Result<bool, flow::FlowError> {
        if record.held_by_pid.is_some() {
            return Ok(true);
        }
        let Some(limit) = record
            .input
            .get("handoff_timeout_secs")
            .and_then(Value::as_i64)
        else {
            // Nessuna scadenza leggibile: si tiene. L'ambiguità si conserva,
            // non si chiude dalla parte comoda.
            return Ok(true);
        };
        Ok(self.now < record.started_at.saturating_add(limit))
    }
}

/// Riprende una corsa: prima riconcilia ciò che è rimasto aperto, poi esegue
/// **con lo stesso identificativo**.
///
/// **PERCHÉ È SEPARATO DA `sailor step close`.** Sono due poteri diversi e vanno
/// tenuti separati: `close` **ricorda** — scrive un esito e non spende niente —
/// mentre `resume` **agisce**, apre fronti e paga chiamate a pagamento. Fonderli
/// vorrebbe dire che dichiarare com'è andato un lavoro fa partire il lavoro
/// dopo, cioè che una scrittura nel deposito spende soldi. Chi chiude a mezzanotte
/// non ha chiesto quello.
///
/// **`reconcile` NON ERA MAI STATO ESEGUITO IN PRODUZIONE.** Fino al 31/08/2026
/// lo chiamavano solo le prove: nessun comando del programma ci passava. Questa
/// è la sua prima messa in servizio, ed è il motivo per cui la sonda qui sopra è
/// scritta per non dichiarare morto niente di cui non sa niente.
pub(super) fn resume_run(run_id: &str) -> Result<String, String> {
    let ledger = crate::step_cmd::open_ledger()?;
    let flow = crate::step_cmd::flow_of_run(&ledger, run_id)?;
    resume_run_in(&ledger, &flow, run_id)
}

/// Il corpo di `resume`, col deposito e il flusso dichiarati invece che dedotti
/// da `HOME` e dalla cartella corrente: sono tutti e due globali al processo, e
/// una prova che li scrivesse rovinerebbe le altre a caso.
pub fn resume_run_in(ledger: &Ledger, flow: &FlowFile, run_id: &str) -> Result<String, String> {
    let root = workspace_root();
    announce_root(root.as_deref());
    let mut store = ledger.clone();
    resume_run_with(ledger, flow, run_id, &mut store, root.as_deref())
}

/// The body of `resume`, with the store and the root declared by the caller.
/// The command line resumes through the bare ledger, in the root it stands
/// in; the window resumes through a store that announces every step, and
/// says which root it resumed in, since the ledger does not keep a run's own.
pub fn resume_run_with(
    ledger: &Ledger,
    flow: &FlowFile,
    run_id: &str,
    store: &mut dyn RecordStore,
    root: Option<&Path>,
) -> Result<String, String> {
    let header = ledger
        .run_header(run_id)
        .map_err(|error| format!("cannot read run {run_id}: {error}"))?
        .ok_or_else(|| catalogue::say("cli.step.no_such_run", &[("run_id", run_id)]))?;
    let registry = default_registry(
        Some(ledger.clone()),
        Some(Arc::new(TerminalWatcher::new()) as Arc<dyn actions::StepSinks>),
    );
    // **L'ISTANTE DI PARTENZA È QUELLO DI PRIMA, NON ADESSO.** L'intestazione si
    // riscrive intera a ogni aggiornamento: mettere qui l'ora della ripresa
    // farebbe risultare la corsa partita quando è stata ripresa. Ne dipendono
    // `last_started_at` — cioè quali flussi `sailor flow due` dichiara dovuti — e
    // la durata che la finestra mostra. Una corsa consegnata e ripresa il giorno
    // dopo risulterebbe durata un minuto.
    let started_at = header.started_at;
    let now = now_secs()?;

    let mut clock = SystemClock;
    // THE RESUME GOES THROUGH THE ONE CONSTRUCTOR, like the first run, and
    // keeps the same id: a request built by hand would lose the workspace
    // root in silence, and a new id would redo every step already paid for.
    let request = registry::execution_request(flow, run_id, root);
    // La riconciliazione vede quello che vedrà l'esecuzione: stessa radice,
    // stesso stato condiviso.
    let shared = request.shared.clone();
    let probe = HandoffLease { now };
    let reconciled = InProcessExecutor
        .reconcile(flow::ReconciliationRequest {
            graph: &flow.graph,
            run_id,
            store: &mut *store,
            actions: &registry,
            shared: &shared,
            processes: &probe,
            clock: &mut clock,
        })
        .map_err(|error| format!("cannot reconcile run {run_id}: {error}"))?;

    let mut report = format!("run {run_id} — flow {}", flow.id);
    if !reconciled.still_running.is_empty() {
        let _ = write!(
            report,
            "\n{}",
            catalogue::say(
                "cli.flow.held_deadline_not_passed",
                &[("steps", &reconciled.still_running.join(", "))],
            )
        );
    }
    if !reconciled.closed_as_broke.is_empty() {
        let _ = write!(
            report,
            "\n{}",
            catalogue::say(
                "cli.flow.expired_back_among_the_ready",
                &[("steps", &reconciled.closed_as_broke.join(", "))],
            )
        );
    }
    if !reconciled.closed_as_waiting.is_empty() {
        let _ = write!(
            report,
            "\nleft to a person: {}",
            reconciled.closed_as_waiting.join(", ")
        );
    }

    let execution = InProcessExecutor
        .execute(&flow.graph, request, &*store, &registry, &SystemClock)
        .map_err(|error| format!("resuming run {run_id} failed: {error}"))?;

    let (status, exit_ok) = execution_status(&execution);
    let why =
        registry::stopped_by_cap(&execution).or_else(|| registry::halted_by_hand(&execution));
    record_run(
        ledger,
        flow,
        run_id,
        status,
        started_at,
        Some(now_secs()?),
        why.clone(),
    )?;
    let _ = write!(report, "\nstato: {status}");
    if exit_ok {
        Ok(report)
    } else {
        match why {
            Some(why) => Err(format!("{report}\n{why}")),
            None => Err(report),
        }
    }
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

/// Esegue un flusso, con un mandato facoltativo che entra dall'innesco.
///
/// **PERCHÉ IL MANDATO SI PASSA E NON SI SCRIVE NEL FILE.** Il 31/08/2026, per
/// misurare quanto consuma un flusso rispetto a un prompt solo, serviva dare a
/// tutti e due lo stesso identico incarico. Dalla riga di comando non si poteva:
/// l'unico modo era riscrivere il `.flow.json` a mano prima di ogni corsa —
/// esattamente il guasto 15, «Sailor non ha nessun comando per operare sui
/// propri flussi, quindi chi ci lavora lo aggira». Un flusso il cui incarico si
/// cambia modificando il file non si può nemmeno lanciare due volte di seguito
/// con due incarichi diversi.
///
/// Il testo sostituisce il campo `text` dell'ingresso del passo di innesco, che
/// è dove i flussi già lo mettono. Un flusso senza innesco lo rifiuta dicendolo,
/// invece di ignorarlo in silenzio — che sarebbe il guasto 20 su un'altra porta.
pub(super) fn run_flow(sources: &[FlowSource], name: &str, mandate: Option<&str>) -> Result<String, String> {
    let (mut flow, _) = one_flow(sources, name)?;
    if let Some(text) = mandate {
        put_mandate(&mut flow, text)?;
    }
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

    let store = ledger.clone();
    let result = execute_flow(&flow, &run_id, &store, &registry, &mut SystemClock);
    match result {
        Ok(execution) => {
            let (status, exit_ok) = execution_status(&execution);
            // Il tetto raggiunto porta con sé i numeri: finiscono nella riga
            // della corsa, così chi rilegge lo storico fra una settimana sa
            // quanto era il tetto allora e quanto si era speso.
            let why = registry::stopped_by_cap(&execution);
            record_run(
                &ledger,
                &flow,
                &run_id,
                status,
                started_at,
                Some(now_secs()?),
                why.clone(),
            )?;
            if exit_ok {
                Ok(format!("flusso {} completato; corsa {run_id}", flow.id))
            } else {
                Err(match why {
                    Some(why) => format!("flusso {}: {why}; corsa {run_id}", flow.id),
                    None => format!(
                        "flusso {} terminato con stato {status}; corsa {run_id}",
                        flow.id
                    ),
                })
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
) -> Result<Execution, Box<flow::FlowError>> {
    let root = workspace_root();
    announce_root(root.as_deref());
    InProcessExecutor
        .execute(
            &flow.graph,
            registry::execution_request(flow, run_id, root.as_deref()),
            store,
            registry,
            clock,
        )
        .map_err(Box::new)
}

/// La radice del progetto per questa corsa, risalendo da dove si è lanciato.
pub(super) fn workspace_root() -> Option<PathBuf> {
    let working = std::env::current_dir().ok()?;
    flow::workspace::find_root(&working)
}

/// **CHI LANCIA DICE DOVE HA DECISO DI LAVORARE, PRIMA DI PARTIRE.**
///
/// Senza questa riga il piano ha un modo silenzioso di sbagliare, ed è
/// **lo stesso** del guasto che chiude: il flusso lavora in un posto che
/// nessuno ha visto scritto da nessuna parte. Che la radice manchi è
/// un'informazione quanto il suo valore — dice in anticipo perché un passo con
/// `workdir` sta per fallire.
fn announce_root(root: Option<&Path>) {
    match root {
        Some(root) => println!("radice del progetto: {}", root.display()),
        None => println!(
            "radice del progetto: nessuna (nessun {} risalendo da qui); \
             i passi che dichiarano «workdir» falliranno",
            flow::workspace::MARKER
        ),
    }
}

/// Com'è finita la corsa. Il corpo sta in `registry`, con la sua gemella del
/// guscio: erano due, e un `Decision` nuovo le avrebbe fatte divergere.
fn execution_status(execution: &Execution) -> (&'static str, bool) {
    registry::execution_status(execution)
}

/// Registra l'intestazione della corsa.
///
/// **IL CORPO STA IN `registry`, E NON PER ELEGANZA.** Queste venti righe erano
/// scritte anche nel guscio della finestra, con un commento che lo dichiarava.
/// Fino al 31/08/2026 tutte e due scrivevano `total_cost_micros: 0` a mano su
/// un campo che la finestra mostra: riparare una sola delle due avrebbe dato
/// due totali diversi per la stessa corsa a seconda di chi l'aveva lanciata.
pub(super) fn record_run(
    ledger: &Ledger,
    flow: &FlowFile,
    run_id: &str,
    status: &str,
    started_at: i64,
    ended_at: Option<i64>,
    error: Option<String>,
) -> Result<(), String> {
    registry::record_flow_run(
        ledger,
        flow,
        registry::FlowRun {
            run_id,
            status,
            started_at,
            ended_at,
            error,
            started_by: seat_of(std::env::var_os("SAILOR_TERMINAL").is_some()),
        },
    )
}

/// Which seat this command line runs from: a pane the terminal host opened
/// carries its mark, a bare shell does not. The window names its own seat.
pub(crate) fn seat_of(in_a_sailor_terminal: bool) -> &'static str {
    if in_a_sailor_terminal {
        "sailor flow, in a Sailor terminal"
    } else {
        "sailor flow, in a shell"
    }
}

/// Il registro delle azioni sta in `crates/registry`, e ci sta per una ragione
/// misurata: questa lista era scritta anche nel guscio della finestra, le due
/// copie si sono disallineate tre volte, e l'ultima — il 30/08/2026 — ha fatto
/// girare lo stesso flusso in due modi diversi a seconda di chi lo lanciava.
use registry::default_registry;

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use flow::{Decision, InMemoryRecordStore, ProcessProbe, StepRecord};
    use registry::{registry_in, House};
    use std::time::{Duration, Instant};

    // ── la sonda della consegna ──────────────────────────────────────────

    fn a_handed_record(started_at: i64, limit: Option<i64>, pid: Option<u32>) -> StepRecord {
        let input = match limit {
            Some(limit) => serde_json::json!({"handoff_timeout_secs": limit}),
            None => serde_json::json!({"mandate": "senza scadenza"}),
        };
        let mut record = StepRecord::started(
            "run-1",
            "implementa",
            1,
            1,
            vec![],
            input,
            vec![],
            started_at,
        );
        record.held_by_pid = pid;
        record
    }

    /// La scadenza nel futuro tiene il passo; passata, lo lascia andare.
    #[test]
    fn the_lease_reads_the_deadline_and_not_the_kernel() {
        let probe = HandoffLease { now: 1_000 };
        assert!(
            probe
                .is_running(&a_handed_record(900, Some(600), None))
                .expect("la sonda risponde"),
            "la scadenza è nel futuro: il passo è tenuto"
        );
        assert!(
            !probe
                .is_running(&a_handed_record(100, Some(600), None))
                .expect("la sonda risponde"),
            "la scadenza è passata: nessuno l'ha preso in carico"
        );
    }

    /// **NON SO VEDERE, QUINDI NON DICHIARO MORTO.** Un record con un pid l'ha
    /// aperto l'esecutore in processo; questa sonda non ha modo di guardare quel
    /// processo — e non deve chiederlo al sistema operativo, che è il guasto 12.
    /// Lo stesso vale per un record senza scadenza leggibile.
    #[test]
    fn what_the_lease_cannot_see_it_does_not_declare_dead() {
        let probe = HandoffLease { now: 1_000_000 };
        assert!(
            probe
                .is_running(&a_handed_record(1, Some(1), Some(4321)))
                .expect("la sonda risponde"),
            "con un pid scritto il passo si tiene, anche con la scadenza passata"
        );
        assert!(
            probe
                .is_running(&a_handed_record(1, None, None))
                .expect("la sonda risponde"),
            "senza una scadenza leggibile l'ambiguità si conserva"
        );
    }

    /// The seat a run is started from is written by the system, not told: a
    /// pane of the terminal host and a bare shell are two different rows.
    #[test]
    fn the_seat_of_a_run_names_the_pane_or_the_shell() {
        assert_eq!(seat_of(true), "sailor flow, in a Sailor terminal");
        assert_eq!(seat_of(false), "sailor flow, in a shell");
        assert_ne!(seat_of(true), seat_of(false));
    }

    /// **UNA CONSEGNA VIVA NON SI CHIUDE SOTTO I PIEDI DI CHI CI LAVORA.**
    ///
    /// È il mutante che conta di tutto questo lavoro: una sonda che risponde
    /// sempre «no» fa chiudere `Broke` un passo che qualcuno sta eseguendo, e la
    /// ripresa lo rilancia — due agenti sullo stesso mandato, e nessuno dei due
    /// lo sa.
    #[test]
    fn a_live_handoff_is_not_closed_under_the_agent_who_holds_it() {
        let flow: FlowFile = serde_json::from_str(
            r#"{
                "id": "consegna-viva",
                "description": "un passo consegnato e ancora nei tempi",
                "graph": {"steps": [{
                    "id": "implementa",
                    "deps": [],
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"},
                    "when": null,
                    "action": "handed_to_agent",
                    "max_attempts": 3
                }]},
                "inputs": {}
            }"#,
        )
        .expect("il flusso di prova è valido");

        let mut store =
            InMemoryRecordStore::from_records(vec![a_handed_record(1_000, Some(3_600), None)]);
        let registry = registry_in(House::empty(), None, None);
        let shared = flow::SharedState::new();
        let probe = HandoffLease { now: 1_100 };
        let mut clock = SystemClock;
        let report = InProcessExecutor
            .reconcile(flow::ReconciliationRequest {
                graph: &flow.graph,
                run_id: "run-1",
                store: &mut store,
                actions: &registry,
                shared: &shared,
                processes: &probe,
                clock: &mut clock,
            })
            .expect("la riconciliazione risponde");

        assert_eq!(
            report.still_running,
            vec!["implementa".to_owned()],
            "il passo è tenuto: la scadenza non è passata"
        );
        assert!(
            report.closed_as_broke.is_empty(),
            "chiuderlo lo rimetterebbe fra i pronti mentre qualcuno ci lavora: {report:?}"
        );
        assert!(
            store.all()[0].outcome.is_none(),
            "il record deve restare aperto"
        );
    }

    /// **UNA RIPRESA NON RISCRIVE L'ISTANTE DI PARTENZA.**
    ///
    /// L'intestazione di una corsa si riscrive intera a ogni aggiornamento.
    /// Mettendoci l'ora della ripresa, una corsa consegnata la sera e ripresa il
    /// mattino dopo risulterebbe partita al mattino: `sailor flow due` la
    /// crederebbe appena girata e non dichiarerebbe dovuto il suo flusso, e la
    /// durata mostrata sarebbe un minuto invece di dieci ore.
    #[test]
    fn resuming_a_run_keeps_the_hour_it_started() {
        let home = TestDirectory::new();
        let flow_file: FlowFile = serde_json::from_str(
            r#"{
                "id": "ripresa-di-prova",
                "description": "un passo gia' andato",
                "graph": {"steps": [{
                    "id": "implementa",
                    "deps": [],
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"},
                    "when": null,
                    "action": "handed_to_agent",
                    "max_attempts": 3
                }]},
                "inputs": {}
            }"#,
        )
        .expect("il flusso di prova è valido");

        let ledger = Ledger::open(&home.0).expect("aprire il deposito");
        ledger
            .record_run(&ledger::RunRecord {
                run_id: "run-vecchia".to_owned(),
                kind: "flow".to_owned(),
                entity: "ripresa-di-prova".to_owned(),
                parent_run_id: None,
                started_by: "prova".to_owned(),
                status: "waiting".to_owned(),
                total_cost_micros: 0,
                error: None,
                started_at: 1_000,
                ended_at: Some(1_500),
                worktree: None,
            })
            .expect("registrare la corsa");
        let mut record = StepRecord::started(
            "run-vecchia",
            "implementa",
            1,
            1,
            vec![],
            serde_json::json!({"handoff_timeout_secs": 60}),
            vec![],
            1_100,
        );
        record.species = Some(flow::StepSpecies::Repeatable);
        ledger
            .append_step_started(&record)
            .expect("aprire il passo");
        ledger
            .close_step(
                "run-vecchia",
                "implementa",
                1,
                1,
                flow::Completion {
                    outcome: flow::Outcome::Went,
                    output: Some(serde_json::json!({})),
                    said: None,
                    failure_class: None,
                    refusal: None,
                    ran: None,
                    ended_at: 1_500,
                    bytes_seen: None,
                    bytes_discarded: None,
                },
            )
            .expect("chiudere il passo");

        let report = resume_run_in(&ledger, &flow_file, "run-vecchia")
            .expect("la corsa si riprende: l'unico passo è già andato");
        assert!(report.contains("complete"), "{report}");

        let header = ledger
            .run_header("run-vecchia")
            .expect("l'intestazione si rilegge")
            .expect("la corsa esiste");
        assert_eq!(
            header.started_at, 1_000,
            "l'istante di partenza resta quello della prima corsa: con l'ora della \
             ripresa, una corsa consegnata la sera e ripresa il mattino dopo \
             risulterebbe partita al mattino"
        );
        assert_eq!(header.status, "complete", "la ripresa aggiorna lo stato");
    }

    // ── il mandato che entra dall'innesco ────────────────────────────

    /// **LO STESSO FLUSSO CON DUE MANDATI DIVERSI, SENZA TOCCARE IL FILE.**
    /// Prima l'unico modo era riscrivere il `.flow.json`, quindi due corse di
    /// seguito con due incarichi diversi non erano possibili — ed è il difetto
    /// che ha reso impossibile misurare un flusso contro un prompt.
    #[test]
    fn a_mandate_from_the_command_line_reaches_the_trigger() {
        let json = r#"{
            "id": "prova", "description": "flusso con innesco",
            "graph": {"steps": [{
                "id": "innesco", "deps": [], "action": "trigger", "max_attempts": 1,
                "when": null, "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
            }]},
            "inputs": {"innesco": {"source": "manual", "text": "quello di prima"}}
        }"#;
        let mut flow: FlowFile = serde_json::from_str(json).expect("caricare il flusso");

        put_mandate(&mut flow, "il lavoro di adesso").expect("il mandato entra");

        assert_eq!(flow.inputs["innesco"]["text"], "il lavoro di adesso");
        assert_eq!(
            flow.inputs["innesco"]["source"], "manual",
            "e non porta via il resto dell'ingresso"
        );
    }

    /// Un flusso senza innesco **rifiuta** il mandato invece di ingoiarlo: un
    /// incarico che non arriva da nessuna parte farebbe girare il flusso su
    /// tutt'altro, e chi lo ha scritto crederebbe di averlo indirizzato.
    #[test]
    fn a_flow_without_a_trigger_refuses_the_mandate() {
        let json = flow_json("shell_check", "[]", "{}");
        let mut flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");

        let refused = put_mandate(&mut flow, "un incarico").expect_err("non deve accettarlo");

        assert!(
            refused.contains("has no trigger step"),
            "e dice perché: {refused}"
        );
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
    fn inputs_become_root_inputs_without_being_changed() {
        let inputs = r#"{"root":{"command":"true","env":{},"timeout_secs":1}}"#;
        let json = flow_json("shell_check", "[]", inputs);
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");

        let request = registry::execution_request(&flow, "corsa-1", None);

        assert_eq!(request.root_inputs, flow.inputs);
        assert_eq!(request.run_id, "corsa-1");
    }

    /// **CIÒ CHE IL FLUSSO DICHIARA ARRIVA ALL'AZIONE, PIÙ LA RADICE.**
    ///
    /// Fino al 31/08/2026 questa prova chiedeva l'uguaglianza esatta con
    /// l'ingresso dichiarato. Adesso non vale più, ed è voluto: chi compone
    /// l'ingresso ci aggiunge `workdir`, altrimenti un passo senza cartella
    /// dichiarata girerebbe dove sta il processo — che è il guasto 25. La
    /// prova chiede tutte e due le cose, perché sono due garanzie diverse:
    /// quello che una persona ha scritto non viene toccato, e la radice c'è.
    #[test]
    fn run_executes_the_registered_action_with_the_declared_input_plus_the_root() {
        let inputs = r#"{"root":{"command":"true","env":{},"timeout_secs":1}}"#;
        let json = flow_json("shell_check", "[]", inputs);
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");
        let store = InMemoryRecordStore::default();

        let execution = execute_flow(
            &flow,
            "corsa-1",
            &store,
            &registry_in(House::empty(), None, None),
            &mut Tick::new(0),
        )
        .expect("eseguire il flusso");

        assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
        assert_eq!(store.all().len(), 1);
        let seen = &store.all()[0].input;
        for (field, value) in flow.inputs["root"].as_object().expect("un oggetto") {
            assert_eq!(seen.get(field), Some(value), "«{field}» non deve cambiare");
        }
        assert_eq!(
            seen.get("workdir").and_then(Value::as_str),
            workspace_root()
                .as_deref()
                .map(|root| root.to_str().expect("un percorso leggibile")),
            "la cartella di lavoro è la radice, non dove sta il processo"
        );
    }
}
