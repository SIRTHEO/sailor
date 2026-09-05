//! The detection tests, on a made-up machine built in here.
//!
//! WHY NOT ON THE REAL MACHINE. A test asserting "claude is here" passes for its
//! author and falls for everyone else, and could not have come out otherwise: it
//! tests an installation, not the detection. Here the search path, home and the
//! variables are a temporary directory, and every case gets built.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use toolbox::{descriptor::Source, Catalog, Machine, Presence, VersionReading};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A directory of our own, which takes away what we put in it.
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Sandbox {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("toolbox-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the test directory is created");
        Sandbox { root }
    }

    fn dir(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::create_dir_all(&path).expect("subdirectory");
        path
    }

    fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.root.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("the file's directory");
        }
        fs::write(&path, content).expect("writing the test file");
        path
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // A directory made unreadable on purpose has to be put back, or it never
        // gets deleted and the next run inherits it.
        restore(&self.root);
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn restore(dir: &Path) {
    let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o755));
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            restore(&entry.path());
        }
    }
}

/// A fake executable that prints whatever we tell it to print.
fn fake_binary(dir: &Path, name: &str, script: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("writing the fake executable");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("execute bit");
    path
}

fn machine(path_dirs: Vec<PathBuf>, home: &Path) -> Machine {
    let mut env = BTreeMap::new();
    env.insert("HOME".to_string(), home.to_string_lossy().into_owned());
    Machine {
        path_dirs,
        home: home.to_path_buf(),
        env,
        version_probes: true,
    }
}

fn catalog_from(path: &Path) -> Catalog {
    Catalog::load(&[Source::File(path.to_path_buf())])
}

// ── a present tool, recognised by the descriptor that named it ──────────

#[test]
fn a_present_tool_is_found_with_the_descriptor_that_named_it() {
    let sandbox = Sandbox::new("present");
    let bin_dir = sandbox.dir("bin");
    fake_binary(&bin_dir, "attrezzo", "echo 'attrezzo 4.2'");
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "attrezzo", "family": "tool", "label": "L'attrezzo",
             "detect": {"command": "attrezzo"},
             "version": {"args": ["--version"]}}]"#,
    );
    let machine = machine(vec![bin_dir.clone()], &sandbox.root);
    let report = toolbox::detect(&catalog_from(&descriptors), &machine);

    let found = &report.findings[0];
    assert_eq!(found.name, "attrezzo");
    // FROM WHICH DESCRIPTOR, AND FROM WHICH FILE: without these two lines the
    // list cannot be contradicted, and a list nobody can contradict never gets
    // corrected.
    assert_eq!(found.descriptor_id, "attrezzo");
    assert_eq!(found.descriptor_source, descriptors.to_string_lossy());
    assert!(
        matches!(&found.presence, Presence::Present(why) if why.contains("attrezzo")),
        "{:?}",
        found.presence
    );
    assert_eq!(
        found.executable.as_deref(),
        Some(bin_dir.join("attrezzo").to_string_lossy().as_ref())
    );
    assert_eq!(
        found.version,
        VersionReading::Declared("attrezzo 4.2".to_string())
    );
}

/// A name that is there but is not executable is not the binary being looked
/// for: the shell behaves this way, and a detection saying otherwise would send
/// a flow off to invoke a text file.
#[test]
fn a_file_without_the_execute_bit_is_not_the_tool() {
    let sandbox = Sandbox::new("notexec");
    let bin_dir = sandbox.dir("bin");
    fs::write(bin_dir.join("attrezzo"), "I am not a program").unwrap();
    fs::set_permissions(bin_dir.join("attrezzo"), fs::Permissions::from_mode(0o644)).unwrap();
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "attrezzo", "family": "tool", "detect": {"command": "attrezzo"}}]"#,
    );
    let machine = machine(vec![bin_dir], &sandbox.root);
    let report = toolbox::detect(&catalog_from(&descriptors), &machine);
    assert!(
        matches!(report.findings[0].presence, Presence::Absent(_)),
        "{:?}",
        report.findings[0].presence
    );
}

