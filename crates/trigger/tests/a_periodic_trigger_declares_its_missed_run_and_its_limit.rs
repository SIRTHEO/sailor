//! The guard `docs/2026-09-01-il-tempo-e-l-ultima-scelta.md` specifies: walk the
//! trigger descriptors and fail if a periodic one does not declare what it does
//! with a missed run and what its concurrency limit is.

// **WHY IT IS WRITTEN FROM THE REFUSAL SIDE TOO.** The type requires the three
// declarations, so a descriptor missing one never reaches the list the walk
// reads, and the walk alone could not go red. What can go red is that such a
// descriptor is turned away **and the message names the field**: a rejection
// saying only "invalid entry" sends the reader hunting.

use std::path::PathBuf;
use trigger::descriptor::Source;
use trigger::{Catalog, Kind, Problem};

fn fixture(label: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sailor-periodic-{}-{label}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("creating the fixture directory");
    let file = dir.join("triggers.json");
    std::fs::write(&file, body).expect("writing the fixture descriptors");
    file
}

fn load(label: &str, body: &str) -> Catalog {
    Catalog::load(&[Source::File(fixture(label, body))])
}

fn refusal(catalog: &Catalog) -> &Problem {
    assert!(
        catalog.live().is_empty(),
        "an incomplete periodic descriptor must not enter the list: {:?}",
        catalog.live()
    );
    assert_eq!(catalog.problems.len(), 1, "{:?}", catalog.problems);
    &catalog.problems[0]
}

const WELL_FORMED: &str = r#"[{
    "id": "ogni-mezz-ora",
    "kind": "periodic",
    "periodic": {
        "every": {"kind": "every_seconds", "seconds": 1800},
        "missed_run": "once_for_all_of_them",
        "at_most_at_once": 1
    }
}]"#;

/// The walk the document specifies, over everything that did load: the shipped
/// descriptors plus one written by hand, so the loop has a periodic entry to
/// judge instead of passing on an empty list.
#[test]
fn every_periodic_trigger_that_loads_declares_both() {
    let catalog = Catalog::load(&[
        Source::Builtin,
        Source::File(fixture("well-formed", WELL_FORMED)),
    ]);
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);

    let mut walked = 0usize;
    for loaded in catalog.live() {
        let descriptor = &loaded.descriptor;
        if descriptor.kind != Kind::Periodic {
            continue;
        }
        walked += 1;
        let periodic = descriptor.periodic.as_ref().unwrap_or_else(|| {
            panic!(
                "the periodic trigger «{}» in {} declares neither its answer to a missed run nor its concurrency limit",
                descriptor.id, loaded.source
            )
        });
        assert!(
            periodic.at_most_at_once >= 1,
            "the periodic trigger «{}» in {} may never run: at_most_at_once is zero",
            descriptor.id,
            loaded.source
        );
    }
    assert_eq!(walked, 1, "the walk must have had something to judge");
}

#[test]
fn a_periodic_trigger_that_declares_nothing_is_refused_by_name() {
    let catalog = load("bare", r#"[{"id": "nudo", "kind": "periodic"}]"#);
    let problem = refusal(&catalog);
    assert_eq!(problem.about, "nudo");
    for named in ["periodic", "missed_run", "at_most_at_once"] {
        assert!(
            problem.reason.contains(named),
            "the refusal must name `{named}`: {}",
            problem.reason
        );
    }
}

#[test]
fn a_periodic_trigger_without_a_missed_run_answer_is_refused_by_name() {
    let catalog = load(
        "no-missed-run",
        r#"[{"id": "muto", "kind": "periodic", "periodic": {
            "every": {"kind": "every_seconds", "seconds": 1800},
            "at_most_at_once": 1}}]"#,
    );
    let problem = refusal(&catalog);
    assert!(problem.reason.contains("missed_run"), "{}", problem.reason);
}

#[test]
fn a_periodic_trigger_without_a_concurrency_limit_is_refused_by_name() {
    let catalog = load(
        "no-limit",
        r#"[{"id": "senza-tetto", "kind": "periodic", "periodic": {
            "every": {"kind": "daily_at", "hour": 3, "minute": 0},
            "missed_run": "catch_up_each_one"}}]"#,
    );
    let problem = refusal(&catalog);
    assert!(
        problem.reason.contains("at_most_at_once"),
        "{}",
        problem.reason
    );
}

/// Zero is a number, so the type takes it: it would be a source that fires
/// nothing while looking declared.
#[test]
fn a_concurrency_limit_of_zero_is_refused() {
    let catalog = load(
        "zero",
        r#"[{"id": "fermo", "kind": "periodic", "periodic": {
            "every": {"kind": "every_seconds", "seconds": 60},
            "missed_run": "catch_up_each_one", "at_most_at_once": 0}}]"#,
    );
    let problem = refusal(&catalog);
    assert!(problem.reason.contains("disabled"), "{}", problem.reason);
}

/// The two answers are both writable, and they are not the same value: a
/// descriptor that says "catch up" must not read back as "once".
#[test]
fn both_answers_to_a_missed_run_read_back_as_written() {
    let catalog = Catalog::load(&[Source::File(fixture(
        "both",
        r#"[{"id": "recupera", "kind": "periodic", "periodic": {
             "every": {"kind": "every_seconds", "seconds": 1800},
             "missed_run": "catch_up_each_one", "at_most_at_once": 4}},
            {"id": "una-volta", "kind": "periodic", "periodic": {
             "every": {"kind": "daily_at", "hour": 3, "minute": 30},
             "missed_run": "once_for_all_of_them", "at_most_at_once": 1}}]"#,
    ))]);
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);

    let catching_up = catalog.find("recupera").expect("it loaded");
    let once = catalog.find("una-volta").expect("it loaded");
    assert_eq!(
        catching_up.descriptor.periodic.unwrap().missed_run,
        trigger::MissedRun::CatchUpEachOne
    );
    assert_eq!(
        once.descriptor.periodic.unwrap().missed_run,
        trigger::MissedRun::OnceForAllOfThem
    );
    assert_ne!(
        catching_up.descriptor.periodic.unwrap().every,
        once.descriptor.periodic.unwrap().every
    );
}

/// A periodic source is fired by the clock: declaring where to listen describes
/// two sources under one name, which is the fault `coherent` exists for.
#[test]
fn a_periodic_trigger_that_also_listens_is_refused() {
    let catalog = load(
        "listens",
        r#"[{"id": "confuso", "kind": "periodic",
             "listen": {"kind": "appended_lines", "files": ["~/x.jsonl"],
                        "text_pointer": ["text"]},
             "periodic": {"every": {"kind": "every_seconds", "seconds": 60},
                          "missed_run": "catch_up_each_one", "at_most_at_once": 1}}]"#,
    );
    let problem = refusal(&catalog);
    assert!(problem.reason.contains("listen"), "{}", problem.reason);
}

/// And the other way round: `periodic` on a source somebody else fires.
#[test]
fn a_manual_trigger_that_declares_a_period_is_refused() {
    let catalog = load(
        "manual-with-period",
        r#"[{"id": "a-mano", "kind": "manual",
             "periodic": {"every": {"kind": "every_seconds", "seconds": 60},
                          "missed_run": "catch_up_each_one", "at_most_at_once": 1}}]"#,
    );
    let problem = refusal(&catalog);
    assert!(problem.reason.contains("periodic"), "{}", problem.reason);
}
