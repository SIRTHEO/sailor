//! What Sailor has left running on this machine, and the gesture that frees it.
//!
//! Fault 4 again, from the other end. The supervisor has known how to answer
//! this since it was written — `left_running`, `close_the_ones_that_stopped_
//! breathing`, `who_holds` — and nothing outside its own tests ever called
//! them. A power nobody invokes is a power the machine never gets.

use crate::Form;
use ledger::Ledger;
use std::path::Path;
use ledger::holdings::{Holding, Whose};
use machine::{
    build_directories_left, BUILD_DIRECTORY, left_running, on_the_port, stop_the_ones_nobody_wants, what_weighs,
    LeftBehind, OnThePort, Teardown, TheLoad, DEV_PORT,
};

pub const USAGE: &[Form] = &[
    Form {
        form: "sailor machine",
        says_key: "cli.machine.says",
    },
    Form {
        form: "sailor machine free",
        says_key: "cli.machine.form.free",
    },
    Form {
        form: "sailor machine free-one <path>",
        says_key: "cli.machine.form.free_one",
    },
];

pub fn run(args: &[String]) -> i32 {
    let verb = args.first().map(String::as_str).unwrap_or("");
    let answered = match verb {
        "" => reading(),
        "free" => free(),
        "free-one" => match args.get(1) {
            Some(path) => free_one(path, open_ledger().ok().as_ref()),
            None => Err(crate::forms_as_lines(USAGE).join("\n")),
        },
        other => Err(catalogue::say("cli.no_such_form", &[("verb", other)])),
    };
    match answered {
        Ok(said) => {
            println!("{said}");
            0
        }
        Err(why) => {
            eprintln!("sailor machine: {why}");
            2
        }
    }
}

fn open_ledger() -> Result<Ledger, String> {
    let directory =
        ledger::default_directory().ok_or_else(|| catalogue::say("cli.no_home", &[]))?;
    Ledger::open(&directory).map_err(|error| error.to_string())
}

/// One row of the reading, already decided.
struct Standing {
    pid: u32,
    purpose: String,
    alive: bool,
    /// True when the run that owns it has ended, or it never had one.
    run_is_over: bool,
}

/// What the store says is running, with the two facts kept apart: whether the
/// pid still breathes, and whether anybody still wants it.
fn standing(store: &Ledger) -> Result<Vec<Standing>, String> {
    let mut found = Vec::new();
    for item in left_running(store).map_err(|error| error.to_string())? {
        // **A PROCESS WHOSE RUN IS OVER IS A LEFTOVER, ALIVE OR NOT.** Asked
        // only whether the pid breathes, a dev server outliving its run for
        // three hours looks exactly like one somebody is using.
        let run_is_over = match &item.record.run_id {
            None => true,
            Some(run) => store
                .run_header(run)
                .map_err(|error| error.to_string())?
                .is_none_or(|header| header.ended_at.is_some()),
        };
        found.push(Standing {
            pid: item.record.pid,
            purpose: item.record.purpose.clone(),
            alive: item.still_alive,
            run_is_over,
        });
    }
    Ok(found)
}

fn reading() -> Result<String, String> {
    let store = match open_ledger() {
        Ok(store) => store,
        Err(why) => return Ok(without_the_store(&why, about_the_disk(None))),
    };
    let rows = standing(&store)?;
    let mut said = String::new();
    for row in &rows {
        let breath = catalogue::say(
            if row.alive { "cli.machine.alive" } else { "cli.machine.gone" },
            &[],
        );
        let tail = if row.run_is_over {
            catalogue::say("cli.machine.run_is_over", &[])
        } else {
            String::new()
        };
        said.push_str(&catalogue::say(
            "cli.machine.row",
            &[
                ("pid", &row.pid.to_string()),
                ("breath", &breath),
                ("purpose", &row.purpose),
                ("tail", &tail),
            ],
        ));
        said.push('\n');
    }
    said.push_str(&catalogue::say(
        "cli.machine.standing",
        &[
            ("running", &rows.iter().filter(|row| row.alive).count().to_string()),
            ("total", &rows.len().to_string()),
        ],
    ));
    // **WHAT NEVER PASSED THROUGH SAILOR CANNOT BE FREED BY IT**, and that is
    // the case that filled this machine: a dev server started by hand holds the
    // port, is in no row, and no sweep will ever find it. Saying so is all this
    // can do, and it is more than saying nothing.
    if let Some(word) = about_the_port(&store)? {
        said.push('\n');
        said.push_str(&word);
    }
    said.push('\n');
    said.push_str(&about_the_load(&store)?);
    if let Some(word) = about_the_disk(Some(&store)) {
        said.push('\n');
        said.push_str(&word);
    }
    Ok(said)
}