// ── a missing tool, recognised as missing ───────────────────────────────

#[test]
fn a_missing_tool_is_absent_not_unknown() {
    let sandbox = Sandbox::new("absent");
    let bin_dir = sandbox.dir("bin");
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "mai-installato", "family": "tool",
             "detect": {"command": "mai-installato"}}]"#,
    );
    let machine = machine(vec![bin_dir], &sandbox.root);
    let report = toolbox::detect(&catalog_from(&descriptors), &machine);

    match &report.findings[0].presence {
        Presence::Absent(reason) => assert!(reason.contains("mai-installato"), "{reason}"),
        other => panic!("a readable, empty directory is a measurement: {other:?}"),
    }
    assert!(report.findings[0].executable.is_none());
    assert!(matches!(
        report.findings[0].version,
        VersionReading::NotAsked(_)
    ));
}

// ── the distinction that makes the inventory useful ─────────────────────

/// THE TEST THIS CRATE STANDS ON. Same descriptor, same missing tool, two
/// different worlds: in one the directories read and the answer is "not here",
/// in the other they do not and the answer is "I do not know". If the code
/// answered `Absent` for both, this test would go red right here — which is
/// exactly what must happen.
#[test]
fn an_unreadable_path_is_undetermined_not_absent() {
    let sandbox = Sandbox::new("blocked");
    let open = sandbox.dir("open");
    let closed = sandbox.dir("closed");
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "attrezzo", "family": "tool", "detect": {"command": "attrezzo"}}]"#,
    );
    let catalog = catalog_from(&descriptors);

    let visible = toolbox::detect(&catalog, &machine(vec![open.clone()], &sandbox.root));
    assert!(
        matches!(visible.findings[0].presence, Presence::Absent(_)),
        "with only the readable directory the answer is a measurement: {:?}",
        visible.findings[0].presence
    );

    let blind = toolbox::detect(&catalog, &machine(vec![open, closed], &sandbox.root));
    match &blind.findings[0].presence {
        Presence::Undetermined(reason) => assert!(reason.contains("closed"), "{reason}"),
        other => panic!("a directory that does not read does not license «not here»: {other:?}"),
    }
}

/// No directory to search in is not "not here": it is "I did not look".
#[test]
fn an_empty_search_path_is_undetermined() {
    let sandbox = Sandbox::new("nopath");
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "attrezzo", "family": "tool", "detect": {"command": "attrezzo"}}]"#,
    );
    let report = toolbox::detect(&catalog_from(&descriptors), &machine(vec![], &sandbox.root));
    assert!(
        matches!(report.findings[0].presence, Presence::Undetermined(_)),
        "{:?}",
        report.findings[0].presence
    );
}

/// The same distinction on paths, not only on executables: a file that is not
/// there and a file that cannot be looked at are two different answers.
#[test]
fn a_path_probe_tells_missing_from_blocked() {
    let sandbox = Sandbox::new("pathprobe");
    let closed = sandbox.dir("closed");
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();
    let descriptors = sandbox.write(
        "tools.json",
        &format!(
            r#"[{{"id": "assente", "family": "tool", "detect": {{"path": "{}/non-c-e.json"}}}},
                {{"id": "coperto", "family": "tool", "detect": {{"path": "{}/coperto.json"}}}}]"#,
            sandbox.root.to_string_lossy(),
            closed.to_string_lossy()
        ),
    );
    let report = toolbox::detect(&catalog_from(&descriptors), &machine(vec![], &sandbox.root));
    // The names looked for are fixture data; the variables holding them are not.
    let absent = report
        .findings
        .iter()
        .find(|f| f.name == "assente")
        .unwrap();
    let covered = report
        .findings
        .iter()
        .find(|f| f.name == "coperto")
        .unwrap();
    assert!(
        matches!(absent.presence, Presence::Absent(_)),
        "{:?}",
        absent.presence
    );
    assert!(
        matches!(covered.presence, Presence::Undetermined(_)),
        "{:?}",
        covered.presence
    );
}

