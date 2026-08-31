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
        [command, name] if command == "run" => run_flow(sources, name, None),
        [command, name, text] if command == "run" => run_flow(sources, name, Some(text)),
        [command, name] if command == "cost" => cost_of(name),
        [command, name] if command == "cap" => cap_of(sources, name),
        [command, name, value] if command == "cap" => set_cap(sources, name, value),
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

// ── il tetto di spesa di un flusso ───────────────────────────────────────

/// Quanto costa una unità di valuta in micro. Un milione: `1_000_000` è un
/// dollaro.
const MICROS_IN_A_UNIT: f64 = 1_000_000.0;

/// **QUANTE CORSE COSTATE SERVONO PER SUGGERIRE UN TETTO.**
///
/// Tre, e non è una soglia arrotondata: sotto tre non c'è nessuna dispersione da
/// guardare. Con due campioni il massimo e il minimo sono gli unici due valori,
/// e chiamare «peggiore osservata» il maggiore di due è un dato inventato con la
/// faccia di una misura — che è precisamente il guasto 22, uno zero mai
/// calcolato passato per una misura, in un'altra forma. Sotto la soglia il
/// comando **rifiuta di suggerire** e dice cosa c'è.
const RUNS_BEFORE_SUGGESTING: usize = 3;

/// Che cosa il deposito ha visto spendere a un flusso.
struct Observed {
    /// Le corse di quel flusso registrate, comunque siano andate.
    runs: usize,
    /// Quelle che hanno speso qualcosa di noto: le sole su cui si può contare.
    costed_runs: usize,
    /// La corsa più cara osservata, in micro.
    worst_run_micros: i64,
    /// La chiamata più cara osservata, in micro.
    dearest_call_micros: i64,
    /// Quante chiamate non hanno dichiarato un costo. Non entrano nelle cifre
    /// sopra, e chi legge un suggerimento deve sapere quante ne mancano.
    calls_without_cost: usize,
}

/// Il conto vero e proprio: una corsa per riga, e per ogni corsa il costo di
/// ciascuna sua chiamata — `None` quando quel motore non l'ha dichiarato.
///
/// **PRENDE I COSTI E NON IL DEPOSITO, PER POTER ESSERE PROVATO.** Il deposito
/// predefinito è uno solo per processo, e le prove girano in parallelo dentro lo
/// stesso: una prova che volesse puntarlo altrove dovrebbe scrivere una
/// variabile d'ambiente, cioè rovinare le altre a caso. Qui la regola — cos'è
/// una «corsa costata», quale sia la peggiore, quale la chiamata più cara — si
/// interroga senza aprire niente.
fn observed_from(runs: &[Vec<Option<i64>>]) -> Observed {
    let mut seen = Observed {
        runs: runs.len(),
        costed_runs: 0,
        worst_run_micros: 0,
        dearest_call_micros: 0,
        calls_without_cost: 0,
    };
    for calls in runs {
        let mut spent = 0i64;
        for call in calls {
            match call {
                Some(cost) => {
                    spent += cost;
                    seen.dearest_call_micros = seen.dearest_call_micros.max(*cost);
                }
                None => seen.calls_without_cost += 1,
            }
        }
        // **UNA CORSA COSTATA È UNA CHE HA SPESO, NON UNA CHE HA CHIAMATO.** Le
        // 28 corse su 34 che questo deposito porta a zero il 31/08/2026 sono il
        // guasto 22 — il costo era la costante zero fino al 30/08 — e contarle
        // come campioni farebbe scendere ogni suggerimento verso lo zero, cioè
        // verso un tetto che ferma ogni flusso prima del primo passo.
        if spent > 0 {
            seen.costed_runs += 1;
            seen.worst_run_micros = seen.worst_run_micros.max(spent);
        }
    }
    seen
}