/// What is still true of the machine when the store will not open.
fn without_the_store(why: &str, disk: Option<String>) -> String {
    let mut said = catalogue::say("cli.machine.no_store", &[("why", why)]);
    if let Some(word) = disk {
        said.push('\n');
        said.push_str(&word);
    }
    said
}

/// Written down as this process's, for the run that wanted it. `for_run` of
/// `None` says it is a cache to reuse, not rubbish once the gesture is over.
pub fn a_build_directory_is_taken(path: &Path, for_run: Option<String>, purpose: &str) {
    let Ok(store) = open_ledger() else {
        return;
    };
    let holding = ledger::holdings::this_process_takes(
        BUILD_DIRECTORY,
        &path.to_string_lossy(),
        for_run,
        purpose,
    );
    let _ = store.holding_taken(&holding);
}

/// A build directory on the disk, and what the register says of it. `taken` is
/// `None` for one Sailor never made: unknown is not the same as free.
struct OnTheDisk {
    left: LeftBehind,
    taken: Option<(Holding, Whose)>,
}

impl OnTheDisk {
    fn is_nobodys(&self) -> bool {
        matches!(self.taken, Some((_, Whose::Nobody)))
    }

    fn whose_it_is(&self) -> String {
        let Some((holding, whose)) = &self.taken else {
            return catalogue::say("cli.machine.never_taken", &[]);
        };
        match whose {
            Whose::TheProcessThatTookIt => catalogue::say(
                "cli.machine.held_now",
                &[
                    ("pid", &holding.held_by_pid.to_string()),
                    ("purpose", &holding.purpose),
                ],
            ),
            Whose::TheRunItWasTakenFor => catalogue::say(
                "cli.machine.its_run_is_open",
                &[("run", holding.for_run.as_deref().unwrap_or(""))],
            ),
            Whose::KeptOnPurpose => {
                catalogue::say("cli.machine.kept_on_purpose", &[("purpose", &holding.purpose)])
            }
            Whose::Nobody => catalogue::say("cli.machine.nobodys_now", &[]),
            Whose::Uncertain => catalogue::say("cli.machine.holder_unsettled", &[]),
        }
    }
}

/// The build directories on the disk, each next to whoever holds it.
///
/// Without a store nothing is claimed by anybody, and so nothing is taken:
/// the register is the only thing that knows, and a machine that cannot ask
/// it must not guess.
fn build_directories(root: &Path, store: Option<&Ledger>) -> Vec<OnTheDisk> {
    let held = store
        .and_then(|store| store.holdings_left_held(BUILD_DIRECTORY).ok())
        .unwrap_or_default();
    build_directories_left(root)
        .into_iter()
        .map(|left| {
            let name = left.path.to_string_lossy().into_owned();
            let taken = held.iter().find(|one| one.name == name).map(|holding| {
                let settled = ledger::holdings::whose(holding, &|run| run_is_open(store, run));
                (holding.clone(), settled)
            });
            OnTheDisk { left, taken }
        })
        .collect()
}

fn run_is_open(store: Option<&Ledger>, run: &str) -> Result<bool, String> {
    let store = store.ok_or_else(|| catalogue::say("cli.no_home", &[]))?;
    store
        .run_header(run)
        .map(|header| header.is_none_or(|one| one.ended_at.is_none()))
        .map_err(|error| error.to_string())
}

fn about_the_disk(store: Option<&Ledger>) -> Option<String> {
    about_the_disk_in(&workspace::root().ok()?, store)
}