// ── the version: asked, not asked, asked with no answer ─────────────────

/// A tool is there but does not report its version: present, with the version
/// "unavailable" and the reason. Confusing that with absence would erase an
/// installed tool from the list — it really happened, with command lines that
/// complain about permissions before answering.
#[test]
fn a_present_tool_whose_version_fails_stays_present() {
    let sandbox = Sandbox::new("badversion");
    let bin_dir = sandbox.dir("bin");
    fake_binary(
        &bin_dir,
        "scontroso",
        "echo 'permission denied' 1>&2; exit 1",
    );
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "scontroso", "family": "tool", "detect": {"command": "scontroso"},
             "version": {"args": ["--version"]}}]"#,
    );
    let report = toolbox::detect(
        &catalog_from(&descriptors),
        &machine(vec![bin_dir], &sandbox.root),
    );
    let found = &report.findings[0];
    assert!(found.presence.is_present(), "{:?}", found.presence);
    match &found.version {
        VersionReading::Unavailable(reason) => {
            assert!(reason.contains("permission denied"), "{reason}")
        }
        other => panic!("a version that was not obtained is not a version: {other:?}"),
    }
}

/// A binary that does not return does not stop the detection: the timeout cuts
/// it off, and the version becomes "unavailable" with the reason.
#[test]
fn a_hanging_version_probe_does_not_hang_the_detection() {
    let sandbox = Sandbox::new("hang");
    let bin_dir = sandbox.dir("bin");
    fake_binary(&bin_dir, "lento", "exec sleep 60");
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "lento", "family": "tool", "detect": {"command": "lento"},
             "version": {"args": ["--version"], "timeout_secs": 1}}]"#,
    );
    let start = std::time::Instant::now();
    let report = toolbox::detect(
        &catalog_from(&descriptors),
        &machine(vec![bin_dir], &sandbox.root),
    );
    assert!(report.findings[0].presence.is_present());
    assert!(matches!(
        report.findings[0].version,
        VersionReading::Unavailable(_)
    ));
    assert!(
        start.elapsed() < std::time::Duration::from_secs(20),
        "the timeout must really cut it off: {:?}",
        start.elapsed()
    );
}

/// A WARNING IS NOT A VERSION, and telling them apart is data in the descriptor,
/// not a code branch for one binary in particular. The case is real: here
/// `ollama --version`, with its service unreachable, prints a warning first —
/// and without this field that line was recorded as the version.
#[test]
fn a_banner_before_the_version_is_skipped_by_the_descriptor() {
    let sandbox = Sandbox::new("banner");
    let bin_dir = sandbox.dir("bin");
    fake_binary(
        &bin_dir,
        "ciarliero",
        "echo 'Warning: no service'; echo 'ciarliero version 9.9'",
    );
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "grezzo", "family": "tool", "detect": {"command": "ciarliero"},
             "version": {"args": ["--version"]}},
            {"id": "preciso", "family": "tool", "detect": {"command": "ciarliero"},
             "version": {"args": ["--version"], "must_contain": "version"}},
            {"id": "sbagliato", "family": "tool", "detect": {"command": "ciarliero"},
             "version": {"args": ["--version"], "must_contain": "mai-scritto"}}]"#,
    );
    let report = toolbox::detect(
        &catalog_from(&descriptors),
        &machine(vec![bin_dir], &sandbox.root),
    );
    let version = |name: &str| {
        report
            .findings
            .iter()
            .find(|f| f.name == name)
            .unwrap()
            .version
            .clone()
    };
    assert_eq!(
        version("grezzo"),
        VersionReading::Declared("Warning: no service".to_string()),
        "without the field the first line is taken: the behaviour the field exists to correct"
    );
    assert_eq!(
        version("preciso"),
        VersionReading::Declared("ciarliero version 9.9".to_string())
    );
    // A line asked for and never printed is not a version: that gets said, and
    // there is no falling back on whichever line comes first.
    match version("sbagliato") {
        VersionReading::Unavailable(reason) => assert!(reason.contains("mai-scritto"), "{reason}"),
        other => panic!("{other:?}"),
    }
}

