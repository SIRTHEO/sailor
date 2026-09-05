//! The battery does not shrink in silence: the test binaries, the test
//! functions and the flow files are counted, and each count moves only with
//! a commit that writes the new number here. A deleted test does not fail,
//! it vanishes, and nothing was looking for it — see fault 52.

use std::path::{Path, PathBuf};

/// Every `.rs` directly under a package's `tests/`, the window's shell included.
const TEST_BINARIES_TODAY: usize = 112;

/// Every `#[test]` in the tree, the window's shell included.
const TEST_FUNCTIONS_TODAY: usize = 1516;

/// Every `.flow.json` in `flows/` and among the shipped ones.
const FLOW_FILES_TODAY: usize = 10;

/// How far a seed may sit from the tree, either way. **Zero.** A seed is a
/// number in a file, and a file merges: a merge keeping the older side would
/// cover a deletion exactly the way fault 52 did. Whoever adds or removes
/// writes the measured number, and the commit that does so says why.
const HOW_STALE_A_SEED_MAY_BE: usize = 0;

/// Where `#[test]` functions are looked for: the crates, and the window's shell.
const WHERE_TESTS_ARE_WRITTEN: &[&str] = &["crates", "desktop/src-tauri"];

/// Where flow files live: this project's own, and the ones shipped in the binary.
const WHERE_FLOWS_LIVE: &[&str] = &["flows", "crates/flow/system"];

const TEST_MARK: &str = "#[test]";
const FLOW_SUFFIX: &str = ".flow.json";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_path_buf()
}

fn entries_of(directory: &Path) -> impl Iterator<Item = PathBuf> {
    std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
}

fn has_suffix(path: &Path, suffix: &str) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(suffix))
}

/// The packages whose `tests/` hold binaries: every crate, and the window's shell.
fn packages(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = entries_of(&root.join("crates"))
        .filter(|path| path.is_dir())
        .collect();
    found.push(root.join("desktop").join("src-tauri"));
    found
}

fn test_binaries(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = packages(root)
        .iter()
        .flat_map(|package| entries_of(&package.join("tests")))
        .filter(|path| path.is_file() && has_suffix(path, ".rs"))
        .collect();
    found.sort();
    found
}

fn rust_sources(directory: &Path, found: &mut Vec<PathBuf>) {
    for path in entries_of(directory) {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.is_dir() {
            if !matches!(name.as_str(), "target" | "node_modules" | ".git") {
                rust_sources(&path, found);
            }
        } else if name.ends_with(".rs") {
            found.push(path);
        }
    }
}

/// The mark at the start of its line: a mark in a comment or a string is not
/// a test, and a mark followed by the function on the same line still is.
fn opens_a_test(line: &str) -> bool {
    line.trim_start().starts_with(TEST_MARK)
}

fn test_functions(root: &Path) -> usize {
    let mut sources = Vec::new();
    for place in WHERE_TESTS_ARE_WRITTEN {
        rust_sources(&root.join(place), &mut sources);
    }
    sources
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .map(|text| text.lines().filter(|line| opens_a_test(line)).count())
        .sum()
}

fn flow_files(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = WHERE_FLOWS_LIVE
        .iter()
        .flat_map(|place| entries_of(&root.join(place)))
        .filter(|path| path.is_file() && has_suffix(path, FLOW_SUFFIX))
        .collect();
    found.sort();
    found
}

struct Battery {
    binaries: usize,
    functions: usize,
    flows: usize,
}

fn measure(root: &Path) -> Battery {
    Battery {
        binaries: test_binaries(root).len(),
        functions: test_functions(root),
        flows: flow_files(root).len(),
    }
}

/// Each count beside its seed and the name of the constant to rewrite.
fn all_three(battery: &Battery) -> [(&'static str, &'static str, usize, usize); 3] {
    [
        ("test binaries", "TEST_BINARIES_TODAY", TEST_BINARIES_TODAY, battery.binaries),
        ("test functions", "TEST_FUNCTIONS_TODAY", TEST_FUNCTIONS_TODAY, battery.functions),
        ("flow files", "FLOW_FILES_TODAY", FLOW_FILES_TODAY, battery.flows),
    ]
}

fn measured_right_now(battery: &Battery) -> String {
    format!(
        "\nMeasured right now, all three: {} test binaries, {} test functions, \
         {} flow files. When you re-measure one, rewrite them all.",
        battery.binaries, battery.functions, battery.flows
    )
}

