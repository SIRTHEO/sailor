//! Skills synced from the account sit two folders deeper than the loose ones,
//! at `skills/synced/<bucket>/<name>/SKILL.md`, and no declared glob reached
//! that depth: nine on disk, none in the census. The second test holds the case
//! that happened -- a second account leaves its bucket behind and the same
//! skill stands twice, paid for twice in every prompt, visible only if both
//! are counted.

use inventory::{collect, Kind, Root};
use std::fs;
use std::path::{Path, PathBuf};

fn fake_home(name: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("inventory-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).unwrap();
    home
}

fn skill(home: &Path, at: &str, name: &str) {
    let path = home.join(at);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        format!("---\nname: {name}\ndescription: what {name} does\n---\n\n# {name}\n"),
    )
    .unwrap();
}

#[test]
fn a_synced_skill_is_counted_beside_a_loose_one() {
    let home = fake_home("synced-visible");
    skill(&home, ".claude/skills/loose/SKILL.md", "loose");
    skill(
        &home,
        ".claude/skills/synced/org-uuid_account-uuid/carried/SKILL.md",
        "carried",
    );

    let found = collect(&[Root::home(&home)]);
    let names: Vec<&str> = found
        .of(Kind::Skill)
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert!(
        names.contains(&"carried"),
        "the synced skill is on disk and missing from the census: {names:?}"
    );
    // The loose one proves the glob widened the net instead of moving it.
    assert!(names.contains(&"loose"), "{names:?}");
    // Bare name: a prefix would send the reader hunting for a plugin.
    assert_eq!(names.len(), 2, "{names:?}");
}

#[test]
fn the_same_skill_left_by_two_accounts_is_listed_twice() {
    let home = fake_home("synced-twice");
    skill(
        &home,
        ".claude/skills/synced/org-a_account-one/carried/SKILL.md",
        "carried",
    );
    skill(
        &home,
        ".claude/skills/synced/org-a_account-two/carried/SKILL.md",
        "carried",
    );

    let found = collect(&[Root::home(&home)]);
    let carried = found
        .of(Kind::Skill)
        .iter()
        .filter(|entry| entry.name == "carried")
        .count();

    assert_eq!(
        carried, 2,
        "a copy left behind by a signed-out account costs the prompt as much as \
         the live one, and stays hidden unless the census counts both"
    );
}
