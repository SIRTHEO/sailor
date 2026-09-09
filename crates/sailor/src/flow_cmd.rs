//! `sailor flow`: loads the declarative files from `flows/`, shows the broken
//! ones too, checks that the actions named exist, and runs the graph in
//! Sailor's shared durable ledger.

// The file format lives in the flow crate: here it is imported, never
// redeclared. Writing it out twice made the two match by luck instead of by
// construction.
use crate::Form;
use flow::{ActionRegistry, FlowFile, Graph};
use ledger::Ledger;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use ui::gather::FlowSource;

mod beat;
mod cap_and_schedule;
pub mod check;
mod cost;
mod create_and_delete;
mod edit;
mod engines;
mod extensions;
mod hazards;
mod relocate;
mod run_and_resume;
pub mod seeds;
#[cfg(test)]
mod test_support;

use beat::{due_flows, tick_flows, waiting_report};
use cap_and_schedule::{cap_of, schedule_of, set_cap, set_schedule};
use check::check_flow;
use cost::cost_of;
use create_and_delete::{delete_flow, new_flow};
use edit::edit_flow;
use relocate::relocate_flow;
pub(crate) use run_and_resume::record_run;
use run_and_resume::{resume_run, run_flow};

pub use run_and_resume::{resume_run_in, resume_run_with};

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
        [command, words @ ..] if command == "search" && !words.is_empty() => {
            search_flows(sources, &words.join(" "))
        }
        [command] if command == "due" => due_flows(sources),
        [command] if command == "tick" => tick_flows(sources),
        [command, name] if command == "check" => check_flow(sources, name, true),
        [command, name, flag] if command == "check" && flag == "--no-engines" => {
            check_flow(sources, name, false)
        }
        [command, name] if command == "run" => run_flow(sources, name, None),
        [command, name, text] if command == "run" => run_flow(sources, name, Some(text)),
        [command, run_id] if command == "resume" => resume_run(run_id),
        [command, name] if command == "cost" => cost_of(name),
        [command] if command == "seeds" => seeds::seeds_report(),
        [command, name] if command == "relocate" => relocate_flow(sources, name, None),
        [command, name, from] if command == "relocate" => relocate_flow(sources, name, Some(from)),
        [command, name] if command == "cap" => cap_of(sources, name),
        [command, name, value] if command == "cap" => set_cap(sources, name, value),
        [command, name] if command == "schedule" => schedule_of(sources, name),
        [command, name, value] if command == "schedule" => set_schedule(sources, name, value, None),
        [command, name, value, weight] if command == "schedule" => {
            set_schedule(sources, name, value, Some(weight))
        }
        [command, name, gesture @ ..] if command == "edit" && !gesture.is_empty() => {
            edit_flow(sources, name, gesture)
        }
        [command, name] if command == "new" => new_flow(sources, name),
        [command, name] if command == "delete" => delete_flow(sources, name),
        [command] if command == "publish" => crate::publish_cmd::publish_flows(sources, None),
        [command, remote] if command == "publish" => {
            crate::publish_cmd::publish_flows(sources, Some(remote))
        }
        _ => Err(usage()),
    }
}

/// The flows this machine sees, each with its origin.
///
/// **THE COMMAND LINE AND THE WINDOW MUST LOOK IN THE SAME PLACES.** This
/// command once read `flows/` under the current directory and nothing else: on
/// a freshly installed machine it answered «no flow found in flows/» while the
/// window, from the same binary, showed two shipped inside it. Two answers to
/// one question give two people telling each other different things about the
/// same product.
///
/// **A FLOW IS NAMED, NOT WALKED.** The name is looked up in a list already
/// built: a name that list does not hold opens nothing, so there is nowhere to
/// escape from — where before the name became a path and needed a guard.
fn known_flows(sources: &[FlowSource]) -> Vec<(String, &'static str, Result<FlowFile, String>)> {
    ui::gather::load_all_flows(sources)
}

