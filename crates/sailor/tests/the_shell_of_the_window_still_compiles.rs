//! The shell of the window still compiles. It declares a workspace of its
//! own, so the workspace battery never builds a line of it, and a crate it
//! depends on can grow a field and leave it red on the trunk with every
//! other judge green.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The shell's manifest, from the root of the tree.
const SHELL_MANIFEST: &str = "desktop/src-tauri/Cargo.toml";

/// What cargo says when the lock no longer describes the manifests it locks.
const THE_LOCK_NEEDS_WRITING: &str = "because --locked was passed";

/// What cargo says when it cannot reach what it builds with. None of these is
/// the shell's own code, and a judge that goes red for a tool it does not own
/// gets turned off by whoever meets it. An index with nothing in it lands here
/// too: a dependency this tree really lacks trips the lock first.
const NOTHING_TO_BUILD_WITH: &[&str] = &[
    "attempting to make an HTTP request",
    "failed to download",
    "failed to fetch",
    "failed to get",
    "failed to load source",
    "no matching package named",
    "registry/cache",
];

#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Compiles,
    /// The first line the compiler wrote.
    Broken(String),
    /// The lock is behind the crates the shell depends on.
    LockIsStale(String),
    /// Cargo could not run, or could not reach what it builds with.
    NothingMeasured(String),
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_path_buf()
}

/// The build directory of whoever runs the judge, when they named one: theirs
/// is reused rather than a cold one filled beside it.
fn callers_build_directory() -> Option<OsString> {
    std::env::var_os("CARGO_TARGET_DIR")
}

/// Every target of the shell, its tests included: the shell's own tests live
/// inside its sources, and a plain check would compile none of them.
fn compiler(root: &Path, offline: bool, build_directory: Option<OsString>) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .arg("check")
        .arg("--manifest-path")
        .arg(root.join(SHELL_MANIFEST))
        .args(["--locked", "--all-targets", "--message-format=short"]);
    if offline {
        command.arg("--offline");
    }
    if let Some(directory) = build_directory {
        command.env("CARGO_TARGET_DIR", directory);
    }
    command
}

fn first_line_with(text: &str, marker: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| line.contains(marker))
        .map(str::to_owned)
}

/// A diagnostic as the short format writes it — `path:line:column: error…` —
/// or cargo's own last word about the crate that did not compile.
fn first_error(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| line.starts_with("error") || line.contains(": error"))
        .map(str::to_owned)
}

/// The last words of a run that said nothing else.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    lines[lines.len().saturating_sub(5)..].join("; ")
}

/// What one run of the compiler says. The order is the order the run stops in:
/// a stale lock stops it before a line is compiled, and a fetch it cannot make
/// stops it before that.
fn verdict_of(succeeded: bool, text: &str) -> Verdict {
    if succeeded {
        return Verdict::Compiles;
    }
    if let Some(said) = first_line_with(text, THE_LOCK_NEEDS_WRITING) {
        return Verdict::LockIsStale(said);
    }
    if let Some(marker) = NOTHING_TO_BUILD_WITH.iter().find(|marker| text.contains(**marker)) {
        let said = first_line_with(text, marker).unwrap_or_else(|| (*marker).to_owned());
        return Verdict::NothingMeasured(said);
    }
    match first_error(text) {
        Some(said) => Verdict::Broken(said),
        None => Verdict::NothingMeasured(tail(text)),
    }
}

fn run(mut command: Command) -> Verdict {
    match command.output() {
        Ok(said) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&said.stdout),
                String::from_utf8_lossy(&said.stderr)
            );
            verdict_of(said.status.success(), &text)
        }
        Err(error) => Verdict::NothingMeasured(format!("cargo check: {error}")),
    }
}

/// The shell built once, and a second time offline when the first run could
/// not reach what it builds with.
fn measured(root: &Path) -> Verdict {
    match run(compiler(root, false, callers_build_directory())) {
        Verdict::NothingMeasured(_) => run(compiler(root, true, callers_build_directory())),
        settled => settled,
    }
}

/// One build of the shell, every target. Measured on one developer machine:
/// 43 s cold into an empty build directory, 1.3 s with the dependencies warm
/// and the shell re-checked. Without cargo, or with nothing to build with,
/// nothing is compared and the test says so.
#[test]
fn the_shell_compiles_against_the_crates_it_is_built_from() {
    match measured(&root()) {
        Verdict::Compiles => {}
        Verdict::Broken(first) => panic!(
            "the shell does not compile, and no workspace test builds it. \
             The compiler's first word:\n      {first}"
        ),
        Verdict::LockIsStale(said) => panic!(
            "{SHELL_MANIFEST} locks crates that have moved: run `cargo check --manifest-path \
             {SHELL_MANIFEST}` and commit the lock it writes, or the drift comes back on \
             every clean tree. Cargo said:\n      {said}"
        ),
        Verdict::NothingMeasured(why) => {
            println!("the shell was not built here: nothing measured, nothing compared — {why}")
        }
    }
}

/// Whoever measures gets measured: a reader that lost the compiler's lines
/// would call every red green, and each shape of failure is told apart by
/// what cargo actually wrote.
#[test]
fn the_check_tells_a_broken_shell_from_a_missing_tool() {
    let broken = "    Checking sailor-desktop v0.1.0\n\
        desktop/src-tauri/src/run.rs:1481:26: error[E0063]: missing field `ended_at`\n\
        error: could not compile `sailor-desktop` (lib test) due to 1 previous error\n";
    assert_eq!(
        verdict_of(false, broken),
        Verdict::Broken(
            "desktop/src-tauri/src/run.rs:1481:26: error[E0063]: missing field `ended_at`"
                .to_owned()
        )
    );
    let stale = "error: cannot update the lock file /x/desktop/src-tauri/Cargo.lock \
                 because --locked was passed to prevent this\n";
    assert!(matches!(verdict_of(false, stale), Verdict::LockIsStale(_)), "{stale}");
    let refused = "error: failed to download `a_crate v0.1.6`\n\nCaused by:\n  \
                   attempting to make an HTTP request, but --offline was specified\n";
    assert!(matches!(verdict_of(false, refused), Verdict::NothingMeasured(_)), "{refused}");
    let denied = "error: failed to open `/home/x/.cargo/registry/cache/index/a_crate.crate`\n";
    assert!(matches!(verdict_of(false, denied), Verdict::NothingMeasured(_)), "{denied}");
    let empty = "error: no matching package named `a_crate` found\n\
                 location searched: crates.io index\n";
    assert!(matches!(verdict_of(false, empty), Verdict::NothingMeasured(_)), "{empty}");
    assert_eq!(verdict_of(true, "    Finished `dev` profile"), Verdict::Compiles);

    let root = root();
    assert!(root.join(SHELL_MANIFEST).is_file(), "the manifest is where the judge looks for it");
    let offline = compiler(&root, true, Some(OsString::from("somewhere")));
    assert!(offline.get_args().any(|word| word == "--offline"), "the second try is offline");
    assert!(
        offline
            .get_envs()
            .any(|(key, value)| key == "CARGO_TARGET_DIR" && value == Some("somewhere".as_ref())),
        "the caller's build directory is handed on"
    );
    let first = compiler(&root, false, None);
    assert!(first.get_args().all(|word| word != "--offline"), "the first try is not");
    assert!(
        first.get_envs().all(|(key, _)| key != "CARGO_TARGET_DIR"),
        "and nothing is invented when the caller named none"
    );
}
