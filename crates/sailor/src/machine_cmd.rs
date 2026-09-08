//! What Sailor has left running on this machine, and the gesture that frees it.
//!
//! Fault 4 again, from the other end. The supervisor has known how to answer
//! this since it was written — `left_running`, `close_the_ones_that_stopped_
//! breathing`, `who_holds` — and nothing outside its own tests ever called
//! them. A power nobody invokes is a power the machine never gets.

use crate::Form;
use ledger::Ledger;
use std::path::Path;
use machine::{
    build_directories_left, left_running, on_the_port, stop_the_ones_nobody_wants, what_weighs,
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
];

pub fn run(args: &[String]) -> i32 {
    let verb = args.first().map(String::as_str).unwrap_or("");
    let answered = match verb {
        "" => reading(),
        "free" => free(),
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
    let store = open_ledger()?;
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
    if let Some(word) = about_the_disk() {
        said.push('\n');
        said.push_str(&word);
    }
    Ok(said)
}

/// **A BUILD DIRECTORY IS SPACE NOBODY OWNS.** Whoever pointed cargo at one
/// walked away from it; the disk filled to 97% under twenty-two of them, and
/// the gesture that frees this machine did not know they existed.
fn about_the_disk() -> Option<String> {
    about_the_disk_in(&workspace::root().ok()?, now())
}

fn about_the_disk_in(root: &Path, now: i64) -> Option<String> {
    let left = build_directories_left(root, now);
    if left.is_empty() {
        return None;
    }
    let held: u64 = left.iter().map(|one| one.bytes).sum();
    let mut said = catalogue::say(
        "cli.machine.build_directories",
        &[
            ("count", &left.len().to_string()),
            ("gigabytes", &gigabytes(held)),
        ],
    );
    for one in left.iter().take(ENOUGH_TO_SEE_THE_TROUBLE) {
        said.push('\n');
        said.push_str(&catalogue::say(
            "cli.machine.build_directory",
            &[
                ("path", &one.path.display().to_string()),
                ("gigabytes", &gigabytes(one.bytes)),
                ("hours", &(one.idle_secs / AN_HOUR).to_string()),
            ],
        ));
    }
    Some(said)
}

/// How many of the heaviest build directories are worth naming.
const ENOUGH_TO_SEE_THE_TROUBLE: usize = 6;

const AN_HOUR: i64 = 3_600;

/// **HOW LONG BEFORE ONE IS NOBODY'S.** A cargo at work writes in its build
/// directory constantly, so an hour of silence is nobody compiling there. It
/// is declared, not guessed at each call: whoever raises it is saying they are
/// willing to interrupt a longer build.
const IDLE_BEFORE_IT_IS_NOBODYS: i64 = AN_HOUR;

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
    let store = open_ledger()?;
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
    if let Some(word) = free_the_disk() {
        said.push('\n');
        said.push_str(&word);
    }
    Ok(said)
}

/// The build directories nobody is compiling in, taken away.
///
/// Only the ones still and quiet: one being written to is left where it is and
/// named in the reading, the same rule the processes above follow.
fn free_the_disk() -> Option<String> {
    free_the_disk_in(&workspace::root().ok()?, now())
}

fn free_the_disk_in(root: &Path, now: i64) -> Option<String> {
    let idle: Vec<LeftBehind> = build_directories_left(root, now)
        .into_iter()
        .filter(|one| one.idle_secs >= IDLE_BEFORE_IT_IS_NOBODYS)
        .collect();
    if idle.is_empty() {
        return None;
    }
    let mut taken = 0u64;
    let mut said = String::new();
    for one in &idle {
        if std::fs::remove_dir_all(&one.path).is_err() {
            continue;
        }
        taken += one.bytes;
        said.push('\n');
        said.push_str(&catalogue::say(
            "cli.machine.build_directory_taken",
            &[
                ("path", &one.path.display().to_string()),
                ("gigabytes", &gigabytes(one.bytes)),
            ],
        ));
    }
    Some(format!(
        "{}{said}",
        catalogue::say("cli.machine.disk_freed", &[("gigabytes", &gigabytes(taken))])
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
    /// Dates a directory and its `debug` back, so it reads as one nobody has
    /// compiled in for a long time.
    fn aged(path: &Path) {
        for at in [path.join("debug"), path.to_path_buf()] {
            let done = std::process::Command::new("touch")
                .args(["-t", "202001010000"])
                .arg(&at)
                .status();
            assert!(done.is_ok_and(|it| it.success()), "the fixture could not be dated back");
        }
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
    /// had filled the disk to 97%: a gesture that frees half a machine.
    #[test]
    fn the_reading_names_the_build_directories_nobody_owns() {
        let root = scratch("build-dirs");
        a_build_directory(&root, "una", 4_096);
        // The tree's ordinary work carries no tag of its own: cargo writes it
        // in the directory it was pointed at, and `target/debug` is not one.
        std::fs::create_dir_all(root.join("target").join("debug")).expect("the ordinary work");

        let said = about_the_disk_in(&root, 1_700_000_000).expect("there is one to name");

        assert!(said.contains("una"), "the build directory is not named: {said}");
        assert!(
            !said.contains("target/debug:"),
            "the tree's own work was offered up as a leftover: {said}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **ONE BEING WRITTEN TO IS NOBODY'S TO TAKE.** A cargo at work touches
    /// its build directory constantly, so the still ones are the free ones and
    /// the busy one is left where it is.
    #[test]
    fn freeing_takes_the_still_build_directories_and_leaves_the_busy_one() {
        let root = scratch("free-dirs");
        a_build_directory(&root, "ferma", 4_096);
        a_build_directory(&root, "al-lavoro", 4_096);

        // «ferma» is dated back to a day nobody was compiling; «al-lavoro» was
        // written a moment ago, which is what a cargo at work looks like.
        aged(&root.join("target").join("ferma"));
        let said = free_the_disk_in(&root, now()).expect("there is one to take");

        assert!(said.contains("ferma"), "the still one was not taken: {said}");
        assert!(
            !root.join("target").join("ferma").exists(),
            "it was named as taken and is still on the disk"
        );
        assert!(
            root.join("target").join("al-lavoro").exists(),
            "a build directory somebody is compiling in was taken away: {said}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
