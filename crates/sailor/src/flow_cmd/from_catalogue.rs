//! `sailor flow catalogue` and `sailor flow from`: the templates and examples
//! the binary carries, and a flow of your own made from one of them. The
//! window reads and makes its flows through [`full_judge`] and
//! [`made_from_catalogue`] too.

use std::collections::{BTreeMap, BTreeSet};

use flow::starters::{self, Created, Destination, Kind, Listed};
use toolbox::privacy::{self, Reason};
use ui::gather::FlowSource;

use super::check::refusals_of;
use super::edit::registry;

/// The list of names a machine keeps private, as far as it can be read.
pub enum PrivateNames {
    Read(Vec<String>),
    /// Nothing declares one: names are not measured, shapes still are.
    NotDeclared,
    /// `SAILOR_PRIVATE_NAMES` names a list that does not read.
    Unreadable,
}

pub fn private_names() -> PrivateNames {
    let declared = std::env::var("SAILOR_PRIVATE_NAMES").ok().filter(|path| !path.is_empty());
    let explicit = declared.is_some();
    let Some(list) = privacy::where_the_names_are(declared, std::env::var("HOME").ok()) else {
        return PrivateNames::NotDeclared;
    };
    match std::fs::read_to_string(list) {
        Ok(text) => PrivateNames::Read(privacy::names_in(&text)),
        Err(_) if explicit => PrivateNames::Unreadable,
        Err(_) => PrivateNames::NotDeclared,
    }
}

/// The judge an entry answers to before anybody sees it, beyond the flow
/// crate's own: the refusals of `flow check`, and what a public text may not carry.
pub fn full_judge(listed: &Listed) -> Vec<String> {
    let names = match private_names() {
        PrivateNames::Read(names) => names,
        PrivateNames::NotDeclared | PrivateNames::Unreadable => Vec::new(),
    };
    judged_against(listed, &names, std::env::var("HOME").ok().as_deref())
}

pub fn judged_against(listed: &Listed, names: &[String], home: Option<&str>) -> Vec<String> {
    let mut refused = match flow::system::flow_of_document(&starters::probe(listed)) {
        Ok(flow) => refusals_of(&flow, &registry()),
        Err(error) => vec![error],
    };
    for (at, text) in starters::every_text(listed) {
        let mut what = BTreeSet::new();
        for reason in privacy::what_a_public_text_cannot_carry(&text, names, home) {
            what.insert(match reason {
                Reason::APrivateName { .. } => "cli.flow.catalogue_private_name",
                Reason::APathOfThisMachine { .. } | Reason::AShapeOfAMachine { .. } => {
                    "cli.flow.catalogue_private_machine"
                }
                Reason::APassageAboutTheMachine { .. } => "cli.flow.catalogue_private_passage",
            });
        }
        if privacy::an_account_address(&text).is_some() {
            what.insert("cli.flow.catalogue_private_account");
        }
        if privacy::a_private_network_address(&text).is_some() {
            what.insert("cli.flow.catalogue_private_network");
        }
        for key in what {
            refused.push(catalogue::say(
                "cli.flow.catalogue_private",
                &[
                    ("entry", &listed.entry.entry),
                    ("what", &catalogue::say(key, &[])),
                    ("at", &at),
                ],
            ));
        }
    }
    refused
}

pub(super) fn list_catalogue() -> Result<String, String> {
    Ok(list_catalogue_in(catalogue::language()))
}

fn list_catalogue_in(language: &str) -> String {
    let say = |key: &str, values: &[(&str, &str)]| {
        catalogue::look(language, key, values).unwrap_or_else(|| key.to_owned())
    };
    let mut lines = Vec::new();
    for (name, judged) in starters::all(&full_judge) {
        let listed = match judged {
            Ok(listed) => listed,
            Err(refused) => {
                lines.push(say(
                    "cli.flow.catalogue_refused",
                    &[("entry", name), ("why", &refused.join("; "))],
                ));
                continue;
            }
        };
        let entry = &listed.entry;
        let kind = say(
            match entry.kind {
                Kind::Template => "cli.flow.catalogue_kind_template",
                Kind::Example => "cli.flow.catalogue_kind_example",
            },
            &[],
        );
        let purpose = starters::said(language, &entry.purpose);
        lines.push(say(
            "cli.flow.catalogue_row",
            &[("entry", &entry.entry), ("kind", &kind), ("purpose", &purpose)],
        ));
        let inputs: Vec<String> = entry
            .inputs
            .iter()
            .map(|(input, declared)| {
                let key = if declared.required {
                    "cli.flow.catalogue_required"
                } else {
                    "cli.flow.catalogue_optional"
                };
                say(key, &[("input", input), ("means", &starters::said(language, &declared.means))])
            })
            .collect();
        let inputs = if inputs.is_empty() {
            say("cli.flow.catalogue_no_inputs", &[])
        } else {
            say("cli.flow.catalogue_inputs", &[("inputs", &inputs.join("; "))])
        };
        lines.push(format!("    {inputs}"));
        if let Some(teaches) = &entry.teaches {
            let capability = say(
                "cli.flow.catalogue_teaches",
                &[("capability", &starters::said(language, &teaches.capability))],
            );
            let result = say(
                "cli.flow.catalogue_you_will_see",
                &[("result", &starters::said(language, &teaches.result))],
            );
            lines.push(format!("    {capability}\n    {result}"));
        }
    }
    lines.join("\n")
}

