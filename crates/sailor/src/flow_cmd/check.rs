//! `sailor flow check`: the report on a flow before it runs.

use flow::{ActionRegistry, FlowFile, Graph};
use registry::default_registry;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use ui::gather::FlowSource;

use super::cap_and_schedule::WHAT_THE_CAP_DOES_NOT_PROMISE_KEY;
use super::cost::{in_units, models_seen_by, what_is_priced};
use super::engines::{engine_lines_into, login_states_into};
use super::extensions::{extensions_of_this_machine_into, undeclared_extensions_named_in_text};
use super::hazards::{
    blind_steps_asking_for_a_session, deciders_that_are_not_checks, handed_without_choices,
    hardcoded_paths,
    outside_text_in_command, pointers_that_cannot_match, undelimited_commits, HardcodedPath,
};
use super::{missing_actions, one_flow, open_default_ledger};

/// **LE RIGHE SI PROVANO SE NESSUNO DICE DI NO.** Un controllo dietro una
/// bandiera è un controllo che nessuno interroga, e il guasto 27 è la prova:
/// nessuno avrebbe scritto `--engines` per scoprire un difetto che non sapeva
/// di avere. `--no-engines` resta per chi lavora scollegato o ha fretta.
pub(super) fn check_flow(sources: &[FlowSource], name: &str, try_engines: bool) -> Result<String, String> {
    let (flow, _) = one_flow(sources, name)?;
    let tools = toolbox::Tools::current();
    let real = actions::RealDryProbe;
    // **UNO STATO DEI PROFILI ILLEGGIBILE NON FERMA IL CONTROLLO**, per la
    // stessa ragione per cui non ferma una corsa: si guarda un mondo senza
    // profili, e la sezione delle case tace invece di dire una cosa falsa.
    let profiles = profiles::store_io::load_store().unwrap_or_default();
    let world = EngineWorld {
        probe: &real,
        profiles: &profiles,
    };
    let registry = default_registry(open_default_ledger(), None);
    let (mut report, unknown) = check_report(
        &flow,
        &registry,
        Some(&tools),
        if try_engines { Some(&world) } else { None },
    );
    // **IL LISTINO SI GUARDA QUI E NON DENTRO `check_report`.** Quel rapporto è
    // puro — flusso, registro, rilevatore, sonda, tutti passati da fuori — e i
    // modelli che un flusso ha usato li sa solo il deposito. Tenerlo fuori
    // lascia `check_report` provabile senza aprirne uno.
    report.push_str(&what_is_priced(
        &actions::current_price_list(),
        models_seen_by(&flow.id).as_ref(),
        flow.spend_cap_micros,
    ));
    // The inventory is this machine's, so it is read here for the same reason
    // as the price list: a test feeds `check_report` a scratch one instead.
    extensions_of_this_machine_into(&mut report, &flow);
    // **UN RINVIO DENTRO UN CAMPO CHE VIENE ESEGUITO SI FERMA QUI.** Prima
    // dell'esecuzione, perché dopo il rinvio è già diventato testo di shell e
    // non si distingue più da ciò che il flusso aveva scritto.
    let montati: Vec<String> = outside_text_in_command(&flow)
        .iter()
        .map(|found| format!("{} in «{}»", found.step, found.field))
        .collect();
    if !montati.is_empty() {
        println!("{report}");
        return Err(catalogue::say(
            "cli.flow.value_mounted_into_an_executed_field",
            &[("flow", &flow.id), ("fields", &montati.join(", "))],
        ));
    }

    // **UN PERCORSO DI POSIZIONE ASSOLUTO È UN ERRORE, NON UN AVVISO.** Il
    // flusso gira in un posto solo: altrove non fallisce, lavora nel posto
    // sbagliato — ed è il modo in cui il guasto 25 è passato inosservato.
    let stuck: Vec<String> = hardcoded_paths(&flow)
        .iter()
        .filter(|path| path.fatal)
        .map(|path| format!("{} in «{}» ({})", path.step, path.field, path.value))
        .collect();
    if !stuck.is_empty() {
        println!("{report}");
        return Err(catalogue::say(
            "cli.flow.absolute_path_in_a_place_field",
            &[("flow", &flow.id), ("fields", &stuck.join("; "))],
        ));
    }
    // **BEFORE THE RUN, BECAUSE ONE OF THE TWO SHAPES IS SILENT.** In `when`
    // a pointer that cannot match makes the step skip, and a skipped step
    // closes green; in a value it breaks at the first run.
    let dead: Vec<String> = pointers_that_cannot_match(&flow)
        .iter()
        .map(|found| {
            if found.field.is_empty() {
                format!("{} in «when» ({})", found.step, found.pointer)
            } else {
                format!("{} in «{}» ({})", found.step, found.field, found.pointer)
            }
        })
        .collect();
    if !dead.is_empty() {
        println!("{report}");
        return Err(catalogue::say(
            "cli.flow.pointer_that_cannot_match",
            &[("flow", &flow.id), ("fields", &dead.join("; "))],
        ));
    }
    // An error, not a warning: the step does not fail, it commits another
    // session's staged work under a message about something else.
    let sweeping: Vec<String> = undelimited_commits(&flow)
        .iter()
        .map(|commit| format!("{} in «{}»", commit.step, commit.field))
        .collect();
    if !sweeping.is_empty() {
        println!("{report}");
        return Err(catalogue::say(
            "cli.flow.commit_without_paths",
            &[("flow", &flow.id), ("steps", &sweeping.join(", "))],
        ));
    }
    let seeing = blind_steps_asking_for_a_session(&flow);
    if !seeing.is_empty() {
        println!("{report}");
        return Err(catalogue::say(
            "cli.flow.blind_and_asking_for_a_session",
            &[("flow", &flow.id), ("steps", &seeing.join(", "))],
        ));
    }
    // An error, not a warning: a run that closes on this step closes without
    // asking anybody, and the whole saving depends on what said yes.
    let deciding = deciders_that_are_not_checks(&flow, &registry);
    if !deciding.is_empty() {
        println!("{report}");
        return Err(catalogue::say(
            "cli.flow.decides_done_without_a_check",
            &[("flow", &flow.id), ("steps", &deciding.join(", "))],
        ));
    }
    let unasked = handed_without_choices(&flow);
    if !unasked.is_empty() {
        println!("{report}");
        return Err(catalogue::say(
            "cli.flow.handed_without_choices",
            &[("flow", &flow.id), ("steps", &unasked.join(", "))],
        ));
    }
    if unknown.is_empty() {
        return Ok(report);
    }
    // IL RAPPORTO SI VEDE ANCHE QUANDO IL FLUSSO È ROTTO. Chi controlla un
    // flusso lo fa per capirlo: rispondere con la sola riga dell'errore
    // costringerebbe a rilanciare il comando per vedere il resto.
    println!("{report}");
    Err(catalogue::say(
        "cli.flow.tools_no_descriptor_declares",
        &[("flow", &flow.id), ("tools", &unknown.join(", "))],
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
/// Chi fa le domande locali ai motori, e in quale casa gliele fa.
///
/// **PERCHÉ I DUE VIAGGIANO INSIEME.** Le domande a costo zero che `flow check`
/// fa a un motore sono due — «la riga che ti monto è sana?» e «la casa da cui
/// parti è autenticata?» — e la seconda non ha senso senza sapere quale casa:
/// lo stato dei profili è l'altra metà della stessa domanda. Passarli separati
/// costringerebbe ogni luogo di chiamata a portarsi due argomenti che valgono
/// sempre la stessa cosa insieme.
///
/// **LO STATO DEI PROFILI ENTRA DA FUORI, E NON SI LEGGE QUI.** `check_report`
/// resta puro — flusso, registro, rilevatore, mondo, tutti passati — ed è la sola
/// ragione per cui una prova può metterci dentro una casa usa-e-getta invece di
/// dipendere da come è configurata la macchina di chi la esegue.
pub(super) struct EngineWorld<'a> {
    pub(super) probe: &'a dyn actions::EngineProbe,
    pub(super) profiles: &'a profiles::ProfileStore,
}

#[cfg(test)]
impl<'a> EngineWorld<'a> {
    /// Un mondo in cui **nessun profilo è dichiarato**: è lo stato di una
    /// macchina appena installata, ed è quello giusto per le prove che parlano
    /// delle righe di comando e non delle case. Senza profilo attivo la sezione
    /// delle credenziali tace, quindi quelle prove restano su ciò che provano.
    pub(super) fn without_profiles(probe: &'a dyn actions::EngineProbe) -> Self {
        static NO_PROFILES: std::sync::OnceLock<profiles::ProfileStore> =
            std::sync::OnceLock::new();
        Self {
            probe,
            profiles: NO_PROFILES.get_or_init(profiles::ProfileStore::default),
        }
    }
}

pub(super) fn check_report(
    flow: &FlowFile,
    registry: &ActionRegistry,
    tools: Option<&toolbox::Tools>,
    world: Option<&EngineWorld>,
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
        if let Some(phase) = &step.phase {
            report.push_str(&catalogue::say("cli.flow.step_phase", &[("phase", phase)]));
        }
    }
    // **IL TETTO STA NEL RAPPORTO, E CON LUI CIÒ CHE NON PROMETTE.** Chi
    // controlla un flusso prima di lanciarlo sta decidendo se può permetterselo:
    // un tetto invisibile qui si scopre solo a corsa fermata, e uno che si vede
    // senza i suoi limiti si legge come una garanzia sulla spesa — che non è.
    // La riga c'è sempre, anche quando il tetto non c'è: la parola di NO_CAP è
    // un'informazione, e un rapporto che tace quando non c'è niente da dire
    // lascia chi legge a chiedersi se il controllo abbia guardato.
    match flow.spend_cap_micros {
        None => report.push_str(&catalogue::say("cli.flow.no_spend_cap", &[])),
        Some(cap) => {
            let _ = write!(
                report,
                "{}{}",
                catalogue::say(
                    "cli.flow.spend_cap",
                    &[("micros", &cap.to_string()), ("units", &in_units(cap))]
                ),
                catalogue::say(WHAT_THE_CAP_DOES_NOT_PROMISE_KEY, &[])
            );
        }
    }
    // **CHI CONTROLLA UN FLUSSO DEVE VEDERE COSA PUÒ SCRIVERCI DENTRO.** Il
    // rapporto nominava solo le azioni mancanti, cioè rispondeva a «questo
    // flusso gira?» e non a «cosa posso mettere nel prossimo passo». L'elenco
    // arriva dal registro, non da una copia scritta qui accanto.
    let _ = write!(
        report,
        "\nazioni disponibili: {}",
        registry.names().join(", ")
    );
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
            fallbacks_into(&mut report, &flow.graph, tools);
            data_pacts_into(&mut report, &flow.graph, tools);
            // Senza sonda il rapporto **tace** su questo, invece di dichiarare
            // sane righe che non ha guardato: è la stessa regola del rilevatore
            // assente qui sopra.
            if let Some(world) = world {
                engine_lines_into(&mut report, &flow.graph, tools, world.probe);
                login_states_into(&mut report, &flow.graph, tools, world);
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

    // What a step's text leans on, named before the run: two steps once
    // followed skills one home had, no field said so, and the flow passed here
    // while working worse everywhere else without a word. See fault 17.
    let leaning: Vec<String> = undeclared_extensions_named_in_text(flow)
        .iter()
        .map(|named| format!("{} in «{}» ({})", named.step, named.field, named.name))
        .collect();
    if !leaning.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.extensions_named_not_declared",
            &[("fields", &leaning.join("; "))],
        ));
    }

    // **IL GUASTO 25, DETTO PRIMA DI PARTIRE.** Un `workdir` assoluto non si
    // vede eseguendo: si vede dopo, guardando quale repository si è sporcato.
    let (fatal, advisory): (Vec<HardcodedPath>, Vec<HardcodedPath>) = hardcoded_paths(flow)
        .into_iter()
        .partition(|path| path.fatal);
    if !fatal.is_empty() {
        let _ = write!(
            report,
            "\npercorsi assoluti in un campo di posizione: {}",
            describe_paths(&fatal)
        );
    }
    if !advisory.is_empty() {
        let _ = write!(
            report,
            "\npercorsi assoluti dentro un testo (il flusso gira, l'istruzione no): {}",
            describe_paths(&advisory)
        );
    }
    (report, unknown)
}

