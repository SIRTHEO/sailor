//! The gate lets nothing through on an exit code alone: a judge hands in its
//! own words and the gate decides from them. Not a judge itself — it opens no
//! source, it drives the gate — so it carries no seed and no receipt.

use sailor::ratchet_cmd::{judges_in, verdict_of, Gate, Handed, Verdict};

/// The element the perimeter must contain: a judge that walked can count it.
const SENTINEL: &str = "the sentinel this perimeter demands";

/// What a judge that really walked hands in.
fn a_judge_that_walked() -> String {
    format!(
        "running 3 tests\n{} 214 sources holding {SENTINEL}{}214 paths git tracks\nok.",
        workspace::MEASURED,
        workspace::AGAINST
    )
}

/// **THE OBSERVER THAT GIVES NO EVIDENCE OF HAVING READ THE SENTINEL**: every
/// test passed, the process exited zero, and that is all it proved.
fn a_judge_that_only_passed() -> String {
    "running 3 tests\ntest result: ok. 3 passed; 0 failed".to_owned()
}

/// **THE PROOF.** The perimeter demands the sentinel; the observer proves
/// nothing about it and still exits zero.
#[test]
fn a_judge_that_proves_nothing_about_the_perimeter_is_unmeasured_and_stops_the_run() {
    let silent = a_judge_that_only_passed();
    let handed = [Handed { judge: "the_sentinel_is_read", passed: true, said: &silent }];

    let gate = Gate::over(&handed);

    assert_eq!(
        verdict_of(true, &silent),
        Verdict::NoReceipt,
        "a judge that exited zero having said nothing about {SENTINEL} was taken for a measurement"
    );
    assert_eq!(gate.unmeasured(), 1, "the run did not count the judge that proved no perimeter");
    assert_eq!(gate.green(), 0, "the green count rose on a judge that measured nothing");
    assert!(
        !gate.lets_through(0, 0),
        "the gate let a run through on an exit code alone: {}",
        gate.closing_line()
    );
    assert!(
        !gate.closing_line().contains("every seed holds"),
        "the closing line called the run clean: {}",
        gate.closing_line()
    );
}

/// The control, or a gate refusing everything would meet the demand above.
#[test]
fn the_same_run_with_a_receipt_is_green_and_goes_through() {
    let walked = a_judge_that_walked();
    let gate = Gate::over(&[Handed { judge: "the_sentinel_is_read", passed: true, said: &walked }]);

    assert_eq!(gate.green(), 1);
    assert_eq!(gate.unmeasured(), 0);
    assert!(gate.lets_through(0, 0), "{}", gate.closing_line());
    assert!(gate.closing_line().contains("every seed holds"), "{}", gate.closing_line());
}

/// A receipt is not a password: an empty perimeter is refused like silence.
#[test]
fn an_empty_perimeter_and_an_empty_oracle_are_refused_like_silence() {
    let empty_walk = format!("{} 0 sources under crates", workspace::MEASURED);
    let empty_oracle =
        format!("{} 214 sources{}0 paths git tracks", workspace::MEASURED, workspace::AGAINST);
    let fell = a_judge_that_walked();

    assert_eq!(verdict_of(true, &empty_walk), Verdict::NoReceipt);
    assert_eq!(verdict_of(true, &empty_oracle), Verdict::NoReceipt);
    assert_eq!(verdict_of(false, &fell), Verdict::Red);
}

/// The two silences are not one number: mixing them would bury the honest
/// declaration under the backlog of judges nobody has converted yet.
#[test]
fn declaring_an_empty_oracle_and_handing_in_nothing_are_counted_apart() {
    let blind = format!("{} nothing here to ask", workspace::MEASURED_NOTHING);
    let silent = a_judge_that_only_passed();
    let gate = Gate::over(&[
        Handed { judge: "blind", passed: true, said: &blind },
        Handed { judge: "silent", passed: true, said: &silent },
    ]);

    assert_eq!(gate.measured_nothing(), 1);
    assert_eq!(gate.unmeasured(), 1);
    assert_eq!(gate.green(), 0);
}