/// Quel che il deposito sa della spesa di un flusso.
///
/// **UN DEPOSITO ASSENTE NON È UN ERRORE**: è una macchina su cui quel flusso
/// non è mai girato, e la risposta giusta è «zero corse», non un guasto. Chi
/// legge deve poter chiedere il tetto di un flusso appena scritto.
fn observed_spending(flow_id: &str) -> Result<Observed, String> {
    let Ok(dir) = default_ledger_dir() else {
        return Ok(observed_from(&[]));
    };
    let Some(data) = ui::gather::gather(&dir).map_err(|error| error.to_string())? else {
        return Ok(observed_from(&[]));
    };
    let runs: Vec<Vec<Option<i64>>> = data
        .runs
        .iter()
        .filter(|run| run.entity == flow_id)
        .map(|run| {
            data.calls_by_run
                .get(&run.run_id)
                .map(|calls| calls.iter().map(|call| call.cost_micros).collect())
                .unwrap_or_default()
        })
        .collect();
    Ok(observed_from(&runs))
}

/// Le micro-unità come le legge una persona.
fn in_units(micros: i64) -> String {
    format!("{:.2}", micros as f64 / MICROS_IN_A_UNIT)
}

/// **QUELLO CHE IL TETTO NON PROMETTE**, scritto ogni volta che un tetto c'è.
///
/// Senza queste due righe il tetto si legge come una garanzia sulla spesa, e non
/// lo è. Chi mette un tetto e poi trova una fattura più alta ha ragione di
/// sentirsi tradito: meglio dirglielo quando lo mette.
const WHAT_THE_CAP_DOES_NOT_PROMISE: &str = "\nquello che il tetto non promette:\
    \n  - non arriva ai motori: il freno sta prima di aprire un fronte, mai dentro \
      una chiamata già partita, e nessun motore sa che il tetto esiste\
    \n  - il primo fronte di una corsa non è mai frenato: senza nessuna chiamata \
      osservata la larghezza resta al soffitto di quattro, perché stringere su un \
      numero che non esiste sarebbe inventarlo\
    \n  - conta solo le chiamate che dichiarano un costo; quelle che non lo \
      dichiarano restano fuori dalla somma, quindi la spesa vera è più alta di \
      quella contata";

/// `sailor flow cap <nome>`: il tetto che c'è, e cosa il deposito ha visto.
fn cap_of(sources: &[FlowSource], name: &str) -> Result<String, String> {
    let (flow, origin) = one_flow(sources, name)?;
    let mut report = format!("flusso: {} ({origin})", flow.id);
    match flow.spend_cap_micros {
        None => report.push_str(
            "\ntetto: nessuno — questo flusso può spendere quanto la corsa richiede",
        ),
        Some(cap) => {
            let _ = write!(
                report,
                "\ntetto: {cap} micro ({} di costo equivalente)",
                in_units(cap)
            );
            report.push_str(WHAT_THE_CAP_DOES_NOT_PROMISE);
        }
    }

    report.push_str(&what_the_ledger_saw(&observed_spending(&flow.id)?));
    Ok(report)
}

/// Cosa il deposito ha visto, e se da lì esce un suggerimento.
///
/// Sta a parte da `cap_of` perché la regola delle tre corse si deve poter
/// interrogare senza un deposito: `cap_of` ne apre uno vero, e un deposito vero
/// in una prova è una variabile d'ambiente globale al processo.
fn what_the_ledger_saw(seen: &Observed) -> String {
    let mut said = format!(
        "\nnel deposito: {} corse, di cui {} {} speso qualcosa di noto",
        seen.runs,
        seen.costed_runs,
        if seen.costed_runs == 1 { "ha" } else { "hanno" }
    );
    if seen.calls_without_cost > 0 {
        let _ = write!(
            said,
            "\n{} chiamate non hanno dichiarato un costo, e non entrano in nessuna \
             delle cifre qui sopra",
            seen.calls_without_cost
        );
    }

    if seen.costed_runs < RUNS_BEFORE_SUGGESTING {
        let _ = write!(
            said,
            "\nnessun suggerimento: servono almeno {RUNS_BEFORE_SUGGESTING} corse \
             costate, e {}. Un numero calcolato su meno campioni è un dato \
             inventato con la faccia di una misura, e chi lo riceve ci appoggia \
             una decisione",
            match seen.costed_runs {
                0 => "non ce n'è nessuna".to_owned(),
                1 => "ce n'è una".to_owned(),
                many => format!("ce ne sono {many}"),
            }
        );
        return said;
    }

    // **PERCHÉ SI SOMMA LA CHIAMATA PIÙ CARA, E NON È PRUDENZA.** Il controllo
    // scatta *prima* di aprire un fronte, mai dentro una chiamata: la corsa si
    // ferma con la granularità di una chiamata, non di un micro. Un tetto messo
    // esattamente sulla peggiore osservata è quindi un tetto che taglia le corse
    // di quella taglia in modo imprevedibile — dipende da come la spesa si
    // distribuisce fra i fronti. La somma dice: «la corsa più cara che ho visto,
    // più la grana con cui so fermarmi».
    let suggested = seen.worst_run_micros + seen.dearest_call_micros;
    let _ = write!(
        said,
        "\nsuggerimento: {suggested} micro ({}) — la corsa più cara osservata \
         ({}) più la chiamata più cara osservata ({}). Il secondo addendo non è \
         un margine di sicurezza: è la grana con cui il tetto sa fermarsi, \
         perché il controllo sta prima di aprire un fronte",
        in_units(suggested),
        in_units(seen.worst_run_micros),
        in_units(seen.dearest_call_micros)
    );
    said
}