/// Passo e campo su ogni riga: un avviso che ne perda uno non si può usare —
/// «c'è un percorso assoluto» non dice quale dei sette passi cambiare.
fn describe_paths(paths: &[HardcodedPath]) -> String {
    paths
        .iter()
        .map(|path| format!("{} in «{}» ({})", path.step, path.field, path.value))
        .collect::<Vec<_>>()
        .join("; ")
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

/// Scrive nel rapporto **chi non può fare il ripiego che la catena gli
/// assegna**, e **quali descrittori si contraddicono**.
///
/// **SONO LO STESSO DIFETTO VISTO DA DUE LATI, ED È PER QUESTO CHE STANNO
/// INSIEME.** Un descrittore che dichiara una capacità senza la riga per usarla
/// (guasto 32) e uno che non dichiara come si esaurisce mentre qualcuno lo mette
/// in mezzo a una catena (guasto 31) sbagliano allo stesso modo: **niente si
/// rompe**. Il primo fa credere che un motore si possa interrogare, il secondo
/// fa credere che un passo abbia due ripieghi quando ne ha zero, e in tutti e
/// due i casi ciò che manca non è un pezzo di codice — è qualcuno che confronti
/// due dichiarazioni. Qui quel confronto arriva **prima di spendere**, invece
/// che alla prima corsa in cui il primo motore muore.
///
/// **LE REGOLE NON SONO SCRITTE QUI.** Vivono in `toolbox::Descriptor`, e le
/// stesse due funzioni le interrogano le prove sui descrittori spediti. Una
/// copia scritta dentro `flow check` sarebbe la seconda regola che diverge dalla
/// prima — il guasto 10 — e a divergere sarebbe quella che una persona legge.
///
/// **È UN AVVISO E NON UN ERRORE**, per la stessa ragione delle capacità qui
/// sopra: un flusso con un ripiego che non scatta gira, e fa il suo lavoro
/// finché il primo motore risponde. Non è rotto: è un flusso che ha meno
/// ripieghi di quanti sembra averne, e chi lancia deve saperlo prima.
/// A step whose text is private, and the pact of every engine it names. An
/// engine nobody measured is not a maybe: the run refuses it before spending,
/// so a flow all of whose engines are refused cannot run anywhere, and saying
/// so before the run is the whole point of a check.
fn data_pacts_into(report: &mut String, graph: &Graph, tools: &toolbox::Tools) {
    use actions::ToolResolver as _;

    let mut shut = Vec::new();
    for step in graph.steps() {
        let Some(with) = step.with.as_ref() else {
            continue;
        };
        if !actions::private_data_asked_in(with) {
            continue;
        }
        let chain = engines_of(with);
        if chain.is_empty() {
            continue;
        }
        let refused: Vec<String> = chain
            .iter()
            .filter(|id| !tools.data_pact(id).accepts_private())
            .map(|id| {
                catalogue::say(
                    "cli.flow.private_step_engine_refused",
                    &[
                        ("step", &step.id),
                        ("engine", id),
                        ("pact", &tools.data_pact(id).to_string()),
                    ],
                )
            })
            .collect();
        if refused.len() == chain.len() {
            shut.extend(refused);
        }
    }
    if !shut.is_empty() {
        let _ = write!(
            report,
            "{}",
            catalogue::say(
                "cli.flow.private_step_no_engine_may_run",
                &[("steps", &shut.join("; "))],
            )
        );
    }
}

fn fallbacks_into(report: &mut String, graph: &Graph, tools: &toolbox::Tools) {
    let mut plugs = Vec::new();
    for step in graph.steps() {
        let Some(with) = step.with.as_ref() else {
            continue;
        };
        // L'ultimo della catena non ha nessuno a cui passare il lavoro:
        // pretendere da lui una dichiarazione di esaurimento sarebbe pretendere
        // una misura che non serve a niente.
        let chain = engines_of(with);
        let Some((_, before_the_last)) = chain.split_last() else {
            continue;
        };
        for tool in before_the_last {
            // Uno strumento che nessun descrittore dichiara è già nominato
            // sopra: ripeterlo qui manderebbe a cercare due difetti dove ce
            // n'è uno.
            if !tools.declares(tool) {
                continue;
            }
            if let Some(why) = tools.cannot_be_a_fallback(tool) {
                plugs.push(format!("{} → {tool}: {why}", step.id));
            }
        }
    }
    if !plugs.is_empty() {
        let _ = write!(
            report,
            "\nmotori messi in posizione di ripiego che non possono farlo (il passo \
             muore su di loro, e i motori dopo non partono): {}",
            plugs.join("; ")
        );
    }

    // Solo i descrittori che questo flusso nomina: un catalogo intero
    // contraddittorio non è un difetto di **questo** flusso, e mostrarlo qui
    // manderebbe a correggere file che questa corsa non tocca.
    let named = tools_wanted(graph);
    let disagreeing: Vec<String> = tools
        .contradictions()
        .into_iter()
        .filter(|found| named.contains(&found.tool))
        .map(|found| found.line())
        .collect();
    if !disagreeing.is_empty() {
        let _ = write!(
            report,
            "\ndescrittori che dicono due cose diverse sullo stesso fatto: {}",
            disagreeing.join("; ")
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

/// The engines a `with` names, in the order written: one name or a chain.
/// The reader is the action's own, so the check and the run cannot read one
/// step in two ways; the order is data, since the first is tried first.
pub(super) fn engines_of(with: &Value) -> Vec<String> {
    actions::engines_named_in(with)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use registry::{registry_in, House};

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

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        assert!(
            report.contains("campi che l'azione non conosce"),
            "il controllo deve nominarli: {report}"
        );
        assert!(
            report.contains("root: prompt"),
            "e dire in quale passo e quale campo: {report}"
        );
    }

    /// A `for_each` step is a step like the others to the check: listed with
    /// its dependencies, its action known, and its stray fields named.
    #[test]
    fn a_for_each_step_is_listed_and_its_stray_fields_are_named() {
        let json = r#"{
            "id": "prova", "description": "flusso di prova",
            "graph": {"steps": [{
                "id": "ripeti", "deps": [], "action": "for_each", "max_attempts": 1,
                "when": null, "input_schema": {"type": "any"}, "output_schema": {"type": "any"},
                "with": {"flow": "foglia", "items": [1, 2], "flusso": "foglia"}
            }]},
            "inputs": {}
        }"#;
        let flow: FlowFile = serde_json::from_str(json).expect("caricare il flusso");

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        assert!(report.contains("ripeti <- nessuna"), "the step is listed: {report}");
        assert!(
            report.contains("azioni mancanti: nessuna"),
            "the action is one the engine registers: {report}"
        );
        assert!(
            report.contains("ripeti: flusso"),
            "and the field it does not know is named: {report}"
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

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

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
        let registry = registry_in(House::empty(), None, None);
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
        toolbox::Tools::new(
            catalog,
            toolbox::Machine::bare(std::path::PathBuf::from(toolbox::probe::NOWHERE)),
        )
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

        let (report, unknown) =
            check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert_eq!(unknown, vec!["questo-non-esiste-in-nessun-catalogo"]);
        assert!(
            report.contains(
                "strumenti che nessun descrittore dichiara: questo-non-esiste-in-nessun-catalogo"
            ),
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

        let (report, unknown) =
            check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

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
        toolbox::Tools::new(
            catalog,
            toolbox::Machine::bare(std::path::PathBuf::from(toolbox::probe::NOWHERE)),
        )
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

        let (report, unknown) =
            check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert!(
            unknown.is_empty(),
            "resta un avviso, non un errore: {unknown:?}"
        );
        assert!(report.contains("root"), "nomina il passo: {report}");
        assert!(report.contains("un-motore"), "nomina il motore: {report}");
        assert!(
            report.contains("response_shape"),
            "nomina la capacità: {report}"
        );
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

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert!(
            report.contains("nessuno ha guardato"),
            "il descrittore tace su quella capacità: {report}"
        );
        assert!(
            !report.contains("dichiara di non averla"),
            "e tacere non è dichiarare un'assenza: {report}"
        );
    }

    /// A flow whose private step names only engines nobody measured cannot run
    /// anywhere, and the check says so before a run finds out by failing.
    #[test]
    fn a_private_step_whose_engines_may_not_take_it_is_named_by_the_check() {
        let flow = private_flow_wanting(&["muto", "taciturno"]);
        let tools = tools_with_pacts(&[("muto", "unknown"), ("taciturno", "trains")]);

        let (report, _) =
            check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert!(
            report.contains("cannot run anywhere") || report.contains("da nessuna parte"),
            "the flow cannot run anywhere and the check must say it: {report}"
        );
        assert!(
            report.contains("muto") && report.contains("taciturno"),
            "and it names every engine that may not take it: {report}"
        );
    }

    /// One engine that may take it is enough: a chain falls back, and a check
    /// that shouted at every unmeasured engine in a chain would stop being read.
    #[test]
    fn a_private_step_with_one_engine_that_may_take_it_raises_no_warning() {
        let flow = private_flow_wanting(&["muto", "misurato"]);
        let tools = tools_with_pacts(&[("muto", "unknown"), ("misurato", "does_not_train")]);

        let (report, _) =
            check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert!(
            !report.contains("cannot run anywhere") && !report.contains("da nessuna parte"),
            "one engine may take it, so the step runs: {report}"
        );
    }

    /// A step that says nothing about its text is public, and a public step
    /// goes to any engine: the check must not read silence as a demand.
    #[test]
    fn a_step_that_declares_no_data_is_not_read_as_private() {
        let flow = flow_wanting_tool("muto");
        let tools = tools_with_pacts(&[("muto", "unknown")]);

        let (report, _) =
            check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert!(
            !report.contains("cannot run anywhere") && !report.contains("da nessuna parte"),
            "silence is public: {report}"
        );
    }

    /// Engines declaring the pact each is given, and a flow whose only step is
    /// private and names the chain asked for.
    fn tools_with_pacts(pacts: &[(&str, &str)]) -> toolbox::Tools {
        static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let entries: Vec<String> = pacts
            .iter()
            .map(|(id, pact)| {
                format!(
                    r#"{{"id":"{id}","family":"ai_cli","label":"{id}",
                        "detect":{{"command":"{id}"}},"data_pact":"{pact}"}}"#
                )
            })
            .collect();
        let file = std::env::temp_dir().join(format!(
            "prova-patti-{}-{}.json",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::write(&file, format!(r#"{{"tools":[{}]}}"#, entries.join(","))).expect("scrivere");
        toolbox::Tools::new(
            toolbox::Catalog::load(&[toolbox::Source::File(file)]),
            toolbox::Machine::bare(std::path::PathBuf::from(toolbox::probe::NOWHERE)),
        )
    }

    fn private_flow_wanting(chain: &[&str]) -> FlowFile {
        let tools: Vec<String> = chain.iter().map(|id| format!("\"{id}\"")).collect();
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
                        "with": {{"tool": [{}], "data": "private", "timeout_secs": 10}},
                        "input_schema": {{"type": "any"}},
                        "output_schema": {{"type": "any"}}
                    }}],
                    "skippable_dependencies": []
                }},
                "inputs": {{}}
            }}"#,
            tools.join(",")
        );
        serde_json::from_str(&json).expect("caricare il flusso")
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

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

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

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert!(
            !report.contains("campi che l'azione non conosce"),
            "{report}"
        );
    }

    // ── chi non può fare il ripiego che la catena gli dà ───────────────

    /// Un catalogo di due motori, il primo dei quali dichiara — o tace su —
    /// come dice di non poter lavorare.
    fn tools_where_the_first_says(exhaustion: &str) -> toolbox::Tools {
        static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let file = std::env::temp_dir().join(format!(
            "prova-ripiego-{}-{}.json",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::write(
            &file,
            format!(
                r#"{{"tools":[
                  {{"id":"primo","family":"ai_cli","label":"primo",
                    "detect":{{"command":"primo"}},
                    "ask":{{"args":["-p"],"prompt":"stdin"{exhaustion}}},
                    "capabilities":{{"ask_without_interaction":{{"args":["-p"]}}}}}},
                  {{"id":"secondo","family":"ai_cli","label":"secondo",
                    "detect":{{"command":"secondo"}},
                    "ask":{{"args":["-p"],"prompt":"stdin","unusable_when":["quota"]}},
                    "capabilities":{{"ask_without_interaction":{{"args":["-p"]}}}}}}
                ]}}"#
            ),
        )
        .expect("scrivere");
        let catalog = toolbox::Catalog::load(&[toolbox::Source::File(file)]);
        toolbox::Tools::new(
            catalog,
            toolbox::Machine::bare(std::path::PathBuf::from(toolbox::probe::NOWHERE)),
        )
    }

    /// Un passo con una catena di due motori.
    fn flow_with_a_chain() -> FlowFile {
        let json = r#"{
            "id": "catena",
            "description": "un passo con un ripiego",
            "graph": {
                "steps": [{
                    "id": "root", "deps": [], "action": "external_engine",
                    "max_attempts": 1, "when": null,
                    "with": {"tool": ["primo", "secondo"], "timeout_secs": 10},
                    "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
                }],
                "skippable_dependencies": []
            },
            "inputs": {}
        }"#;
        serde_json::from_str(json).expect("caricare il flusso")
    }

    /// **IL GUASTO 31, DETTO PRIMA DI SPENDERE E SUI FLUSSI DI CHI LANCIA.**
    ///
    /// Una prova sui flussi di questo albero sorveglia questo albero. Chi scrive
    /// un flusso suo, con un descrittore suo in `~/.config/sailor/tools.d/`,
    /// rifarebbe lo stesso difetto senza che nulla diventi rosso da nessuna
    /// parte: la catena avrebbe l'aria di un ripiego e non ne avrebbe nessuno.
    ///
    /// **I DUE CASI SONO NELLA STESSA PROVA APPOSTA.** L'unica differenza fra i
    /// due ingressi è che il primo motore dichiari o no le proprie parole: due
    /// rapporti uguali direbbero che il controllo non le sta guardando, e una
    /// prova che cercasse solo la frase resterebbe verde davanti a un mutante
    /// che la stampa sempre.
    #[test]
    fn a_chain_whose_first_engine_cannot_fall_back_is_named_by_the_check() {
        let flow = flow_with_a_chain();
        let registry = registry_in(House::empty(), None, None);

        let silent = tools_where_the_first_says("");
        let (about_the_silent, _) = check_report(&flow, &registry, Some(&silent), None);
        let speaking = tools_where_the_first_says(r#","unusable_when":["weekly limit"]"#);
        let (about_the_speaking, _) = check_report(&flow, &registry, Some(&speaking), None);

        assert!(
            about_the_silent.contains("motori messi in posizione di ripiego che non possono farlo")
                && about_the_silent.contains("root → primo"),
            "{about_the_silent}"
        );
        assert!(
            !about_the_speaking.contains("posizione di ripiego"),
            "chi dichiara le proprie parole non va segnalato: {about_the_speaking}"
        );
        // E il difetto è del **primo**: l'ultimo non ha nessuno a cui passare il
        // lavoro, e pretendere da lui una misura sarebbe pretenderla per niente.
        assert!(
            !about_the_silent.contains("root → secondo"),
            "{about_the_silent}"
        );
    }

    /// Senza rilevatore il rapporto tace sugli strumenti invece di chiamarli
    /// tutti sconosciuti: non aver potuto guardare non è aver visto che manca.
    #[test]
    fn without_a_detector_the_check_says_nothing_about_tools() {
        let flow = flow_wanting_tool("qualunque");

        let (report, unknown) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        assert!(unknown.is_empty());
        assert!(!report.contains("strument"), "{report}");
    }

    // ── il tetto di spesa: `flow check` e `flow cap` ────────────────────

    /// **DUE FLUSSI IDENTICI TRANNE IL TETTO DANNO DUE RAPPORTI DIVERSI.**
    ///
    /// **IL CONFRONTO È FRA I DUE RAPPORTI, NON CON UNA PAROLA.** Una prova che
    /// cercasse «tetto» resterebbe verde davanti a un mutante che stampa sempre
    /// la stessa riga — la parola ci sarebbe comunque. Qui l'unica differenza
    /// fra i due ingressi è il tetto, quindi due uscite uguali dicono che il
    /// controllo non lo sta guardando.
    ///
    /// *Mutante eseguito*: nel ramo `Some(cap)` di `check_report` stampare
    /// `"\ntetto di spesa: nessuno"` come nel ramo `None`. I due rapporti
    /// diventano identici e questa prova diventa rossa.
    #[test]
    fn two_flows_that_differ_only_by_the_cap_get_two_different_reports() {
        let json = flow_json("shell_check", "[]", "{}");
        let without: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");
        let mut with = without.clone();
        with.spend_cap_micros = Some(2_500_000);

        let registry = registry_in(House::empty(), None, None);
        let (said_without, _) = check_report(&without, &registry, None, None);
        let (said_with, _) = check_report(&with, &registry, None, None);

        assert_ne!(
            said_without, said_with,
            "il tetto non compare nel rapporto: {said_with}"
        );
        assert!(said_without.contains("spend cap: none"), "{said_without}");
        assert!(said_with.contains("2500000 micro"), "{said_with}");
    }

    /// **UN TETTO CHE C'È PORTA CON SÉ CIÒ CHE NON PROMETTE.**
    ///
    /// Un numero da solo si legge come una garanzia sulla spesa. I tre limiti
    /// veri — il freno non arriva ai motori, il primo fronte non è mai frenato,
    /// le chiamate senza costo restano fuori — devono stare accanto al numero,
    /// non in un documento che nessuno apre mentre lancia.
    #[test]
    fn a_cap_in_the_report_declares_what_it_does_not_promise() {
        let json = flow_json("shell_check", "[]", "{}");
        let mut flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");
        flow.spend_cap_micros = Some(1);

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        assert!(report.contains("does not reach the engines"), "{report}");
        assert!(report.contains("first front"), "{report}");
        assert!(report.contains("stay out of the sum"), "{report}");
    }

    #[test]
    fn check_reports_steps_dependencies_and_every_missing_action() {
        let json = flow_json("azione_assente", "[]", "{}");
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        assert!(report.contains("passi: 1"), "{report}");
        assert!(report.contains("cicli: nessuno"), "{report}");
        assert!(report.contains("dipendenze: 0"), "{report}");
        assert!(report.contains("root <- nessuna"), "{report}");
        assert!(
            report.contains("azioni mancanti: azione_assente"),
            "{report}"
        );
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

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        assert!(report.contains("dipendenze: 1"), "{report}");
        assert!(report.contains("child <- root"), "{report}");
    }

    /// The phase is for whoever reads the report, so it sits on the step's own
    /// line — and only there: a step that names none gets no label, or the
    /// reader would take the hole for a phase nobody wrote.
    #[test]
    fn check_names_the_phase_of_a_step_that_has_one_and_stays_quiet_otherwise() {
        let json = r#"{
            "id": "fasi",
            "description": "one step names its phase",
            "graph": {
                "steps": [
                    {"id":"root","deps":[],"action":"shell_check","max_attempts":1,"when":null,"input_schema":{"type":"any"},"output_schema":{"type":"any"}},
                    {"id":"child","deps":["root"],"action":"shell_check","max_attempts":1,"when":null,"input_schema":{"type":"any"},"output_schema":{"type":"any"},"phase":"build"}
                ]
            },
            "inputs": {}
        }"#;
        let flow: FlowFile = serde_json::from_str(json).expect("the flow loads");

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        let labelled = format!(
            "child <- root{}",
            catalogue::say("cli.flow.step_phase", &[("phase", "build")])
        );
        assert!(report.contains(&labelled), "{report}");
        assert!(report.contains("root <- nessuna\n"), "{report}");
    }

    #[test]
    fn both_default_actions_are_known_to_check() {
        let registry = registry_in(House::empty(), None, None);
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
        let registry = registry_in(House::empty(), None, None);
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

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        assert!(report.contains("azioni disponibili: "), "{report}");
        assert!(report.contains("history_ask"), "{report}");
        assert!(report.contains("external_engine"), "{report}");
    }

    /// Il nodo di ingresso e il rilevatore sono azioni come le altre: un flusso
    /// che li nomina si controlla senza che nessuno le registri a mano.
    #[test]
    fn the_trigger_and_the_detector_are_known_to_check() {
        let registry = registry_in(House::empty(), None, None);
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
        let registry = registry_in(House::empty(), None, None);
        let engine = registry
            .get("external_engine")
            .expect("il motore è registrato");
        let input = serde_json::json!({
            "tool": "nessuno-strumento-si-chiama-cosi",
            "timeout_secs": 1
        });

        let error = engine
            .execute(&input, &flow::SharedState::new())
            .expect_err("quell'identificativo non esiste");

        assert_eq!(error.class, "tool_unavailable", "{}", error.said);
    }
}
