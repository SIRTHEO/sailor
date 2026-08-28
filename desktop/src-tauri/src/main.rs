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
use ui::gather::{default_flows_dir, load_flow_registry};

mod flows;

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
    Loaded { flow: flow::FlowFile },
    Broken { broken: BrokenFlow },
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
    load_flow_registry(&default_flows_dir())
        .into_iter()
        .map(|(name, entry)| match entry {
            Ok(flow) => FlowEntry::Loaded { flow },
            Err(reason) => FlowEntry::Broken {
                broken: BrokenFlow { name, reason },
            },
        })
        .collect()
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![flows, flows::save_flow, flows::delete_flow])
        .run(tauri::generate_context!())
        .expect("la finestra di Sailor non si è aperta");
}