/// With executions switched off nothing gets run, and the version says so
/// instead of staying empty.
#[test]
fn with_version_probes_off_nothing_is_executed() {
    let sandbox = Sandbox::new("noprobe");
    let bin_dir = sandbox.dir("bin");
    let probe = sandbox.root.join("spia");
    fake_binary(
        &bin_dir,
        "attrezzo",
        &format!("touch '{}'; echo 1.0", probe.to_string_lossy()),
    );
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "attrezzo", "family": "tool", "detect": {"command": "attrezzo"},
             "version": {"args": ["--version"]}}]"#,
    );
    let mut machine = machine(vec![bin_dir], &sandbox.root);
    machine.version_probes = false;
    let report = toolbox::detect(&catalog_from(&descriptors), &machine);
    assert!(report.findings[0].presence.is_present());
    assert!(matches!(
        report.findings[0].version,
        VersionReading::NotAsked(_)
    ));
    assert!(!probe.exists(), "switched off, no execution must happen");
}

// ── a malformed descriptor does not bring the detection down ────────────

#[test]
fn a_malformed_descriptor_does_not_take_the_others_down() {
    let sandbox = Sandbox::new("malformed");
    let bin_dir = sandbox.dir("bin");
    fake_binary(&bin_dir, "buono", "echo 'buono 1.0'");
    let descriptors = sandbox.write(
        "tools.json",
        r#"[
            {"id": "senza-famiglia"},
            {"id": "senza-verifica", "family": "tool"},
            {"id": "campo-inventato", "family": "tool", "detect": {"command": "x"}, "boh": 1},
            {"id": "buono", "family": "tool", "detect": {"command": "buono"},
             "version": {"args": ["--version"]}}
        ]"#,
    );
    let catalog = catalog_from(&descriptors);
    // **TWO LOST AND ONE KEPT, AND IT USED TO BE THREE LOST.** `campo-inventato`
    // was in that list: a field this version does not know took it down along
    // with the genuinely broken ones, and the tool vanished with it. The two
    // lines that say neither who they are nor how they are checked stay lost,
    // because there is nothing in them to save.
    assert_eq!(
        catalog.problems.len(),
        2,
        "two unrecoverable lines, two problems: {:?}",
        catalog.problems
    );
    // The problem says WHO IT IS ABOUT: "a descriptor is wrong" cannot be fixed,
    // one naming the offending entry can.
    let about: Vec<&str> = catalog.problems.iter().map(|p| p.about.as_str()).collect();
    assert!(about.contains(&"senza-famiglia"), "{about:?}");
    assert!(about.contains(&"senza-verifica"), "{about:?}");
    assert!(
        !about.contains(&"campo-inventato"),
        "an unknown field is not a lost entry: {about:?}"
    );
    let noted: Vec<&str> = catalog.notes.iter().map(|p| p.about.as_str()).collect();
    assert_eq!(
        noted,
        vec!["campo-inventato"],
        "but it is not passed over in silence either"
    );

    let report = toolbox::detect(&catalog, &machine(vec![bin_dir], &sandbox.root));
    // Two now: the good one, and the one that used to be lost — which comes back
    // absent, because its binary `x` really is not there. Absent is an answer;
    // vanished was not.
    assert_eq!(report.findings.len(), 2, "{:?}", report.findings);
    let good = report
        .findings
        .iter()
        .find(|f| f.name == "buono")
        .expect("the good one is there");
    assert_eq!(
        good.version,
        VersionReading::Declared("buono 1.0".to_string())
    );
    // The problems travel with the report: the reader must know the list is
    // partial, or will believe what is missing does not exist.
    assert_eq!(report.problems.len(), 2);
}