/// La parola che toglie il tetto invece di metterne uno.
///
/// Serve perché senza di lei il comando saprebbe entrare in uno stato e non
/// uscirne: `0` non è «nessuno», è «questo flusso non deve spendere niente».
const NO_CAP: &str = "nessuno";

/// `sailor flow cap <nome> <micro|nessuno>`: mette o toglie il tetto.
fn set_cap(sources: &[FlowSource], name: &str, value: &str) -> Result<String, String> {
    let wanted = if value == NO_CAP {
        None
    } else {
        let micros: i64 = value.parse().map_err(|_| {
            format!(
                "«{value}» non è un numero di micro-unità né la parola «{NO_CAP}». \
                 Un milione è un'unità di valuta: `sailor flow cap {name} 1000000` \
                 mette un tetto di 1,00"
            )
        })?;
        if micros < 0 {
            return Err(format!(
                "un tetto negativo non vuol dire niente: {micros}. Zero è «non deve \
                 spendere niente», «{NO_CAP}» è «nessuno ha messo un limite»"
            ));
        }
        Some(micros)
    };

    let (mut flow, source) = where_it_lives(sources, name)?;
    // **UN FLUSSO DI SISTEMA NON SI RISCRIVE, E NON SE NE SCRIVE UNO DI
    // NASCOSTO.** Sta dentro il binario: non c'è nessun file da modificare. La
    // strada è un omonimo in casa propria, che vince per la regola di
    // precedenza — ma quel file lo deve creare chi lo vuole, sapendo di averne
    // creato uno. Scriverne uno qui vorrebbe dire che da domani gira un flusso
    // diverso da quello spedito senza che nessuno l'abbia deciso, e la sola
    // traccia sarebbe l'origine in una colonna di `sailor flow list`.
    if source.is_builtin() {
        return Err(format!(
            "«{name}» è un flusso di sistema, spedito dentro il binario: non c'è \
             nessun file da riscrivere. Per dargli un tetto scrivi un flusso con lo \
             stesso nome in casa tua o nel progetto — vince il tuo — e mettilo lì. \
             Non lo faccio io: un flusso comparso da sé è un flusso che nessuno sa \
             di avere"
        ));
    }

    // **IL FILE SI CHIAMA COME L'`id`, O NE COMPARIREBBE UN SECONDO.** Il
    // registro indicizza per nome di file, la scrittura per `id`: dove i due
    // divergono, riscrivere non sostituirebbe niente — creerebbe un flusso
    // gemello che da domani vince o perde a seconda dell'ordine alfabetico. È lo
    // stesso rifiuto del flusso di sistema, per la stessa ragione: qui non
    // compare niente che nessuno abbia chiesto.
    let target = source.dir.join(format!("{}.flow.json", flow.id));
    if !target.exists() {
        return Err(format!(
            "«{name}» sta in un file che non si chiama «{}.flow.json», che è come si \
             chiamerebbe scrivendolo: riscriverlo creerebbe un secondo flusso invece \
             di sostituire questo. Rinomina il file come il suo `id`, o cambia l'`id` \
             perché coincida col nome del file",
            flow.id
        ));
    }

    let before = flow.spend_cap_micros;
    if before == wanted {
        return Ok(format!(
            "flusso {name} ({}): il tetto era già {}, non ho toccato niente",
            source.origin,
            said_cap(before)
        ));
    }
    flow.spend_cap_micros = wanted;
    flow::system::save_in(&source.dir, &flow)?;
    Ok(format!(
        "flusso {name} ({}): tetto {} → {}; scritto in {}",
        source.origin,
        said_cap(before),
        said_cap(wanted),
        source.dir.display()
    ))
}