pub(super) fn flow_from(
    sources: &[FlowSource],
    entry: &str,
    name: &str,
    options: &[String],
) -> Result<String, String> {
    let (home, given) = options_of(options)?;
    let destination = if home {
        Destination::Home
    } else {
        starters::default_destination(sources)
    };
    let created = made_from_catalogue(sources, entry, name, &given, destination)?;
    Ok(catalogue::say(
        "cli.flow.created_from_catalogue",
        &[
            ("flow", &created.flow),
            ("origin", created.origin),
            ("entry", &created.entry),
            ("directory", &created.dir.display().to_string()),
        ],
    ))
}

/// One road from an entry to a written flow, for the command line and the
/// window alike: the entry's full judge and the refusals of `flow check` stop it.
pub fn made_from_catalogue(
    sources: &[FlowSource],
    entry: &str,
    name: &str,
    given: &BTreeMap<String, String>,
    destination: Destination,
) -> Result<Created, String> {
    starters::create(sources, entry, name, given, destination, &full_judge, &|flow| {
        refusals_of(flow, &registry())
            .into_iter()
            .next()
            .map_or(Ok(()), Err)
    })
}

fn options_of(options: &[String]) -> Result<(bool, BTreeMap<String, String>), String> {
    let mut home = false;
    let mut given = BTreeMap::new();
    let mut words = options.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--home" => home = true,
            "--input" => {
                let pair = words.next().map(String::as_str).unwrap_or_default();
                let (input, value) = pair
                    .split_once('=')
                    .filter(|(input, _)| !input.is_empty())
                    .ok_or_else(|| catalogue::say("cli.flow.input_not_key_value", &[("given", pair)]))?;
                if given.insert(input.to_owned(), value.to_owned()).is_some() {
                    return Err(catalogue::say("cli.flow.input_given_twice", &[("input", input)]));
                }
            }
            _ => return Err(super::usage()),
        }
    }
    Ok((home, given))
}

#[cfg(test)]
mod tests {
    use super::super::check::check_report;
    use super::super::dispatch;
    use super::super::test_support::TestDirectory;
    use super::*;
    use serde_json::Value;
    use std::fs;

    const AN_ENGINE: &str = "the-engine-this-test-declares";

    struct Place {
        _home: TestDirectory,
        _project: TestDirectory,
        home: std::path::PathBuf,
        project: std::path::PathBuf,
        sources: Vec<FlowSource>,
    }

    fn home_and_project() -> Place {
        let home = TestDirectory::new();
        let project = TestDirectory::new();
        let sources = vec![
            FlowSource::builtin(),
            FlowSource { origin: flow::system::YOUR_ORIGIN, dir: home.0.clone() },
            FlowSource { origin: flow::workspace::ORIGIN_DECLARED, dir: project.0.clone() },
        ];
        Place { home: home.0.clone(), project: project.0.clone(), _home: home, _project: project, sources }
    }

    fn words(said: &[&str]) -> Vec<String> {
        said.iter().map(|word| (*word).to_owned()).collect()
    }

    fn written(dir: &std::path::Path, name: &str) -> flow::FlowFile {
        let text = fs::read_to_string(dir.join(format!("{name}.flow.json"))).expect("the flow is on disk");
        serde_json::from_str(&text).expect("the engine loads it")
    }