/// One whole unreadable file does not take the other descriptor files down.
#[test]
fn a_broken_file_does_not_take_the_other_files_down() {
    let sandbox = Sandbox::new("brokenfile");
    let bin_dir = sandbox.dir("bin");
    fake_binary(&bin_dir, "buono", "echo ok");
    let broken = sandbox.write("rotto.json", "{ this is not json");
    let sound = sandbox.write(
        "sano.json",
        r#"[{"id": "buono", "family": "tool", "detect": {"command": "buono"}}]"#,
    );
    let catalog = Catalog::load(&[Source::File(broken), Source::File(sound)]);
    assert_eq!(catalog.problems.len(), 1, "{:?}", catalog.problems);
    let report = toolbox::detect(&catalog, &machine(vec![bin_dir], &sandbox.root));
    assert_eq!(report.findings.len(), 1);
    assert!(report.findings[0].presence.is_present());
}

// ── the list is data: extended, rewritten, switched off ─────────────────

/// The project's promise, tested: a new command line is added by writing a
/// descriptor, and no line of this crate names it.
#[test]
fn a_new_cli_is_declared_without_recompiling_anything() {
    let sandbox = Sandbox::new("newcli");
    let bin_dir = sandbox.dir("bin");
    fake_binary(&bin_dir, "openrouter", "echo '1.0.2'");
    let descriptors = sandbox.write(
        "mio.json",
        r#"[{"id": "openrouter-cli", "family": "ai_cli", "label": "OpenRouter CLI",
             "detect": {"command": "openrouter"},
             "version": {"args": ["--version"]}}]"#,
    );
    let report = toolbox::detect(
        &catalog_from(&descriptors),
        &machine(vec![bin_dir], &sandbox.root),
    );
    let found = &report.findings[0];
    assert_eq!(found.family, "ai_cli");
    assert!(found.presence.is_present(), "{:?}", found.presence);
    assert_eq!(found.version, VersionReading::Declared("1.0.2".to_string()));
}

/// Whoever arrives later wins on the `id`, and `disabled` deletes. It is the way
/// to get a shipped descriptor out of the way without recompiling.
#[test]
fn a_user_file_overrides_and_switches_off_a_shipped_descriptor() {
    let sandbox = Sandbox::new("override");
    let bin_dir = sandbox.dir("bin");
    fake_binary(&bin_dir, "attrezzo", "echo 1.0");
    let shipped = sandbox.write(
        "spediti.json",
        r#"[{"id": "attrezzo", "family": "tool", "label": "the stock one",
             "detect": {"command": "attrezzo"}},
            {"id": "da-togliere", "family": "tool", "detect": {"command": "attrezzo"}}]"#,
    );
    let mine = sandbox.write(
        "miei.json",
        r#"[{"id": "attrezzo", "family": "tool", "label": "mine",
             "detect": {"command": "attrezzo"}},
            {"id": "da-togliere", "family": "tool", "detect": {"command": "attrezzo"},
             "disabled": true}]"#,
    );
    let catalog = Catalog::load(&[Source::File(shipped), Source::File(mine.clone())]);
    let report = toolbox::detect(&catalog, &machine(vec![bin_dir], &sandbox.root));
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    assert_eq!(report.findings[0].label, "mine");
    assert_eq!(report.findings[0].descriptor_source, mine.to_string_lossy());
}

/// The descriptors shipped with the product all load: if one of them were badly
/// written, this test would say so here instead of on the machine of whoever
/// installs it.
#[test]
fn the_shipped_descriptors_all_load() {
    let catalog = Catalog::load(&[Source::Builtin]);
    assert!(
        catalog.problems.is_empty(),
        "the shipped descriptors do not read: {:?}",
        catalog.problems
    );
    assert!(catalog.live().len() > 10, "{}", catalog.live().len());
    let families: Vec<&str> = catalog
        .live()
        .iter()
        .map(|l| l.descriptor.family.as_str())
        .collect();
    for expected in ["ai_cli", "mcp_server", "tool"] {
        assert!(
            families.contains(&expected),
            "the {expected} family is missing"
        );
    }
}

