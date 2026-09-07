//! What Sailor has left running on this machine, and the gesture that frees it.
//!
//! Fault 4 again, from the other end. The supervisor has known how to answer
//! this since it was written — `left_running`, `close_the_ones_that_stopped_
//! breathing`, `who_holds` — and nothing outside its own tests ever called
//! them. A power nobody invokes is a power the machine never gets.

use crate::Form;
use ledger::Ledger;
use machine::{left_running, stop_the_ones_nobody_wants, Teardown};

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
    Ok(said)
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
    Ok(said)
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
}