fn about_the_disk_in(root: &Path, store: Option<&Ledger>) -> Option<String> {
    let on_disk = build_directories(root, store);
    if on_disk.is_empty() {
        return None;
    }
    let held: u64 = on_disk.iter().map(|one| one.left.bytes).sum();
    let mut said = catalogue::say(
        "cli.machine.build_directories",
        &[
            ("count", &on_disk.len().to_string()),
            ("gigabytes", &gigabytes(held)),
            ("nobodys", &on_disk.iter().filter(|one| one.is_nobodys()).count().to_string()),
        ],
    );
    for one in on_disk.iter().take(ENOUGH_TO_SEE_THE_TROUBLE) {
        said.push('\n');
        said.push_str(&catalogue::say(
            "cli.machine.build_directory",
            &[
                ("path", &one.left.path.display().to_string()),
                ("gigabytes", &gigabytes(one.left.bytes)),
                ("whose", &one.whose_it_is()),
            ],
        ));
    }
    Some(said)
}

/// How many of the heaviest build directories are worth naming.
const ENOUGH_TO_SEE_THE_TROUBLE: usize = 6;

fn gigabytes(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1_073_741_824.0)
}

/// **«0 OF 0» ON A MACHINE THAT IS GRINDING.** The rows above are Sailor's own,
/// and what fills a machine is usually what never passed through it. Asked of
/// the system, and marked where a row does name it.
fn about_the_load(store: &Ledger) -> Result<String, String> {
    match what_weighs(store).map_err(|error| error.to_string())? {
        TheLoad::CouldNotLook(why) => Ok(catalogue::say("cli.machine.could_not_look", &[("why", &why)])),
        TheLoad::Seen { load, heaviest } => {
            let mut said = catalogue::say(
                "cli.machine.load",
                &[
                    ("one", &format!("{:.2}", load[0])),
                    ("five", &format!("{:.2}", load[1])),
                    ("fifteen", &format!("{:.2}", load[2])),
                ],
            );
            for one in &heaviest {
                said.push('\n');
                // Two sentences, not one with a hole: an empty catalogue entry
                // is a sentence nobody wrote, and the catalogue refuses it.
                said.push_str(&catalogue::say(
                    if one.sailor_lit { "cli.machine.weighs_ours" } else { "cli.machine.weighs" },
                    &[
                        ("pid", &one.pid.to_string()),
                        ("megabytes", &format!("{}", one.kilobytes / 1024)),
                        ("command", &one.command),
                    ],
                ));
            }
            Ok(said)
        }
    }
}

/// The dev port, when it has something to say. A port Sailor itself lit is not
/// news — it is already a row above.
fn about_the_port(store: &Ledger) -> Result<Option<String>, String> {
    match on_the_port(store, DEV_PORT).map_err(|error| error.to_string())? {
        OnThePort::Free | OnThePort::Ours(_) => Ok(None),
        OnThePort::Somebody => Ok(Some(catalogue::say(
            "cli.machine.somebody_on_the_port",
            &[("port", &DEV_PORT.to_string())],
        ))),
        // A machine that would not let us look says so: read as «in use», a
        // refusal sends a person hunting for a process that is not there.
        OnThePort::CouldNotLook(why) => Ok(Some(catalogue::say(
            "cli.machine.could_not_look_at_the_port",
            &[("port", &DEV_PORT.to_string()), ("why", &why)],
        ))),
    }
}

/// **A GESTURE THAT ONLY REPORTS IS NOT A GESTURE** — and one that acts where
/// it cannot tell is worse. Only a process whose run demonstrably ended is
/// stopped: a run still open means somebody may be using it, and no run at all
/// means nobody wrote down who wanted it, which is not the same as nobody.
fn free() -> Result<String, String> {
    let store = match open_ledger() {
        Ok(store) => store,
        Err(why) => return Ok(without_the_store(&why, free_the_disk(None))),
    };
    let done = stop_the_ones_nobody_wants(&store, now(), &|record| {
        let Some(run) = record.run_id.as_deref() else {
            // Left alone, and named in the reading instead.
            return Ok(true);
        };
        Ok(store.run_header(run)?.is_none_or(|header| header.ended_at.is_none()))
    })
    .map_err(|error| error.to_string())?;
    let mut said = catalogue::say(
        "cli.machine.freed",
        &[(
            "closed",
            &done
                .iter()
                .filter(|one| matches!(one, Teardown::Gone { .. }))
                .count()
                .to_string(),
        )],
    );
    for one in &done {
        said.push('\n');
        said.push_str(&match one {
            Teardown::Gone { pid, purpose, .. } => catalogue::say(
                "cli.machine.stopped",
                &[("pid", &pid.to_string()), ("purpose", purpose)],
            ),
            Teardown::StillThere { pid, purpose, why, .. } => catalogue::say(
                "cli.machine.would_not_stop",
                &[("pid", &pid.to_string()), ("purpose", purpose), ("why", why)],
            ),
        });
    }
    if let Some(word) = free_the_disk(Some(&store)) {
        said.push('\n');
        said.push_str(&word);
    }
    Ok(said)
}