/// The flow called that, with the origin it comes from.
fn one_flow(sources: &[FlowSource], name: &str) -> Result<(FlowFile, &'static str), String> {
    let known = known_flows(sources);
    match known.iter().find(|(known, _, _)| known == name) {
        Some((_, origin, Ok(flow))) => Ok((flow.clone(), origin)),
        Some((_, origin, Err(reason))) => Err(catalogue::say(
            "cli.flow.does_not_load",
            &[("flow", name), ("origin", origin), ("reason", reason)],
        )),
        None => {
            if let Some(became) = flow::system::past_name(name) {
                return Err(match became {
                    flow::system::PastName::CarriedOnBy(now) => catalogue::say(
                        "cli.flow.that_name_is_carried_on",
                        &[("flow", name), ("now", now)],
                    ),
                    flow::system::PastName::NothingCarriesIt => {
                        catalogue::say("cli.flow.that_name_is_past", &[("flow", name)])
                    }
                });
            }
            let names: Vec<&str> = known.iter().map(|(name, _, _)| name.as_str()).collect();
            let in_sight = match names.is_empty() {
                true => catalogue::say("cli.flow.none_in_sight", &[]),
                false => names.join(", "),
            };
            Err(catalogue::say(
                "cli.flow.no_flow_by_that_name",
                &[("flow", name), ("names", &in_sight)],
            ))
        }
    }
}

/// Where it looked, always after an empty list: an empty list that does not
/// say where it searched is indistinguishable from a fault.
fn nothing_found(sources: &[FlowSource]) -> String {
    catalogue::say(
        "cli.flow.nothing_found",
        &[(
            "places",
            &sources
                .iter()
                .map(|source| format!("{}: {}", source.origin, source.dir.display()))
                .collect::<Vec<_>>()
                .join("\n  "),
        )],
    )
}

/// The forms of `sailor flow`, one per line.
///
/// **THE LIST OF GESTURES IS HERE, IN ONE PLACE ONLY.** A test demands that
/// every subcommand `dispatch` accepts appear in this list: a gesture the
/// program can do and nobody knows to ask for does not exist, and not finding
/// one is how fault 15 got worked around with `python3`.
///
/// **AND IT IS A `const`, NOT A STRING INSIDE `usage()`, BECAUSE THE WINDOW
/// READS IT TOO.** A string printed by a private function cannot be queried by
/// a program: the window's help page would have been a second copy diverging at
/// the first option added. `Command::usage` points here. The two rules do not
/// exclude each other: one says the list is complete, the other that it is one.
pub const USAGE: &[Form] = &[
    Form {
        form: "sailor flow list",
        says_key: "",
    },
    Form {
        form: "sailor flow search <words>",
        says_key: "",
    },
    Form {
        form: "sailor flow due",
        says_key: "",
    },
    Form {
        form: "sailor flow tick",
        says_key: "",
    },
    Form {
        form: "sailor flow check <name> [--no-engines]",
        says_key: "",
    },
    Form {
        form: "sailor flow run <name> [mandate]",
        says_key: "",
    },
    Form {
        form: "sailor flow resume <run>",
        says_key: "",
    },
    Form {
        form: "sailor flow cost <name>",
        says_key: "",
    },
    Form {
        form: "sailor flow seeds",
        says_key: "cli.flow.form.seeds",
    },
    // **`micro`, `nessuno`, `leggero` AND `pesante` STAY AS THEY ARE, AND IT
    // IS NOT AN OVERSIGHT.** They are the words a user really types and the
    // code compares, and a `schedule` already written keeps them in the ledger.
    // Translating them here without the parser would make the help lie;
    // translating both places would break schedules already registered — the
    // same reason flow `id`s stay in Italian (`AGENTS.md`, the ledger data
    // line). If they are ever wanted in English, the way is to accept both
    // languages and show the new one, never to replace them.
    Form {
        form: "sailor flow cap <name> [micros|none]",
        says_key: "",
    },
    Form {
        form: "sailor flow schedule <name> [3600s|07:30|none] [light|heavy]",
        says_key: "",
    },
    Form {
        form: "sailor flow edit <name> trigger <3600s|07:30|none> [light|heavy]",
        says_key: "",
    },
    Form {
        form: "sailor flow edit <name> cap <micros|none>",
        says_key: "",
    },
    Form {
        form: "sailor flow edit <name> add-step <step> <action>",
        says_key: "",
    },
    Form {
        form: "sailor flow edit <name> remove-step <step>",
        says_key: "",
    },
    Form {
        form: "sailor flow edit <name> connect <from> <to>",
        says_key: "cli.flow.form.connect",
    },
    Form {
        form: "sailor flow edit <name> disconnect <from> <to>",
        says_key: "",
    },
    Form {
        form: "sailor flow edit <name> engines <step> <a,b|none>",
        says_key: "",
    },
    Form {
        form: "sailor flow edit <name> field <step> <key> <value|none>",
        says_key: "cli.flow.form.field",
    },
    Form {
        form: "sailor flow new <name>",
        says_key: "cli.flow.form.new",
    },
    Form {
        form: "sailor flow delete <name>",
        says_key: "cli.flow.form.delete",
    },
    Form {
        form: "sailor flow relocate <name> [prefix-to-strip]",
        says_key: "",
    },
    Form {
        form: "sailor flow publish [remote]",
        says_key: "",
    },
];