// ── the entries discovered by reading a configuration ───────────────────

#[test]
fn servers_declared_in_a_config_file_are_discovered_one_by_one() {
    let sandbox = Sandbox::new("enumerate");
    sandbox.write(
        "config.json",
        r#"{"mcpServers": {"socraticode": {"command": "npx"}, "context7": {"command": "npx"}}}"#,
    );
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "server", "family": "mcp_server",
             "enumerate": {"json_keys": {"files": ["~/config.json"],
                                         "pointer": ["mcpServers"]}}}]"#,
    );
    let report = toolbox::detect(&catalog_from(&descriptors), &machine(vec![], &sandbox.root));
    let names: Vec<&str> = report.findings.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["context7", "socraticode"], "{names:?}");
    // Every entry knows where it came from: the descriptor and the file that
    // declares it.
    assert_eq!(report.findings[0].descriptor_id, "server");
    assert!(
        matches!(&report.findings[0].presence, Presence::Present(why) if why.contains("config.json")),
        "{:?}",
        report.findings[0].presence
    );
}

/// A file that is there and declares nothing is a measurement; a file that does
/// not read is not. The same distinction, on the other shape of descriptor.
#[test]
fn an_unreadable_config_is_undetermined_while_an_empty_one_is_absent() {
    let sandbox = Sandbox::new("enumblocked");
    sandbox.write("vuoto.json", r#"{"mcpServers": {}}"#);
    let broken = sandbox.write("rotto.json", "{ not json");
    let descriptors = sandbox.write(
        "tools.json",
        &format!(
            r#"[{{"id": "vuoto", "family": "mcp_server",
                  "enumerate": {{"json_keys": {{"files": ["~/vuoto.json"],
                                                "pointer": ["mcpServers"]}}}}}},
                {{"id": "rotto", "family": "mcp_server",
                  "enumerate": {{"json_keys": {{"files": ["{}"],
                                                "pointer": ["mcpServers"]}}}}}},
                {{"id": "inesistente", "family": "mcp_server",
                  "enumerate": {{"json_keys": {{"files": ["~/mai-scritto.json"],
                                                "pointer": ["mcpServers"]}}}}}}]"#,
            broken.to_string_lossy()
        ),
    );
    let report = toolbox::detect(&catalog_from(&descriptors), &machine(vec![], &sandbox.root));
    let by = |name: &str| {
        report
            .findings
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("{name} is missing"))
            .presence
            .clone()
    };
    assert!(
        matches!(by("vuoto"), Presence::Absent(_)),
        "{:?}",
        by("vuoto")
    );
    assert!(
        matches!(by("rotto"), Presence::Undetermined(_)),
        "{:?}",
        by("rotto")
    );
    assert!(
        matches!(by("inesistente"), Presence::Absent(_)),
        "{:?}",
        by("inesistente")
    );
}

/// A `*` in the path collects the entries declared one level down: without it,
/// everything held per project would stay invisible and the list would say zero.
#[test]
fn a_star_in_the_pointer_reaches_the_per_project_declarations() {
    let sandbox = Sandbox::new("star");
    sandbox.write(
        "config.json",
        r#"{"projects": {"/a": {"mcpServers": {"uno": {}}},
                          "/b": {"mcpServers": {"due": {}}}}}"#,
    );
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "server", "family": "mcp_server",
             "enumerate": {"json_keys": {"files": ["~/config.json"],
                                         "pointer": ["projects", "*", "mcpServers"]}}}]"#,
    );
    let report = toolbox::detect(&catalog_from(&descriptors), &machine(vec![], &sandbox.root));
    let names: Vec<&str> = report.findings.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["due", "uno"], "{names:?}");
}

// ── where the configuration lives ───────────────────────────────────────

