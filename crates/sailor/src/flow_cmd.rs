//! `sailor flow`: carica i file dichiarativi da `flows/`, mostra anche quelli
//! guasti, controlla che le azioni nominate esistano ed esegue il grafo nel
//! deposito durevole comune di Sailor.

// Il formato del file vive nel crate del flusso: qui si importa, non si
// ridichiara. Averlo scritto due volte, il 28/08/2026, li ha fatti coincidere
// per fortuna e non per costruzione.
use flow::{
    ActionRegistry, Execution, Executor, FlowFile, Graph, InProcessExecutor, RecordStore,
    SystemClock,
};
use actions::reference;
use ledger::Ledger;
use models::pricing::{Known, PriceList};
use serde_json::Value;
use ui::gather::FlowSource;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
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
        [command, run_id] if command == "resume" => resume_run(run_id),
        [command, name] if command == "cost" => cost_of(name),
        [command, name] if command == "relocate" => relocate_flow(sources, name, None),
        [command, name, from] if command == "relocate" => relocate_flow(sources, name, Some(from)),
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
    Ok(spending_report(&view, &actions::current_price_list()))
}

/// Il consumo di una corsa, per una persona.
///
/// **IL LISTINO ARRIVA DA FUORI, E NON È PIGNOLERIA:** così questa funzione si
/// interroga con un listino scritto nella prova, invece di dipendere da quale
/// file esista sulla macchina che esegue la batteria.
fn spending_report(view: &ui::dashboard::ExecutionView, prices: &PriceList) -> String {
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
    // **LA CIFRA NON SI COMPONE QUI.** Se la scrivesse questa funzione,
    // rifarebbe la regola dei tre casi in un `format!` — e la prima volta che
    // qualcuno tocca uno dei due posti le due versioni divergono in silenzio.
    // Chi decide quanti passi aprire e chi legge il consumo devono leggere la
    // stessa frase.
    let _ = write!(report, "\n{}", ui::dashboard::how_the_cost_reads(&tokens.cost_reading()));
    // **QUELLO CHE MANCA SI DICE, O IL TOTALE SI LEGGE COME COMPLETO.** È la
    // stessa regola della finestra: una somma che tace su ciò che non ha
    // contato è una rassicurazione, non una misura. Resta anche adesso che il
    // costo lo dice da sé: i token mancanti sono un'altra lacuna, e una corsa
    // può avere quella e non l'altra.
    if tokens.calls_without_tokens > 0 || tokens.calls_without_cost > 0 {
        let _ = write!(
            report,
            "\nparziale: {} chiamate senza token dichiarati, {} senza costo noto",
            tokens.calls_without_tokens, tokens.calls_without_cost
        );
    }
    // **E SI DICE QUALE MODELLO, NON SOLO QUANTE CHIAMATE.** «Tre senza costo
    // noto» è un numero su cui non si può agire; il nome del modello scoperto è
    // una riga da scrivere nel listino. È la seconda metà della cura del guasto
    // 35: chi non ha un prezzo per un modello deve saperlo, non dedurlo da uno
    // zero. I nomi vengono da `tokens_by_model`, cioè da chi ha davvero
    // risposto in questa corsa.
    let unpriced = cannot_be_priced(
        prices,
        &view
            .tokens_by_model
            .keys()
            .filter(|name| !name.trim().is_empty())
            .cloned()
            .collect(),
    );
    if !unpriced.is_empty() {
        let _ = write!(
            report,
            "\nmodelli che il listino non sa prezzare: {}\
             \n  il loro consumo è nei token qui sopra, il loro costo no: la cifra in \
             valuta è più bassa di quella vera",
            unpriced.join(", ")
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

/// I modelli che le corse passate di questo flusso hanno davvero usato.
///
/// **DAL DEPOSITO E NON DAL FLUSSO, PERCHÉ IL FLUSSO NON LO SA.** Un passo nomina
/// lo strumento — `claude-code`, `codex` — non il modello: quale modello risponda
/// lo decide quella riga di comando, e lo si scopre solo dopo, da ciò che ha
/// dichiarato. Un elenco dedotto dal file del flusso sarebbe indovinato.
///
/// **`None` NON È UN INSIEME VUOTO, ED È LA DISTINZIONE CHE CONTA.** Un deposito
/// che non si apre — non c'è, non si legge, i permessi lo negano — non dice
/// «questo flusso non ha mai usato nessun modello»: dice che nessuno ha potuto
/// guardare. Confonderli farebbe stampare «mai girato qui» a un flusso girato
/// cento volte, ed è la stessa regola per cui il rilevatore assente fa tacere il
/// rapporto invece di dichiarare sano ciò che non ha visto.
fn models_seen_by(flow_id: &str) -> Option<BTreeSet<String>> {
    let dir = default_ledger_dir().ok()?;
    let data = ui::gather::gather(&dir).ok()??;
    Some(
        data.runs
            .iter()
            .filter(|run| run.entity == flow_id)
            .filter_map(|run| data.calls_by_run.get(&run.run_id))
            .flatten()
            // Una chiamata senza modello dichiarato non è un modello senza
            // prezzo: è un motore che non dice chi ha risposto, e nominare la
            // stringa vuota fra i modelli scoperti manderebbe a cercare una
            // voce di listino per un nome che non esiste.
            .filter(|call| !call.actual_model.trim().is_empty())
            .map(|call| call.actual_model.clone())
            .collect(),
    )
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

// ── quello che il listino non sa prezzare ────────────────────────────────

/// I nomi che questo listino non sa prezzare, ciascuno col perché.
///
/// **IL PERCHÉ STA ACCANTO AL NOME PERCHÉ SONO DUE RIPARAZIONI DIVERSE.** Un
/// nome che il listino non conosce si ripara aggiungendo una voce — o un alias,
/// se è lo stesso modello con un altro nome; una voce che c'è ma non ha i prezzi
/// si ripara scrivendo i prezzi. Un elenco di soli nomi manderebbe a riscrivere
/// una voce che esiste già.
fn cannot_be_priced(prices: &PriceList, seen: &BTreeSet<String>) -> Vec<String> {
    seen.iter()
        .filter_map(|name| match prices.knows(name) {
            Known::Priced => None,
            Known::Absent => Some(format!("{name} (nessuna voce nel listino)")),
            Known::ListedWithoutPrice => Some(format!("{name} (voce senza prezzi)")),
        })
        .collect()
}

/// Che cosa il listino sa dire di questo flusso, **prima** di lanciarlo.
///
/// **PERCHÉ UN FRENO CHE NON FRENA SI DEVE VEDERE PRIMA.** È la seconda metà
/// della cura del guasto 35, e senza di lei la prima non basta: adesso il
/// listino viaggia col prodotto, ma i modelli che nessuno ha prezzato — quelli di
/// OpenAI e di Google, tenuti fuori di proposito perché nessuno ne ha verificato
/// i prezzi — continuano a lasciare il costo sconosciuto. Chi non ha un prezzo
/// per un modello deve **saperlo**, non scoprirlo con uno zero.
///
/// **I MODELLI ARRIVANO DAL DEPOSITO, NON DAL FLUSSO.** Un passo nomina lo
/// strumento, non il modello: nessuno *chiede* un modello, e indovinarne uno
/// sarebbe inventarlo. L'unica fonte onesta è chi ha già risposto, cioè le corse
/// passate — per questo un flusso mai girato qui non riceve un elenco vuoto ma
/// una frase che dice che non si sa, come per il rilevatore assente.
fn what_is_priced(
    prices: &PriceList,
    seen: Option<&BTreeSet<String>>,
    cap: Option<i64>,
) -> String {
    let mut said = format!("\nlistino: {} modelli prezzati", prices.entries.len());
    let Some(seen) = seen else {
        said.push_str(
            "\nmodelli: il deposito non si è potuto leggere, quindi non si sa quali \
             modelli questo flusso abbia usato",
        );
        return said;
    };
    if seen.is_empty() {
        said.push_str(
            "\nmodelli: questo flusso non è mai girato qui, quindi non si sa quali \
             risponderanno né se il listino saprà prezzarli",
        );
        return said;
    }
    let unpriced = cannot_be_priced(prices, seen);
    if unpriced.is_empty() {
        let _ = write!(
            said,
            "\nmodelli usati dalle corse passate: tutti prezzati ({})",
            seen.iter().cloned().collect::<Vec<_>>().join(", ")
        );
        return said;
    }
    let _ = write!(
        said,
        "\nmodelli usati dalle corse passate che il listino non sa prezzare: {}\
         \n  il loro costo resta sconosciuto, non zero: la spesa contata è più bassa \
         di quella vera\
         \n  si ripara scrivendo la voce in ~/.config/sailor/pricing.json, che vince su \
         quella spedita",
        unpriced.join(", ")
    );
    // **LA RIGA DEL TETTO SOLO QUANDO LE DUE COSE COINCIDONO.** Un tetto senza
    // modelli scoperti non ha niente da dichiarare, e modelli scoperti senza
    // tetto non fermano niente: è la coincidenza a essere pericolosa, ed è la
    // frase per cui il guasto 35 è stato scritto — un freno che non frena si
    // deve vedere prima di lanciare, non a fattura arrivata.
    if cap.is_some() {
        said.push_str(
            "\n  e il tetto di spesa non conterà quelle chiamate: si fermerà più tardi \
             di quanto dichiara, o non si fermerà affatto",
        );
    }
    said
}

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
    "uso: sailor flow <list|due|check <nome> [--no-engines]|run <nome> [mandato]|\
     resume <corsa>|cost <nome>|cap <nome> [micro|nessuno]|\
     relocate <nome> [prefisso-da-togliere]>"
        .to_owned()
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
fn resume_run(run_id: &str) -> Result<String, String> {
    let ledger = crate::step_cmd::open_ledger()?;
    let flow = crate::step_cmd::flow_of_run(&ledger, run_id)?;
    resume_run_in(&ledger, &flow, run_id)
}

/// Il corpo di `resume`, col deposito e il flusso dichiarati invece che dedotti
/// da `HOME` e dalla cartella corrente: sono tutti e due globali al processo, e
/// una prova che li scrivesse rovinerebbe le altre a caso.
fn resume_run_in(ledger: &Ledger, flow: &FlowFile, run_id: &str) -> Result<String, String> {
    let header = ledger
        .run_header(run_id)
        .map_err(|error| format!("non riesco a leggere la corsa {run_id}: {error}"))?
        .ok_or_else(|| format!("nessuna corsa si chiama {run_id} in questo deposito"))?;
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

    let mut store = ledger.clone();
    let mut clock = SystemClock;
    // **LA RIPRESA PASSA DAL COSTRUTTORE UNICO**, come la prima corsa. Costruire
    // qui una richiesta a mano rimetterebbe in piedi la seconda copia che
    // `registry::execution_request` esiste per togliere — e questa copia
    // perderebbe in silenzio la radice del workspace, facendo lavorare i passi
    // riconciliati in un posto diverso da quello dove sono nati.
    let root = workspace_root();
    announce_root(root.as_deref());
    // LO STESSO IDENTIFICATIVO, e non uno nuovo: una ripresa che aprisse una
    // corsa nuova perderebbe i passi già andati e li rifarebbe tutti, pagandoli
    // due volte.
    let request = registry::execution_request(flow, run_id, root.as_deref());
    // La riconciliazione vede quello che vedrà l'esecuzione: stessa radice,
    // stesso stato condiviso.
    let shared = request.shared.clone();
    let probe = HandoffLease { now };
    let reconciled = InProcessExecutor
        .reconcile(flow::ReconciliationRequest {
            graph: &flow.graph,
            run_id,
            store: &mut store,
            actions: &registry,
            shared: &shared,
            processes: &probe,
            clock: &mut clock,
        })
        .map_err(|error| format!("non riesco a riconciliare la corsa {run_id}: {error}"))?;

    let mut report = format!("corsa {run_id} — flusso {}", flow.id);
    if !reconciled.still_running.is_empty() {
        let _ = write!(
            report,
            "\ntenuti, la scadenza non è passata: {}",
            reconciled.still_running.join(", ")
        );
    }
    if !reconciled.closed_as_broke.is_empty() {
        let _ = write!(
            report,
            "\nscaduti e rimessi fra i pronti: {}",
            reconciled.closed_as_broke.join(", ")
        );
    }
    if !reconciled.closed_as_waiting.is_empty() {
        let _ = write!(
            report,
            "\nlasciati a una persona: {}",
            reconciled.closed_as_waiting.join(", ")
        );
    }

    let execution = InProcessExecutor
        .execute(&flow.graph, request, ledger, &registry, &SystemClock)
        .map_err(|error| format!("la ripresa della corsa {run_id} è fallita: {error}"))?;

    let (status, exit_ok) = execution_status(&execution);
    let why = registry::stopped_by_cap(&execution);
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

/// Le corse ferme in attesa di qualcuno, in coda a un elenco.
///
/// **STA IN CODA A `list` E A `due` PERCHÉ È LÌ CHE SI GUARDA.** Una consegna
/// che nessuno raccoglie non compare da nessuna parte: non è un passo aperto,
/// quindi `unfinished_runs` non la trova, e il flusso da cui viene risulta
/// «girato di recente», quindi `due` lo dichiara non dovuto. Sparisce due volte.
fn waiting_report() -> String {
    let waiting = default_ledger_dir()
        .ok()
        .filter(|dir| dir.join("state.db").exists())
        .and_then(|dir| Ledger::open(&dir).ok())
        .and_then(|ledger| ledger.waiting_runs().ok())
        .unwrap_or_default();
    if waiting.is_empty() {
        return "nessuna corsa in attesa di qualcuno".to_owned();
    }
    let mut report = format!("{} corse aspettano qualcuno:", waiting.len());
    for run in waiting {
        let _ = write!(
            report,
            "\n  {}\t{}\tsailor flow resume {}",
            run.run_id, run.entity, run.run_id
        );
    }
    report
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
        "{due} dovuti adesso; {unplanned} senza pianificazione, che partono solo a mano\n{}",
        waiting_report()
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
    let _ = write!(report, "{}", waiting_report());
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
    // **UNO STATO DEI PROFILI ILLEGGIBILE NON FERMA IL CONTROLLO**, per la
    // stessa ragione per cui non ferma una corsa: si guarda un mondo senza
    // profili, e la sezione delle case tace invece di dire una cosa falsa.
    let profiles = profiles::store_io::load_store().unwrap_or_default();
    let world = EngineWorld {
        probe: &real,
        profiles: &profiles,
    };
    let (mut report, unknown) = check_report(
        &flow,
        &default_registry(open_default_ledger(), None),
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
        return Err(format!(
            "il flusso {} ha un percorso assoluto in un campo di posizione: {}. \
             Un flusso non deve sapere dove sta il repository — la radice viene da chi \
             lancia. Si toglie con «sailor flow relocate {}».",
            flow.id,
            stuck.join("; "),
            flow.id
        ));
    }
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
struct EngineWorld<'a> {
    probe: &'a dyn actions::EngineProbe,
    profiles: &'a profiles::ProfileStore,
}

#[cfg(test)]
impl<'a> EngineWorld<'a> {
    /// Un mondo in cui **nessun profilo è dichiarato**: è lo stato di una
    /// macchina appena installata, ed è quello giusto per le prove che parlano
    /// delle righe di comando e non delle case. Senza profilo attivo la sezione
    /// delle credenziali tace, quindi quelle prove restano su ciò che provano.
    fn without_profiles(probe: &'a dyn actions::EngineProbe) -> Self {
        static NO_PROFILES: std::sync::OnceLock<profiles::ProfileStore> =
            std::sync::OnceLock::new();
        Self {
            probe,
            profiles: NO_PROFILES.get_or_init(profiles::ProfileStore::default),
        }
    }
}

fn check_report(
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

    // **IL GUASTO 25, DETTO PRIMA DI PARTIRE.** Un `workdir` assoluto non si
    // vede eseguendo: si vede dopo, guardando quale repository si è sporcato.
    let (fatal, advisory): (Vec<HardcodedPath>, Vec<HardcodedPath>) =
        hardcoded_paths(flow).into_iter().partition(|path| path.fatal);
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

/// Toglie da un flusso i percorsi assoluti che stanno sotto la radice.
///
/// **PERCHÉ LO FA UN COMANDO E NON UNO SCRIPT.** È il guasto 15: il 29/08/2026
/// per cambiare l'innesco di un flusso è stato usato uno script Python che
/// riscriveva il JSON, perché `sailor flow` aveva solo `list`, `due`, `check`
/// e `run`. Uno strumento che si aggira non registra niente di ciò che gli
/// succede intorno.
///
/// **RISCRIVE I CAMPI, NON I PROMPT.** Un `workdir` è un campo: il suo valore
/// ha un significato solo per il programma, e sostituirlo è una traduzione. Il
/// testo di un prompt è un'istruzione scritta da una persona per un'altra
/// intelligenza: riscriverlo è riscrivere l'istruzione, e nessuno ha chiesto a
/// questo comando di farlo. Quelli li **stampa** e basta.
///
/// **IL PREFISSO SI PUÒ DICHIARARE, PERCHÉ IL CASO NORMALE È UN ALTRO ALBERO.**
/// Un flusso da spostare quasi sempre nomina la copia su cui è stato scritto —
/// un altro clone, o la macchina di qualcun altro — e quel percorso **non sta
/// sotto** la radice di chi lo sta spostando. Senza dirlo, il comando non può
/// sapere se `/Users/tizio/progetto` volesse dire «la radice» o una cartella
/// vera che deve restare dov'è: indovinare qui vorrebbe dire riscrivere un
/// percorso legittimo. Si dichiara come secondo argomento — posizionale come
/// il mandato di `run`, che è la forma di questa riga di comando — e quello che
/// non combacia si vede nel rapporto invece di sparire.
fn relocate_flow(
    sources: &[FlowSource],
    name: &str,
    from: Option<&str>,
) -> Result<String, String> {
    let root = workspace_root().ok_or_else(|| {
        format!(
            "non c'è nessuna radice di progetto risalendo da qui: manca un {}. \
             Si crea con «sailor workspace init»",
            flow::workspace::MARKER
        )
    })?;
    // Il prefisso da togliere: quello dichiarato, o la radice stessa quando il
    // flusso è stato scritto proprio qui.
    let old_root = from.map(PathBuf::from).unwrap_or_else(|| root.clone());
    let path = flow_file_path(sources, name)?;
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("non riesco a leggere {}: {error}", path.display()))?;
    // Si lavora sul documento grezzo e non sul `FlowFile` tipato: un flusso può
    // avere campi che questa versione non conosce, e riscriverlo dal tipo li
    // perderebbe in silenzio — è il guasto 8 applicato a un file dell'utente.
    let mut document: Value = serde_json::from_str(&text)
        .map_err(|error| format!("{} non è un JSON valido: {error}", path.display()))?;

    let (moved, left_alone) = relocate_workdirs(&mut document, &old_root)
        .ok_or_else(|| format!("{} non ha passi da spostare", path.display()))?;

    if !moved.is_empty() {
        let mut rewritten = serde_json::to_string_pretty(&document)
            .map_err(|error| format!("non riesco a ricomporre il flusso: {error}"))?;
        rewritten.push('\n');
        std::fs::write(&path, rewritten)
            .map_err(|error| format!("non riesco a scrivere {}: {error}", path.display()))?;
    }

    let mut report = format!(
        "radice: {}\nprefisso tolto: {}\nflusso: {}",
        root.display(),
        old_root.display(),
        path.display()
    );
    let _ = write!(
        report,
        "\ncampi spostati: {}",
        if moved.is_empty() {
            "nessuno".to_owned()
        } else {
            format!("{}\n  {}", moved.len(), moved.join("\n  "))
        }
    );
    if !left_alone.is_empty() {
        let _ = write!(
            report,
            "\nfuori dal prefisso, lasciati come stanno \
             (il prefisso si dichiara come secondo argomento): {}",
            left_alone.join("; ")
        );
    }
    // I percorsi dentro i testi si mostrano e non si toccano: chi legge decide.
    let flow: FlowFile = serde_json::from_str(&text)
        .map_err(|error| format!("{} non è un flusso valido: {error}", path.display()))?;
    let in_text: Vec<String> = hardcoded_paths(&flow)
        .iter()
        .filter(|found| !found.fatal)
        .map(|found| format!("{} in «{}» ({})", found.step, found.field, found.value))
        .collect();
    if !in_text.is_empty() {
        let _ = write!(
            report,
            "\n\nDA CORREGGERE A MANO — percorsi dentro un testo, {} in tutto:\n  {}\n\
             Non li riscrivo: un prompt è un'istruzione, e riscriverla è deciderla.",
            in_text.len(),
            in_text.join("\n  ")
        );
    }
    Ok(report)
}

/// Toglie il prefisso dai `workdir` del documento, senza toccare il disco.
///
/// Sta separata dal comando perché è la parte che si può provare: quella
/// intorno legge la cartella corrente e scrive un file, e una prova che
/// cambiasse la cartella del processo rovinerebbe le altre che girano insieme
/// — è il guasto 21, che qui si evita non avendo bisogno del processo.
///
/// Torna `None` se il documento non ha nemmeno un elenco di passi.
fn relocate_workdirs(
    document: &mut Value,
    old_root: &Path,
) -> Option<(Vec<String>, Vec<String>)> {
    let mut moved = Vec::new();
    let mut left_alone = Vec::new();
    let steps = document
        .get_mut("graph")
        .and_then(|graph| graph.get_mut("steps"))
        .and_then(Value::as_array_mut)?;
    for step in steps {
        let step_id = step
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("(senza id)")
            .to_owned();
        let Some(with) = step.get_mut("with").and_then(Value::as_object_mut) else {
            continue;
        };
        // Solo un testo: un `{"$from": …}` è un rinvio, e va risolto a
        // esecuzione da chi sa contro cosa. Riscriverlo sarebbe inventare.
        let Some(Value::String(declared)) = with.get(WORKDIR_KEY).cloned() else {
            continue;
        };
        match relative_to(old_root, &declared) {
            // Coincide con la radice: il campo non serve più, e un `workdir`
            // che vale la radice è rumore che invita a riscriverlo assoluto.
            Some(rest) if rest.is_empty() => {
                // **`shift_remove` E NON `remove`.** Con `preserve_order`
                // acceso — e lo è — `remove` è uno *swap*: tira l'ultima
                // chiave dentro il buco e riordina il file. Un comando che
                // toglie un campo e in cambio rimescola l'oggetto produce un
                // diff illeggibile, e chi lo rilegge non distingue più ciò che
                // è stato deciso da ciò che è stato spostato. Misurato il
                // 31/08/2026: 62 righe cambiate al posto di 7.
                with.shift_remove(WORKDIR_KEY);
                moved.push(format!("{step_id}: tolto (era la radice)"));
            }
            Some(rest) => {
                with.insert(WORKDIR_KEY.to_owned(), Value::String(rest.clone()));
                moved.push(format!("{step_id}: «{declared}» → «{rest}»"));
            }
            // Fuori dal prefisso: non è questo comando a decidere cosa voleva
            // dire chi l'ha scritto.
            None => left_alone.push(format!("{step_id}: «{declared}»")),
        }
    }
    Some((moved, left_alone))
}

/// Il file da cui viene un flusso, cercato nelle sorgenti che sono cartelle.
fn flow_file_path(sources: &[FlowSource], name: &str) -> Result<PathBuf, String> {
    // Si guarda dalla più specifica alla meno: è quella che vince a esecuzione,
    // e riscrivere una che non gira lascerebbe il guasto dov'era.
    for source in sources.iter().rev() {
        if source.is_builtin() {
            continue;
        }
        let candidate = source.dir.join(format!("{name}.flow.json"));
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "il flusso {name} non è un file su disco: i flussi spediti col prodotto \
         stanno dentro il binario e non si riscrivono — se ne scrive uno con lo \
         stesso nome nel progetto"
    ))
}

/// Il resto di `path` sotto `root`, o `None` se non ci sta sotto.
///
/// Restituisce la stringa vuota quando i due coincidono: è il caso in cui il
/// campo va tolto, non riscritto.
fn relative_to(root: &Path, path: &str) -> Option<String> {
    let candidate = Path::new(path);
    let rest = candidate.strip_prefix(root).ok()?;
    Some(rest.display().to_string())
}

/// Il campo che dice dove un passo lavora. Il nome sta nel crate del flusso:
/// due costanti con lo stesso valore in due crate sono il guasto 10 in piccolo.
const WORKDIR_KEY: &str = flow::WORKDIR_FIELD;

/// I campi che dicono **dove** un passo lavora o **quale** binario esegue.
///
/// Un percorso assoluto qui non è un dettaglio del testo: è la posizione in cui
/// il passo lavorerà davvero, ed è il guasto 25 parola per parola — sette passi
/// con `"workdir": "/home/someone/personal/sailor"`, un flusso che lanciato da un
/// clone commetteva nel repository principale senza dirlo.
const POSITION_FIELDS: [&str; 2] = ["workdir", "bin"];

/// I prefissi che fanno di un pezzo di testo un percorso assoluto.
///
/// **È UN ELENCO DICHIARATO, NON UN ANALIZZATORE, E IL PREZZO È DICHIARATO.**
/// È la stessa scelta già pagata da `identifiers_are_in_english`: un elenco non
/// ha falsi positivi e lascia passare ciò che non conosce, mentre un
/// analizzatore di percorsi dentro il testo libero di un prompt chiamerebbe
/// percorso ogni `/` — a cominciare dai puntatori JSON — e verrebbe spento
/// entro un giorno. Chi incontra un percorso che questo elenco non vede lo
/// aggiunge qui.
const ABSOLUTE_PREFIXES: [&str; 4] = ["/Users/", "/home/", "/private/", "~/"];

/// Un percorso assoluto trovato scritto a mano dentro un flusso.
struct HardcodedPath {
    step: String,
    field: String,
    value: String,
    /// Vero quando sta in un campo di posizione: il flusso **non gira** altrove,
    /// quindi è un errore. Falso quando sta dentro un testo: lì il flusso gira
    /// lo stesso e il percorso è un'istruzione a chi legge, quindi è un avviso.
    fatal: bool,
}

/// I percorsi assoluti scritti a mano in un flusso.
///
/// **DUE ESITI, PERCHÉ SONO DUE GUASTI DIVERSI.** Un `workdir` assoluto decide
/// dove il passo lavora: il flusso si può eseguire in un posto solo, e altrove
/// fa danno invece di fallire. Un percorso dentro il testo di un prompt non
/// impedisce al flusso di girare — è un'istruzione che diventa sbagliata
/// altrove, e chi la riscrive sta riscrivendo un'istruzione, non un campo.
/// Perciò il primo è un errore e il secondo un avviso: chiamarli allo stesso
/// modo vorrebbe dire o bloccare flussi sani, o lasciar passare il guasto 25.
///
/// **I PUNTATORI NON SONO PERCORSI.** `{"$from": "/answer/verdict"}` comincia
/// per `/` e non è un percorso: è un puntatore JSON, e compare in quattro
/// flussi su cinque. Il valore di `$from` e quello di `$json` si saltano. Il
/// testo letterale dentro un `$join` invece si guarda: l'elenco di prefissi non
/// può scambiare `/answer/verdict` per un percorso, quindi saltarlo non
/// comprerebbe niente e perderebbe i prompt composti a pezzi — che è dove i due
/// percorsi di `sviluppa-sailor` stanno davvero.
fn hardcoded_paths(flow: &FlowFile) -> Vec<HardcodedPath> {
    let mut found = Vec::new();
    for step in flow.graph.steps() {
        if let Some(with) = step.with.as_ref() {
            walk_for_paths(&step.id, "", with, &mut found);
        }
    }
    // L'ingresso dichiarato è scritto a mano quanto il `with`, ed è dove sta il
    // testo dell'innesco.
    for (name, declared) in &flow.inputs {
        walk_for_paths(name, "", declared, &mut found);
    }
    found
}

fn walk_for_paths(step: &str, field: &str, value: &Value, found: &mut Vec<HardcodedPath>) {
    match value {
        Value::Object(fields) => {
            for (key, inner) in fields {
                // Il valore di un puntatore non è un percorso, e guardarci
                // dentro riempirebbe il rapporto di falsi positivi.
                if key == reference::FROM_KEY || key == reference::JSON_KEY {
                    continue;
                }
                let trail = if field.is_empty() {
                    key.clone()
                } else {
                    format!("{field}.{key}")
                };
                walk_for_paths(step, &trail, inner, found);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_for_paths(step, field, item, found);
            }
        }
        Value::String(text) => {
            let is_position = field
                .rsplit('.')
                .next()
                .is_some_and(|last| POSITION_FIELDS.contains(&last));
            if is_position && (text.starts_with('/') || text.starts_with("~/")) {
                found.push(HardcodedPath {
                    step: step.to_owned(),
                    field: field.to_owned(),
                    value: text.clone(),
                    fatal: true,
                });
            } else if let Some(prefix) = ABSOLUTE_PREFIXES
                .iter()
                .find(|prefix| text.contains(**prefix))
            {
                found.push(HardcodedPath {
                    step: step.to_owned(),
                    field: field.to_owned(),
                    value: (*prefix).to_owned(),
                    fatal: false,
                });
            }
        }
        _ => {}
    }
}

// ── le case di credenziali, chieste al motore ────────────────────────────

/// **UNA CASA DICHIARATA E VUOTA SI APPLICA IN SILENZIO, ED È QUELLO CHE QUESTA
/// SEZIONE ROMPE.**
///
/// Dal 01/09/2026 un motore lanciato da un passo parte nella casa del profilo
/// attivo — è la cura del guasto 18 — e un profilo che punta a una cartella
/// senza credenziali fa partire ogni chiamata **non autenticata** senza che
/// niente lo dica. Il vaglio a secco non può vederlo e non deve provarci: toglie
/// la domanda apposta, quindi il motore si ferma su «non mi hai dato niente da
/// fare» e non arriva mai ai controlli che vengono dopo. È il guasto 39, e la
/// metà che restava scoperta.
///
/// **SI CHIEDE AL MOTORE, E COME SI CHIEDE LO DICE IL DESCRITTORE.** Non si va a
/// cercare `auth.json` sul disco: sarebbe una seconda copia della verità, una per
/// motore, da tenere allineata a mano. Chi non dichiara `login_status` non fa
/// scattare niente — **vuoto vuol dire «nessuno ha guardato», mai «è
/// autenticato»** — ed è la stessa regola di `refuses_without_prompt`.
///
/// **NON FA FALLIRE IL CONTROLLO, E IL VERSO È DELIBERATO.** Fermare un flusso
/// perché un profilo non è autenticato punirebbe chi non c'entra — è la cura
/// sbagliata del guasto 35 — e chi controlla un flusso lo fa anche per capirlo,
/// non solo per lanciarlo. Deve **vedersi**, e basta.
///
/// **COSTA ZERO E LO STESSO ESEGUE.** `codex login status` e `claude auth status`
/// leggono un file locale: nessun modello, nessun fornitore, nessun denaro.
/// Restano processi avviati, quindi vivono dietro la stessa sonda delle righe di
/// comando e tacciono insieme a lei con `--no-engines`.
fn login_states_into(
    report: &mut String,
    graph: &Graph,
    tools: &toolbox::Tools,
    world: &EngineWorld,
) {
    use actions::{LoginVerdict, ToolResolver};

    let mut unauthenticated = Vec::new();
    let mut authenticated = Vec::new();
    let mut unknown = Vec::new();

    let mut asked: BTreeSet<String> = BTreeSet::new();
    for wanted in engines_wanted(graph) {
        // Un motore si interroga UNA VOLTA SOLA anche quando lo nominano sei
        // passi: la casa viene dal profilo attivo, non dal passo, quindi sei
        // domande darebbero sei volte la stessa risposta. Il rapporto nomina il
        // motore e il profilo, che è ciò che chi legge deve cambiare.
        if !tools.declares(&wanted.tool) || !asked.insert(wanted.tool.clone()) {
            continue;
        }
        // Un motore che non è invocabile qui è già nominato dalla sezione delle
        // righe: ripeterlo manderebbe a cercare due difetti dove ce n'è uno.
        let Ok(bin) = tools.resolve(&wanted.tool) else {
            continue;
        };
        // **SOLO DOVE UN PROFILO È IN FORZA.** Senza profilo attivo il motore
        // parte nella casa di chi ha aperto il terminale, che è la casa di
        // sempre: non c'è nessuna scelta di Sailor da rendere visibile, e
        // un avviso qui parlerebbe di una cosa che questo comando non governa.
        let equipment = actions::equipment_for(world.profiles, &bin, &BTreeMap::new());
        if equipment.profile.is_empty() {
            continue;
        }
        // La casa si mostra come la riceve il motore — variabile e valore — e
        // non ricalcolata da un'altra parte: due strade che compongono la stessa
        // cosa divergono al primo che cambia.
        let home = equipment
            .env
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join(" ");
        let who = format!("{} (profilo «{}», {home})", wanted.tool, equipment.profile);

        let Some(recipe) = tools.login_recipe(&wanted.tool) else {
            unknown.push(format!(
                "{who}: il suo descrittore non dichiara come chiedergli se è \
                 autenticato (`login_status`), quindi nessuno ha guardato — che \
                 non vuol dire che lo sia"
            ));
            continue;
        };
        match actions::probe_login_status(world.probe, &bin, &equipment.env, &recipe) {
            LoginVerdict::LoggedIn { .. } => authenticated.push(who),
            // LE PAROLE DEL MOTORE, come per una riga rotta: «non autenticato»
            // detto da noi non dice quale credenziale manca, e la frase sua sì.
            LoginVerdict::LoggedOut { said } => unauthenticated.push(format!("{who}: «{said}»")),
            LoginVerdict::NotDeclared => unknown.push(format!(
                "{who}: il suo descrittore dichiara `login_status` a metà — servono \
                 le parole del sì e quelle del no — quindi non si può leggere niente"
            )),
            LoginVerdict::Unrecognised { said } => unknown.push(format!(
                "{who}: ha risposto «{said}», che non somiglia a nessuna delle due \
                 forme dichiarate"
            )),
            LoginVerdict::NoAnswer { why } => {
                unknown.push(format!("{who}: nessuna risposta — {why}"))
            }
        }
    }

    if !unauthenticated.is_empty() {
        let _ = write!(
            report,
            "\nCASE SENZA CREDENZIALI (la corsa parte lo stesso, e le chiamate a \
             questi motori partiranno NON AUTENTICATE): {}",
            unauthenticated.join("; ")
        );
    }
    if !authenticated.is_empty() {
        let _ = write!(
            report,
            "\ncase autenticate (chiesto al motore, senza spendere): {}",
            authenticated.join("; ")
        );
    }
    if !unknown.is_empty() {
        let _ = write!(
            report,
            "\ncase di cui non si sa se sono autenticate: {}",
            unknown.join("; ")
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
    let root = workspace_root();
    announce_root(root.as_deref());
    InProcessExecutor.execute(
        &flow.graph,
        registry::execution_request(flow, run_id, root.as_deref()),
        store,
        registry,
        clock,
    )
}

/// La radice del progetto per questa corsa, risalendo da dove si è lanciato.
fn workspace_root() -> Option<PathBuf> {
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
    use flow::{Clock, Decision, InMemoryRecordStore, ProcessProbe, StepRecord};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
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

        let mut store = InMemoryRecordStore::from_records(vec![a_handed_record(
            1_000,
            Some(3_600),
            None,
        )]);
        let registry = default_registry(None, None);
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
        ledger.append_step_started(&record).expect("aprire il passo");
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
        let (said_without, _) = check_report(&without, &registry, None, None);
        let (said_with, _) = check_report(&with, &registry, None, None);

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

        let (report, _) = check_report(&flow, &default_registry(None, None), None, None);

        assert!(report.contains("non arriva ai motori"), "{report}");
        assert!(report.contains("primo fronte"), "{report}");
        assert!(report.contains("restano fuori dalla somma"), "{report}");
    }

    // ── quello che il listino non sa prezzare ────────────────────────────

    /// Un listino che conosce un modello solo, con i suoi prezzi, e una voce
    /// dichiarata a metà: bastano a distinguere le tre risposte.
    fn a_small_price_list() -> PriceList {
        PriceList::parse(
            r#"{"currency":"USD","models":[
                {"id":"prezzato","input_per_million":5.0,"output_per_million":25.0},
                {"id":"a-meta","input_per_million":5.0}
            ]}"#,
        )
        .expect("il listino di prova si legge")
    }

    fn names(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    /// **CHI NON HA UN PREZZO PER UN MODELLO DEVE SAPERLO, E SAPERE QUALE.**
    ///
    /// È la seconda metà della cura del guasto 35. Il primo modello è prezzato e
    /// non deve comparire; gli altri due no, e devono comparire **col perché**,
    /// perché si riparano in due modi diversi.
    ///
    /// *Mutante eseguito*: far restituire a `cannot_be_priced` un elenco vuoto.
    /// Il rapporto torna a tacere e questa prova diventa rossa — che è
    /// esattamente il difetto: uno zero al posto di una risposta.
    #[test]
    fn a_model_without_a_price_is_named_and_the_reason_with_it() {
        let said = what_is_priced(
            &a_small_price_list(),
            Some(&names(&["prezzato", "a-meta", "mai-visto"])),
            None,
        );

        assert!(!said.contains("prezzato ("), "un modello prezzato non si segnala: {said}");
        assert!(said.contains("mai-visto (nessuna voce nel listino)"), "{said}");
        assert!(said.contains("a-meta (voce senza prezzi)"), "{said}");
        assert!(said.contains("sconosciuto"), "e dice cosa gli succede: {said}");
    }

    /// **QUANDO SONO TUTTI PREZZATI LO DICE LO STESSO.** Un rapporto che tace
    /// lascia chi legge a chiedersi se il controllo abbia guardato — è la stessa
    /// regola per cui la riga del tetto c'è anche quando il tetto non c'è.
    #[test]
    fn when_everything_is_priced_the_report_says_so_instead_of_falling_silent() {
        let said = what_is_priced(&a_small_price_list(), Some(&names(&["prezzato"])), None);

        assert!(said.contains("tutti prezzati"), "{said}");
        assert!(!said.contains("nessuna voce"), "{said}");
    }

    /// **«MAI GIRATO QUI» E «NON HO POTUTO GUARDARE» SONO DUE FRASI DIVERSE.**
    ///
    /// Un deposito che non si apre — non c'è, i permessi lo negano — non dice
    /// che il flusso non è mai girato: dice che nessuno ha potuto guardare.
    /// Confonderli fa stampare «mai girato qui» a un flusso girato cento volte,
    /// ed è la stessa regola per cui il rilevatore assente fa tacere il rapporto
    /// invece di dichiarare sano ciò che non ha visto.
    ///
    /// *Mutante eseguito*: far collassare il ramo `None` su quello dell'insieme
    /// vuoto. Le due frasi diventano una e questa prova diventa rossa.
    #[test]
    fn a_ledger_that_could_not_be_read_is_not_a_flow_that_never_ran() {
        let unreadable = what_is_priced(&a_small_price_list(), None, None);
        let never_ran = what_is_priced(&a_small_price_list(), Some(&BTreeSet::new()), None);

        assert_ne!(unreadable, never_ran);
        assert!(unreadable.contains("non si è potuto leggere"), "{unreadable}");
        assert!(!unreadable.contains("mai girato"), "{unreadable}");
    }

    /// **UN FLUSSO MAI GIRATO QUI NON RICEVE UN ELENCO VUOTO, MA UNA FRASE.**
    ///
    /// I modelli si sanno solo da chi ha già risposto: un passo nomina lo
    /// strumento, non il modello. Dire «tutti prezzati» senza aver visto niente
    /// sarebbe una rassicurazione costruita sul nulla — è la stessa distinzione
    /// che il rilevatore tiene fra «non c'è» e «non ho potuto guardare».
    #[test]
    fn a_flow_that_never_ran_here_is_told_that_nothing_is_known_yet() {
        let said = what_is_priced(&a_small_price_list(), Some(&BTreeSet::new()), None);

        assert!(!said.contains("tutti prezzati"), "{said}");
        assert!(said.contains("mai girato"), "{said}");
    }

    /// **UN TETTO CHE NON PUÒ SCATTARE SI DEVE VEDERE PRIMA DI LANCIARE.**
    ///
    /// È la frase per cui il guasto 35 è stato scritto: il tetto si misura sui
    /// costi noti, quindi un modello senza prezzo lo rende più largo di quanto
    /// dice — e chi lancia lo scopre a fattura arrivata. La riga compare solo
    /// quando tutte e due le condizioni ci sono, perché è la loro coincidenza a
    /// essere pericolosa.
    ///
    /// *Mutante eseguito*: togliere il ramo che guarda `cap` e stampare la frase
    /// sempre. Il terzo braccio — flusso senza tetto — diventa rosso.
    #[test]
    fn a_cap_that_cannot_fire_is_declared_before_the_run_not_after() {
        let unpriced = names(&["mai-visto"]);
        let priced = names(&["prezzato"]);

        let with_cap = what_is_priced(&a_small_price_list(), Some(&unpriced), Some(5_000_000));
        assert!(with_cap.contains("il tetto"), "{with_cap}");

        let all_priced = what_is_priced(&a_small_price_list(), Some(&priced), Some(5_000_000));
        assert!(
            !all_priced.contains("il tetto"),
            "senza modelli scoperti il tetto non ha niente da dichiarare: {all_priced}"
        );

        let no_cap = what_is_priced(&a_small_price_list(), Some(&unpriced), None);
        assert!(
            !no_cap.contains("il tetto"),
            "un flusso senza tetto non ha un tetto da avvisare: {no_cap}"
        );
    }

    /// Una chiamata registrata: il modello che ha risposto, i suoi token, e se
    /// un costo è stato calcolato o no.
    fn a_call(actual_model: &str, cost: Option<i64>) -> ledger::ModelCallRecord {
        ledger::ModelCallRecord {
            call_id: format!("call-{actual_model}"),
            run_id: "run-1".to_owned(),
            step_id: None,
            purpose: "external_engine".to_owned(),
            cli: "claude-code".to_owned(),
            requested_model: String::new(),
            actual_model: actual_model.to_owned(),
            input_tokens: Some(100),
            output_tokens: Some(100),
            cached_tokens: None,
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: Some(1),
            cost_micros: cost,
            declared_cost_micros: None,
            price_currency: cost.map(|_| "USD".to_owned()),
            input_price_micros_per_million: None,
            output_price_micros_per_million: None,
            cached_price_micros_per_million: None,
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            mandate_name: String::new(),
            mandate_version: String::new(),
            retry_chain: vec![],
            error_type: None,
            started_at: 0,
            ended_at: Some(1),
            session_id: None,
        }
    }

    fn a_finished_run() -> ledger::RunRecord {
        ledger::RunRecord {
            run_id: "run-1".to_owned(),
            kind: "flow".to_owned(),
            entity: "prova".to_owned(),
            parent_run_id: None,
            started_by: "prova".to_owned(),
            status: "went".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: 0,
            ended_at: Some(10),
        }
    }

    /// **`flow cost` NOMINA IL MODELLO SCOPERTO, NON SOLO QUANTE CHIAMATE.**
    ///
    /// «Una chiamata senza costo noto» è un numero su cui non si può agire; il
    /// nome del modello è una riga da scrivere nel listino. La corsa ha due
    /// chiamate — una prezzata e una no — e solo la seconda deve comparire:
    /// nominarle tutte e due manderebbe a correggere una voce che c'è già.
    ///
    /// *Mutante eseguito*: far restituire a `cannot_be_priced` un elenco vuoto.
    /// Il rapporto torna a dire solo «1 senza costo noto» e questa diventa rossa.
    #[test]
    fn the_cost_report_names_the_model_that_has_no_price() {
        let calls = vec![a_call("prezzato", Some(1_000)), a_call("mai-visto", None)];
        let view = ui::dashboard::summarize_run(&a_finished_run(), &[], &calls, 100);

        let said = spending_report(&view, &a_small_price_list());

        assert!(said.contains("mai-visto (nessuna voce nel listino)"), "{said}");
        assert!(
            !said.contains("prezzato ("),
            "un modello prezzato non si segnala: {said}"
        );
        assert!(said.contains("più bassa di quella vera"), "{said}");
    }

    /// La gemella: quando tutto è prezzato la riga non compare. Senza di lei un
    /// mutante che la stampasse sempre passerebbe la prova qui sopra.
    #[test]
    fn a_run_where_everything_is_priced_gets_no_such_line() {
        let calls = vec![a_call("prezzato", Some(1_000))];
        let view = ui::dashboard::summarize_run(&a_finished_run(), &[], &calls, 100);

        let said = spending_report(&view, &a_small_price_list());

        assert!(!said.contains("non sa prezzare"), "{said}");
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
            .execute(&input, &mut flow::SharedState::new())
            .expect_err("quell'identificativo non esiste");

        assert_eq!(error.class, "tool_unavailable", "{}", error.said);
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
        let seen = &store.all()[0].input;
        for (field, value) in flow.inputs["root"].as_object().expect("un oggetto") {
            assert_eq!(seen.get(field), Some(value), "«{field}» non deve cambiare");
        }
        assert_eq!(
            seen.get("workdir").and_then(Value::as_str),
            workspace_root().as_deref().map(|root| root.to_str().expect("un percorso leggibile")),
            "la cartella di lavoro è la radice, non dove sta il processo"
        );
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

    // ── le case di credenziali ────────────────────────────────────────

    /// Un finto `codex` che si comporta come quello vero **su questa domanda**:
    /// risponde su stderr, dice «Not logged in» quando in casa non c'è
    /// `auth.json`, e «Logged in using ChatGPT» quando c'è. Anche i due codici
    /// d'uscita sono quelli misurati il 01/09/2026 — 1 e 0 — apposta: se un
    /// giorno qualcuno facesse dipendere il verdetto dall'esito, questa prova
    /// resterebbe verde, e la prova gemella in `crates/actions/tests` dice
    /// perché non basterebbe.
    ///
    /// **SI CHIAMA `codex` PERCHÉ IL LEGAME È L'ESEGUIBILE**: è su quel nome che
    /// `profiles::cli_for_executable` decide quale variabile sposta la casa.
    fn a_fake_codex_that_answers_about_its_home(dir: &Path) -> String {
        let path = dir.join("codex");
        std::fs::write(
            &path,
            "#!/bin/sh\n\
             if [ \"$1\" = login ] && [ \"$2\" = status ]; then\n\
             \x20 if [ -f \"$CODEX_HOME/auth.json\" ]; then\n\
             \x20   echo 'Logged in using ChatGPT' >&2; exit 0\n\
             \x20 fi\n\
             \x20 echo 'Not logged in' >&2; exit 1\n\
             fi\n\
             echo 'No prompt provided via stdin.' >&2\n\
             exit 1\n",
        )
        .expect("scrivere il finto motore");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("bit di esecuzione");
        }
        path.to_string_lossy().into_owned()
    }

    /// Una cartella usa-e-getta con dentro il finto motore e il suo descrittore.
    fn a_machine_with_a_real_fake_codex(declares_login: bool) -> (PathBuf, toolbox::Tools) {
        static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("prova-case-{}-{serial}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("la cartella di prova");
        a_fake_codex_that_answers_about_its_home(&dir);

        let login = if declares_login {
            r#","login_status":{"args":["login","status"],
               "logged_in_when":["logged in using"],
               "logged_out_when":["not logged in"]}"#
        } else {
            ""
        };
        let file = dir.join("tools.json");
        std::fs::write(
            &file,
            format!(
                r#"{{"tools":[{{"id":"codex","family":"ai_cli","label":"codex",
                   "detect":{{"command":"codex"}},
                   "ask":{{"args":["exec"],"prompt":"stdin",
                           "refuses_without_prompt":["no prompt provided via stdin"]}}
                   {login}}}]}}"#
            ),
        )
        .expect("scrivere i descrittori");
        let catalog = toolbox::Catalog::load(&[toolbox::Source::File(file)]);
        let tools = toolbox::Tools::new(
            catalog,
            toolbox::Machine {
                path_dirs: vec![dir.clone()],
                home: dir.clone(),
                env: BTreeMap::new(),
                version_probes: false,
            },
        );
        (dir, tools)
    }

    /// Uno stato dei profili che dichiara una casa sola, attiva.
    fn a_store_pointing_at(home: &Path) -> profiles::ProfileStore {
        profiles::ProfileStore {
            profiles: vec![profiles::Profile {
                name: "prove".to_owned(),
                cli_id: "codex".to_owned(),
                home_dir: home.to_path_buf(),
            }],
            active: [("codex".to_owned(), "prove".to_owned())]
                .into_iter()
                .collect(),
        }
    }

    /// **IL GUASTO 39, L'ALTRA METÀ, CONTRO UN PROCESSO VERO.**
    ///
    /// Il vaglio a secco continua a dire «riga sana» in tutti e due i casi — è
    /// quello che deve fare, toglie la domanda apposta — e accanto compare la
    /// cosa che nessuno diceva: da quale casa parte questo motore, e se quella
    /// casa ha delle credenziali.
    ///
    /// **DUE BRACCI, E SERVONO TUTTI E DUE.** Il primo da solo resterebbe verde
    /// con un controllo che gridasse sempre; il secondo da solo resterebbe verde
    /// con un controllo che non guarda niente. Insieme dicono che la risposta
    /// viene dalla casa.
    ///
    /// **E LA SONDA È QUELLA VERA.** `RealDryProbe` avvia un processo: una
    /// finta risponderebbe quello che le diciamo noi, cioè proverebbe che
    /// sappiamo scrivere una risposta. Qui il motore la legge dal disco.
    ///
    /// *Mutanti eseguiti*: (a) leggere `logged_in_when` prima di
    /// `logged_out_when` in `judge_login_status` — il primo braccio diventa
    /// rosso, cioè si rimette il silenzio originale; (b) togliere
    /// `login_status` dal descrittore — vedi la prova qui sotto.
    #[test]
    fn a_flow_check_says_which_home_the_engine_starts_from_and_whether_it_has_credentials() {
        let (dir, tools) = a_machine_with_a_real_fake_codex(true);
        let flow = flow_with_chain(r#""codex""#);
        let real = actions::RealDryProbe;

        let empty = dir.join("casa-vuota");
        std::fs::create_dir_all(&empty).expect("la casa senza credenziali");
        let store = a_store_pointing_at(&empty);
        let (report, unknown) = check_report(
            &flow,
            &default_registry(None, None),
            Some(&tools),
            Some(&EngineWorld {
                probe: &real,
                profiles: &store,
            }),
        );
        assert!(
            report.contains("CASE SENZA CREDENZIALI"),
            "una casa senza credenziali si applica in silenzio: {report}"
        );
        assert!(
            report.contains(&empty.display().to_string()) && report.contains("codex/prove"),
            "chi legge deve sapere QUALE profilo e QUALE casa, o non sa cosa cambiare: {report}"
        );
        assert!(
            report.contains("Not logged in"),
            "le parole del motore sono la diagnosi: {report}"
        );
        assert!(
            report.contains("righe di comando sane"),
            "il vaglio a secco continua a dire la sua, e continua a dire il vero: {report}"
        );
        assert!(
            unknown.is_empty(),
            "un profilo senza credenziali NON fa fallire il controllo: punire chi non \
             c'entra è la cura sbagliata"
        );

        let full = dir.join("casa-piena");
        std::fs::create_dir_all(&full).expect("la casa autenticata");
        std::fs::write(full.join("auth.json"), "{}").expect("le credenziali");
        let store = a_store_pointing_at(&full);
        let (report, _) = check_report(
            &flow,
            &default_registry(None, None),
            Some(&tools),
            Some(&EngineWorld {
                probe: &real,
                profiles: &store,
            }),
        );
        assert!(
            report.contains("case autenticate"),
            "una casa piena deve risultare piena: {report}"
        );
        assert!(
            !report.contains("CASE SENZA CREDENZIALI"),
            "e non deve comparire fra quelle vuote: {report}"
        );
    }

    /// **CHI NON DICHIARA IL BLOCCO NON FA SCATTARE NIENTE — E NON DICE
    /// «AUTENTICATO».**
    ///
    /// È il mutante (b) scritto una volta per tutte invece che eseguito una
    /// volta sola: la casa è vuota identica a quella del primo braccio qui
    /// sopra, e il solo cambiamento è che il descrittore non dice come si
    /// chiede. Il rapporto deve dire **che nessuno ha guardato**, mai tacere e
    /// mai rassicurare. Un predefinito comodo qui rimetterebbe il difetto per
    /// ogni motore che il blocco non ce l'ha ancora — cioè per tutti quelli che
    /// verranno.
    #[test]
    fn a_descriptor_without_the_block_makes_the_check_say_nobody_looked() {
        let (dir, tools) = a_machine_with_a_real_fake_codex(false);
        let flow = flow_with_chain(r#""codex""#);
        let real = actions::RealDryProbe;
        let empty = dir.join("casa-vuota");
        std::fs::create_dir_all(&empty).expect("la casa senza credenziali");
        let store = a_store_pointing_at(&empty);

        let (report, _) = check_report(
            &flow,
            &default_registry(None, None),
            Some(&tools),
            Some(&EngineWorld {
                probe: &real,
                profiles: &store,
            }),
        );

        assert!(
            report.contains("case di cui non si sa se sono autenticate")
                && report.contains("nessuno ha guardato"),
            "un'assenza deve dirsi: {report}"
        );
        assert!(
            !report.contains("case autenticate"),
            "«nessuno ha guardato» non è «è autenticato»: {report}"
        );
        assert!(
            !report.contains("CASE SENZA CREDENZIALI"),
            "e non è nemmeno «non è autenticato»: inventare un no dove non si è \
             guardato manderebbe a riparare una casa sana: {report}"
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

    /// Alla domanda sulle credenziali non risponde niente: queste prove parlano
    /// delle righe di comando, e senza profilo attivo la domanda non si fa
    /// nemmeno. Un finto che rispondesse qualcosa direbbe qualcosa di questo
    /// mondo, e ci sono prove apposta per quello.
    impl actions::LoginProbe for ScriptedProbe {
        fn ask(
            &self,
            _bin: &str,
            _args: &[String],
            _env: &BTreeMap<String, String>,
        ) -> actions::DryRun {
            actions::DryRun::NoAnswer {
                why: "questa sonda non risponde alla domanda sulle credenziali".to_owned(),
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
            Some(&EngineWorld::without_profiles(&probe)),
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
            Some(&EngineWorld::without_profiles(&probe)),
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
            Some(&EngineWorld::without_profiles(&probe)),
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
            Some(&EngineWorld::without_profiles(&probe)),
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
            Some(&EngineWorld::without_profiles(&probe)),
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
            Some(&EngineWorld::without_profiles(&probe)),
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
            session_id: None,
        }
    }

    // ── il guasto 25: i percorsi assoluti scritti dentro un flusso ────

    /// Un flusso con un solo passo, il cui `with` è quello che le si passa.
    fn flow_with(with: &str) -> FlowFile {
        let json = format!(
            r#"{{
                "id": "prova", "description": "un passo solo",
                "graph": {{"steps": [{{
                    "id": "unico", "deps": [], "action": "external_engine",
                    "max_attempts": 1, "when": null,
                    "input_schema": {{"type": "any"}},
                    "output_schema": {{"type": "any"}},
                    "with": {with}
                }}]}},
                "inputs": {{}}
            }}"#
        );
        serde_json::from_str(&json).expect("caricare il flusso")
    }

    /// IL CASO DEL GUASTO 25. Un `workdir` assoluto decide dove il passo
    /// lavora: il flusso si può eseguire in un posto solo, e altrove non
    /// fallisce — fa danno nel posto sbagliato.
    #[test]
    fn an_absolute_workdir_is_an_error() {
        let flow = flow_with(r#"{"workdir": "/home/someone/personal/sailor"}"#);

        let found = hardcoded_paths(&flow);

        assert_eq!(found.len(), 1, "uno solo: {:?}", found.len());
        assert!(found[0].fatal, "un campo di posizione è un errore");
        assert_eq!(found[0].step, "unico");
        assert_eq!(found[0].field, "workdir");
    }

    /// Un `workdir` relativo è esattamente ciò che si vuole ottenere: si
    /// risolve sulla radice di chi lancia, e non ha niente da segnalare.
    #[test]
    fn a_relative_workdir_is_clean() {
        let flow = flow_with(r#"{"workdir": "crates/flow"}"#);

        assert!(hardcoded_paths(&flow).is_empty());
    }

    /// **UN PUNTATORE NON È UN PERCORSO.** `{"$from": "/answer/verdict"}`
    /// comincia per `/` e compare in quattro flussi su cinque: se il controllo
    /// lo chiamasse percorso nascerebbe pieno di falsi positivi e verrebbe
    /// spento entro un giorno.
    #[test]
    fn a_json_pointer_is_not_a_path() {
        let flow = flow_with(
            r#"{"stdin": {"$from": "/answer/verdict"}, "env": {"X": {"$json": "/shape"}}}"#,
        );

        assert!(hardcoded_paths(&flow).is_empty());
    }

    /// Un percorso dentro il testo di un prompt non impedisce al flusso di
    /// girare: è un'istruzione che diventa sbagliata altrove. Avviso, non
    /// errore — e riscriverlo è riscrivere un'istruzione, quindi lo fa una
    /// persona.
    #[test]
    fn an_absolute_path_inside_a_prompt_is_a_warning() {
        let flow = flow_with(
            r#"{"stdin": {"$join": ["Lavora solo dentro /home/someone/personal/sailor.\n"]}}"#,
        );

        let found = hardcoded_paths(&flow);

        assert_eq!(found.len(), 1);
        assert!(!found[0].fatal, "dentro un testo è un avviso");
        assert_eq!(found[0].field, "stdin.$join");
    }

    // ── spostare un flusso da un albero all'altro ─────────────────────

    fn document_with_workdir(workdir: &str) -> Value {
        serde_json::json!({
            "id": "prova", "description": "d",
            "graph": {"steps": [{
                "id": "unico", "deps": [], "action": "external_engine",
                "max_attempts": 1, "when": null,
                "input_schema": {"type": "any"}, "output_schema": {"type": "any"},
                // **`workdir` NON È IL PENULTIMO, E LA POSIZIONE È LA PROVA.**
                // Togliendo il penultimo campo, lo swap e lo scorrimento danno
                // lo stesso ordine: una fixture così lascia passare il difetto.
                // Qui ne restano due dopo, quindi i due modi divergono.
                "with": {
                    "tool": "git", "workdir": workdir,
                    "timeout_secs": 5, "args": ["status"]
                }
            }]},
            "inputs": {}
        })
    }

    /// Coincide con la radice: il campo sparisce. Tenerlo come `"."` sarebbe
    /// rumore che invita il prossimo a riscriverlo assoluto.
    #[test]
    fn a_workdir_equal_to_the_root_is_removed() {
        let mut document = document_with_workdir("/vecchio/albero");

        let (moved, left) = relocate_workdirs(&mut document, Path::new("/vecchio/albero"))
            .expect("ha dei passi");

        assert_eq!(moved.len(), 1);
        assert!(left.is_empty());
        assert!(document["graph"]["steps"][0]["with"]
            .get("workdir")
            .is_none());
    }

    /// Sotto la radice: resta il pezzo relativo, che è ciò che rende il flusso
    /// eseguibile su un clone qualunque.
    #[test]
    fn a_workdir_under_the_root_keeps_only_the_rest() {
        let mut document = document_with_workdir("/vecchio/albero/desktop");

        relocate_workdirs(&mut document, Path::new("/vecchio/albero")).expect("ha dei passi");

        assert_eq!(document["graph"]["steps"][0]["with"]["workdir"], "desktop");
    }

    /// **TOGLIERE UN CAMPO NON DEVE RIORDINARE IL FILE.** Con `preserve_order`
    /// acceso `Map::remove` è uno *swap*: tira l'ultima chiave dentro il buco.
    /// Misurato il 31/08/2026 sul flusso vero: 62 righe cambiate invece di 7,
    /// e un diff in cui non si distingue più ciò che è stato deciso da ciò che
    /// è stato spostato.
    #[test]
    fn removing_a_workdir_does_not_reorder_the_other_fields() {
        let mut document = document_with_workdir("/vecchio/albero");

        relocate_workdirs(&mut document, Path::new("/vecchio/albero")).expect("ha dei passi");

        let keys: Vec<&str> = document["graph"]["steps"][0]["with"]
            .as_object()
            .expect("un oggetto")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec!["tool", "timeout_secs", "args"],
            "l'ordine resta quello: uno swap metterebbe «args» prima di «timeout_secs»"
        );
    }

    /// Fuori dal prefisso: si lascia stare e si dice. Indovinare che
    /// `/altro/posto` volesse dire «la radice» vorrebbe dire riscrivere un
    /// percorso che qualcuno aveva messo lì apposta.
    #[test]
    fn a_workdir_outside_the_prefix_is_left_alone_and_reported() {
        let mut document = document_with_workdir("/altro/posto");

        let (moved, left) = relocate_workdirs(&mut document, Path::new("/vecchio/albero"))
            .expect("ha dei passi");

        assert!(moved.is_empty());
        assert_eq!(left.len(), 1);
        assert_eq!(document["graph"]["steps"][0]["with"]["workdir"], "/altro/posto");
    }

    /// **UN RINVIO NON SI RISCRIVE.** `{"$from": "/innesco/text"}` è un
    /// puntatore che si risolve a esecuzione contro l'ingresso vero: qui non
    /// c'è niente da spostare, e toccarlo vorrebbe dire inventare.
    #[test]
    fn a_workdir_that_is_a_reference_is_never_touched() {
        let mut document = serde_json::json!({
            "id": "prova", "description": "d",
            "graph": {"steps": [{
                "id": "unico", "deps": [], "action": "external_engine",
                "max_attempts": 1, "when": null,
                "input_schema": {"type": "any"}, "output_schema": {"type": "any"},
                "with": {"workdir": {"$from": "/innesco/text"}}
            }]},
            "inputs": {}
        });

        let (moved, left) = relocate_workdirs(&mut document, Path::new("/vecchio/albero"))
            .expect("ha dei passi");

        assert!(moved.is_empty() && left.is_empty());
        assert_eq!(
            document["graph"]["steps"][0]["with"]["workdir"],
            serde_json::json!({"$from": "/innesco/text"})
        );
    }

    /// **LA PROVA CHE MISURA IL LAVORO.** È nata rossa su
    /// `flows/sviluppa-sailor.flow.json` — sette errori e due avvisi, cioè il
    /// guasto 25 contato — e diventa verde solo quando quel flusso è stato
    /// davvero spostato. Non certifica: dice quanto lavoro c'era.
    ///
    /// Legge il file vero e non una copia: una copia si aggiornerebbe insieme
    /// alla riparazione e resterebbe verde per sempre.
    #[test]
    fn the_real_development_flow_has_no_hardcoded_paths() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../flows/sviluppa-sailor.flow.json");
        let text = fs::read_to_string(&path).expect("il flusso di sviluppo è versionato");
        let flow: FlowFile = serde_json::from_str(&text).expect("è un flusso valido");

        let found = hardcoded_paths(&flow);
        let described: Vec<String> = found
            .iter()
            .map(|entry| {
                let kind = if entry.fatal { "errore" } else { "avviso" };
                format!("{kind}: {} in «{}» ({})", entry.step, entry.field, entry.value)
            })
            .collect();

        assert!(
            found.is_empty(),
            "il flusso di sviluppo non gira su un clone: {}",
            described.join("; ")
        );
    }

    // ── il totale che contiene un'incognita ──────────────────────────────

    /// Una chiamata come il deposito la conserva, coi soli campi che questo
    /// conto guarda. `cost` a `None` è una chiamata **non misurata**: è la
    /// forma che `sailor step close --turns` scrive per un passo consegnato.
    ///
    /// **IL NOME DICE DA COSA SI RICONOSCE**, e non è pignoleria: fino al
    /// 01/09/2026 questa e la sorella qui sopra si chiamavano tutte e due
    /// `a_call`, nate su due rami diversi lo stesso giorno. Git le ha fuse senza
    /// segnalare niente — nessuna riga in comune — e a rifiutare l'albero è
    /// stato `cargo`. È il guasto 36 di `docs/guasti-incontrati.md` che si
    /// ripete: il confine sui file non vede i nomi che vivono nello stesso
    /// modulo.
    fn a_call_named(call_id: &str, cost: Option<i64>) -> ledger::ModelCallRecord {
        ledger::ModelCallRecord {
            call_id: call_id.to_owned(),
            run_id: "run-1".to_owned(),
            step_id: None,
            purpose: "prova".to_owned(),
            cli: "claude".to_owned(),
            requested_model: "m".to_owned(),
            actual_model: "m".to_owned(),
            input_tokens: Some(10),
            output_tokens: Some(2),
            cached_tokens: Some(1),
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: Some(3),
            cost_micros: cost,
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
            started_at: 0,
            ended_at: Some(1),
            session_id: None,
        }
    }

    fn a_run() -> ledger::RunRecord {
        ledger::RunRecord {
            run_id: "run-1".to_owned(),
            kind: "flow".to_owned(),
            entity: "prova".to_owned(),
            parent_run_id: None,
            started_by: "prova".to_owned(),
            status: "succeeded".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: 0,
            ended_at: Some(10),
        }
    }

    /// **IL LISTINO CONOSCE IL MODELLO DI QUESTE PROVE, ED È DELIBERATO.** Qui
    /// si guarda una cosa sola: la forma del totale quando una chiamata non è
    /// misurata. Con un listino che non conoscesse `m`, ogni rapporto porterebbe
    /// anche la riga dei modelli scoperti — un secondo motivo di lacuna,
    /// scritto sopra al primo — e una prova rossa non direbbe più quale delle
    /// due cose si è rotta.
    fn report_for(calls: &[ledger::ModelCallRecord]) -> String {
        let prices = PriceList::parse(
            r#"{"currency":"USD","models":[
                {"id":"m","input_per_million":5.0,"output_per_million":25.0}
            ]}"#,
        )
        .expect("il listino di prova si legge");
        let view = ui::dashboard::summarize_run(&a_run(), &[], calls, 100);
        spending_report(&view, &prices)
    }

    /// La cifra secca, come la scriverebbe un totale completo. Se compare in un
    /// rapporto parziale, chi legge ha in mano un numero che non è il totale.
    fn bare_total(micros: i64) -> String {
        format!("costo equivalente: {:.4}", micros as f64 / 1_000_000.0)
    }

    /// **UN TOTALE CHE CONTIENE UN'INCOGNITA NON È UN TOTALE.**
    ///
    /// È il guasto 37 misurato: la corsa consegnata dell'A/B del 31/08/2026 ha
    /// stampato `1,6674` mentre era costata `7,2080` — 4,3 volte — perché tre
    /// chiamate su quattro non avevano un costo e la nota che lo diceva stava
    /// **sotto** il numero. Chi legge un totale legge il numero, non la nota.
    /// Qui la nota prende il posto del numero: la cifra secca non deve esistere
    /// da nessuna parte nel rapporto.
    #[test]
    fn a_total_with_an_unmeasured_call_is_never_a_bare_figure() {
        let report = report_for(&[
            a_call_named("misurata", Some(1_667_400)),
            a_call_named("consegnata-1", None),
            a_call_named("consegnata-2", None),
            a_call_named("consegnata-3", None),
        ]);

        assert!(
            !report.contains(&bare_total(1_667_400)),
            "la cifra secca non deve comparire: si legge come il totale vero.\n{report}"
        );
        assert!(
            report.contains("almeno"),
            "il numero va letto come un pavimento, non come una somma.\n{report}"
        );
        assert!(
            report.contains("3 chiamate su 4"),
            "quanto manca si dice accanto alla cifra, non in fondo.\n{report}"
        );
    }

    /// **E QUANDO SI SA TUTTO, IL NUMERO RESTA SECCO.** Senza questa metà
    /// l'avviso non varrebbe niente: un rapporto che si dichiara incompleto
    /// sempre non distingue più i due casi, ed è lo stesso difetto al contrario.
    #[test]
    fn a_total_where_every_call_is_measured_stays_a_plain_figure() {
        let report = report_for(&[
            a_call_named("una", Some(1_000_000)),
            a_call_named("due", Some(667_400)),
        ]);

        assert!(
            report.contains(&bare_total(1_667_400)),
            "tutto misurato: la somma è la somma.\n{report}"
        );
        assert!(
            !report.contains("almeno"),
            "niente pavimenti dove non manca niente.\n{report}"
        );
    }

    /// **NESSUNA CHIAMATA MISURATA NON È «ALMENO ZERO».** È il terzo caso di
    /// `Spend`, quello che un `Option` collasserebbe: «almeno 0,0000» è vero e
    /// non dice niente, e chi lo legge crede di aver visto una spesa piccola.
    #[test]
    fn a_run_where_nothing_is_measured_says_unknown_instead_of_at_least_zero() {
        let report = report_for(&[a_call_named("consegnata", None)]);

        assert!(
            report.contains("sconosciuto"),
            "senza nemmeno una misura non c'è un pavimento da dichiarare.\n{report}"
        );
        assert!(
            !report.contains(&bare_total(0)),
            "e soprattutto non c'è uno zero.\n{report}"
        );
    }
}
