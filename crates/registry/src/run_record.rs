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

use flow::FlowFile;
use ledger::{Ledger, RunRecord};

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
            parent_run_id: None,
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