#[test]
fn the_config_paths_say_which_ones_are_really_there() {
    let sandbox = Sandbox::new("config");
    let bin_dir = sandbox.dir("bin");
    fake_binary(&bin_dir, "attrezzo", "echo 1.0");
    sandbox.write("settings.json", "{}");
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "attrezzo", "family": "tool", "detect": {"command": "attrezzo"},
             "config": ["~/settings.json", "~/mai-scritto.json"]}]"#,
    );
    let report = toolbox::detect(
        &catalog_from(&descriptors),
        &machine(vec![bin_dir], &sandbox.root),
    );
    let config = &report.findings[0].config;
    assert_eq!(config.len(), 2);
    assert!(config[0].presence.is_present(), "{:?}", config[0]);
    assert!(
        matches!(config[1].presence, Presence::Absent(_)),
        "{:?}",
        config[1]
    );
}

/// A variable that does not exist stays written as it is: replacing it with
/// nothing would build a plausible, wrong path, and the reader would not see why
/// the detection looked somewhere they never named.
#[test]
fn an_undefined_variable_stays_visible_in_the_path() {
    let sandbox = Sandbox::new("var");
    let mut m = machine(vec![], &sandbox.root);
    m.env.insert("MY_HOME".to_string(), "/a/home".to_string());
    assert_eq!(m.expand("$MY_HOME/x"), "/a/home/x");
    assert_eq!(m.expand("${MY_HOME}/x"), "/a/home/x");
    assert_eq!(m.expand("$DOES_NOT_EXIST/x"), "$DOES_NOT_EXIST/x");
    assert_eq!(m.expand("~/y"), sandbox.root.join("y").to_string_lossy());
}

// ── the flow action ─────────────────────────────────────────────────────

#[test]
fn the_flow_action_answers_with_the_findings() {
    use flow::{Action, ActionOutcome, SharedState};
    let sandbox = Sandbox::new("action");
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "mai-installato-davvero", "family": "prova",
             "detect": {"command": "mai-installato-davvero"}}]"#,
    );
    let input = serde_json::json!({
        "descriptor_paths": [descriptors.to_string_lossy()],
        "include_defaults": false,
        "version_probes": false
    });
    let shared = SharedState::new();
    let ActionOutcome::Went(output) = toolbox::DetectToolsAction
        .execute(&input, &shared)
        .expect("a detection does not fail over how the world is")
    else {
        panic!("a detection that ran is always Went")
    };
    assert_eq!(output["total"], 1);
    assert_eq!(output["present"], 0);
    assert_eq!(
        output["findings"][0]["descriptor_id"],
        "mai-installato-davvero"
    );
    assert_eq!(output["findings"][0]["presence"]["state"], "absent");
}

/// An input that does not read is the only fault belonging to the action: it was
/// written by whoever wrote the step, and it must be told to them.
#[test]
fn the_flow_action_rejects_an_input_it_cannot_read() {
    use flow::{Action, SharedState};
    let shared = SharedState::new();
    let input = serde_json::json!({"famiglia": "ai_cli"});
    assert!(toolbox::DetectToolsAction
        .execute(&input, &shared)
        .is_err());
}

#[test]
fn the_registry_finds_the_action_by_its_stable_name() {
    let mut registry = flow::ActionRegistry::default();
    toolbox::register_default(&mut registry);
    assert!(registry.get(toolbox::DETECT_TOOLS_ACTION).is_some());
}

// ── discovery by path: the names, not "the directory is not empty" ──────

