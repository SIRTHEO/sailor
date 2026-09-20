//! The census walks the repository it is standing in, and every entry names
//! the root that loads it. A repository held 271 skills while the command
//! answering "what does this machine offer" said 23: the home and a
//! warehouse, never the repository's own configuration (fault 252). Every
//! tree below is built inside a throwaway directory, so nothing here reads a
//! path of the machine running it.

use inventory::{collect, default_roots_from, Kind};
use std::fs;
use std::path::{Path, PathBuf};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("roots-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// A skill as a command line loads one: a folder with a `SKILL.md` carrying
/// frontmatter, under a `skills/` of the configuration directory.
fn skill(config: &Path, name: &str) {
    let folder = config.join("skills").join(name);
    fs::create_dir_all(&folder).expect("a skill folder");
    fs::write(
        folder.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: what {name} is for\n---\n\nbody\n"),
    )
    .expect("a skill file");
}

/// A home with one skill, and a repository with another, two levels under a
/// directory that is not the home. Returns the home, the repository, and a
/// directory deep inside it: a session is opened in a subdirectory far more
/// often than at the root.
fn a_home_and_a_repository(label: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = temp(label);
    let home = dir.join("home");
    skill(&home.join(".claude"), "from-the-home");

    let repository = dir.join("work").join("a-repository");
    skill(&repository.join(".claude"), "from-the-repository");
    let deep = repository.join("crates").join("inventory").join("src");
    fs::create_dir_all(&deep).expect("a directory deep in the repository");

    (home, repository, deep)
}

#[test]
fn the_repository_one_is_standing_in_is_a_root() {
    let (home, repository, deep) = a_home_and_a_repository("is-a-root");

    let survey = default_roots_from(&home, &[], Some(&deep));
    let walked: Vec<&Path> = survey.roots.iter().map(|root| root.path.as_path()).collect();

    assert!(
        walked.contains(&repository.as_path()),
        "the configuration directory of the repository one is standing in is \
         read by the engine of any session opened here, so the census reads it \
         too. Roots walked: {walked:?}"
    );
    assert!(
        survey.roots.iter().any(|root| root.is_home),
        "the home stays a root: this adds one, it replaces none"
    );
}

#[test]
fn a_skill_of_the_repository_is_counted_and_says_which_root_loads_it() {
    let (home, _repository, deep) = a_home_and_a_repository("counted");

    let found = collect(&default_roots_from(&home, &[], Some(&deep)).roots);
    let skills: Vec<(&str, &str)> = found
        .of(Kind::Skill)
        .into_iter()
        .map(|entry| (entry.name.as_str(), entry.root.as_str()))
        .collect();

    assert!(
        skills.contains(&("from-the-repository", "a-repository")),
        "a skill in the repository's own configuration must be counted, and \
         must name the root that loads it: a count nobody can trace back to a \
         directory cannot be contradicted. Found: {skills:?}"
    );
    assert!(
        skills.contains(&("from-the-home", "home")),
        "the home's skills keep naming the home. Found: {skills:?}"
    );
}

#[test]
fn every_entry_names_a_root_the_inventory_says_it_walked() {
    let (home, _repository, deep) = a_home_and_a_repository("every-entry");

    let found = collect(&default_roots_from(&home, &[], Some(&deep)).roots);
    assert!(!found.entries.is_empty(), "the fixture must produce entries");
    for entry in &found.entries {
        assert!(
            found
                .roots
                .iter()
                .any(|root| root.starts_with(&format!("{}:", entry.root))),
            "{} names the root {:?}, which is not among the roots walked: {:?}",
            entry.name,
            entry.root,
            found.roots
        );
    }
}

/// Standing in the home itself adds nothing: the home is already a root, and
/// labelling it a second time as a repository would name the directory after
/// whoever owns it.
#[test]
fn standing_in_the_home_adds_no_second_root() {
    let dir = temp("in-the-home");
    let home = dir.join("home");
    skill(&home.join(".claude"), "from-the-home");

    let survey = default_roots_from(&home, &[], Some(&home));
    assert_eq!(
        survey.roots.len(),
        1,
        "the home counted twice: {:?}",
        survey.roots.iter().map(|r| &r.label).collect::<Vec<_>>()
    );
    assert!(survey.roots[0].is_home);
}

/// A directory under no repository at all yields no root, rather than the
/// first directory upwards that happens to exist.
#[test]
fn a_directory_carrying_no_configuration_yields_no_repository() {
    let dir = temp("no-configuration");
    let home = dir.join("home");
    fs::create_dir_all(&home).expect("a home");
    let loose = dir.join("elsewhere").join("nothing-here");
    fs::create_dir_all(&loose).expect("a loose directory");

    let survey = default_roots_from(&home, &[], Some(&loose));
    assert_eq!(survey.roots.len(), 1, "only the home was there to find");
    assert!(survey.roots[0].is_home);
}