/// The flows that mention the words, best first, with the line that matched.
fn search_flows(sources: &[FlowSource], query: &str) -> Result<String, String> {
    let hits = actions::search::rank_flows(&known_flows(sources), query)?;
    if hits.is_empty() {
        return Ok(catalogue::say("cli.flow.search_nothing", &[("query", query)]));
    }
    let mut lines = vec![catalogue::say(
        "cli.flow.search_found",
        &[("count", &hits.len().to_string()), ("query", query)],
    )];
    for hit in &hits {
        lines.push(format!(
            "  {} · {}\n      {}",
            hit["flow"].as_str().unwrap_or_default(),
            hit["origin"].as_str().unwrap_or_default(),
            hit["excerpt"].as_str().unwrap_or_default().replace('\n', " ")
        ));
    }
    Ok(lines.join("\n"))
}

fn usage() -> String {
    format!(
        "{}\n  {}",
        catalogue::say("cli.usage_heading", &[]),
        crate::forms_as_lines(USAGE).join("\n  ")
    )
}

fn list_flows(sources: &[FlowSource]) -> Result<String, String> {
    let known = known_flows(sources);
    if known.is_empty() {
        return Ok(nothing_found(sources));
    }
    let mut report = String::new();
    // THE ORIGIN IS IN THE LIST, and it is no ornament: two flows of the same
    // name in two places are one in here — the most specific wins — and whoever
    // cannot see where the running one comes from edits the other.
    for (name, origin, entry) in known {
        match entry {
            Ok(flow) => {
                let _ = writeln!(
                    report,
                    "{}\t{} steps\t{origin}\t{}",
                    flow.id,
                    flow.graph.steps().len(),
                    flow.description
                );
            }
            Err(error) => {
                let _ = writeln!(
                    report,
                    "{}",
                    catalogue::say(
                        "cli.flow.list_row_does_not_load",
                        &[("name", &name), ("origin", origin), ("error", &error)],
                    )
                );
            }
        }
    }
    let _ = write!(report, "{}", waiting_report());
    Ok(report)
}

/// The default ledger if it opens, `None` if it is absent or will not open.
///
/// It does not report the error: the caller is doing a static check, and an
/// absent ledger is no fault of the flow it is looking at. Whoever must *run*
/// opens the ledger itself and demands that it succeed.
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
    ledger::default_directory().ok_or_else(|| catalogue::say("cli.flow.no_home_no_ledger", &[]))
}