/// The build directories nobody is compiling in, taken away.
///
/// Only the ones still and quiet: one being written to is left where it is and
/// named in the reading, the same rule the processes above follow.
fn free_the_disk(store: Option<&Ledger>) -> Option<String> {
    free_the_disk_in(&workspace::root().ok()?, store)
}

fn free_the_disk_in(root: &Path, store: Option<&Ledger>) -> Option<String> {
    let nobodys: Vec<OnTheDisk> = build_directories(root, store)
        .into_iter()
        .filter(OnTheDisk::is_nobodys)
        .collect();
    if nobodys.is_empty() {
        return None;
    }
    let mut taken = 0u64;
    let mut said = String::new();
    for one in &nobodys {
        if std::fs::remove_dir_all(&one.left.path).is_err() {
            continue;
        }
        if let (Some(store), Some((holding, _))) = (store, &one.taken) {
            let _ = store.holding_let_go(BUILD_DIRECTORY, &holding.name);
        }
        taken += one.left.bytes;
        said.push('\n');
        said.push_str(&catalogue::say(
            "cli.machine.build_directory_taken",
            &[
                ("path", &one.left.path.display().to_string()),
                ("gigabytes", &gigabytes(one.left.bytes)),
            ],
        ));
    }
    Some(format!(
        "{}{said}",
        catalogue::say("cli.machine.disk_freed", &[("gigabytes", &gigabytes(taken))])
    ))
}

/// One build directory named by a person, taken whatever the register says —
/// except while a process is compiling in it, which is not theirs to interrupt.
fn free_one(path: &str, store: Option<&Ledger>) -> Result<String, String> {
    free_one_in(&workspace::root()?, path, store)
}