/// Un tetto come lo legge una persona, compreso quando non c'è.
fn said_cap(cap: Option<i64>) -> String {
    match cap {
        None => NO_CAP.to_owned(),
        Some(micros) => format!("{micros} micro ({})", in_units(micros)),
    }
}

/// Il flusso **e la sorgente da cui viene**: per riscriverlo serve la cartella,
/// non il nome dell'origine.
///
/// Si guarda dalla più specifica alla meno specifica, cioè al contrario
/// dell'ordine in cui le sorgenti sono elencate: a parità di nome vince
/// l'ultima, e chi riscrive deve riscrivere **quella che gira**. Riscrivere la
/// copia meno specifica lascerebbe il comando a dire «fatto» mentre la corsa
/// continua a leggere l'altra.
fn where_it_lives<'a>(
    sources: &'a [FlowSource],
    name: &str,
) -> Result<(FlowFile, &'a FlowSource), String> {
    for source in sources.iter().rev() {
        match flow::system::registry_of(source).remove(name) {
            Some(Ok(flow)) => return Ok((flow, source)),
            Some(Err(reason)) => {
                return Err(format!(
                    "il flusso {name} ({}) non si carica, quindi non lo riscrivo: {reason}",
                    source.origin
                ))
            }
            None => continue,
        }
    }
    match one_flow(sources, name) {
        // Lo stesso messaggio di `one_flow`, con lo stesso elenco di nomi: due
        // parole diverse per lo stesso «non c'è» manderebbero a cercare due
        // difetti dove ce n'è uno.
        Err(reason) => Err(reason),
        // Non può succedere — `one_flow` legge le stesse sorgenti del giro qui
        // sopra — e se succedesse vorrebbe dire che le due strade che cercano un
        // flusso si sono separate. Dirlo vale più che panicare in mano a chi sta
        // usando il comando.
        Ok(_) => Err(format!(
            "il flusso {name} si carica ma non risulta in nessuna sorgente: le due \
             strade che cercano i flussi si sono separate"
        )),
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
    "uso: sailor flow <list|due|check <nome>|run <nome> [mandato]|cost <nome>|\
     cap <nome> [micro|nessuno]>"
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
    // **IL TETTO STA NEL RAPPORTO, E CON LUI CIÒ CHE NON PROMETTE.** Chi
    // controlla un flusso prima di lanciarlo sta decidendo se può permetterselo:
    // un tetto invisibile qui si scopre solo a corsa fermata, e uno che si vede
    // senza i suoi limiti si legge come una garanzia sulla spesa — che non è.
    // La riga c'è sempre, anche quando il tetto non c'è: «nessuno» è
    // un'informazione, e un rapporto che tace quando non c'è niente da dire
    // lascia chi legge a chiedersi se il controllo abbia guardato.
    match flow.spend_cap_micros {
        None => report.push_str("\ntetto di spesa: nessuno"),
        Some(cap) => {
            let _ = write!(
                report,
                "\ntetto di spesa: {cap} micro ({} di costo equivalente){WHAT_THE_CAP_DOES_NOT_PROMISE}",
                in_units(cap)
            );
        }
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
        let engines: Vec<String> = match with.get("tool") {
            Some(Value::String(id)) => vec![id.clone()],
            Some(Value::Array(chain)) => chain
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            _ => Vec::new(),
        };
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

        let (report, _) = check_report(&flow, &default_registry(None, None), None);

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

        let (report, _) = check_report(&flow, &default_registry(None, None), None);

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

        let (report, unknown) = check_report(&flow, &default_registry(None, None), Some(&tools));

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

        let (report, _) = check_report(&flow, &default_registry(None, None), Some(&tools));

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

        let (report, _) = check_report(&flow, &default_registry(None, None), Some(&tools));

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

        let (report, _) = check_report(&flow, &default_registry(None, None), Some(&tools));

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

        let (report, unknown) = check_report(&flow, &default_registry(None, None), None);

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

        let registry = default_registry(None, None);
        let (said_without, _) = check_report(&without, &registry, None);
        let (said_with, _) = check_report(&with, &registry, None);

        assert_ne!(
            said_without, said_with,
            "il tetto non compare nel rapporto: {said_with}"
        );
        assert!(said_without.contains("tetto di spesa: nessuno"), "{said_without}");
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

        let (report, _) = check_report(&flow, &default_registry(None, None), None);

        assert!(report.contains("non arriva ai motori"), "{report}");
        assert!(report.contains("primo fronte"), "{report}");
        assert!(report.contains("restano fuori dalla somma"), "{report}");
    }

    /// **SOTTO TRE CORSE COSTATE NON SI SUGGERISCE NIENTE, E SI DICE PERCHÉ.**
    ///
    /// È la regola che tiene fuori il guasto 22: nel deposito di questa macchina
    /// il 31/08/2026 sei corse su trentaquattro hanno un costo diverso da zero,
    /// e nessun flusso ne ha tre. Una mediana su quella colonna darebbe zero per
    /// ogni flusso, cioè un tetto che ferma ogni corsa prima del primo passo.
    #[test]
    fn under_three_costed_runs_no_cap_is_suggested() {
        let two = observed_from(&[vec![Some(100)], vec![Some(200)]]);

        let said = what_the_ledger_saw(&two);

        assert!(said.contains("nessun suggerimento"), "{said}");
        // La riga del suggerimento comincia a capo: cercare «suggerimento: »
        // senza l'a-capo troverebbe anche «nessun suggerimento: ».
        assert!(!said.contains("\nsuggerimento: "), "{said}");
        assert!(said.contains("ce ne sono 2"), "e dice cosa c'è: {said}");
    }

    /// **CON TRE, IL SUGGERIMENTO È LA PEGGIORE PIÙ LA CHIAMATA PIÙ CARA.**
    ///
    /// La gemella di quella sopra: senza di lei un comando che non suggerisse
    /// mai passerebbe l'altra e non servirebbe a niente.
    #[test]
    fn with_three_costed_runs_the_suggestion_is_the_worst_plus_the_dearest_call() {
        let three = observed_from(&[
            vec![Some(100), Some(50)],
            vec![Some(400), Some(300)],
            vec![Some(200)],
        ]);

        let said = what_the_ledger_saw(&three);

        // Peggiore corsa 700, chiamata più cara 400: 1100.
        assert!(said.contains("\nsuggerimento: 1100 micro"), "{said}");
        assert!(!said.contains("nessun suggerimento"), "{said}");
    }

    /// **UNA CORSA CHE NON HA SPESO NON È UN CAMPIONE.**
    ///
    /// Ventotto delle trentaquattro corse di questo deposito portano zero perché
    /// il costo *era* la costante zero fino al 30/08/2026. Contarle farebbe
    /// scendere ogni suggerimento verso lo zero — cioè verso un tetto che ferma
    /// tutto — con l'aria di una misura su molti campioni.
    #[test]
    fn runs_that_spent_nothing_are_not_samples() {
        let seen = observed_from(&[
            vec![Some(0)],
            vec![],
            vec![None, None],
            vec![Some(900)],
        ]);

        assert_eq!(seen.runs, 4, "le corse ci sono tutte");
        assert_eq!(seen.costed_runs, 1, "ma una sola ha speso");
        assert_eq!(seen.worst_run_micros, 900);
        assert_eq!(seen.calls_without_cost, 2, "e due chiamate restano fuori");
    }

    /// **UN FLUSSO DI SISTEMA NON SI RISCRIVE, E NON NE COMPARE UNO NUOVO.**
    ///
    /// Sta dentro il binario: non c'è nessun file da modificare. La strada è un
    /// omonimo in casa propria — che quel file lo crei chi lo vuole, sapendo di
    /// averlo creato. Un flusso comparso da sé cambierebbe cosa gira senza che
    /// nessuno l'abbia deciso.
    #[test]
    fn a_system_flow_refuses_the_cap_instead_of_growing_a_twin() {
        let home = TestDirectory::new();
        let sources = flow::system::sources(&home.0, None, None);
        let shipped = flow::system::FLOWS[0].0;

        let error = set_cap(&sources, shipped, "1000000").expect_err("un flusso di sistema");

        assert!(error.contains("di sistema"), "{error}");
        assert!(
            entries_of(&home.0).is_empty(),
            "non deve essere comparso nessun file in casa: {:?}",
            entries_of(&home.0)
        );
    }

    /// Mettere il tetto scrive **nella cartella da cui il flusso viene**, e non
    /// tocca nient'altro del file.
    #[test]
    fn setting_the_cap_writes_where_the_flow_lives() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        let said = set_cap(&sources, "prova", "750000").expect("il tetto si scrive");

        assert!(said.contains("750000 micro"), "{said}");
        let after = written_flow(&home.0, "prova");
        assert_eq!(after.spend_cap_micros, Some(750_000));
        assert_eq!(after.description, "flusso di prova", "il resto è intatto");
        assert_eq!(after.graph.steps().len(), 1);
        assert_eq!(
            entries_of(&home.0).len(),
            1,
            "e non è comparso nessun gemello: {:?}",
            entries_of(&home.0)
        );
    }

    /// **UN FILE CHE NON SI CHIAMA COME IL PROPRIO `id` NON SI RISCRIVE.**
    ///
    /// Il registro indicizza per nome di file, la scrittura per `id`: dove i due
    /// divergono, riscrivere creerebbe un secondo flusso invece di sostituire
    /// questo. Senza questo rifiuto il comando direbbe «fatto» lasciando in
    /// cartella due flussi con lo stesso `id`.
    #[test]
    fn a_file_named_differently_from_its_id_is_refused_instead_of_duplicated() {
        let home = TestDirectory::new();
        home.write("altro-nome.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        let error = set_cap(&sources, "altro-nome", "500").expect_err("nome e id divergono");

        assert!(error.contains("secondo flusso"), "{error}");
        assert_eq!(entries_of(&home.0).len(), 1, "nessun gemello sul disco");
    }

    /// Un valore che non è un numero né «nessuno» si rifiuta dicendo cos'è un
    /// micro: chi sbaglia unità mette un tetto mille volte più basso di quello
    /// che credeva, e la corsa si ferma senza che lui capisca perché.
    #[test]
    fn a_cap_that_is_not_a_number_is_refused_with_the_unit_spelled_out() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        let error = set_cap(&sources, "prova", "1,50").expect_err("non è un numero di micro");
        assert!(error.contains("micro"), "{error}");

        let negative = set_cap(&sources, "prova", "-1").expect_err("un tetto negativo");
        assert!(negative.contains("negativo"), "{negative}");
    }

    /// **`nessuno` TOGLIE IL TETTO, E NON LO METTE A ZERO.** Senza questa parola
    /// il comando saprebbe entrare in uno stato e non uscirne: `0` è «non deve
    /// spendere niente», che ferma la corsa prima del primo passo.
    #[test]
    fn the_word_for_no_cap_clears_it_instead_of_setting_zero() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);
        set_cap(&sources, "prova", "500").expect("prima si mette");

        set_cap(&sources, "prova", NO_CAP).expect("poi si toglie");

        assert_eq!(written_flow(&home.0, "prova").spend_cap_micros, None);
    }

    fn written_flow(dir: &std::path::Path, name: &str) -> FlowFile {
        let text = fs::read_to_string(dir.join(format!("{name}.flow.json")))
            .expect("il flusso scritto si rilegge");
        serde_json::from_str(&text).expect("e si deserializza")
    }

    fn entries_of(dir: &std::path::Path) -> Vec<String> {
        fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
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