    /// Descriptors this test decides, so the check does not read the machine.
    fn tools_declaring(id: &str) -> toolbox::Tools {
        let file = std::env::temp_dir().join(format!("sailor-catalogue-tools-{}.json", std::process::id()));
        let tool = format!(r#"{{"tools":[{{"id":"{id}","family":"tool","label":"{id}","detect":{{"command":"{id}"}}}}]}}"#);
        fs::write(&file, tool).expect("the descriptor writes");
        let catalog = toolbox::Catalog::load(&[toolbox::Source::File(file)]);
        toolbox::Tools::new(catalog, toolbox::Machine::bare(std::path::PathBuf::from(toolbox::probe::NOWHERE)))
    }

    fn the_check_passes(flow: &flow::FlowFile) {
        let tools = tools_declaring(AN_ENGINE);
        let (report, unknown) = check_report(flow, &registry(), Some(&tools), None);
        let refused = refusals_of(flow, &registry());
        assert!(refused.is_empty() && unknown.is_empty(), "{refused:?} {unknown:?}\n{report}");
    }

    /// A real entry with one field changed, judged as the product judges it
    /// but against names and a home the test declares.
    fn full_refusals_of(entry: &str, names: &[&str], change: impl FnOnce(&mut Value)) -> Vec<String> {
        let (_, text) = starters::ENTRIES.iter().find(|(name, _)| *name == entry).expect("the entry exists");
        let mut value: Value = serde_json::from_str(text).expect("JSON");
        change(&mut value);
        let text = serde_json::to_string_pretty(&value).expect("it writes back");
        let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
        match starters::read(entry, &text, &|listed| judged_against(listed, &names, None)) {
            Ok(_) => Vec::new(),
            Err(refused) => refused,
        }
    }

    #[test]
    fn every_entry_passes_the_full_judge_with_this_machine_s_names_when_it_keeps_a_list() {
        match private_names() {
            PrivateNames::Read(_) => {}
            PrivateNames::NotDeclared => {
                println!("private names: not measured, no list of them is declared here")
            }
            PrivateNames::Unreadable => panic!("SAILOR_PRIVATE_NAMES names a list that does not read"),
        }
        for (name, judged) in starters::all(&full_judge) {
            if let Err(refused) = judged {
                panic!("«{name}» is refused:\n{}", refused.join("\n"));
            }
        }
    }

    #[test]
    fn private_material_of_every_kind_is_refused_by_the_full_judge() {
        let path = format!("/{}/{}/notes", "Users", "a-person");
        let account = format!("send it to someone{}example.org", '@');
        let server = format!("the server at http://{}.{}.1.20:8080", 192, 168);
        let cases: Vec<(&str, Vec<String>, &str)> = vec![
            (
                "a person",
                full_refusals_of("a-check-that-stops-the-run", &["a-private-person"], |entry| {
                    entry["flow"]["description"] = "Written for a-private-person to run.".into();
                }),
                "a private name",
            ),
            (
                "an account",
                full_refusals_of("a-check-that-stops-the-run", &[], |entry| {
                    entry["flow"]["inputs"]["trigger"]["text"] = account.clone().into();
                }),
                "an account address",
            ),
            (
                "a path",
                full_refusals_of("a-check-that-stops-the-run", &[], |entry| {
                    entry["flow"]["inputs"]["trigger"]["text"] = path.clone().into();
                }),
                "a path or a shape of one machine",
            ),
            (
                "a home server",
                full_refusals_of("a-check-that-stops-the-run", &[], |entry| {
                    entry["flow"]["description"] = server.clone().into();
                }),
                "an address of a private network",
            ),
            (
                "a private flow",
                full_refusals_of("one-flow-calls-another", &[], |entry| {
                    entry["needs"]["flows"] = serde_json::json!(["somebody-s-own-flow"]);
                    entry["flow"]["graph"]["steps"][0]["with"]["flow"] = "somebody-s-own-flow".into();
                }),
                "does not ship",
            ),
            (
                "a pinned model",
                full_refusals_of("a-text-piece-from-a-brief", &[], |entry| {
                    entry["flow"]["graph"]["steps"][1]["with"]["model"] = serde_json::json!({"an-engine": "a-model"});
                }),
                "pins a model",
            ),
        ];
        for (what, refused, said) in cases {
            assert!(refused.iter().any(|reason| reason.contains(said)), "{what}: nobody said «{said}»: {refused:?}");
        }
        assert!(full_refusals_of("a-check-that-stops-the-run", &["a-private-person"], |_| {}).is_empty());
    }

    #[test]
    fn an_action_nobody_registers_is_refused_by_the_full_judge_even_when_declared() {
        let refused = full_refusals_of("a-check-that-stops-the-run", &[], |entry| {
            entry["needs"]["actions"] = serde_json::json!(["no_such_action", "shell_check", "trigger"]);
            entry["flow"]["graph"]["steps"][2]["action"] = "no_such_action".into();
        });

        assert!(refused.iter().any(|reason| reason.contains("no_such_action")), "{refused:?}");
    }

    #[test]
    fn catalogue_lists_every_entry_with_its_kind_purpose_and_inputs() {
        let place = home_and_project();

        let said = dispatch(&words(&["catalogue"]), &place.sources).expect("the catalogue is listed");

        for (name, _) in flow::starters::ENTRIES {
            assert!(said.contains(name), "«{name}» is missing:\n{said}");
        }
        assert!(said.contains("(template)") && said.contains("(example)"), "{said}");
        assert!(said.contains("engine (required)"), "{said}");
        assert!(said.contains("no input to fill"), "{said}");
    }

    #[test]
    fn catalogue_in_italian_says_every_sentence_in_italian() {
        let italian = list_catalogue_in("it");
        let english = list_catalogue_in("en");

        for (name, judged) in starters::all(&starters::no_extra) {
            let listed = judged.expect("the entry passes");
            for (at, key) in starters::sentence_keys(&listed.entry) {
                let sentence = starters::said("it", &key);
                assert!(italian.contains(&sentence), "«{name}» {at} is not said in Italian:\n{italian}");
                assert!(!english.contains(&sentence), "«{name}» {at}: the English listing says the Italian sentence");
            }
        }
        assert!(italian.contains("(esempio)") && italian.contains("engine (obbligatorio)"), "{italian}");
    }

    #[test]
    fn every_entry_makes_a_flow_the_check_passes() {
        for (name, judged) in flow::starters::all(&starters::no_extra) {
            let listed = judged.unwrap_or_else(|refused| panic!("«{name}»: {refused:?}"));
            let given: BTreeMap<String, String> =
                listed.entry.inputs.keys().map(|input| (input.clone(), AN_ENGINE.to_owned())).collect();
            let document = starters::instantiate(&listed, &format!("{name}-made"), &given).expect("made");
            let flow = flow::system::flow_of_document(&document).expect("the engine loads it");
            the_check_passes(&flow);
        }
    }

    #[test]
    fn from_writes_into_the_project_with_its_new_name_and_provenance() {
        let place = home_and_project();

        let said = dispatch(
            &words(&["from", "a-text-piece-from-a-brief", "release-notes", "--input", &format!("engine={AN_ENGINE}")]),
            &place.sources,
        )
        .expect("the flow is made");

        let flow = written(&place.project, "release-notes");
        assert_eq!(flow.id, "release-notes");
        assert_eq!(flow.from.as_ref().expect("provenance").catalogue, "a-text-piece-from-a-brief");
        assert!(!place.home.join("release-notes.flow.json").exists());
        assert!(said.contains("release-notes") && said.contains("a-text-piece-from-a-brief"), "{said}");
        assert!(said.contains(&place.project.display().to_string()), "{said}");
        the_check_passes(&flow);
    }

    #[test]
    fn from_with_home_writes_into_your_own_flows() {
        let place = home_and_project();

        dispatch(&words(&["from", "a-check-that-stops-the-run", "my-check", "--home"]), &place.sources)
            .expect("the flow is made");

        assert_eq!(written(&place.home, "my-check").id, "my-check");
        assert!(!place.project.join("my-check.flow.json").exists());
    }

    #[test]
    fn from_refuses_a_namesake_a_missing_input_an_unknown_entry_and_a_bad_option() {
        let place = home_and_project();
        let (shipped, _) = flow::system::FLOWS.first().expect("a flow ships");
        let refused = |said: &[&str]| dispatch(&words(said), &place.sources).expect_err("refused");

        let namesake = refused(&["from", "a-check-that-stops-the-run", shipped]);
        let missing = refused(&["from", "a-text-piece-from-a-brief", "a-piece"]);
        let unknown = refused(&["from", "no-such-entry", "a-piece"]);
        let not_a_pair = refused(&["from", "a-text-piece-from-a-brief", "a-piece", "--input", "engine"]);
        let twice = refused(&["from", "a-text-piece-from-a-brief", "a-piece", "--input", "engine=a", "--input", "engine=b"]);
        let stray = refused(&["from", "a-check-that-stops-the-run", "a-piece", "--everywhere"]);

        assert!(namesake.contains("already exists") && namesake.contains("built in"), "{namesake}");
        assert!(missing.contains("needs the input «engine»"), "{missing}");
        assert!(unknown.contains("no entry called «no-such-entry»"), "{unknown}");
        assert!(not_a_pair.contains("--input key=value"), "{not_a_pair}");
        assert!(twice.contains("given twice"), "{twice}");
        assert!(stray.contains("sailor flow from"), "{stray}");
        for dir in [&place.home, &place.project] {
            assert_eq!(fs::read_dir(dir).expect("readable").count(), 0, "a refusal wrote a file");
        }
    }
}