/// The one sentence a fall gets. A deleted test does not fail, it vanishes,
/// so the only thing that can notice is a number somebody has to rewrite.
fn does_not_fall(what: &str, seed_name: &str, seed: usize, measured: usize, battery: &Battery) {
    assert!(
        measured >= seed,
        "counted {measured} {what}, the seed says {seed}: {} vanished and nothing went red. \
         Declare it in the seed with the commit that removes it, {seed_name} = {measured}{}",
        seed - measured,
        measured_right_now(battery)
    );
}

#[test]
fn no_test_binary_vanishes_without_a_word() {
    let battery = measure(&root());
    does_not_fall(
        "test binaries",
        "TEST_BINARIES_TODAY",
        TEST_BINARIES_TODAY,
        battery.binaries,
        &battery,
    );
}

#[test]
fn no_test_function_vanishes_without_a_word() {
    let battery = measure(&root());
    does_not_fall(
        "test functions",
        "TEST_FUNCTIONS_TODAY",
        TEST_FUNCTIONS_TODAY,
        battery.functions,
        &battery,
    );
}

#[test]
fn no_flow_file_vanishes_without_a_word() {
    let battery = measure(&root());
    does_not_fall(
        "flow files",
        "FLOW_FILES_TODAY",
        FLOW_FILES_TODAY,
        battery.flows,
        &battery,
    );
}

/// The other side of the ratchet: a tree that grew past its seed. A seed
/// that stopped describing the tree would let the next deletion fall back
/// to it unseen, so the seed follows the tree up as strictly as it holds it.
#[test]
fn a_seed_that_no_longer_describes_the_tree_is_a_seed_nobody_re_measured() {
    let battery = measure(&root());
    for (what, seed_name, seed, measured) in all_three(&battery) {
        assert!(
            seed + HOW_STALE_A_SEED_MAY_BE >= measured,
            "the seed «{what}» says {seed}, the tree holds {measured}: {} more than declared. \
             Raise it in the commit that adds them and say what they are, {seed_name} = {measured}{}",
            measured - seed,
            measured_right_now(&battery)
        );
    }
}

/// Whoever measures gets measured: a counter that stopped seeing would let
/// all three numbers fall to zero, and the tests above would only ever ask
/// whether the seed had followed.
#[test]
fn the_counters_can_still_see_what_they_count() {
    let scratch = std::env::temp_dir().join(format!("sailor-battery-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let package = scratch.join("crates").join("one");
    let shell = scratch.join("desktop").join("src-tauri");
    let shipped = scratch.join("crates").join("flow").join("system");
    for directory in [
        package.join("tests").join("nested"),
        package.join("src"),
        package.join("target"),
        shell.join("tests"),
        shell.join("src"),
        scratch.join("flows"),
        shipped.clone(),
    ] {
        std::fs::create_dir_all(&directory).expect("the scratch tree");
    }
    let write = |path: PathBuf, text: &str| std::fs::write(&path, text).expect("write");
    write(package.join("tests").join("first.rs"), "#[test] fn a() {}\n");
    write(package.join("tests").join("second.rs"), "  #[test]\nfn b() {}\n");
    write(package.join("tests").join("nested").join("mod.rs"), "#[test] fn shared() {}\n");
    write(
        package.join("src").join("lib.rs"),
        "fn code() {}\n// #[test] in a comment is not one\n#[test]\nfn c() {}\n    #[test]\n    fn d() {}\nconst MARK: &str = \"#[test]\";\n",
    );
    write(package.join("target").join("built.rs"), "#[test] fn never() {}\n");
    write(shell.join("tests").join("window.rs"), "#[test] fn e() {}\n");
    write(shell.join("src").join("main.rs"), "#[test]\nfn f() {}\n");
    write(scratch.join("flows").join("own.flow.json"), "{}");
    write(scratch.join("flows").join("notes.md"), "");
    write(shipped.join("shipped.flow.json"), "{}");
    write(shipped.join("shipped.md"), "");

    let battery = measure(&scratch);
    let binaries = test_binaries(&scratch);
    let _ = std::fs::remove_dir_all(&scratch);

    assert_eq!(
        binaries.len(),
        3,
        "two under the crate, one under the shell, none nested: {binaries:?}"
    );
    assert_eq!(
        battery.functions,
        7,
        "the marks opening a line, in any file but the built ones, and never in a comment or a string"
    );
    assert_eq!(battery.flows, 2, "one of this project's own and one shipped, no markdown");

    let real = measure(&root());
    println!(
        "today: {} test binaries, {} test functions, {} flow files",
        real.binaries, real.functions, real.flows
    );
    assert!(
        real.binaries > 0 && real.functions > 0 && real.flows > 0,
        "zero somewhere: the counter is not looking"
    );
}