fn free_one_in(root: &Path, path: &str, store: Option<&Ledger>) -> Result<String, String> {
    let asked = std::path::Path::new(path);
    let Some(one) = build_directories(root, store)
        .into_iter()
        .find(|one| one.left.path == asked)
    else {
        return Err(catalogue::say("cli.machine.no_such_build_directory", &[("path", path)]));
    };
    if matches!(one.taken, Some((_, Whose::TheProcessThatTookIt))) {
        return Err(one.whose_it_is());
    }
    std::fs::remove_dir_all(&one.left.path).map_err(|error| error.to_string())?;
    if let (Some(store), Some((holding, _))) = (store, &one.taken) {
        let _ = store.holding_let_go(BUILD_DIRECTORY, &holding.name);
    }
    Ok(catalogue::say(
        "cli.machine.build_directory_taken",
        &[("path", path), ("gigabytes", &gigabytes(one.left.bytes))],
    ))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger::{ProcessRecord, RunRecord};
    use std::path::PathBuf;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sailor-machine-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("the scratch directory");
        dir
    }

    fn started(process_id: &str, run_id: Option<&str>) -> ProcessRecord {
        ProcessRecord {
            born_at: None,
            process_id: process_id.to_owned(),
            // A pid nothing on this machine holds: the reading must say «gone»
            // without the answer depending on who else is running.
            pid: i32::MAX as u32 - 1,
            command: "npx".to_owned(),
            args: vec!["vite".to_owned()],
            working_directory: "/work/a-project/desktop".to_owned(),
            port: None,
            purpose: "live".to_owned(),
            started_by: "supervisor".to_owned(),
            run_id: run_id.map(str::to_owned),
            started_at: 1_700_000_000,
        }
    }

    fn a_run(run_id: &str, ended_at: Option<i64>) -> RunRecord {
        RunRecord {
            run_id: run_id.to_owned(),
            kind: "flow".to_owned(),
            entity: "a-flow".to_owned(),
            parent_run_id: None,
            started_by: "person".to_owned(),
            status: "running".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: 1_700_000_000,
            ended_at,
            worktree: None,
            stop_reason: None,
        }
    }

    /// **BREATHING AND WANTED ARE TWO QUESTIONS.** Asked only whether the pid
    /// is alive, a dev server outliving its run by three hours looks exactly
    /// like one somebody is using — which is how a machine fills up.
    #[test]
    fn a_process_whose_run_has_ended_is_a_leftover_even_while_it_breathes() {
        let dir = scratch("leftover");
        let store = Ledger::open(&dir).expect("the store");
        store.record_run(&a_run("over", Some(1_700_000_100))).expect("the ended run");
        store.record_run(&a_run("going", None)).expect("the open run");
        store.record_process_started(&started("p-over", Some("over"))).expect("start");
        store.record_process_started(&started("p-going", Some("going"))).expect("start");
        store.record_process_started(&started("p-alone", None)).expect("start");

        let rows = standing(&store).expect("the reading");
        let over = rows.iter().find(|row| row.purpose == "live" && row.run_is_over);
        assert!(over.is_some(), "a process of an ended run is not called a leftover");
        assert_eq!(
            rows.iter().filter(|row| row.run_is_over).count(),
            2,
            "the one still wanted was counted a leftover, or the ownerless one was not"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A pid nobody holds is «gone», and the store still called it running:
    /// that is the ghost row the free gesture exists to close.
    #[test]
    fn a_row_the_store_calls_running_over_a_pid_that_is_gone_is_closed() {
        let dir = scratch("ghost");
        let store = Ledger::open(&dir).expect("the store");
        store.record_process_started(&started("p-ghost", None)).expect("start");
        assert_eq!(standing(&store).expect("before").len(), 1);
        assert!(!standing(&store).expect("before")[0].alive, "the fixture pid is alive here");

        // Nobody wants it and its pid is gone: the teardown closes the row
        // without a signal, because there is nothing left to signal.
        let done = machine::stop_the_ones_nobody_wants(&store, 1_700_000_200, &|_| Ok(false))
            .expect("free");
        assert!(done.is_empty(), "a pid that was already gone was signalled: {done:?}");
        assert!(standing(&store).expect("after").is_empty(), "the row is still called running");
        let _ = std::fs::remove_dir_all(&dir);
    }
    /// Writes down a build directory as taken by a process that is gone, for
    /// the run named. A pid nothing on this machine holds settles the kernel's
    /// half without depending on who else is running.
    fn taken_by_a_dead_process(store: &Ledger, path: &Path, for_run: Option<&str>) {
        store
            .holding_taken(&Holding {
                kind: BUILD_DIRECTORY.to_owned(),
                name: path.to_string_lossy().into_owned(),
                held_by_pid: i32::MAX as u32 - 1,
                held_by_born_at: Some(1_700_000_000),
                for_run: for_run.map(str::to_owned),
                taken_at: 1_700_000_000,
                purpose: "a measurement".to_owned(),
            })
            .expect("the holding goes in");
    }

    fn taken_by_this_process(store: &Ledger, path: &Path) {
        store
            .holding_taken(&ledger::holdings::this_process_takes(
                BUILD_DIRECTORY,
                &path.to_string_lossy(),
                Some("still-going".to_owned()),
                "compiling right now",
            ))
            .expect("the holding goes in");
    }

    /// A build directory of cargo's, made by hand, with the tag cargo writes.
    fn a_build_directory(root: &Path, name: &str, bytes: usize) {
        let path = root.join("target").join(name);
        std::fs::create_dir_all(path.join("debug")).expect("a build directory");
        std::fs::write(
            path.join("CACHEDIR.TAG"),
            "Signature: 8a477f597d28d172789f06886806bc55\n",
        )
        .expect("the tag cargo writes");
        std::fs::write(path.join("debug").join("artefact"), "x".repeat(bytes)).expect("an artefact");
    }

    /// **THE DISK IS PART OF THE MACHINE.** The reading named processes, load
    /// and the port, and said nothing about twenty-two build directories that
    /// had filled the disk to 97%.
    #[test]
    fn the_reading_names_every_build_directory_and_says_whose_each_is() {
        let root = scratch("build-dirs");
        let store = Ledger::open(root.join("store")).expect("the store");
        a_build_directory(&root, "di-nessuno", 4_096);
        a_build_directory(&root, "mai-presa", 4_096);
        store.record_run(&a_run("over", Some(1_700_000_100))).expect("the ended run");
        taken_by_a_dead_process(&store, &root.join("target").join("di-nessuno"), Some("over"));
        // The tree's ordinary work carries no tag of its own: cargo writes it
        // in the directory it was pointed at, and `target/debug` is not one.
        std::fs::create_dir_all(root.join("target").join("debug")).expect("the ordinary work");

        let said = about_the_disk_in(&root, Some(&store)).expect("there is one to name");

        assert!(said.contains("di-nessuno"), "the one nobody holds is not named: {said}");
        assert!(said.contains("mai-presa"), "the one sailor never took is not named: {said}");
        assert!(
            said.contains(&catalogue::say("cli.machine.never_taken", &[])),
            "a directory sailor never took is not said to be somebody else's: {said}"
        );
        assert!(
            !said.contains("target/debug:"),
            "the tree's own work was offered up as a leftover: {said}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **THE CHAIN DECIDES, NOT THE CLOCK.** Taking one because nothing was
    /// written in it for an hour deletes a slow build; leaving one because it
    /// was written to a moment ago hoards a directory nobody will come back
    /// to. The process and the run answer both, and neither is a guess.
    #[test]
    fn freeing_takes_only_what_no_process_and_no_run_still_wants() {
        let root = scratch("free-dirs");
        let store = Ledger::open(root.join("store")).expect("the store");
        for name in ["di-nessuno", "corsa-aperta", "in-uso", "mai-presa"] {
            a_build_directory(&root, name, 4_096);
        }
        store.record_run(&a_run("over", Some(1_700_000_100))).expect("the ended run");
        store.record_run(&a_run("still-going", None)).expect("the open run");
        taken_by_a_dead_process(&store, &root.join("target").join("di-nessuno"), Some("over"));
        taken_by_a_dead_process(
            &store,
            &root.join("target").join("corsa-aperta"),
            Some("still-going"),
        );
        taken_by_this_process(&store, &root.join("target").join("in-uso"));

        let said = free_the_disk_in(&root, Some(&store)).expect("there is one to take");

        assert!(said.contains("di-nessuno"), "the one nobody holds was not taken: {said}");
        assert!(
            !root.join("target").join("di-nessuno").exists(),
            "it was named as taken and is still on the disk"
        );
        for kept in ["corsa-aperta", "in-uso", "mai-presa"] {
            assert!(
                root.join("target").join(kept).exists(),
                "«{kept}» was taken away, and somebody still answers for it: {said}"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A directory sailor never took stays until a person names it, and one a
    /// live process is compiling in is refused even then.
    #[test]
    fn a_directory_named_by_a_person_is_taken_unless_a_process_is_in_it() {
        let root = scratch("free-one");
        let store = Ledger::open(root.join("store")).expect("the store");
        a_build_directory(&root, "in-uso", 4_096);
        taken_by_this_process(&store, &root.join("target").join("in-uso"));

        let on_disk = build_directories(&root, Some(&store));
        let busy = on_disk
            .iter()
            .find(|one| one.left.path.ends_with("in-uso"))
            .expect("the busy one is on the disk");

        assert!(
            !busy.is_nobodys(),
            "a directory this very process holds was called nobody's"
        );
        assert!(
            busy.whose_it_is().contains(&std::process::id().to_string()),
            "the reading does not say which process holds it: {}",
            busy.whose_it_is()
        );

        let refused = free_one_in(&root, &busy.left.path.to_string_lossy(), Some(&store));
        assert!(
            refused.is_err(),
            "a directory a live process is compiling in was taken on request: {refused:?}"
        );
        assert!(root.join("target").join("in-uso").exists(), "it was taken anyway");

        a_build_directory(&root, "mai-presa", 4_096);
        let mine = root.join("target").join("mai-presa");
        let taken = free_one_in(&root, &mine.to_string_lossy(), Some(&store));
        assert!(taken.is_ok(), "a directory named by a person was refused: {taken:?}");
        assert!(!mine.exists(), "it was said to be taken and is still on the disk");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_store_that_will_not_open_still_leaves_the_machine_readable() {
        let said = without_the_store(
            "sqlite: unable to open database file",
            Some("16 build directories nobody owns".to_owned()),
        );

        assert!(
            said.contains("unable to open database file"),
            "the reason the store gave is not carried out: {said}"
        );
        assert!(
            said.contains("16 build directories nobody owns"),
            "what the store does not own is dropped with it: {said}"
        );
    }
}
