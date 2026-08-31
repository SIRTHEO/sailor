//! `sailor flow`: carica i file dichiarativi da `flows/`, mostra anche quelli
//! guasti, controlla che le azioni nominate esistano ed esegue il grafo nel
//! deposito durevole comune di Sailor.

// Il formato del file vive nel crate del flusso: qui si importa, non si
// ridichiara. Averlo scritto due volte, il 28/08/2026, li ha fatti coincidere
// per fortuna e non per costruzione.
use flow::{
    ActionRegistry, Execution, ExecutionRequest, Executor, FlowFile, Graph,
    InProcessExecutor, RecordStore, SharedState, SystemClock,
};
use ledger::Ledger;
use serde_json::Value;
use ui::gather::FlowSource;
use std::collections::{BTreeMap, BTreeSet};
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
        [command, name] if command == "check" => check_flow(sources, name, true),
        [command, name, flag] if command == "check" && flag == "--no-engines" => {
            check_flow(sources, name, false)
        }
        [command, name] if command == "run" => run_flow(sources, name, None),
        [command, name, text] if command == "run" => run_flow(sources, name, Some(text)),
        [command, name] if command == "cost" => cost_of(name),
        _ => Err(usage()),
    }
}

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
        .ok_or_else(|| {
            format!(
                "il flusso {} non ha un passo di innesco: non c'è dove mettere un mandato",
                flow.id
            )
        })?;
    let entry = flow
        .inputs
        .entry(trigger)
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    match entry {
        Value::Object(fields) => {
            fields.insert("text".to_owned(), Value::String(text.to_owned()));
            Ok(())
        }
        other => Err(format!(
            "l'ingresso dell'innesco di {} non è un oggetto ma {other}: non so dove mettere il testo",
            flow.id
        )),
    }
}

/// Quanto ha consumato l'ultima corsa di un flusso.
///
/// **PERCHÉ ESISTE, E PERCHÉ ADESSO.** Il consumo entra nel deposito dal
/// 30/08/2026, e fino al 31 l'unico modo di leggerlo era aprire SQLite a mano —
/// cioè aggirare Sailor, che è il guasto 15: uno strumento aggirato non
/// registra niente di ciò che gli succede intorno. Serve a rispondere alla
/// domanda che decide come si scrivono i flussi: **un flusso consuma più o meno
/// di un prompt solo?** Senza un modo di chiederlo, quella domanda si risponde a
/// impressione.
///
/// **I TOKEN VENGONO PRIMA DEL COSTO, E NON È UNA PREFERENZA DI STILE.** Con una
/// riga di comando locale non si paga a chiamata: si paga un abbonamento, e
/// quello che si consuma è **quota**, che si misura in token. La cifra in valuta
/// è quanto sarebbe costato via API — un metro utile per confrontare, non una
/// fattura. E ci sono motori che i token li dichiarano e il costo no: mostrare
/// solo il costo li renderebbe invisibili.
fn cost_of(flow: &str) -> Result<String, String> {
    let dir = default_ledger_dir()?;
    let Some(data) = ui::gather::gather(&dir).map_err(|error| error.to_string())? else {
        return Err(format!(
            "nessun deposito in {}: non è ancora girato niente",
            dir.display()
        ));
    };
    // L'ultima per inizio, non l'ultima scritta: una corsa aperta e una chiusa
    // possono arrivare in ordine inverso nella proiezione.
    let run = data
        .runs
        .iter()
        .filter(|run| run.entity == flow)
        .max_by_key(|run| run.started_at)
        .ok_or_else(|| format!("il flusso {flow} non è mai girato su questa macchina"))?;
    let view = ui::dashboard::summarize_run(
        run,
        data.steps_by_run.get(&run.run_id).map_or(&[], Vec::as_slice),
        data.calls_by_run.get(&run.run_id).map_or(&[], Vec::as_slice),
        now_secs()?,
    );
    Ok(spending_report(&view))
}

