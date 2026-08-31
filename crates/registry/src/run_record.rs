//! L'intestazione di una corsa, scritta da un posto solo.
//!
//! **PERCHÉ STA QUI E NON NEI DUE CHIAMANTI.** Queste venti righe erano scritte
//! due volte — in `sailor::flow_cmd` e nel guscio della finestra — e la copia
//! del guscio portava un commento che dichiarava la duplicazione e spiegava
//! perché non si poteva chiudere: «rendere pubblico mezzo `flow_cmd` per un
//! guscio che vive fuori dal workspace sposterebbe il problema». Dal 30/08/2026
//! quel posto esiste ed è questo crate, quindi la ragione è decaduta.
//!
//! Non è un timore astratto: il 31/08/2026 tutte e due scrivevano
//! `total_cost_micros: 0` a mano, su un campo che la finestra mostra. Riparare
//! una sola delle due avrebbe dato due numeri diversi per la stessa corsa a
//! seconda di chi l'aveva lanciata — che è esattamente il guasto per cui questo
//! crate è nato.

use flow::{Decision, Execution, FlowFile, SpendStop};
use ledger::{Ledger, RunRecord};

/// Com'è finita una corsa, e se il processo che l'ha lanciata può uscire con
/// zero.
///
/// **ERA SCRITTA DUE VOLTE, E LE DUE SONO SEMPRE STATE D'ACCORDO PER FORTUNA.**
/// Il guscio ne teneva una copia identica meno il booleano, con sopra un
/// commento che diceva «la stessa traduzione di `flow_cmd::execution_status`».
/// Un `Decision` nuovo — ed è successo il 31/08/2026, con il tetto di spesa —
/// obbliga a toccarle tutte e due: il compilatore lo chiede su entrambe, ma
/// nessuno garantisce che ricevano la **stessa** parola. Due parole diverse per
/// lo stesso stato sono due storici che non si possono confrontare.
/// **DAL 31/08/2026 IL CORPO STA IN `flow::run_status`, E PER LA TERZA VOLTA
/// PER LA STESSA RAGIONE.** Ne serviva una copia anche al passo `subflow`, che
/// vive nel crate del flusso e in `registry` non può guardare: sarebbe stata la
/// terza. La traduzione è scesa accanto a `Decision`, cioè accanto al tipo che
/// traduce, dove la vedono tutti e tre. Questo nome resta perché due chiamanti
/// lo usano — la riga di comando e il guscio della finestra.
pub fn execution_status(execution: &Execution) -> (&'static str, bool) {
    flow::run_status(execution)
}

/// La riga che spiega a una persona perché la corsa si è fermata.
///
/// **DICE ANCHE QUELLO CHE NON SA.** Il totale è la somma dei costi noti: se
/// qualche chiamata non ne aveva uno, la frase lo porta — la spesa vera è più
/// alta di quella scritta, e chi sta per alzare il tetto e rilanciare deve
/// saperlo prima, non dopo.
pub fn why_it_stopped(stop: &SpendStop) -> String {
    let unknown = if stop.spent.is_complete() {
        String::new()
    } else {
        format!(
            ", e {} delle {} chiamate non hanno dichiarato un costo — la spesa vera è più alta",
            stop.spent.calls_without_cost, stop.spent.calls
        )
    };
    format!(
        "fermata dal tetto di spesa: {} spesi su un tetto di {}{unknown}. Passi non partiti: {}",
        in_units(stop.spent.micros),
        in_units(stop.cap_micros),
        if stop.not_started.is_empty() {
            "nessuno".to_owned()
        } else {
            stop.not_started.join(", ")
        }
    )
}

/// Perché la corsa si è fermata, se si è fermata per il tetto.
pub fn stopped_by_cap(execution: &Execution) -> Option<String> {
    match execution.decisions.last() {
        Some(Decision::CapReached(stop)) => Some(why_it_stopped(stop)),
        _ => None,
    }
}

/// Le micro-unità come le legge una persona, con due decimali.
fn in_units(micros: i64) -> String {
    format!("{:.2}", micros as f64 / 1_000_000.0)
}