/// ONE SERVICE PER FILE, AND THE READER WANTS THE NAMES. `detect` answers "here
/// or not here", useless on a directory of operating-system services: whoever
/// decides what to migrate needs to know *which*. The name is the whole path, on
/// purpose — two files with the same name in two directories are two different
/// automations, and the merge of same-named entries would count them as one.
#[test]
fn enumerating_paths_names_every_file_that_matches() {
    let sandbox = Sandbox::new("paths");
    let home = sandbox.dir("home");
    let agents = sandbox.dir("home/agents");
    fs::write(agents.join("uno.plist"), "<plist/>").expect("a service");
    fs::write(agents.join("due.plist"), "<plist/>").expect("another service");
    fs::write(agents.join("nota.txt"), "not a service").expect("an unrelated file");
    let descriptors = sandbox.write(
        "elenco.json",
        r#"[{
            "id": "servizi",
            "family": "automation_schedule",
            "label": "the services",
            "enumerate": { "paths": ["~/agents/*.plist"] }
        }]"#,
    );

    let report = toolbox::detect(&catalog_from(&descriptors), &machine(Vec::new(), &home));

    let mut names: Vec<&str> = report.findings.iter().map(|f| f.name.as_str()).collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            agents.join("due.plist").to_string_lossy().as_ref(),
            agents.join("uno.plist").to_string_lossy().as_ref(),
        ],
        "one name per file, with the whole path, and the unrelated file left out"
    );
    assert!(report.findings.iter().all(|f| f.presence.is_present()));
}

/// "NOTHING TO MIGRATE" TOLD TO SOMEONE WITH TWENTY SERVICES is the worst lie
/// this list can tell, and it is the one that came naturally: `glob` swallows
/// the `read_dir` error and returns zero paths both for an empty directory and
/// for one that does not read. The two answers differ and must stay different —
/// which is why this test file exists.
#[test]
fn a_folder_that_cannot_be_read_is_not_a_folder_without_automations() {
    let sandbox = Sandbox::new("closed-folder");
    let home = sandbox.dir("home");
    let closed = sandbox.dir("home/agents");
    fs::write(closed.join("uno.plist"), "<plist/>").expect("a service that really is there");
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).expect("closed directory");
    let descriptors = sandbox.write(
        "elenco.json",
        r#"[{
            "id": "servizi",
            "family": "automation_schedule",
            "label": "the services",
            "enumerate": { "paths": ["~/agents/*.plist"] }
        }]"#,
    );

    let report = toolbox::detect(&catalog_from(&descriptors), &machine(Vec::new(), &home));

    assert_eq!(report.findings.len(), 1);
    let presence = &report.findings[0].presence;
    assert!(
        matches!(presence, Presence::Undetermined(_)),
        "a directory that does not read is not an empty directory: {presence:?}"
    );
}

/// A directory that really is not there is a measured absence, and the reason
/// says so: without this half the test above would satisfy itself by always
/// answering "I could not look", which is the other way of saying nothing.
#[test]
fn a_folder_that_is_not_there_is_a_measured_absence() {
    let sandbox = Sandbox::new("absent-folder");
    let home = sandbox.dir("home");
    let descriptors = sandbox.write(
        "elenco.json",
        r#"[{
            "id": "servizi",
            "family": "automation_schedule",
            "label": "the services",
            "enumerate": { "paths": ["~/agents/*.plist"] }
        }]"#,
    );

    let report = toolbox::detect(&catalog_from(&descriptors), &machine(Vec::new(), &home));

    assert_eq!(report.findings.len(), 1);
    match &report.findings[0].presence {
        Presence::Absent(reason) => assert!(
            reason.contains("does not exist"),
            "the reason must say the directory is not there: {reason}"
        ),
        other => panic!("a directory that does not exist is an absence: {other:?}"),
    }
}

/// AN `enumerate` THAT SAYS NOWHERE TO LOOK is not an empty list: it is a badly
/// written one, and the two read the same — "there is nothing here" — while only
/// one of them can be repaired.
#[test]
fn an_enumerate_without_anywhere_to_look_is_a_problem() {
    let sandbox = Sandbox::new("empty-enumerate");
    let descriptors = sandbox.write(
        "elenco.json",
        r#"[{ "id": "vuoto", "family": "tool", "label": "nothing", "enumerate": {} }]"#,
    );

    let catalog = catalog_from(&descriptors);

    assert!(catalog.live().is_empty());
    assert_eq!(catalog.problems.len(), 1, "{:?}", catalog.problems);
    assert!(
        catalog.problems[0].reason.contains("json_keys"),
        "{:?}",
        catalog.problems[0]
    );
}
