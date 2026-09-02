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
mod faults;
mod flows;
mod ledger;
mod live;
mod manual;
mod models;
mod profiles;
mod run;
mod terminal;
mod tools;
mod workspaces;
mod worktree;

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

/// Where a flow gets written, and what that place is called.
///
/// **A FLOW THAT EXISTS IS SAVED WHERE IT WAS.** Saving it elsewhere would make
/// a second copy that beats the first by position, and whoever edited it would
/// see the edit work here and vanish on another machine — or worse, the
/// original would keep running with nobody able to say why the edit does
/// nothing.
///
/// **A NEW ONE GOES TO THE LAST SOURCE**, the most specific: the project, if
/// one is being looked at, otherwise the home of whoever uses Sailor. That is
/// the place the writer is looking at right then.
///
/// **AND THE ORIGIN COMES BACK WITH THE FOLDER.** Whoever saves a new flow is
/// the only one who knows where it landed, and while that stayed here the
/// window had to guess: a freshly created flow showed up with no origin, and a
/// list grouped by origin would have had to invent one. The origin is a
/// `&'static str` living in `flow::system`, so returning it copies nothing.
fn place_for(name: &str) -> (&'static str, std::path::PathBuf) {
    let sources = flow_sources();
    for source in &sources {
        if source.dir.join(format!("{name}.flow.json")).exists() {
            return (source.origin, source.dir.clone());
        }
    }
    sources
        .last()
        .map(|source| (source.origin, source.dir.clone()))
        .unwrap_or_else(|| (flow::system::YOUR_ORIGIN, ui::gather::default_flows_dir()))
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
            tools::tools_sweep,
            faults::faults,
            faults::fault_status,
            ledger::ledger_held,
            flows::engine_actions,
            run::flow_trigger,
            run::start_run,
            run::run_snapshot,
            run::known_runs,
            run::open_runs,
            run::step_history,
            run::run_usage,
            board::execution_history,
            board::day_summary,
            board::machine_inventory,
            manual::manual,
            terminal::terminal_open,
            terminal::terminal_submit,
            terminal::terminal_press,
            terminal::terminal_resize,
            terminal::terminal_close,
            terminal::terminal_list,
            workspaces::workspaces,
            workspaces::workspace_declaration,
            profiles::profiles,
            profiles::profile_command_lines,
            profiles::profile_switch,
            profiles::profile_create,
            models::models_catalogue,
            models::quota,
            models::model_set,
            worktree::worktree_list,
            worktree::worktree_create,
            worktree::worktree_remove
        ])
        .run(tauri::generate_context!())
        .expect("la finestra di Sailor non si è aperta");
}

#[cfg(test)]
mod tests {
    /// **THE ORIGIN AND THE FOLDER COME FROM ONE SOURCE, OR THEY LIE
    /// TOGETHER.** A pair built in two steps can name the origin of one place
    /// and the path of another, and the column would draw a saved flow under a
    /// heading it does not belong to with nothing to say so — the file is
    /// written correctly either way. Which place wins depends on the machine;
    /// that the two halves are one source does not.
    #[test]
    fn the_place_a_flow_is_written_to_names_itself_with_its_own_origin() {
        let sources = super::flow_sources();
        assert!(
            !sources.is_empty(),
            "no source at all: the rest measures nothing"
        );

        let (origin, dir) = super::place_for("a-flow-nobody-has-ever-written");
        let matching = sources
            .iter()
            .find(|source| source.dir == dir)
            .unwrap_or_else(|| panic!("the folder {} belongs to no source", dir.display()));
        assert_eq!(
            origin, matching.origin,
            "the origin says «{origin}» and the folder is the one of «{}»",
            matching.origin,
        );

        // THE ABSURD CASE FIRST would be a name that exists everywhere; the
        // cheap one available here is the opposite: a flow that exists nowhere
        // must land in the last source, the most specific one. If it did not,
        // the loop above would be matching by luck.
        assert_eq!(
            dir,
            sources.last().expect("checked above").dir,
            "a flow nobody owns did not go to the most specific place",
        );
    }
}
