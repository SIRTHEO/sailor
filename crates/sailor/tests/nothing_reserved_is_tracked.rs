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
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if !workspace::is_the_top_of_its_repository(&root) {
        workspace::measured_nothing("this tree is not the top of a repository, so nothing is tracked here to refuse");
        return None;
    }
    let listed = Command::new("git")
        .arg("-C")
        .arg(&root)
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

#[test]
fn only_the_shipped_flows_and_this_projects_own_are_tracked() {
    let Some(tracked) = tracked() else {
        return;
    };
    let allowed = flows_that_may_be_tracked();
    let strangers: Vec<String> = tracked
        .into_iter()
        .filter(|path| path.ends_with(".flow.json"))
        .filter(|path| !allowed.contains(path))
        .collect();
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
    let personal: Vec<String> = tracked
        .into_iter()
        .filter(|path| {
            let name = path.rsplit('/').next().unwrap_or(path);
            NAMES_THAT_ARE_A_PERSONS.contains(&name)
        })
        .collect();
    assert!(personal.is_empty(), "a person's files in a public tree: {personal:?}");
}
