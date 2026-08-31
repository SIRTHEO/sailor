//! Il guscio nativo della finestra di Sailor.
//!
//! PERCHÉ UN GUSCIO E NON UN INDIRIZZO. Fino a stasera la tela dei flussi
//! esisteva solo come pagina servita da Vite, e `sailor ui` serviva una seconda
//! pagina in sola lettura su `127.0.0.1:47831`. Tutte e due chiedono a chi
//! guarda di aprire un browser e ricordarsi una porta. La decisione del
//! 27/08/2026 dice l'opposto — «un programma vero, in una finestra nativa» — e
//! finché il guscio non esiste quella riga descrive un'intenzione, non un
//! prodotto.
//!
//! QUI DENTRO NON C'È LOGICA, ED È VOLUTO. Il guscio apre la finestra e le
//! passa quello che il motore già sa: i conti sui flussi stanno nel motore, il
//! disegno sta nella tela. Ogni riga di giudizio che finisse qui sarebbe una
//! quarta verità accanto a `crates/flow`, `crates/ui` e `desktop/src/flow.ts`,
//! che oggi coincidono per disciplina e non per costruzione.

use serde::Serialize;
use ui::gather::{flow_sources, load_all_flows};

mod board;
mod flows;
mod live;
mod run;
mod tools;

/// Un flusso come lo riceve la tela. Ricalca `FlowEntry` di
/// `desktop/src/flow.ts`, tag compreso: chi cambia l'uno cambia l'altro.
///
/// UN FLUSSO ROTTO NON SPARISCE, arriva col suo motivo. È la stessa scelta che
/// `load_flow_registry` documenta: un elenco che si accorcia in silenzio fa
/// credere che il flusso non esista, e nessuno va a cercare un file che
/// secondo la finestra non c'è.
#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum FlowEntry {
    Loaded {
        flow: flow::FlowFile,
        /// Da quale sorgente viene: «tuoi», «del progetto», «dichiarati». Chi
        /// vede due flussi con lo stesso nome deve poter capire quale gira.
        origin: String,
    },
    Broken {
        broken: BrokenFlow,
        origin: String,
    },
}

#[derive(Serialize)]
struct BrokenFlow {
    name: String,
    reason: String,
}

/// I flussi dichiarati, letti dal disco a ogni richiesta.
///
/// **Si rilegge, non si tiene in memoria.** `sailor ui` carica il registro una
/// volta sola all'avvio, e da lì in poi un flusso aggiunto o corretto non
/// compare finché qualcuno non riavvia il servitore — un difetto che in una
/// finestra sempre aperta si nota subito e in un servitore no. Sono quattordici
/// file di poche decine di righe: rileggerli costa meno che spiegare a chi
/// guarda perché non vede quello che ha appena scritto.
#[tauri::command]
fn flows() -> Vec<FlowEntry> {
    load_all_flows(&flow_sources())
        .into_iter()
        .map(|(name, origin, entry)| match entry {
            Ok(flow) => FlowEntry::Loaded {
                flow,
                origin: origin.to_owned(),
            },
            Err(reason) => FlowEntry::Broken {
                broken: BrokenFlow { name, reason },
                origin: origin.to_owned(),
            },
        })
        .collect()
}

/// Dove la finestra ha guardato, e cosa ha trovato in ciascun posto.
///
/// **SERVE QUANDO NON TROVA NIENTE**, ed è il motivo per cui esiste: il
/// 29/08/2026 la finestra diceva «nessun flusso» mentre quattro flussi
/// esistevano a una cartella di distanza, e da dentro non c'era modo di sapere
/// dove stesse cercando. Una lista vuota senza il posto in cui si è guardato è
/// indistinguibile da un guasto.
#[tauri::command]
fn flow_places() -> Vec<Place> {
    flow_sources()
        .into_iter()
        .map(|source| Place {
            origin: source.origin.to_owned(),
            path: source.dir.display().to_string(),
            exists: source.dir.is_dir(),
            count: ui::gather::load_flow_registry(&source.dir).len(),
        })
        .collect()
}

/// Dove si scrive un flusso.
///
/// **UN FLUSSO CHE ESISTE SI SALVA DOV'ERA.** Salvarlo altrove ne creerebbe una
/// seconda copia che vince sulla prima per posizione, e chi lo ha modificato
/// vedrebbe la sua modifica funzionare qui e sparire su un'altra macchina —
/// oppure, peggio, resterebbe l'originale a girare senza che nessuno capisca
/// perché la modifica non ha effetto.
///
/// **UNO NUOVO VA NELL'ULTIMA SORGENTE**, cioè la più specifica: il progetto se
/// se ne sta guardando uno, altrimenti la casa di chi usa Sailor. È il posto che
/// chi scrive sta guardando in quel momento.
fn flows_dir_for(name: &str) -> std::path::PathBuf {
    let sources = flow_sources();
    for source in &sources {
        if source.dir.join(format!("{name}.flow.json")).exists() {
            return source.dir.clone();
        }
    }
    sources
        .last()
        .map(|source| source.dir.clone())
        .unwrap_or_else(ui::gather::default_flows_dir)
}

#[derive(Serialize)]
struct Place {
    origin: String,
    path: String,
    exists: bool,
    count: usize,
}

fn main() {
    tauri::Builder::default()
        // LE CORSE VIVONO NEL GUSCIO, NON NELLA PAGINA. Chi chiude il pannello
        // della vista o ricarica la tela non deve fermare un flusso che sta
        // girando: il registro sta qui, e chi si riaffaccia ritrova tutto
        // quello che è stato detto mentre non guardava.
        .manage(std::sync::Arc::new(run::Runs::default()))
        // LA MODALITÀ VIVA SI DICHIARA — guasto 11. Il supervisore
        // (`sailor-live`) tiene accesa questa finestra anche quando la
        // ricostruzione fallisce; senza questa riga la terrebbe accesa **in
        // silenzio**, cioè mostrerebbe codice vecchio facendolo passare per
        // nuovo. Fuori dalla modalità viva il file di stato non esiste e questo
        // filo non dice mai niente.
        .setup(|app| {
            live::watch(&app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            flows,
            flow_places,
            live::live_status,
            flows::save_flow,
            flows::delete_flow,
            tools::discover_tools,
            run::flow_trigger,
            run::start_run,
            run::run_snapshot,
            run::known_runs,
            run::open_runs,
            run::step_history,
            run::run_usage,
            board::execution_history,
            board::day_summary,
            board::machine_inventory
        ])
        .run(tauri::generate_context!())
        .expect("la finestra di Sailor non si è aperta");
}
