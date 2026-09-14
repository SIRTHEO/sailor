//! The catalogue hands out templates and examples, and runs none of them: no
//! source finds an entry, every entry passes its judge, and a flow made from
//! one has a name of its own and says where it came from.

use flow::starters::{self, Destination, Listed};
use flow::system::{self, FlowSource};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new(line: u32) -> Scratch {
        let dir = std::env::temp_dir().join(format!("sailor-catalogue-{}-{line}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn home_and_project(scratch: &Scratch) -> Vec<FlowSource> {
    let home = scratch.0.join("home");
    let project = scratch.0.join("project");
    std::fs::create_dir_all(&home).expect("a home");
    std::fs::create_dir_all(&project).expect("a project");
    vec![
        FlowSource::builtin(),
        FlowSource { origin: system::YOUR_ORIGIN, dir: home },
        FlowSource { origin: flow::workspace::ORIGIN_DECLARED, dir: project },
    ]
}

fn given(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
}

fn accept(_: &flow::FlowFile) -> Result<(), String> {
    Ok(())
}

fn read_back(dir: &Path, name: &str) -> Value {
    let text = std::fs::read_to_string(dir.join(format!("{name}.flow.json"))).expect("the flow is on disk");
    serde_json::from_str(&text).expect("it is JSON")
}

fn template() -> Listed {
    starters::find("a-text-piece-from-a-brief").expect("the template is in the catalogue")
}

/// A real entry with one field changed: every broken fixture starts from one
/// that passes, so a refusal is about the change and nothing else.
fn mutant(entry: &str, change: impl FnOnce(&mut Value)) -> String {
    let (_, text) = starters::ENTRIES
        .iter()
        .find(|(name, _)| *name == entry)
        .expect("the entry exists");
    let mut value: Value = serde_json::from_str(text).expect("the entry is JSON");
    change(&mut value);
    serde_json::to_string_pretty(&value).expect("it writes back")
}

#[test]
fn every_entry_in_the_catalogue_passes_its_judge() {
    assert!(starters::ENTRIES.len() >= 2, "the catalogue holds a template and an example");
    for (name, judged) in starters::all() {
        let listed = judged.unwrap_or_else(|refused| panic!("«{name}» is refused:\n{}", refused.join("\n")));
        assert_eq!(listed.entry.entry, name);
        assert_eq!(listed.version.len(), 64, "the version is a whole digest");
    }
    let kinds: Vec<starters::Kind> = starters::all()
        .into_iter()
        .filter_map(|(_, judged)| judged.ok().map(|listed| listed.entry.kind))
        .collect();
    assert!(kinds.contains(&starters::Kind::Template) && kinds.contains(&starters::Kind::Example), "{kinds:?}");
}

#[test]
fn no_source_finds_an_entry_and_nothing_resolves_or_runs_one() {
    let scratch = Scratch::new(line!());
    let sources = home_and_project(&scratch);
    let loaded: Vec<String> = system::load_all(&sources).into_iter().map(|(name, _, _)| name).collect();
    let resolved: Vec<String> = system::resolve(&sources, None)
        .into_iter()
        .map(|resolved| resolved.chain.name)
        .collect();
    let shipped = system::builtin_registry();
    assert!(!loaded.is_empty(), "the shipped flows were read, so the absence below means something");
    for (entry, _) in starters::ENTRIES {
        assert!(!loaded.iter().any(|name| name == entry), "«{entry}» is among the flows load_all finds");
        assert!(!resolved.iter().any(|name| name == entry), "«{entry}» resolves as a flow");
        assert!(!shipped.contains_key(*entry), "«{entry}» is among the shipped flows");
        assert!(!system::FLOWS.iter().any(|(name, _)| name == entry), "«{entry}» ships as a flow");
    }
}

#[test]
fn a_broken_entry_is_refused_for_the_reason_it_is_broken() {
    let cases: Vec<(&str, &str, String)> = vec![
        ("an empty purpose", "purpose", mutant("a-check-that-stops-the-run", |entry| entry["purpose"] = "".into())),
        ("a pinned engine", "pins the engine", mutant("a-text-piece-from-a-brief", |entry| {
            entry["flow"]["graph"]["steps"][1]["with"]["tool"] = serde_json::json!(["an-engine"]);
        })),
        ("a pinned model", "pins a model", mutant("a-text-piece-from-a-brief", |entry| {
            entry["flow"]["graph"]["steps"][1]["with"]["model"] = serde_json::json!({"an-engine": "a-model"});
        })),
        ("a path of one machine", "path of one machine", mutant("a-check-that-stops-the-run", |entry| {
            entry["flow"]["inputs"]["trigger"]["text"] = "/Users/somebody/notes".into();
        })),
        ("an input nobody declared", "declares no such input", mutant("a-check-that-stops-the-run", |entry| {
            entry["flow"]["inputs"]["trigger"]["text"] = serde_json::json!({"$input": "brief"});
        })),
        ("an input without a meaning", "without saying what it means", mutant("a-text-piece-from-a-brief", |entry| {
            entry["inputs"]["engine"]["means"] = "".into();
        })),
        ("an example that teaches nothing", "teaches", mutant("one-flow-calls-another", |entry| {
            entry.as_object_mut().expect("an object").remove("teaches");
        })),
        ("a flow it calls and does not declare", "declares the flows", mutant("one-flow-calls-another", |entry| {
            entry["needs"]["flows"] = serde_json::json!([]);
        })),
        ("a flow that does not ship", "does not ship", mutant("one-flow-calls-another", |entry| {
            entry["needs"]["flows"] = serde_json::json!(["somebody-s-own-flow"]);
            entry["flow"]["graph"]["steps"][0]["with"]["flow"] = "somebody-s-own-flow".into();
        })),
        ("an action it does not declare", "declares the actions", mutant("a-check-that-stops-the-run", |entry| {
            entry["needs"]["actions"] = serde_json::json!(["trigger"]);
        })),
        ("an identity of its own", "writes «id»", mutant("a-check-that-stops-the-run", |entry| {
            entry["flow"]["id"] = "a-check-that-stops-the-run".into();
        })),
        ("a graph the engine refuses", "does not make a flow", mutant("a-check-that-stops-the-run", |entry| {
            entry["flow"]["graph"]["steps"][1]["deps"] = serde_json::json!(["nobody"]);
        })),
    ];
    for (what, said, text) in cases {
        let name = serde_json::from_str::<Value>(&text).expect("JSON")["entry"].as_str().expect("a name").to_owned();
        let refused = starters::judge(&name, &text);
        assert!(
            refused.iter().any(|reason| reason.contains(said)),
            "{what}: the judge did not say «{said}»: {refused:?}"
        );
    }
}

#[test]
fn a_flow_made_from_a_template_has_its_name_its_input_and_its_provenance() {
    let scratch = Scratch::new(line!());
    let sources = home_and_project(&scratch);

    let created = starters::create(
        &sources,
        "a-text-piece-from-a-brief",
        "notes-in-plain-words",
        &given(&[("engine", "the-engine-i-have")]),
        starters::default_destination(&sources),
        &accept,
    )
    .expect("the flow is made");

    assert_eq!(created.origin, flow::workspace::ORIGIN_DECLARED, "the project is where it goes");
    let written = read_back(&scratch.0.join("project"), "notes-in-plain-words");
    assert_eq!(written["id"], "notes-in-plain-words");
    assert_eq!(written["graph"]["steps"][1]["with"]["tool"], serde_json::json!(["the-engine-i-have"]));
    assert_eq!(written["from"]["catalogue"], "a-text-piece-from-a-brief");
    assert_eq!(written["from"]["version"], template().version);
    let flow: flow::FlowFile = serde_json::from_value(written).expect("the engine loads it");
    assert_eq!(flow.from.expect("provenance").catalogue, "a-text-piece-from-a-brief");
    assert!(!scratch.0.join("project/a-text-piece-from-a-brief.flow.json").exists());
}

#[test]
fn home_is_where_it_goes_when_asked_or_when_no_project_is_in_sight() {
    let scratch = Scratch::new(line!());
    let sources = home_and_project(&scratch);
    let only_home: Vec<FlowSource> = sources[..2].to_vec();

    starters::create(&sources, "a-check-that-stops-the-run", "asked-for-home", &given(&[]), Destination::Home, &accept)
        .expect("made at home");
    assert_eq!(starters::default_destination(&only_home), Destination::Home);
    let refused = starters::place(&only_home, Destination::Workspace).expect_err("no project in sight");

    assert!(scratch.0.join("home/asked-for-home.flow.json").exists());
    assert!(!scratch.0.join("project/asked-for-home.flow.json").exists());
    assert!(refused.contains("no workspace"), "{refused}");
}

#[test]
fn a_namesake_a_missing_input_an_unknown_input_and_an_unknown_entry_are_refused() {
    let scratch = Scratch::new(line!());
    let sources = home_and_project(&scratch);
    let (shipped, _) = system::FLOWS.first().expect("a flow ships");
    let make = |entry: &str, name: &str, inputs: &[(&str, &str)]| {
        starters::create(&sources, entry, name, &given(inputs), Destination::Workspace, &accept)
            .expect_err("refused")
    };

    let namesake = make("a-check-that-stops-the-run", shipped, &[]);
    let missing = make("a-text-piece-from-a-brief", "a-piece", &[]);
    let blank = make("a-text-piece-from-a-brief", "a-piece", &[("engine", " ")]);
    let unknown_input = make("a-check-that-stops-the-run", "a-check", &[("colour", "blue")]);
    let unknown_entry = make("no-such-entry", "a-check", &[]);
    let own_name = make("a-check-that-stops-the-run", "a-check-that-stops-the-run", &[]);

    assert!(namesake.contains("already exists") && namesake.contains("built in"), "{namesake}");
    assert!(missing.contains("needs the input «engine»"), "{missing}");
    assert!(blank.contains("needs the input «engine»"), "{blank}");
    assert!(unknown_input.contains("no input called «colour»"), "{unknown_input}");
    assert!(unknown_entry.contains("no entry called «no-such-entry»"), "{unknown_entry}");
    assert!(own_name.contains("a name of its own"), "{own_name}");
    let written = std::fs::read_dir(scratch.0.join("project")).expect("readable").count();
    assert_eq!(written, 0, "a refusal wrote a file");
}

#[test]
fn a_refusal_of_the_callers_check_writes_nothing() {
    let scratch = Scratch::new(line!());
    let sources = home_and_project(&scratch);

    let refused = starters::create(
        &sources,
        "a-check-that-stops-the-run",
        "checked-and-refused",
        &given(&[]),
        Destination::Workspace,
        &|_| Err("the caller refuses it".to_owned()),
    )
    .expect_err("the check refused");

    assert_eq!(refused, "the caller refuses it");
    assert!(!scratch.0.join("project/checked-and-refused.flow.json").exists());
}

#[test]
fn an_optional_input_nobody_gives_takes_its_field_away() {
    let text = mutant("a-text-piece-from-a-brief", |entry| {
        entry["inputs"]["seconds"] = serde_json::json!({"means": "how long the engine may take", "required": false});
        entry["flow"]["graph"]["steps"][1]["with"]["timeout_secs"] = serde_json::json!({"$input": "seconds"});
    });
    let listed = starters::read("a-text-piece-from-a-brief", &text).expect("the mutant passes the judge");

    let without = starters::instantiate(&listed, "no-seconds", &given(&[("engine", "e")])).expect("made");
    let with = starters::instantiate(&listed, "seconds", &given(&[("engine", "e"), ("seconds", "60")])).expect("made");

    assert!(without["graph"]["steps"][1]["with"].get("timeout_secs").is_none(), "{without}");
    assert_eq!(with["graph"]["steps"][1]["with"]["timeout_secs"], "60");
}
