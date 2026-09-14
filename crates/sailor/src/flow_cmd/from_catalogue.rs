//! `sailor flow catalogue` and `sailor flow from`: the templates and examples
//! the binary carries, and a flow of your own made from one of them. The
//! window makes its flows through [`made_from_catalogue`] too.

use std::collections::BTreeMap;

use flow::starters::{self, Created, Destination, Kind};
use ui::gather::FlowSource;

use super::check::refusals_of;
use super::edit::{refuse_unknown_actions, registry};

pub(super) fn list_catalogue() -> Result<String, String> {
    let mut lines = Vec::new();
    for (name, judged) in starters::all() {
        let listed = match judged {
            Ok(listed) => listed,
            Err(refused) => {
                lines.push(catalogue::say(
                    "cli.flow.catalogue_refused",
                    &[("entry", name), ("why", &refused.join("; "))],
                ));
                continue;
            }
        };
        let entry = &listed.entry;
        let kind = catalogue::say(
            match entry.kind {
                Kind::Template => "cli.flow.catalogue_kind_template",
                Kind::Example => "cli.flow.catalogue_kind_example",
            },
            &[],
        );
        lines.push(catalogue::say(
            "cli.flow.catalogue_row",
            &[("entry", &entry.entry), ("kind", &kind), ("purpose", &entry.purpose)],
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
                catalogue::say(key, &[("input", input), ("means", &declared.means)])
            })
            .collect();
        let inputs = if inputs.is_empty() {
            catalogue::say("cli.flow.catalogue_no_inputs", &[])
        } else {
            catalogue::say("cli.flow.catalogue_inputs", &[("inputs", &inputs.join("; "))])
        };
        lines.push(format!("    {inputs}"));
        if let Some(teaches) = &entry.teaches {
            let capability = catalogue::say("cli.flow.catalogue_teaches", &[("capability", &teaches.capability)]);
            let result = catalogue::say("cli.flow.catalogue_you_will_see", &[("result", &teaches.result)]);
            lines.push(format!("    {capability}\n    {result}"));
        }
    }
    Ok(lines.join("\n"))
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
/// window alike: the refusals `flow new` and `flow check` would give stop it.
pub fn made_from_catalogue(
    sources: &[FlowSource],
    entry: &str,
    name: &str,
    given: &BTreeMap<String, String>,
    destination: Destination,
) -> Result<Created, String> {
    starters::create(sources, entry, name, given, destination, &|flow| {
        refuse_unknown_actions(&flow.graph)?;
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
    fn every_entry_makes_a_flow_the_check_passes() {
        for (name, judged) in flow::starters::all() {
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