/// Il consumo di una corsa, per una persona.
fn spending_report(view: &ui::dashboard::ExecutionView) -> String {
    let tokens = &view.tokens;
    let mut report = format!(
        "corsa {} — flusso {} — {}\npassi: {} ({} andati, {} rotti)\nchiamate: {}",
        view.run_id, view.entity, view.status, view.steps_total, view.steps_went, view.steps_broke,
        tokens.calls
    );
    // **I TURNI ACCANTO ALLE CHIAMATE, E NON È UN DETTAGLIO.** Una chiamata a un
    // motore agentico non è un giro: ne sono decine, e il conto lo fa quel
    // numero. Misurato il 31/08/2026: una catena di quattro passi legge per
    // turno l'8% in più di una sessione sola, e consuma il doppio — perché di
    // turni ne fa il doppio. Chi legge «7 chiamate» senza sapere quanti turni
    // sono non ha in mano la quantità su cui si interviene.
    //
    // **IL CONTEGGIO GREZZO E BASTA: QUI NON SI STAMPA `cache letta ÷ turni`.**
    // Sembra «il contesto di una richiesta» e non lo è: sui quattro passi di una
    // corsa vera il quoziente dà 21.165 / 13.566 / 48.984 / 50.885 contro i
    // 46.702 / 31.651 / 63.266 / 71.173 che le richieste leggono davvero — sbaglia
    // da 1,29 a 2,33 volte, e il fattore cambia **fra i passi della stessa
    // corsa**. È la media di una rampa, non il prefisso, e non è confrontabile
    // né fra passi né fra un flusso e una sessione sola. Un numero stampato
    // viene usato per decidere: questo manderebbe a intervenire nel posto
    // sbagliato con l'aria di una misura.
    if tokens.turns > 0 {
        let _ = write!(report, " in {} turni", tokens.turns);
    }
    let _ = write!(
        report,
        "\ntoken: {} in · {} out · {} letti da cache · {} scritti in cache",
        tokens.input_tokens,
        tokens.output_tokens,
        tokens.cached_tokens,
        tokens.cache_write_tokens
    );
    if tokens.total_tokens_only > 0 {
        let _ = write!(
            report,
            "\ntotali non ripartiti (chi non dichiara i due lati): {}",
            tokens.total_tokens_only
        );
    }
    let _ = write!(
        report,
        "\ncosto equivalente: {:.4} (quanto sarebbe costato via API, non una spesa)",
        tokens.cost_micros as f64 / 1_000_000.0
    );
    // **QUELLO CHE MANCA SI DICE, O IL TOTALE SI LEGGE COME COMPLETO.** È la
    // stessa regola della finestra: una somma che tace su ciò che non ha
    // contato è una rassicurazione, non una misura.
    if tokens.calls_without_tokens > 0 || tokens.calls_without_cost > 0 {
        let _ = write!(
            report,
            "\nparziale: {} chiamate senza token dichiarati, {} senza costo noto",
            tokens.calls_without_tokens, tokens.calls_without_cost
        );
    }
    report
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
    "uso: sailor flow <list|due|check <nome> [--no-engines]|run <nome> [mandato]|cost <nome>>"
        .to_owned()
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

/// **LE RIGHE SI PROVANO SE NESSUNO DICE DI NO.** Un controllo dietro una
/// bandiera è un controllo che nessuno interroga, e il guasto 27 è la prova:
/// nessuno avrebbe scritto `--engines` per scoprire un difetto che non sapeva
/// di avere. `--no-engines` resta per chi lavora scollegato o ha fretta.
fn check_flow(sources: &[FlowSource], name: &str, try_engines: bool) -> Result<String, String> {
    let (flow, _) = one_flow(sources, name)?;
    let tools = toolbox::Tools::current();
    let real = actions::RealDryProbe;
    let probe: Option<&dyn actions::DryProbe> = if try_engines { Some(&real) } else { None };
    let (report, unknown) = check_report(
        &flow,
        &default_registry(open_default_ledger(), None),
        Some(&tools),
        probe,
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
    probe: Option<&dyn actions::DryProbe>,
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
            capabilities_into(&mut report, &flow.graph, tools);
            // Senza sonda il rapporto **tace** su questo, invece di dichiarare
            // sane righe che non ha guardato: è la stessa regola del rilevatore
            // assente qui sopra.
            if let Some(probe) = probe {
                engine_lines_into(&mut report, &flow.graph, tools, probe);
            }
        }
    }

    // **I CAMPI CHE L'AZIONE NON CONOSCE, DETTI PRIMA DI SPENDERE.** Il guasto
    // 20: `"prompt"` scritto dove va `"stdin"` partiva in silenzio, il motore
    // riceveva una riga monca, e l'errore che tornava era suo — dopo aver
    // pagato la chiamata. Qui si guarda solo ciò che una persona ha scritto a
    // mano nel flusso, dove un campo di troppo non è l'uscita di nessuno.
    let stray = stray_fields(flow, registry);
    if !stray.is_empty() {
        let _ = write!(
            report,
            "\ncampi che l'azione non conosce (verranno ignorati): {}",
            stray.join("; ")
        );
    }
    (report, unknown)
}

/// Una capacità chiesta da un passo a un motore preciso.
///
/// I tre nomi stanno insieme perché un avviso che ne perda uno non si può usare:
/// «manca `response_shape`» non dice a chi legge quale passo cambiare, e in un
/// flusso che chiede lo stesso al primo e al terzo motore della catena non dice
/// nemmeno quale dei due.
struct WantedCapability {
    step: String,
    tool: String,
    capability: String,
}

/// Le capacità che i passi chiedono, passo per passo e motore per motore.
///
/// **IL PRODOTTO CARTESIANO È VOLUTO.** Un passo che scrive `"tool":
/// ["claude-code", "agy"]` chiede quella capacità a tutti e due: il ripiego può
/// finire su chiunque della catena, e un controllo che guardasse solo il primo
/// tacerebbe proprio sul motore su cui la corsa finisce quando il primo muore.
/// È la stessa ragione per cui `tools_wanted` conta i motori dentro una catena.
fn capabilities_wanted(graph: &Graph) -> Vec<WantedCapability> {
    let mut wanted = Vec::new();
    for step in graph.steps() {
        let Some(with) = step.with.as_ref() else {
            continue;
        };
        let asked: Vec<String> = match with.get("needs_capabilities") {
            Some(Value::Array(names)) => names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            // Un nome solo si scrive senza le parentesi quadre, come ovunque.
            Some(Value::String(name)) => vec![name.clone()],
            _ => continue,
        };
        let engines = engines_of(with);
        for capability in &asked {
            for tool in &engines {
                wanted.push(WantedCapability {
                    step: step.id.clone(),
                    tool: tool.clone(),
                    capability: capability.clone(),
                });
            }
        }
    }
    wanted
}

