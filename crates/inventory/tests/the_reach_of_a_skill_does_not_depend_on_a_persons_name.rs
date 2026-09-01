//! A skill collection installed **as a folder** is reachable, and its name is
//! not needed to know that. Reachability used to be decided in part by
//! `plugin.contains("mattpocock")`: one person's name deciding what Sailor
//! counts as installed. The line slept here because that folder does not
//! exist — the day it appeared it would come alive unasked. It belongs on the
//! **origin**: from the plugin cache ask the enabled list, from a folder do not.

use inventory::{collect, Kind, Reach, Root};
use std::fs;
use std::path::{Path, PathBuf};

fn fake_home(name: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("reachability-test-{name}"));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).unwrap();
    home
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn skill(home: &Path, at: &str, name: &str) {
    write(
        &home.join(at),
        &format!("---\nname: {name}\ndescription: what {name} does\n---\n\n# {name}\n"),
    );
}

fn found<'a>(entries: &[&'a inventory::Entry], name: &str) -> &'a inventory::Entry {
    entries
        .iter()
        .copied()
        .find(|entry| entry.name.ends_with(name))
        .unwrap_or_else(|| {
            panic!(
                "\"{name}\" is not in the inventory; these are: {:?}",
                entries.iter().map(|e| &e.name).collect::<Vec<_>>()
            )
        })
}

/// **ANY COLLECTION, UNDER A NAME NOBODY COMPILED IN.** If this passes, the
/// rule looks at the shape of the installation and not at the identity of
/// whoever published it.
#[test]
fn a_collection_installed_as_a_folder_is_reachable_without_being_a_plugin() {
    let home = fake_home("collection-as-folder");
    // No plugin enabled: if reachability depended on `enabledPlugins`, this
    // skill would read as switched off.
    write(
        &home.join(".claude/settings.json"),
        r#"{"enabledPlugins": {}}"#,
    );
    skill(
        &home,
        ".claude/skills/somebodys-collection/skills/cutting/SKILL.md",
        "cutting",
    );

    let inventory = collect(&[Root::home(&home)]);
    let entry = found(&inventory.of(Kind::Skill), "cutting");

    assert_eq!(
        entry.reach,
        Reach::Active,
        "a collection installed as a folder is reachable: it is not a plugin, \
         and asking `enabledPlugins` whether it is on does not apply to it"
    );
    assert!(
        entry.name.starts_with("somebodys-collection:"),
        "the prefix comes from the folder holding it, not from a list of known \
         names: {}",
        entry.name
    );
}

/// The rule still holds where it has to: a **plugin** that is off stays off,
/// and says why. Without this, "everything reachable" would pass the others.
#[test]
fn a_plugin_that_is_switched_off_is_still_switched_off() {
    let home = fake_home("plugin-off-stays-off");
    write(
        &home.join(".claude/settings.json"),
        r#"{"enabledPlugins": {"switched-off@1.0.0": false}}"#,
    );
    skill(
        &home,
        ".claude/plugins/cache/market/switched-off/skills/sewing/SKILL.md",
        "sewing",
    );

    let inventory = collect(&[Root::home(&home)]);
    let entry = found(&inventory.of(Kind::Skill), "sewing");

    match &entry.reach {
        Reach::Inactive(reason) => assert!(
            reason.contains("switched-off") && reason.contains("not enabled"),
            "the reason must name the plugin that is off: {reason}"
        ),
        other => panic!("a disabled plugin is not reachable, and here it reads as {other:?}"),
    }
}

/// **THE GUARD THAT BLOCKS THE RETURN.** The other two say the rule is right
/// today; this one says it cannot be undone tomorrow — and it is needed,
/// because the violation was not a logic error but a habit: letting the name of
/// whatever was at hand into the code. It reads the sources off disk because
/// the compiler cannot be asked to "name nobody".
#[test]
fn no_ones_name_decides_what_is_reachable() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    for file in ["lib.rs", "discovery.rs"] {
        let source = fs::read_to_string(crate_root.join(file)).expect("the source");
        let code: String = source
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                !trimmed.starts_with("//") && !trimmed.starts_with("///")
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            !code.contains("mattpocock"),
            "`{file}` names a single collection again. Reachability is decided \
             on the **origin** — plugin cache, or folder — not on the identity \
             of whoever published the skills."
        );
    }
}
