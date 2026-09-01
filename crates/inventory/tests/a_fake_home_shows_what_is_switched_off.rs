//! The inventory on a fake home, built so that every verdict could come out
//! different from the one expected. THE QUESTION THESE TESTS DEFEND is not "how
//! many things are there" — `ls` answers that — it is "which ones do not work,
//! and nobody knows it". The value sits in the two lines saying *plugin
//! switched off* and *points at a file that is gone*: they are the only two
//! that, stopping, would leave the inventory green and false.

use inventory::{collect, Kind, Reach, Root};
use std::fs;
use std::path::{Path, PathBuf};

/// A throwaway home under the temp directory, deleted and rebuilt every run: a
/// test that inherits the previous one's dirt proves nothing.
fn fake_home(name: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("inventory-test-{name}"));
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

#[test]
fn a_skill_inside_a_switched_off_plugin_stays_in_the_list_and_says_why() {
    let home = fake_home("plugin-switched-off");
    // One plugin on and one off, declared the way Claude Code declares them:
    // the key carries the version after the at sign.
    write(
        &home.join(".claude/settings.json"),
        r#"{"enabledPlugins": {"switched-on@1.0.0": true, "switched-off@1.0.0": false}}"#,
    );
    skill(
        &home,
        ".claude/plugins/cache/market/switched-on/skills/first/SKILL.md",
        "first",
    );
    skill(
        &home,
        ".claude/plugins/cache/market/switched-off/skills/second/SKILL.md",
        "second",
    );

    let found = collect(&[Root::home(&home)]);
    let skills = found.of(Kind::Skill);
    assert_eq!(skills.len(), 2, "{skills:#?}");

    let first = skills
        .iter()
        .find(|e| e.name == "switched-on:first")
        .unwrap();
    assert_eq!(first.reach, Reach::Active);
    assert_eq!(first.origin, "plugin switched-on");

    let second = skills
        .iter()
        .find(|e| e.name == "switched-off:second")
        .unwrap();
    match &second.reach {
        Reach::Inactive(reason) => assert!(
            reason.contains("switched-off") && reason.contains("not enabled"),
            "the reason must name the plugin that is off: {reason}"
        ),
        other => panic!("a skill inside a disabled plugin reads as {other:?}"),
    }
}

/// The opposite arm of the test above: switch the same plugin on and the same
/// skill has to change verdict. Without it, "off" could be a constant answer.
#[test]
fn switching_the_plugin_on_changes_the_verdict_of_the_same_skill() {
    let home = fake_home("plugin-switched-back-on");
    write(
        &home.join(".claude/settings.json"),
        r#"{"enabledPlugins": {"my-market@1.0.0": true}}"#,
    );
    skill(
        &home,
        ".claude/plugins/cache/market/my-market/skills/only-one/SKILL.md",
        "only-one",
    );

    let found = collect(&[Root::home(&home)]);
    let only = found
        .of(Kind::Skill)
        .into_iter()
        .find(|e| e.name.ends_with("only-one"))
        .unwrap();
    assert_eq!(only.reach, Reach::Active, "{only:#?}");
}

#[test]
fn a_hook_pointing_at_a_file_that_is_gone_is_reported_as_dead() {
    let home = fake_home("dead-hook");
    let script = home.join(".claude/scripts/alive.sh");
    write(&script, "#!/bin/sh\nexit 0\n");
    write(
        &home.join(".claude/settings.json"),
        &format!(
            r#"{{"hooks": {{
                "PreToolUse": [
                  {{"matcher": "Bash", "hooks": [{{"command": "{} --check"}}]}},
                  {{"matcher": "Write", "hooks": [{{"command": "{}/.claude/scripts/vanished.sh"}}]}}
                ]
            }}}}"#,
            script.to_string_lossy(),
            home.to_string_lossy()
        ),
    );

    let found = collect(&[Root::home(&home)]);
    let hooks = found.of(Kind::Hook);
    assert_eq!(hooks.len(), 2, "{hooks:#?}");

    // The name carries which hook it is, not only where it fires: without that,
    // two hooks on the same event and matcher would be indistinguishable — and
    // on a real machine eight of them live on `PreToolUse · Bash`.
    assert_ne!(hooks[0].name, hooks[1].name, "{hooks:#?}");

    let alive = hooks.iter().find(|e| e.name.contains("Bash")).unwrap();
    assert_eq!(alive.reach, Reach::Active, "{alive:#?}");

    let dead = hooks.iter().find(|e| e.name.contains("Write")).unwrap();
    match &dead.reach {
        Reach::Inactive(reason) => assert!(
            reason.contains("vanished.sh"),
            "the reason must name the missing file: {reason}"
        ),
        other => panic!("a hook pointing at nothing reads as {other:?}"),
    }
}

/// The two false alarms taken off a real disk, kept here so they do not come
/// back: shell punctuation glued to the path, and a word starting with `/` that
/// is not a file.
#[test]
fn shell_punctuation_and_slash_arguments_do_not_kill_a_living_hook() {
    let home = fake_home("false-alarms");
    let script = home.join(".claude/scripts/alive.sh");
    write(&script, "#!/bin/sh\nexit 0\n");
    write(
        &home.join(".claude/settings.json"),
        &format!(
            r#"{{"hooks": {{
                "PermissionRequest": [
                  {{"matcher": "*", "hooks": [{{"command": "sh -c 'exec {}';"}}]}}
                ],
                "SessionStart": [
                  {{"matcher": "startup", "hooks": [{{"command": "{} --on /clear"}}]}}
                ]
            }}}}"#,
            script.to_string_lossy(),
            script.to_string_lossy()
        ),
    );

    let found = collect(&[Root::home(&home)]);
    for hook in found.of(Kind::Hook) {
        assert_eq!(hook.reach, Reach::Active, "{hook:#?}");
    }
}