/// Quel che si sa di una corsa nel momento in cui la si registra.
///
/// **PERCHÉ UNA STRUTTURA E NON OTTO ARGOMENTI.** Otto ce n'erano, ed erano
/// posizionali: `started_at` e `ended_at` adiacenti, entrambi tempi, uno `i64` e
/// l'altro `Option<i64>`. Scambiarli non è un errore che il compilatore prende
/// in tutti i casi, e il risultato sarebbe una corsa finita prima di cominciare.
/// La copia precedente zittiva l'avviso di clippy con un `allow`: qui l'avviso
/// aveva ragione.
pub struct FlowRun<'a> {
    pub run_id: &'a str,
    /// `running`, `complete`, `failed`, `waiting`, `stopped`.
    pub status: &'a str,
    pub started_at: i64,
    /// `None` finché la corsa è aperta.
    pub ended_at: Option<i64>,
    pub error: Option<String>,
    /// Chi l'ha avviata: il pulsante della finestra, la riga di comando, una
    /// pianificazione. Si legge nel deposito, e distingue corse altrimenti
    /// identiche.
    pub started_by: &'a str,
}

/// Registra — o aggiorna — l'intestazione di una corsa.
///
/// **IL TOTALE NON SI DICHIARA, SI CHIEDE.** Prima era la costante `0` in
/// entrambe le copie, e nessuno la calcolava mai: ogni corsa risultava costata
/// zero mentre le sue chiamate portavano il costo giusto una per una. Adesso
/// viene da `spent_in_run`, cioè dalla somma delle righe che quella corsa ha
/// davvero scritto.
///
/// **CHE COSA QUEL TOTALE NON DICE.** È la somma dei costi **noti**: un motore
/// che non dichiara i propri token lascia la riga senza costo, e quella riga
/// non entra. Il totale è quindi un «almeno», non un «esattamente», e chi lo
/// mostra deve mostrare accanto quante chiamate ne sono fuori — che è ciò che
/// `Spend::is_complete` serve a sapere. Qui non si può fare di meglio senza
/// inventare un numero: il campo nel deposito è uno solo ed è un intero.
///
/// Si ricalcola a ogni scrittura, compresa quella d'apertura, dove viene zero
/// perché non è stato ancora speso niente.
pub fn record_flow_run(ledger: &Ledger, flow: &FlowFile, run: FlowRun<'_>) -> Result<(), String> {
    write_run(ledger, flow, run, None)
}

/// La stessa riga, per una corsa **figlia**: in più il legame con la corsa che
/// l'ha chiamata.
///
/// **PERCHÉ UNA FUNZIONE E NON UN CAMPO IN PIÙ SU `FlowRun`.** Un campo nuovo su
/// una struttura costruita a letterale in due crate li avrebbe rotti tutti e
/// due per un dato che riguarda un solo chiamante. Qui il corpo è uno; la firma
/// dice chi ha bisogno di cosa.
///
/// **IL LEGAME VA NEL DEPOSITO, NON SOLO NEL NOME.** `parent_run_id` è una
/// colonna che il deposito aveva già e che nessuno riempiva: una corsa figlia
/// senza quel valore si può solo indovinare dal prefisso del proprio
/// identificativo, e indovinare non è risalire. Il verso opposto lo porta il
/// passo del padre, che si tiene il `run_id` del figlio nella propria uscita.
pub fn record_child_run(
    ledger: &Ledger,
    flow: &FlowFile,
    run: FlowRun<'_>,
    parent_run_id: &str,
) -> Result<(), String> {
    write_run(ledger, flow, run, Some(parent_run_id.to_owned()))
}

fn write_run(
    ledger: &Ledger,
    flow: &FlowFile,
    run: FlowRun<'_>,
    parent_run_id: Option<String>,
) -> Result<(), String> {
    let spent = ledger.spent_in_run(run.run_id).map_err(|error| {
        format!(
            "non riesco a leggere la spesa della corsa {}: {error}",
            run.run_id
        )
    })?;
    ledger
        .record_run(&RunRecord {
            run_id: run.run_id.to_owned(),
            kind: "flow".to_owned(),
            entity: flow.id.clone(),
            parent_run_id,
            started_by: run.started_by.to_owned(),
            status: run.status.to_owned(),
            total_cost_micros: spent.micros,
            error: run.error,
            started_at: run.started_at,
            ended_at: run.ended_at,
        })
        .map_err(|error| {
            format!(
                "non riesco a registrare la corsa {}: {error}",
                run.run_id
            )
        })
}
