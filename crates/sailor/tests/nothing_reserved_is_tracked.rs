//! The repository is public: no flow of a person's own, no profile home, no
//! credential is in the tree. What `git ls-files` may hold is listed here,
//! and the rest is refused.

use std::process::Command;

/// The flows that ship with the product, and this project's own.
/// This project's own flows, which are not shipped inside the binary.
const THIS_PROJECTS_OWN: &[&str] = &["flows/passa-il-testimone.flow.json"];

/// What may be tracked: the shipped flows, **asked of the product** instead of
/// copied here. A hand list beside `flow::system::FLOWS` is the same list
/// written twice, and it went stale the day a flow was shipped.
fn flows_that_may_be_tracked() -> Vec<String> {
    flow::system::FLOWS
        .iter()
        .map(|(name, _)| format!("crates/flow/system/{name}.flow.json"))
        .chain(THIS_PROJECTS_OWN.iter().map(|path| (*path).to_owned()))
        .collect()
}

/// File names that are a person's, wherever they sit.
const NAMES_THAT_ARE_A_PERSONS: &[&str] = &[
    ".credentials.json",
    "credentials.json",
    "auth.json",
    "profili.json",
    "cooldowns.json",
    "budgets.json",
    ".env",
];

/// The tracked files. `None` and not an empty list where there is nothing to
/// ask: an empty list reads as «nothing reserved is tracked» — fault 100.
fn tracked() -> Option<Vec<String>> {
    tracked_at(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

/// The same reading, of whatever repository it is pointed at, so the verdict can
/// be put to a tree with a violation planted in it.
fn tracked_at(root: &std::path::Path) -> Option<Vec<String>> {
    if !workspace::is_the_top_of_its_repository(root) {
        workspace::measured_nothing("this tree is not the top of a repository, so nothing is tracked here to refuse");
        return None;
    }
    let listed = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .expect("git lists the tree");
    assert!(listed.status.success(), "git ls-files answers");
    let paths: Vec<String> = String::from_utf8_lossy(&listed.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect();
    workspace::measured_against(
        paths.len(),
        "tracked paths read",
        NAMES_THAT_ARE_A_PERSONS.len(),
        "names a file must never carry",
    );
    Some(paths)
}

/// The tracked flows that are neither shipped nor this project's own.
fn strangers_among(tracked: Vec<String>) -> Vec<String> {
    let allowed = flows_that_may_be_tracked();
    tracked
        .into_iter()
        .filter(|path| path.ends_with(".flow.json"))
        .filter(|path| !allowed.contains(path))
        .collect()
}

/// The tracked files whose name is a person's, wherever they sit.
fn a_persons_files_among(tracked: Vec<String>) -> Vec<String> {
    tracked
        .into_iter()
        .filter(|path| {
            let name = path.rsplit('/').next().unwrap_or(path);
            NAMES_THAT_ARE_A_PERSONS.contains(&name)
        })
        .collect()
}

#[test]
fn only_the_shipped_flows_and_this_projects_own_are_tracked() {
    let Some(tracked) = tracked() else {
        return;
    };
    let strangers = strangers_among(tracked);
    assert!(
        strangers.is_empty(),
        "flows in the tree that are not shipped nor this project's: {strangers:?}. A person's flows live in the home, or in the private repository of their own"
    );
}

#[test]
fn no_file_that_is_a_persons_is_tracked() {
    let Some(tracked) = tracked() else {
        return;
    };
    let personal = a_persons_files_among(tracked);
    assert!(personal.is_empty(), "a person's files in a public tree: {personal:?}");
}

/// **A CLEAN TREE IS NOT A WORKING CHECK.** Both verdicts above have only ever
/// been asked of a tree that holds nothing to refuse. Here a throwaway
/// repository is built under the temporary directory, a person's flow and a
/// credentials file are committed into it, and the same two verdicts are asked
/// of that index.
#[test]
fn a_persons_flow_and_credentials_planted_in_a_throwaway_repository_are_found() {
    let root = std::env::temp_dir().join(format!(
        "sailor-planted-reserved-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("flows")).expect("a throwaway flows directory");
    std::fs::create_dir_all(root.join("crates/sample")).expect("a throwaway crate directory");
    std::fs::write(root.join("flows/a-private-errand.flow.json"), "{}").expect("the flow writes");
    std::fs::write(root.join("crates/sample/credentials.json"), "{}").expect("the secret writes");
    let started = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["init", "-q"])
        .status()
        .expect("git starts a throwaway repository");
    assert!(started.success(), "the throwaway repository was not started");
    let added = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args([
            "add",
            "--",
            "flows/a-private-errand.flow.json",
            "crates/sample/credentials.json",
        ])
        .status()
        .expect("git tracks the planted files");
    assert!(added.success(), "the planted files were not tracked");

    let planted = tracked_at(&root).expect("the throwaway tree is the top of its own repository");
    let strangers = strangers_among(planted.clone());
    let personal = a_persons_files_among(planted);
    std::fs::remove_dir_all(&root).expect("the throwaway repository goes");

    assert_eq!(
        strangers,
        vec!["flows/a-private-errand.flow.json".to_owned()],
        "a flow that is neither shipped nor this project's was tracked and the \
         verdict did not name it"
    );
    assert_eq!(
        personal,
        vec!["crates/sample/credentials.json".to_owned()],
        "a credentials file was tracked and the verdict did not name it"
    );
}

/// **THE JUDGE MUST BE ABLE TO SAY IT DID NOT MEASURE.** A directory that is
/// not the top of a repository tracks nothing, and nothing tracked is not
/// «nothing reserved is tracked»: it is no reading at all — fault 100.
#[test]
fn a_tree_that_is_not_a_repository_makes_the_judge_declare_it_measured_nothing() {
    let plain = std::env::temp_dir()
        .join(format!("sailor-unreserved-{}-{}", std::process::id(), line!()));
    let _ = std::fs::remove_dir_all(&plain);
    std::fs::create_dir_all(plain.join("flows")).expect("a scratch");
    std::fs::write(plain.join("flows").join("a-private-errand.flow.json"), "{}")
        .expect("a flow no repository tracks");

    assert!(
        tracked_at(&plain).is_none(),
        "outside a repository the judge must hand back nothing, never an empty \
         list: an empty list reads as a tree with nothing to refuse, and this \
         one carries a flow that would be refused"
    );

    let _ = std::fs::remove_dir_all(&plain);
}