/// Scrive nel rapporto le capacità chieste dai passi e come stanno messe.
///
/// **È UN AVVISO, NON UN ERRORE, E LA DIFFERENZA È LA STESSA DEL 28/08/2026.**
/// Uno strumento che qui non è installato non rende rotto un flusso; una
/// capacità che un motore non ha non lo rende rotto nemmeno: chi non sa imporre
/// una forma alla risposta se la fa chiedere nel prompt e paga più token. È il
/// vincolo permanente «indipendenza dal modello» — una capacità assente è una
/// condizione dichiarata, non un guasto — e per questo il flusso continua a
/// passare il controllo. Quello che cambia è che chi lancia lo sa **prima** di
/// spendere, invece di leggerlo nella risposta del motore.
///
/// **E LE DUE ASSENZE SI DICONO CON DUE FRASI DIVERSE.** «Dichiara di non
/// averla» si ripara cambiando motore; «nessuno ha guardato» si ripara
/// misurando quello che si ha. Metterle sotto la stessa parola farebbe passare
/// per misurata ogni omissione — che è esattamente ciò che il blocco
/// `capabilities` esiste per non fare.
fn capabilities_into(report: &mut String, graph: &Graph, tools: &toolbox::Tools) {
    let mut available = Vec::new();
    let mut gaps = Vec::new();
    for wanted in capabilities_wanted(graph) {
        // Uno strumento che nessun descrittore dichiara è già stato nominato
        // sopra: ripeterlo qui con parole diverse manderebbe a cercare due
        // difetti dove ce n'è uno.
        let Some(state) = tools.capability(&wanted.tool, &wanted.capability) else {
            continue;
        };
        let line = format!(
            "{} chiede {} a {}",
            wanted.step, wanted.capability, wanted.tool
        );
        match state {
            toolbox::CapabilityState::Available => available.push(line),
            toolbox::CapabilityState::Absent => {
                gaps.push(format!("{line}, che dichiara di non averla"))
            }
            toolbox::CapabilityState::NotLookedAt => gaps.push(format!(
                "{line}, che non la dichiara — nessuno ha guardato se ce l'ha"
            )),
        }
    }
    if !available.is_empty() {
        let _ = write!(report, "\ncapacità chieste: {}", available.join("; "));
    }
    if !gaps.is_empty() {
        let _ = write!(
            report,
            "\ncapacità che il motore non dichiara (il passo funziona lo stesso, \
             pagando di più): {}",
            gaps.join("; ")
        );
    }
}

// ── le righe di comando, montate e provate senza domanda ────────────────

/// Un motore su cui un passo si affida al descrittore per comporre la riga.
struct WantedEngine {
    step: String,
    tool: String,
}

/// I motori di cui `flow check` deve provare la riga, passo per passo e
/// **motore per motore della catena**.
///
/// **TUTTA LA CATENA, NON IL PRIMO.** Il guasto 16 è nato da sei passi che
/// nominavano un motore solo; il guasto 27 dice che il difetto stava nel
/// *secondo* — nessun flusso mette `agy` per primo, quindi quel ramo non era
/// mai stato eseguito e la riga sbagliata è vissuta indisturbata. Guardare solo
/// il primo motore è non guardare dove il difetto era.
///
/// **E SOLO I PASSI CHE LA RIGA NON SE LA SCRIVONO.** Un passo che dichiara i
/// propri `args` vince sulla ricetta — lo decide `ExternalEngineAction`, e qui
/// si legge la stessa regola, non una seconda copia di essa. Sono i passi che
/// invocano `cargo` o `git` attraverso la stessa azione: la loro riga non viene
/// da nessun blocco `ask`, e chiamarla «non montabile» sarebbe un allarme su un
/// passo sano.
fn engines_wanted(graph: &Graph) -> Vec<WantedEngine> {
    let mut wanted = Vec::new();
    for step in graph.steps() {
        let Some(with) = step.with.as_ref() else {
            continue;
        };
        if with.get("args").is_some() {
            continue;
        }
        for tool in engines_of(with) {
            wanted.push(WantedEngine {
                step: step.id.clone(),
                tool,
            });
        }
    }
    wanted
}

/// Cosa si è potuto sapere della riga di un motore.
enum EngineOutcome {
    /// Il motore non è invocabile qui, e il rilevatore dice perché.
    NotHere(String),
    /// Nessun blocco `ask`: la riga non si compone affatto, e non c'è niente
    /// da provare. È un'assenza nel descrittore, non un difetto della riga.
    NotAssemblable,
    /// La riga si è montata e si è provata: ecco com'è venuta e cosa ha detto.
    Tried {
        line: String,
        verdict: actions::ProbeVerdict,
    },
}