/// A repo's rules and commands are not active everywhere, and calling them
/// active would be the convenient lie: they hold only for whoever opens a
/// session in there.
#[test]
fn what_lives_in_a_repo_is_never_reported_as_active_everywhere() {
    let home = fake_home("repo-rules");
    let repo = home.join("work/their-repo");
    write(
        &repo.join(".claude/rules/a-rule.md"),
        "# How this thing is done\n\nthe text.\n",
    );
    write(
        &repo.join(".claude/commands/their-command.md"),
        "---\ndescription: does its own thing\n---\n\nthe body.\n",
    );

    let found = collect(&[Root::home(&home), Root::repo(&repo)]);

    let rule = found.of(Kind::Rule).into_iter().next().unwrap();
    assert_eq!(rule.name, "a-rule");
    assert_eq!(rule.description, "How this thing is done");
    assert_eq!(rule.origin, "repo their-repo");
    match &rule.reach {
        Reach::Unknown(reason) => assert!(
            reason.contains("their-repo"),
            "the reason must name the repo it holds in: {reason}"
        ),
        other => panic!("a repo rule reads as {other:?}"),
    }

    let command = found.of(Kind::Command).into_iter().next().unwrap();
    assert_eq!(command.name, "/their-command");
    assert_eq!(command.description, "does its own thing");
}

/// The two commands the first version dropped, both found on a real disk: one
/// the model may not invoke, and one without frontmatter. Both exist, and a
/// person types them.
#[test]
fn a_command_is_listed_even_without_frontmatter_or_model_invocation() {
    let home = fake_home("hidden-commands");
    write(
        &home.join(".claude/commands/by-hand.md"),
        "---\ndescription: only whoever types\ndisable-model-invocation: true\n---\n\nthe body.\n",
    );
    write(
        &home.join(".claude/commands/bare.md"),
        "# The bare command\n\nno frontmatter, and that is fine.\n",
    );

    let found = collect(&[Root::home(&home)]);
    let commands = found.of(Kind::Command);
    assert_eq!(commands.len(), 2, "{commands:#?}");

    let by_hand = commands.iter().find(|e| e.name == "/by-hand").unwrap();
    assert!(!by_hand.by_model, "{by_hand:#?}");
    assert_eq!(by_hand.description, "only whoever types");

    let bare = commands.iter().find(|e| e.name == "/bare").unwrap();
    assert!(bare.by_model);
    assert_eq!(bare.description, "The bare command");
}

/// A skills warehouse no configuration loads: the ones linked among the home's
/// skills stay reachable, the others do not.
///
/// THE TWO ARMS ARE THE POINT. On a real disk there are 128 in two warehouses
/// and 6 are linked: "all off" would be convenient and false, and would cost
/// the inventory its credit on the 122 lines it is right about.
#[test]
fn a_warehouse_skill_counts_as_reachable_only_once_it_is_linked() {
    let home = fake_home("warehouse");
    let warehouse = home.join("warehouse").join("skills");
    skill(&home, "warehouse/skills/linked-one/SKILL.md", "linked-one");
    skill(&home, "warehouse/skills/on-its-own/SKILL.md", "on-its-own");

    // The link as Claude Code makes it: an entry among the home's skills
    // carrying the folder's name.
    fs::create_dir_all(home.join(".claude/skills")).unwrap();
    std::os::unix::fs::symlink(
        warehouse.join("linked-one"),
        home.join(".claude/skills/linked-one"),
    )
    .unwrap();

    let found = collect(&[Root::home(&home), Root::warehouse("warehouse", &warehouse)]);
    let stored: Vec<_> = found
        .of(Kind::Skill)
        .into_iter()
        .filter(|e| e.origin.starts_with("warehouse"))
        .collect();
    assert_eq!(stored.len(), 2, "{stored:#?}");

    let linked = stored.iter().find(|e| e.name == "linked-one").unwrap();
    assert_eq!(linked.reach, Reach::Active, "{linked:#?}");

    let alone = stored.iter().find(|e| e.name == "on-its-own").unwrap();
    match &alone.reach {
        Reach::Inactive(reason) => assert!(
            reason.contains("no configuration loads") && reason.contains("link it"),
            "the reason must say both why it is off and what to do about it: {reason}"
        ),
        other => panic!("a skill that was never linked reads as {other:?}"),
    }
}

/// The inventory declares where it looked. A list that does not say so cannot
/// be contradicted: whoever reads "zero agents" cannot tell whether there are
/// none or whether nobody went to look.
#[test]
fn the_inventory_names_the_roots_it_walked() {
    let home = fake_home("declared-roots");
    let found = collect(&[Root::home(&home)]);
    assert_eq!(found.roots.len(), 1);
    assert!(found.roots[0].starts_with("home: "), "{:?}", found.roots);
    assert!(
        found.roots[0].contains("inventory-test-declared-roots"),
        "{:?}",
        found.roots
    );
}
