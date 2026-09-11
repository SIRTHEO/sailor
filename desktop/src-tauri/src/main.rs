//! The native shell of the Sailor window.
//!
//! WHY A SHELL AND NOT AN ADDRESS. The flows canvas existed as a page served by
//! Vite, and `sailor ui` served a second, read-only page on `127.0.0.1:47831`.
//! Both ask the watcher to open a browser and remember a port. The decision
//! says the opposite — "a real program, in a native window" — and while the
//! shell does not exist that line describes an intention, not a product.
//!
//! THERE IS NO LOGIC IN HERE, AND THAT IS DELIBERATE. The shell opens the
//! window and hands it what the engine already knows: the reckoning about the
//! flows lives in the engine, the drawing lives in the canvas. Any line of
//! judgement landing here would be a fourth truth beside `crates/flow`,
//! `crates/ui` and `desktop/src/flow.ts`, which today agree by discipline and
//! not by construction.

use serde::Serialize;
use ui::gather::{flow_sources, load_all_flows};

mod beat;
mod board;
mod changes;
mod engines;
mod events;
mod faults;
mod flows;
mod handoff;
mod keeps;
mod ledger;
mod live;
mod locks;
mod machine;
mod manual;
mod models;
mod profiles;
mod run;
mod terminal;
mod tools;
mod workspaces;
mod worktree;

/// A flow as the canvas receives it. It mirrors `FlowEntry` of
/// `desktop/src/flow.ts`, tag included: whoever changes either changes both.
///
/// A BROKEN FLOW DOES NOT VANISH, it arrives with its reason. It is the choice
/// `load_flow_registry` documents: a list that shortens in silence makes people
/// believe the flow does not exist, and nobody goes hunting for a file that, as
/// far as the window says, is not there.
#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum FlowEntry {
    Loaded {
        flow: flow::FlowFile,
        /// Which source it comes from: "yours", "the project's", "declared".
        /// Seeing two flows of one name, you must be able to tell which runs.
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

/// The declared flows, read from disk on every request.
///
/// **Reread, never held in memory.** `sailor ui` loads the registry once at
/// startup, and from then on a flow added or corrected fails to show up until
/// somebody restarts the server — a defect that in an always-open window is
/// noticed at once and in a server is not. It is fourteen files of a few dozen
/// lines each: rereading them costs less than explaining to the watcher why
/// what was just written is missing.
#[tauri::command]
fn flows(app: tauri::AppHandle) -> Vec<FlowEntry> {
    // READING IS THE EVENT: whoever looks at the flows has the due ones
    // started, so a schedule expires when somebody looks, not only on the clock.
    beat::on_read(&app);
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

/// Where the window looked, and what it found in each place.
///
/// **IT EARNS ITS KEEP WHEN NOTHING IS FOUND**, and that is why it exists: the
/// window used to say "no flow" while four flows existed one folder away, and
/// from inside there was no way to tell where it was searching. An empty list
/// lacking the place that was searched is indistinguishable from a fault.
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
        // THE RUNS LIVE IN THE SHELL, NOT IN THE PAGE. Closing the view panel
        // or reloading the canvas must not stop a flow that is running: the
        // registry lives here, and whoever looks back in finds everything said
        // while they were away.
        .manage(std::sync::Arc::new(run::Runs::default()))
        .manage(std::sync::Arc::new(beat::Beat::default()))
        // LIVE MODE DECLARES ITSELF — fault 11. The supervisor (`sailor-live`)
        // keeps this window lit even when the rebuild fails; lacking this line
        // it would keep it lit **in silence**, showing old code and passing it
        // off as new. Outside live mode the status file does not exist and this
        // thread never says a thing.
        .setup(|app| {
            live::watch(&app.handle().clone());
            // THE DEADLINE: due flows start from in here, on a clock of their
            // own, whether or not anybody reads the flows or types a command.
            beat::keep(&app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            flows,
            flow_places,
            live::live_status,
            live::take_new_build,
            flows::save_flow,
            flows::delete_flow,
            flows::flow_texts,
            tools::discover_tools,
            tools::tools_sweep,
            engines::engines,
            faults::faults,
            faults::fault_status,
            ledger::ledger_held,
            ledger::ledger_tables,
            ledger::ledger_query,
            keeps::what_sailor_keeps,
            machine::what_sailor_lit,
            machine::the_dev_port,
            machine::what_weighs_here,
            machine::free_the_machine,
            flows::engine_actions,
            run::flow_trigger,
            run::start_run,
            run::stop_run,
            handoff::handed_steps,
            handoff::take_handed_step,
            handoff::close_handed_step,
            run::run_snapshot,
            run::known_runs,
            run::open_runs,
            run::step_history,
            run::run_usage,
            beat::beat_report,
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
            terminal::terminals_abandoned,
            terminal::terminal_backlog,
            workspaces::workspaces,
            workspaces::left_column,
            workspaces::work_here,
            workspaces::workspace_declaration,
            profiles::profiles,
            profiles::profile_command_lines,
            profiles::profile_switch,
            profiles::profile_create,
            profiles::profile_adopt,
            models::models_catalogue,
            models::quota,
            models::model_set,
            worktree::worktree_list,
            worktree::worktree_create,
            worktree::worktree_remove,
            changes::workspace_changes,
            changes::open_in_editor,
            changes::who_opens_files
        ])
        .run(tauri::generate_context!())
        .expect("Sailor's window did not open");
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