/// Monta la riga di ogni motore di ogni catena, la prova **senza dare la
/// domanda**, e scrive nel rapporto come sta messa.
///
/// **QUI `flow check` CAMBIA NATURA, E VA DETTO.** `resolver.rs` dichiara in
/// testa che risolvere un nome non deve eseguire niente, e resta vero: è questa
/// funzione che avvia processi, non la risoluzione. Da qui in poi `flow check`
/// avvia un processo per ogni motore dichiarato — **senza rete, senza denaro,
/// con un tetto di tempo**, perché senza la domanda nessuno di quei processi
/// chiama un fornitore. Il prezzo è che un controllo statico non è più solo
/// statico; il ricavo è che la cura scritta accanto al guasto 1 esiste davvero.
///
/// **ACCESO IN MODO PREDEFINITO.** Un controllo dietro una bandiera è un
/// controllo che nessuno interroga: il guasto 27 sarebbe rimasto invisibile
/// esattamente come è rimasto, perché nessuno avrebbe scritto la bandiera. Chi
/// non lo vuole scrive `--no-engines`, e allora il rapporto **tace** invece di
/// dichiarare sane righe che non ha guardato.
///
/// **L'ASSE «È STATO CHIAMATO DAVVERO» NON È QUESTO, E RESTA SEPARATO.** Una
/// riga sana non dice che quel motore abbia mai risposto a una domanda vera:
/// quello lo sa il deposito, che registra le chiamate. Mescolare le due cose
/// farebbe passare per «usato» un motore che nessuna corsa ha mai nominato —
/// che è precisamente il guasto 32.
fn engine_lines_into(
    report: &mut String,
    graph: &Graph,
    tools: &toolbox::Tools,
    probe: &dyn actions::DryProbe,
) {
    use actions::{ProbeVerdict, ToolResolver};

    // Un motore si prova UNA VOLTA SOLA anche quando lo nominano sei passi: la
    // riga che si monta viene dal descrittore, non dal passo, quindi sei prove
    // avvierebbero sei processi per sapere sei volte la stessa cosa. Il
    // rapporto resta passo per passo, che è ciò che chi legge deve correggere.
    let mut judged: BTreeMap<String, EngineOutcome> = BTreeMap::new();

    let mut sound = Vec::new();
    let mut broken = Vec::new();
    let mut untried = Vec::new();
    let mut unassemblable = Vec::new();
    let mut exhausted = Vec::new();

    for wanted in engines_wanted(graph) {
        // Uno strumento che nessun descrittore dichiara è già nominato sopra:
        // ripeterlo qui manderebbe a cercare due difetti dove ce n'è uno.
        if !tools.declares(&wanted.tool) {
            continue;
        }
        if !judged.contains_key(&wanted.tool) {
            let outcome = match tools.resolve(&wanted.tool) {
                Err(reason) => EngineOutcome::NotHere(reason),
                Ok(bin) => match tools.ask_recipe(&wanted.tool) {
                    None => EngineOutcome::NotAssemblable,
                    Some(recipe) => {
                        let line = std::iter::once(bin.clone())
                            .chain(actions::command_line(&recipe))
                            .collect::<Vec<_>>()
                            .join(" ");
                        EngineOutcome::Tried {
                            verdict: actions::probe_dry_run(probe, &bin, &recipe),
                            line,
                        }
                    }
                },
            };
            judged.insert(wanted.tool.clone(), outcome);
        }

        let who = format!("{} → {}", wanted.step, wanted.tool);
        match judged.get(&wanted.tool).expect("appena inserito") {
            EngineOutcome::NotHere(reason) => {
                untried.push(format!("{who}: il motore non è invocabile qui — {reason}"))
            }
            EngineOutcome::NotAssemblable => unassemblable.push(format!(
                "{who}: il suo descrittore non dichiara un blocco `ask`, quindi non \
                 esiste nessuna riga da montare e il passo dovrà scrivere da sé le opzioni"
            )),
            EngineOutcome::Tried { line, verdict } => match verdict {
                ProbeVerdict::Sound => sound.push(who),
                // LE PAROLE DEL MOTORE PER INTERO, E LA RIGA CHE LE HA
                // PRODOTTE. Sul guasto 27 la frase di `agy` diceva quale
                // bandiera aveva mangiato quale argomento: una diagnosi che
                // nessuna parola nostra avrebbe potuto sostituire. Tagliarla, o
                // riassumerla, riporterebbe chi legge a indovinare.
                ProbeVerdict::Broken { said } => broken.push(format!(
                    "{who}: riga montata «{line}»; il motore ha risposto: «{said}»"
                )),
                ProbeVerdict::CannotWork { said } => {
                    exhausted.push(format!("{who}: «{said}»"))
                }
                ProbeVerdict::NotDeclared => untried.push(format!(
                    "{who}: il suo descrittore non dichiara come rifiuta la riga senza \
                     domanda (`refuses_without_prompt`), quindi non c'è modo di dire se \
                     la riga «{line}» sia sana — si misura eseguendola senza la domanda"
                )),
                ProbeVerdict::TimedOut { why } => untried.push(format!(
                    "{who}: nessuna risposta alla riga «{line}» — {why}"
                )),
            },
        }
    }

    if !sound.is_empty() {
        let _ = write!(
            report,
            "\nrighe di comando sane (montate e provate senza domanda, senza spendere): {}",
            sound.join("; ")
        );
    }
    if !broken.is_empty() {
        let _ = write!(
            report,
            "\nrighe di comando ROTTE (il motore si è lamentato di qualcosa che non è \
             la domanda mancante): {}",
            broken.join("; ")
        );
    }
    if !exhausted.is_empty() {
        let _ = write!(
            report,
            "\nmotori che adesso non possono lavorare (la riga non c'entra, si \
             riprova quando tornano): {}",
            exhausted.join("; ")
        );
    }
    if !untried.is_empty() {
        let _ = write!(
            report,
            "\nrighe di comando non provate (non si sa se siano sane): {}",
            untried.join("; ")
        );
    }
    if !unassemblable.is_empty() {
        let _ = write!(
            report,
            "\nrighe di comando non montabili (non c'è niente da provare): {}",
            unassemblable.join("; ")
        );
    }
}

/// I campi scritti a mano che l'azione del passo non riconosce.
///
/// Guarda in due posti, e sono i due posti dove scrive una persona: il `with`
/// del passo nel grafo, e l'ingresso dichiarato in `inputs`. Non guarda
/// l'ingresso che il passo riceve davvero — quello contiene l'uscita delle
/// dipendenze, dove i campi estranei sono la normalità e non un errore.
fn stray_fields(flow: &FlowFile, registry: &ActionRegistry) -> Vec<String> {
    let mut found = Vec::new();
    for step in flow.graph.steps() {
        let Some(action) = registry.get(&step.action) else {
            // L'azione non c'è: lo dice già `azioni mancanti`, e dirlo due
            // volte con parole diverse manderebbe a cercare due difetti.
            continue;
        };
        for declared in [step.with.as_ref(), flow.inputs.get(&step.id)]
            .into_iter()
            .flatten()
        {
            let stray = action.unknown_fields(declared);
            if !stray.is_empty() {
                found.push(format!("{}: {}", step.id, stray.join(", ")));
            }
        }
    }
    found
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
    graph
        .steps()
        .iter()
        .filter_map(|step| step.with.as_ref())
        .flat_map(engines_of)
        .collect()
}