/// A repository of its own, holding one source and one commit.
fn a_repository(label: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("sailor-gate-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("crates")).expect("a scratch");
    std::fs::write(root.join("crates").join("one.rs"), "// held at HEAD\n").expect("a source");
    for args in [
        vec!["init", "--quiet"],
        vec!["add", "--all"],
        vec!["-c", "user.name=a", "-c", "user.email=a@b", "commit", "--quiet", "-m", "held"],
    ] {
        let done = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(&args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .expect("git runs");
        assert!(done.success(), "git {args:?}");
    }
    root
}

/// **THE GATE MUST HAND ITS JUDGES A TREE THEY CAN ASK.** `git archive` carries
/// the files and not the `.git`, so every judge that reads what is tracked
/// found nothing and said so — the guard against this machine's paths reaching
/// a public repository never looked at the tree the gate was about to pass.
/// Put the `git init` back and this test goes red on the first assertion.
#[test]
fn the_tree_laid_over_is_the_top_of_a_repository_that_tracks_it() {
    let root = a_repository("laid-over");
    std::fs::write(root.join("crates").join("two.rs"), "// only in the working tree\n")
        .expect("a change");
    let into = root.join("target").join("ratchet-tree");

    let moved = sailor::ratchet_cmd::clean_tree_with_changes(&root, &into).expect("the tree");
    assert_eq!(moved.laid_over, 1, "the untracked change was not laid over");
    assert!(
        workspace::is_the_top_of_its_repository(&into),
        "the laid-over tree answers with the repository above it, so every judge \
         that asks git measures nothing"
    );

    let tracked = std::process::Command::new("git")
        .arg("-C")
        .arg(&into)
        .args(["ls-files"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    let listed = String::from_utf8_lossy(&tracked.stdout);
    assert!(listed.contains("crates/one.rs"), "what HEAD held is not tracked here: {listed}");
    assert!(listed.contains("crates/two.rs"), "what was laid over is not tracked here: {listed}");
    let _ = std::fs::remove_dir_all(&root);
}

/// A tree with one test file planted in each of the places a judge could live.
fn a_tree_of_judges(label: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("sailor-judges-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let names = "reads the sources through env!(\"CARGO_MANIFEST_DIR\")";
    let another_road = "reads the sources through include_str!(\"../descriptors/default.json\")";
    for (at, body) in [
        ("crates/a_crate/tests/a_judge.rs", names),
        ("crates/a_crate/tests/a_judge_by_another_road.rs", another_road),
        ("desktop/src-tauri/tests/a_judge_of_the_shell.rs", names),
    ] {
        let path = root.join(at);
        std::fs::create_dir_all(path.parent().expect("a tests directory")).expect("a scratch");
        std::fs::write(&path, format!("// {body}\n")).expect("a test file");
    }
    root
}

/// **WHAT THE GATE NEVER ASKS, NO RECEIPT CAN COVER.** The finder is put to a
/// tree with a judge planted in each of the three places one could live, and it
/// comes back with one. The two misses are its declared limit: `crates/` is the
/// only place it opens, `CARGO_MANIFEST_DIR` the only road it recognises. If
/// either goes red the finder reaches further, and this says so — deliberately.
#[test]
fn the_finder_reaches_one_of_the_three_places_a_judge_can_live() {
    let root = a_tree_of_judges("planted");
    let found = judges_in(&root);

    let named: Vec<&str> = found.iter().map(|judge| judge.test.as_str()).collect();
    assert!(
        named.contains(&"a_judge"),
        "the planted judge under crates/ was not found, so nothing below measures anything"
    );
    assert!(found.iter().any(|judge| judge.package == "a_crate"), "{found:?}");

    assert!(
        !named.contains(&"a_judge_of_the_shell"),
        "the finder now reaches desktop/src-tauri: it opens only <root>/crates, and the \
         shell is a workspace of its own with sources of its own"
    );
    assert!(
        !named.contains(&"a_judge_by_another_road"),
        "the finder now recognises a judge that reads the sources without naming \
         CARGO_MANIFEST_DIR: include_str! and current_dir() are the other two roads"
    );
    assert_eq!(named.len(), 1, "one of the three, and no more: {named:?}");

    let _ = std::fs::remove_dir_all(&root);
}