fn now_secs() -> Result<i64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .map_err(|error| {
            catalogue::say(
                "cli.clock_before_the_epoch",
                &[("error", &error.to_string())],
            )
        })
}

fn new_run_id(flow_id: &str) -> Result<String, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| format!("{flow_id}-{}", duration.as_nanos()))
        .map_err(|error| {
            catalogue::say(
                "cli.clock_before_the_epoch",
                &[("error", &error.to_string())],
            )
        })
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use registry::{registry_in, House};
    use std::fs;

    #[test]
    fn list_keeps_an_unloadable_flow_visible_with_its_reason() {
        let directory = TestDirectory::new();
        directory.write("buono.flow.json", &flow_json("shell_check", "[]", "{}"));
        directory.write("rotto.flow.json", "{ non-json");

        let report = list_flows(&[FlowSource {
            origin: "di prova",
            dir: directory.0.clone(),
        }])
        .expect("listing the flows");

        assert!(report.contains("prova\t1 steps\tdi prova"), "{report}");
        assert!(
            report.contains("rotto\tdi prova\tdoes not load:"),
            "{report}"
        );
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

        let error =
            serde_json::from_str::<FlowFile>(json).expect_err("the cycle must be refused");

        assert!(error.to_string().contains("backward dependency"), "{error}");
    }

    /// **EVERY GESTURE `dispatch` CAN DO IS WRITTEN IN THE USAGE.**
    ///
    /// A command the program runs and nobody knows to ask for does not exist:
    /// whoever cannot find it leaves the system, which is how fault 15 happened
    /// — `python3` in place of a gesture nobody knew they had. The usage line
    /// is the only interface of whoever is at the terminal.
    ///
    /// **THE SOURCE IS READ INSTEAD OF RUN, AND THE REASON IS FAULT 5.**
    /// Calling `dispatch` for every word would make `cost` and `resume` open
    /// **this machine's** ledger: a test reading the state of whoever runs it
    /// goes red on a cleanup, at unchanged code. The mutant that fells it adds
    /// an arm to `dispatch` without naming it in `usage()`.
    #[test]
    fn every_arm_of_the_dispatcher_is_written_in_the_usage_line() {
        let source = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/flow_cmd.rs");
        let text = fs::read_to_string(&source).expect("this file reads back");
        let body = text
            .split_once("fn dispatch(")
            .and_then(|(_, after)| after.split_once("\nfn "))
            .map(|(body, _)| body)
            .expect("the body of dispatch");

        let mut arms: BTreeSet<String> = BTreeSet::new();
        for piece in body.split("command == \"").skip(1) {
            let word = piece
                .split_once('"')
                .map(|(word, _)| word.to_owned())
                .expect("a word between quotes");
            arms.insert(word);
        }
        assert!(
            arms.len() >= 8,
            "too few arms were found, the way of reading them has broken: {arms:?}"
        );
        assert!(arms.contains("schedule"), "the new arm is there: {arms:?}");

        let usage = usage();
        let missing: Vec<&String> = arms.iter().filter(|arm| !usage.contains(*arm)).collect();
        assert!(
            missing.is_empty(),
            "these gestures exist and are written down nowhere: {missing:?}\n{usage}\n\
             A gesture nobody knows they can ask for is a gesture that is not there, \
             and whoever cannot find it leaves Sailor to do it by hand"
        );
    }

    /// A USER'S FLOWS ARE NOT A FIXTURE. This test once included a file from
    /// `flows/` at compile time: the day that folder was emptied — a legitimate
    /// gesture by whoever uses the program — **the crate stopped compiling**.
    /// A test suite cannot depend on user data.
    ///
    /// What the test meant still holds for all: every flow present loads in the
    /// decided shape and names no action the engine cannot run. An empty folder
    /// is no failure — there is nothing to verify — but it does not pass for a
    /// verification either: the count is printed, so whoever reads the green
    /// knows how many files it went over.
    #[test]
    fn every_flow_on_disk_loads_and_names_only_registered_actions() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../flows");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            println!("no flows directory: there is nothing to check");
            return;
        };
        let mut checked = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.to_string_lossy().ends_with(".flow.json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("reading the flow");
            let flow: FlowFile = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("{} does not load: {e}", path.display()));
            let unknown = missing_actions(&flow.graph, &registry_in(House::empty(), None, None));
            assert!(
                unknown.is_empty(),
                "{} names actions the engine does not know: {unknown:?}",
                path.display()
            );
            checked += 1;
        }
        println!("flows checked: {checked}");
    }

    /// The decided file shape, on a fixture of ours: this test must fail if the
    /// format changes, never if someone deletes a file of their own.
    #[test]
    fn the_decided_file_shape_still_loads() {
        let inputs = r#"{"solo":{"command":"true","env":{},"timeout_secs":1}}"#;
        let json = flow_json("shell_check", "[]", inputs);
        let flow: FlowFile = serde_json::from_str(&json).expect("the decided shape loads");
        assert_eq!(flow.graph.steps().len(), 1);
        assert!(missing_actions(&flow.graph, &registry_in(House::empty(), None, None)).is_empty());
    }

    /// A NAME NO LONGER BECOMES A PATH, and the protection changes in kind:
    /// `../segreto` used to be joined to the folder and needed a guard to
    /// refuse it; the name is now looked up in a list already built, so it
    /// opens nothing because nothing is called that. The test stays because the
    /// guarantee must: no name may make a file that is not a flow of this
    /// machine be read.
    #[test]
    fn a_name_that_is_not_a_known_flow_opens_nothing() {
        let directory = TestDirectory::new();
        directory.write("buono.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = [FlowSource {
            origin: "di prova",
            dir: directory.0.clone(),
        }];

        for name in [
            "../segreto",
            "cartella/segreto",
            "",
            "..",
            "buono.flow.json",
        ] {
            let refused = one_flow(&sources, name).expect_err(&format!(
                "«{name}» is not a flow of this machine and must not open"
            ));
            assert!(refused.contains("no flow is called"), "«{name}»: {refused}");
        }
        assert!(
            one_flow(&sources, "buono").is_ok(),
            "the real flow does open"
        );
    }

    /// A NAME THE LEDGER HOLDS IS ANSWERED, NOT LISTED AGAINST. Offering the
    /// names in sight to somebody who typed the old name of a flow reads as a
    /// typo, while the runs under that name are in the ledger.
    #[test]
    fn a_name_the_catalogue_no_longer_ships_is_answered_for() {
        let directory = TestDirectory::new();
        let sources = [FlowSource {
            origin: "di prova",
            dir: directory.0.clone(),
        }];

        let renamed = one_flow(&sources, "smista-il-lavoro").expect_err("it ships under its name");
        assert_eq!(
            catalogue::say(
                "cli.flow.that_name_is_carried_on",
                &[("flow", "smista-il-lavoro"), ("now", "dispatch-the-work")]
            ),
            renamed
        );

        let gone = one_flow(&sources, "che-cosa-gira").expect_err("nothing carries it");
        assert_eq!(
            catalogue::say("cli.flow.that_name_is_past", &[("flow", "che-cosa-gira")]),
            gone
        );
    }

    /// THE SHIPPED FLOWS ARE SEEN FROM THE COMMAND LINE TOO. The defect this
    /// test exists to catch: `sailor flow list` answered «no flow» on a freshly
    /// installed machine while the window, from the same binary, showed two.
    #[test]
    fn the_command_line_sees_the_shipped_flows_too() {
        let report = list_flows(&[FlowSource::builtin()]).expect("elencare i flussi");
        for (name, _) in flow::system::FLOWS {
            assert!(report.contains(name), "manca «{name}» in:\n{report}");
        }
        assert!(report.contains("built in"), "{report}");
    }
}