/// I motori che un `with` nomina, **nell'ordine in cui li ha scritti chi ha
/// scritto il passo**: un nome solo, o una catena.
///
/// **UNA COPIA SOLA PERCHÉ LA DOMANDA È UNA SOLA.** Fino al 31/08/2026 questa
/// lettura stava scritta due volte — dentro `tools_wanted` e dentro
/// `capabilities_wanted` — e la seconda era nata perché la prima buttava via il
/// passo. Due copie della stessa regola divergono sul primo dettaglio che
/// qualcuno cambia a una sola delle due, ed è il guasto 10: qui ne serviva una
/// terza, e la terza è il momento giusto per fermarsi.
///
/// **L'ORDINE È UN DATO, NON UN CASO.** In una catena il primo è quello che si
/// prova per primo e gli altri sono il ripiego; un `BTreeSet` lo perderebbe, e
/// chi legge il rapporto non saprebbe più su quale motore finisce una corsa
/// quando il primo muore.
fn engines_of(with: &Value) -> Vec<String> {
    match with.get("tool") {
        Some(Value::String(id)) => vec![id.clone()],
        Some(Value::Array(chain)) => chain
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
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
fn run_flow(sources: &[FlowSource], name: &str, mandate: Option<&str>) -> Result<String, String> {
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
        // Il tetto è del flusso e viaggia con la corsa: chi lancia non lo
        // inventa, lo porta.
        spend_cap_micros: flow.spend_cap_micros,
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
fn record_run(
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
            started_by: "sailor flow",
        },
    )
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
    use flow::{Clock, Decision, InMemoryRecordStore};
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
            refused.contains("non ha un passo di innesco"),
            "e dice perché: {refused}"
        );
    }

    // ── i campi che l'azione non conosce ─────────────────────────────

    /// **IL REFUSO DEL 30/08/2026, PRESO PRIMA DI PAGARLO.**
    ///
    /// Un flusso scriveva `"prompt"` dove va `"stdin"`. Il passo è partito lo
    /// stesso, il motore ha ricevuto una riga di comando monca, e l'errore che è
    /// tornato era suo: «Input must be provided either through stdin». Una
    /// chiamata a pagamento per un refuso di sette lettere.
    #[test]
    fn a_field_the_action_does_not_know_is_named_before_the_run() {
        let inputs = r#"{"root":{"tool":"claude-code","prompt":"ciao","timeout_secs":10}}"#;
        let json = flow_json("external_engine", "[]", inputs);
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");

        let (report, _) = check_report(&flow, &default_registry(None, None), None, None);

        assert!(
            report.contains("campi che l'azione non conosce"),
            "il controllo deve nominarli: {report}"
        );
        assert!(
            report.contains("root: prompt"),
            "e dire in quale passo e quale campo: {report}"
        );
    }

    /// La gemella: lo **stesso** flusso col campo giusto non dice niente.
    ///
    /// Senza di lei, un controllo che si lamentasse sempre passerebbe la prova
    /// sopra e renderebbe illeggibile ogni rapporto.
    #[test]
    fn the_same_flow_written_right_says_nothing() {
        let inputs = r#"{"root":{"tool":"claude-code","stdin":"ciao","timeout_secs":10}}"#;
        let json = flow_json("external_engine", "[]", inputs);
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");

        let (report, _) = check_report(&flow, &default_registry(None, None), None, None);

        assert!(
            !report.contains("campi che l'azione non conosce"),
            "un flusso scritto bene non deve essere accusato: {report}"
        );
    }

    /// **NESSUN FLUSSO SPEDITO PORTA UN CAMPO IGNOTO.** Vale come misura del
    /// controllo appena aggiunto: se dicesse cose a caso, questa lo direbbe
    /// subito su codice vero invece che su un flusso inventato.
    #[test]
    fn no_shipped_flow_carries_a_field_nobody_reads() {
        let registry = default_registry(None, None);
        for (name, text) in flow::system::FLOWS {
            let flow: FlowFile = serde_json::from_str(text)
                .unwrap_or_else(|why| panic!("il flusso «{name}» non si carica: {why}"));
            assert!(
                stray_fields(&flow, &registry).is_empty(),
                "«{name}» ha campi che nessuno legge: {:?}",
                stray_fields(&flow, &registry)
            );
        }
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
        let entries: Vec<String> = ids
            .iter()
            .map(|id| {
                format!(
                    r#"{{"id":"{id}","family":"tool","label":"{id}","detect":{{"command":"{id}"}}}}"#
                )
            })
            .collect();
        let file = std::env::temp_dir().join(format!("prova-strumenti-{}.json", ids.join("-")));
        std::fs::write(&file, format!(r#"{{"tools":[{}]}}"#, entries.join(","))).expect("scrivere");
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

        let (report, unknown) = check_report(&flow, &default_registry(None, None), Some(&tools), None);

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

        let (report, unknown) = check_report(&flow, &default_registry(None, None), Some(&tools), None);

        assert!(unknown.is_empty(), "non è un errore: {unknown:?}");
        assert!(
            report.contains("strumenti chiesti: strumento-dichiarato-mai-installato"),
            "{report}"
        );
    }

    /// Un catalogo con un motore solo, e le capacità che la prova gli attribuisce.
    ///
    /// **IL NOME DEL FILE PORTA UN CONTATORE**, e non è pignoleria: `cargo test`
    /// manda le prove sullo stesso processo, quindi due prove che scrivono lo
    /// stesso identificativo si ruberebbero il file a vicenda — è il guasto 21,
    /// che si vede una volta su venti e sempre su una prova diversa.
    fn tools_with_capabilities(id: &str, capabilities: &str) -> toolbox::Tools {
        static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let file = std::env::temp_dir().join(format!(
            "prova-capacita-{}-{}-{id}.json",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::write(
            &file,
            format!(
                r#"{{"tools":[{{"id":"{id}","family":"ai_cli","label":"{id}",
                    "detect":{{"command":"{id}"}},"capabilities":{capabilities}}}]}}"#
            ),
        )
        .expect("scrivere");
        let catalog = toolbox::Catalog::load(&[toolbox::Source::File(file)]);
        toolbox::Tools::new(catalog, toolbox::Machine::current())
    }

    fn flow_needing_capability(tool: &str, capability: &str) -> FlowFile {
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
                        "with": {{"tool": "{tool}", "needs_capabilities": ["{capability}"], "timeout_secs": 10}},
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

    /// **IL TERZO CASO CHE IL CONTROLLO NON SAPEVA DIRE.** Sapeva distinguere
    /// «lo strumento non c'è qui» da «non esiste in nessun catalogo»; un passo
    /// che chiede a un motore qualcosa che quel motore non sa fare passava per
    /// buono, e il difetto si scopriva pagando la chiamata.
    #[test]
    fn a_capability_the_engine_declares_absent_is_named_with_step_and_engine() {
        let flow = flow_needing_capability("un-motore", "response_shape");
        let tools = tools_with_capabilities("un-motore", r#"{"response_shape": false}"#);

        let (report, unknown) = check_report(&flow, &default_registry(None, None), Some(&tools), None);

        assert!(unknown.is_empty(), "resta un avviso, non un errore: {unknown:?}");
        assert!(report.contains("root"), "nomina il passo: {report}");
        assert!(report.contains("un-motore"), "nomina il motore: {report}");
        assert!(report.contains("response_shape"), "nomina la capacità: {report}");
        assert!(
            report.contains("dichiara di non averla"),
            "e dice che qualcuno ha guardato: {report}"
        );
    }

    /// **E LA DISTINZIONE ARRIVA FINO A CHI LEGGE.** Se le due frasi fossero
    /// una sola, il blocco `capabilities` avrebbe potuto essere un elenco di ciò
    /// che c'è, e ogni silenzio passerebbe per una misura. Il rimedio è diverso:
    /// nel caso di sopra si cambia motore, qui si misura quello che si ha.
    #[test]
    fn a_capability_nobody_measured_is_told_apart_from_one_declared_absent() {
        let flow = flow_needing_capability("un-motore", "response_shape");
        let tools = tools_with_capabilities("un-motore", r#"{"choose_model": true}"#);

        let (report, _) = check_report(&flow, &default_registry(None, None), Some(&tools), None);

        assert!(
            report.contains("nessuno ha guardato"),
            "il descrittore tace su quella capacità: {report}"
        );
        assert!(
            !report.contains("dichiara di non averla"),
            "e tacere non è dichiarare un'assenza: {report}"
        );
    }

    /// Una capacità dichiarata e ottenibile non produce nessun avviso: un
    /// controllo che si lamenta anche quando va tutto bene smette di essere letto.
    #[test]
    fn a_capability_the_engine_has_raises_no_warning() {
        let flow = flow_needing_capability("un-motore", "response_shape");
        let tools = tools_with_capabilities(
            "un-motore",
            r#"{"response_shape": {"args": ["--json-schema"], "takes_value": true}}"#,
        );

        let (report, _) = check_report(&flow, &default_registry(None, None), Some(&tools), None);

        assert!(
            !report.contains("capacità che il motore non dichiara"),
            "{report}"
        );
        assert!(
            report.contains("capacità chieste: root chiede response_shape a un-motore"),
            "quello che c'è si vede lo stesso: {report}"
        );
    }

    /// **UN PASSO CHE DICHIARA LE PROPRIE CAPACITÀ NON HA UN CAMPO DI TROPPO.**
    /// Senza `needs_capabilities` dentro la specifica del motore, lo stesso
    /// rapporto direbbe anche «campi che l'azione non conosce», e chi legge
    /// andrebbe a cercare un refuso che non c'è: è il guasto 20 al contrario,
    /// un avviso vero su un campo giusto.
    #[test]
    fn declaring_needed_capabilities_is_not_a_stray_field() {
        let flow = flow_needing_capability("un-motore", "response_shape");
        let tools = tools_with_capabilities("un-motore", r#"{"response_shape": true}"#);

        let (report, _) = check_report(&flow, &default_registry(None, None), Some(&tools), None);

        assert!(
            !report.contains("campi che l'azione non conosce"),
            "{report}"
        );
    }

    /// Senza rilevatore il rapporto tace sugli strumenti invece di chiamarli
    /// tutti sconosciuti: non aver potuto guardare non è aver visto che manca.
    #[test]
    fn without_a_detector_the_check_says_nothing_about_tools() {
        let flow = flow_wanting_tool("qualunque");

        let (report, unknown) = check_report(&flow, &default_registry(None, None), None, None);

        assert!(unknown.is_empty());
        assert!(!report.contains("strument"), "{report}");
    }

    #[test]
    fn check_reports_steps_dependencies_and_every_missing_action() {
        let json = flow_json("azione_assente", "[]", "{}");
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");

        let (report, _) = check_report(&flow, &default_registry(None, None), None, None);

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

        let (report, _) = check_report(&flow, &default_registry(None, None), None, None);

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

        let (report, _) = check_report(&flow, &default_registry(None, None), None, None);

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

    /// **IL TOTALE DI UNA CORSA È QUELLO DELLE SUE CHIAMATE, NON UNO ZERO.**
    /// Il difetto che questa prova esiste per prendere ha vissuto in silenzio
    /// finché il costo delle chiamate non è diventato vero: `record_run`
    /// scriveva `total_cost_micros: 0` a mano, e la finestra mostrava quello
    /// zero accanto alla somma giusta calcolata altrove.
    #[test]
    fn a_runs_total_is_what_its_calls_cost() {
        let directory = TestDirectory::new();
        let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
        let flow: FlowFile = serde_json::from_str(&flow_json("shell_check", "[]", "{}"))
            .expect("caricare il flusso");
        for (call_id, cost) in [("prima", 96_310), ("seconda", 3_690)] {
            ledger
                .record_model_call(&spent_call(call_id, "corsa-costosa", cost))
                .expect("registrare la chiamata");
        }

        record_run(&ledger, &flow, "corsa-costosa", "complete", 100, Some(110), None)
            .expect("registrare la corsa");

        let dump = ledger.projection_dump().expect("leggere la proiezione");
        let run = dump["runs"]
            .as_array()
            .expect("l'elenco delle corse c'è")
            .iter()
            .find(|row| row[0] == "corsa-costosa")
            .expect("la corsa registrata si ritrova");
        assert_eq!(
            run[6],
            serde_json::json!(100_000),
            "il totale è la somma delle due chiamate, non uno zero scritto a mano"
        );
    }

    // ── le righe di comando provate a secco ───────────────────────────

    /// Una macchina finta con dei motori dentro, e i loro descrittori.
    ///
    /// Niente dipende da cosa è installato su chi esegue: il percorso è una
    /// cartella temporanea, e i motori sono file vuoti col bit di esecuzione —
    /// non vengono mai avviati, perché la sonda di queste prove è finta.
    fn tools_with_engines(entries: &[(&str, &str)]) -> toolbox::Tools {
        static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "prova-motori-{}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("la cartella di prova");
        let mut declared = Vec::new();
        for (id, ask) in entries {
            let path = dir.join(id);
            std::fs::write(&path, "").expect("il finto eseguibile");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                    .expect("bit di esecuzione");
            }
            declared.push(format!(
                r#"{{"id":"{id}","family":"ai_cli","label":"{id}","detect":{{"command":"{id}"}}{ask}}}"#
            ));
        }
        let file = dir.join("tools.json");
        std::fs::write(&file, format!(r#"{{"tools":[{}]}}"#, declared.join(","))).expect("scrivere");
        let catalog = toolbox::Catalog::load(&[toolbox::Source::File(file)]);
        toolbox::Tools::new(
            catalog,
            toolbox::Machine {
                path_dirs: vec![dir.clone()],
                home: dir,
                env: BTreeMap::new(),
                version_probes: false,
            },
        )
    }

    /// Una sonda che non esegue niente e risponde ciò che le diciamo, in base a
    /// come si chiama l'eseguibile che le viene passato.
    struct ScriptedProbe(Vec<(&'static str, &'static str)>);

    impl actions::DryProbe for ScriptedProbe {
        fn run(&self, bin: &str, _args: &[String], _stdin: Option<Vec<u8>>) -> actions::DryRun {
            let said = self
                .0
                .iter()
                .find(|(name, _)| bin.ends_with(name))
                .map(|(_, said)| *said)
                .unwrap_or("");
            actions::DryRun::Answered {
                stdout: String::new(),
                stderr: said.to_owned(),
            }
        }
    }

    fn flow_with_chain(chain: &str) -> FlowFile {
        let json = format!(
            r#"{{
                "id": "prova",
                "description": "flusso di prova",
                "graph": {{
                    "steps": [{{
                        "id": "chiedi",
                        "deps": [],
                        "action": "external_engine",
                        "max_attempts": 1,
                        "when": null,
                        "with": {{"tool": {chain}, "stdin": "ciao", "timeout_secs": 10}},
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

    const REFUSES: &str = r#","ask":{"args":["-p"],"prompt":"stdin","refuses_without_prompt":["input must be provided"]}"#;
    const SAYS_NOTHING: &str = r#","ask":{"args":["-p"],"prompt":"stdin"}"#;
    const NO_ASK: &str = "";

    /// **UNA RIGA SANA SI VEDE, E COSTA ZERO.** È il controllo che il guasto 1
    /// aveva chiesto il 28/08 e che nessuno aveva scritto perché sembrava voler
    /// dire spendere.
    #[test]
    fn a_line_the_engine_only_complains_about_the_missing_prompt_is_called_sound() {
        let flow = flow_with_chain(r#""motore""#);
        let tools = tools_with_engines(&[("motore", REFUSES)]);
        let probe = ScriptedProbe(vec![("motore", "Input must be provided through stdin")]);

        let (report, _) = check_report(
            &flow,
            &default_registry(None, None),
            Some(&tools),
            Some(&probe),
        );

        assert!(report.contains("righe di comando sane"), "{report}");
        assert!(report.contains("chiedi → motore"), "{report}");
    }

    /// **LE PAROLE DEL MOTORE SONO LA DIAGNOSI, E VANNO SCRITTE PER INTERO.**
    /// Sul guasto 27 la frase di `agy` diceva quale bandiera aveva mangiato
    /// quale argomento; un rapporto che dicesse solo «rotta» rimanderebbe a
    /// indovinare, cioè non varrebbe più della sua assenza.
    #[test]
    fn a_broken_line_is_reported_with_the_engines_own_words_and_the_line_that_produced_it() {
        let flow = flow_with_chain(r#""motore""#);
        let tools = tools_with_engines(&[("motore", REFUSES)]);
        let probe = ScriptedProbe(vec![(
            "motore",
            "--print took \"--output-format\" as its prompt",
        )]);

        let (report, _) = check_report(
            &flow,
            &default_registry(None, None),
            Some(&tools),
            Some(&probe),
        );

        assert!(report.contains("righe di comando ROTTE"), "{report}");
        assert!(
            report.contains("--print took \"--output-format\" as its prompt"),
            "senza le parole del motore la riga rossa non dice cosa correggere: {report}"
        );
        assert!(
            report.contains("riga montata «") && report.contains("-p»"),
            "e senza la riga montata non si sa nemmeno cosa è stato provato: {report}"
        );
    }

    /// **SI GUARDA TUTTA LA CATENA, NON IL PRIMO.** Il guasto 27 stava nel
    /// **secondo** motore di ogni catena, ed è vissuto indisturbato proprio
    /// perché nessun flusso lo metteva per primo. Un controllo che leggesse
    /// solo il primo motore sarebbe un controllo che non guarda dov'era il
    /// difetto.
    #[test]
    fn every_engine_of_the_chain_is_tried_not_only_the_first() {
        let flow = flow_with_chain(r#"["primo", "secondo", "terzo"]"#);
        let tools = tools_with_engines(&[
            ("primo", REFUSES),
            ("secondo", REFUSES),
            ("terzo", REFUSES),
        ]);
        let probe = ScriptedProbe(vec![
            ("primo", "Input must be provided through stdin"),
            ("secondo", "took --output-format as its prompt"),
            ("terzo", "Input must be provided through stdin"),
        ]);

        let (report, _) = check_report(
            &flow,
            &default_registry(None, None),
            Some(&tools),
            Some(&probe),
        );

        assert!(
            report.contains("chiedi → secondo"),
            "il secondo della catena non è stato guardato: {report}"
        );
        assert!(
            report.contains("took --output-format as its prompt"),
            "{report}"
        );
        assert!(report.contains("chiedi → terzo"), "né il terzo: {report}");
    }

    /// **«NON PROVATA» E «NON MONTABILE» SONO DUE FATTI DIVERSI.** Un motore
    /// senza blocco `ask` non ha nessuna riga da provare — si ripara scrivendo
    /// il descrittore; uno che ha la riga ma non dichiara come rifiuta ce l'ha
    /// e nessuno l'ha guardata — si ripara eseguendola. Sotto la stessa parola
    /// manderebbero a fare il lavoro sbagliato, ed è il guasto 32 che vive
    /// nella prima delle due.
    #[test]
    fn a_missing_ask_block_is_not_confused_with_a_line_nobody_looked_at() {
        let flow = flow_with_chain(r#"["senza-ask", "senza-rifiuto"]"#);
        let tools = tools_with_engines(&[("senza-ask", NO_ASK), ("senza-rifiuto", SAYS_NOTHING)]);
        let probe = ScriptedProbe(vec![("senza-rifiuto", "un errore qualunque")]);

        let (report, _) = check_report(
            &flow,
            &default_registry(None, None),
            Some(&tools),
            Some(&probe),
        );

        let untried = report
            .lines()
            .find(|line| line.starts_with("righe di comando non provate"))
            .unwrap_or_else(|| panic!("manca la riga «non provate»: {report}"));
        let unassemblable = report
            .lines()
            .find(|line| line.starts_with("righe di comando non montabili"))
            .unwrap_or_else(|| panic!("manca la riga «non montabili»: {report}"));

        assert!(untried.contains("senza-rifiuto"), "{untried}");
        assert!(
            !untried.contains("senza-ask"),
            "un motore senza `ask` non è una riga non provata: {untried}"
        );
        assert!(unassemblable.contains("senza-ask"), "{unassemblable}");
        assert!(
            !unassemblable.contains("senza-rifiuto"),
            "{unassemblable}"
        );
    }

    /// **UN MOTORE ESAURITO NON È UNA RIGA ROTTA**, e la sua frase è la quarta.
    /// Confonderli manderebbe a correggere un descrittore sano mentre bastava
    /// aspettare.
    #[test]
    fn an_engine_that_cannot_work_now_gets_its_own_sentence() {
        let flow = flow_with_chain(r#""motore""#);
        let tools = tools_with_engines(&[(
            "motore",
            r#","ask":{"args":["-p"],"prompt":"stdin","unusable_when":["weekly limit"],"refuses_without_prompt":["input must be provided"]}"#,
        )]);
        let probe = ScriptedProbe(vec![("motore", "You've hit your weekly limit")]);

        let (report, _) = check_report(
            &flow,
            &default_registry(None, None),
            Some(&tools),
            Some(&probe),
        );

        assert!(
            report.contains("motori che adesso non possono lavorare"),
            "{report}"
        );
        assert!(
            !report.contains("righe di comando ROTTE"),
            "la riga è sana, è la quota che è finita: {report}"
        );
    }

    /// **SENZA SONDA IL RAPPORTO TACE**, non dichiara sane righe che non ha
    /// guardato: è la stessa regola del rilevatore assente, e senza di essa
    /// `--no-engines` diventerebbe un modo per far dire al controllo una cosa
    /// che non ha verificato.
    #[test]
    fn with_no_engines_the_report_says_nothing_about_command_lines() {
        let flow = flow_with_chain(r#""motore""#);
        let tools = tools_with_engines(&[("motore", REFUSES)]);

        let (report, _) = check_report(&flow, &default_registry(None, None), Some(&tools), None);

        assert!(!report.contains("righe di comando"), "{report}");
    }

    /// I passi che scrivono i propri `args` non compongono nessuna riga dal
    /// descrittore: sono quelli che invocano `cargo` o `git` attraverso la
    /// stessa azione, e chiamarli «non montabili» sarebbe un allarme su un
    /// passo sano — cioè rumore che insegna a non leggere il rapporto.
    #[test]
    fn a_step_that_writes_its_own_arguments_is_not_reported_as_unassemblable() {
        let json = r#"{
            "id": "prova",
            "description": "flusso di prova",
            "graph": {
                "steps": [{
                    "id": "prove",
                    "deps": [],
                    "action": "external_engine",
                    "max_attempts": 1,
                    "when": null,
                    "with": {"tool": "cargo", "args": ["test"], "timeout_secs": 10},
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"}
                }],
                "skippable_dependencies": []
            },
            "inputs": {}
        }"#;
        let flow: FlowFile = serde_json::from_str(json).expect("caricare il flusso");
        let tools = tools_with_engines(&[("cargo", NO_ASK)]);
        let probe = ScriptedProbe(vec![]);

        let (report, _) = check_report(
            &flow,
            &default_registry(None, None),
            Some(&tools),
            Some(&probe),
        );

        assert!(!report.contains("righe di comando"), "{report}");
    }

    /// Una chiamata già costata, per misurare il totale di una corsa.
    fn spent_call(call_id: &str, run_id: &str, cost: i64) -> ledger::ModelCallRecord {
        ledger::ModelCallRecord {
            call_id: call_id.to_owned(),
            run_id: run_id.to_owned(),
            step_id: Some("chiedi".to_owned()),
            purpose: "external_engine".to_owned(),
            cli: "claude-code".to_owned(),
            requested_model: String::new(),
            actual_model: String::new(),
            input_tokens: Some(2),
            output_tokens: Some(4),
            cached_tokens: None,
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: None,
            cost_micros: Some(cost),
            declared_cost_micros: None,
            price_currency: None,
            input_price_micros_per_million: None,
            output_price_micros_per_million: None,
            cached_price_micros_per_million: None,
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            mandate_name: String::new(),
            mandate_version: String::new(),
            retry_chain: vec![],
            error_type: None,
            started_at: 100,
            ended_at: Some(105),
        }
    }
}
