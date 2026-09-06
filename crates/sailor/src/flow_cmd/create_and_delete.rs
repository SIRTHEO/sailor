//! `sailor flow new` and `sailor flow delete`: a whole flow is born and dies
//! from the command line, through the same doors `flow edit` and the window
//! already use. See fault 15.

use serde_json::json;
use ui::gather::FlowSource;

use super::cap_and_schedule::a_flow_i_may_rewrite;
use super::check::refusals_of;
use super::edit::{refuse_unknown_actions, registry};
use super::known_flows;

/// The most specific source a person can write into: sources are listed from
/// the binary outwards, so the last one that is not built in is the project
/// when there is one, and the home otherwise.
fn where_a_new_flow_goes(sources: &[FlowSource]) -> Result<&FlowSource, String> {
    sources
        .iter()
        .rev()
        .find(|source| !source.is_builtin())
        .ok_or_else(|| catalogue::say("cli.flow.nowhere_to_create", &[]))
}

pub(super) fn new_flow(sources: &[FlowSource], name: &str) -> Result<String, String> {
    let source = where_a_new_flow_goes(sources)?;
    // A namesake of a flow already in sight — shipped ones included — would
    // hide it in silence; the person is told which one and where it comes from.
    if let Some((_, origin, _)) = known_flows(sources)
        .iter()
        .find(|(known, _, _)| known == name)
    {
        return Err(catalogue::say(
            "cli.flow.already_exists",
            &[("flow", name), ("origin", origin)],
        ));
    }
    let document = json!({
        "id": name,
        "description": "",
        "inputs": {},
        "graph": {"steps": []}
    });
    let flow = flow::system::flow_of_document(&document)?;
    refuse_unknown_actions(&flow.graph)?;
    if let Some(refusal) = refusals_of(&flow, &registry()).into_iter().next() {
        return Err(refusal);
    }
    flow::system::save_document_in(&source.dir, &document)?;
    Ok(catalogue::say(
        "cli.flow.created",
        &[
            ("flow", name),
            ("origin", source.origin),
            ("directory", &source.dir.display().to_string()),
        ],
    ))
}

pub(super) fn delete_flow(sources: &[FlowSource], name: &str) -> Result<String, String> {
    let (_, source) = a_flow_i_may_rewrite(sources, name)?;
    flow::system::delete_in(&source.dir, name)?;
    Ok(catalogue::say(
        "cli.flow.deleted",
        &[
            ("flow", name),
            ("origin", source.origin),
            ("directory", &source.dir.display().to_string()),
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::dispatch;
    use super::*;
    use std::fs;

    fn a_home_with_a_flow() -> (TestDirectory, Vec<FlowSource>) {
        let home = TestDirectory::new();
        home.write(
            "da-cambiare.flow.json",
            &flow_json("shell_check", "[]", "{}").replace("\"prova\"", "\"da-cambiare\""),
        );
        let sources = flow::system::sources(&home.0, None, None);
        (home, sources)
    }

    fn files_in(home: &TestDirectory) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&home.0)
            .expect("the test directory is readable")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn flow_words(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn new_writes_a_file_the_engine_reloads_with_that_id_and_no_steps() {
        let (home, sources) = a_home_with_a_flow();

        let said = dispatch(&flow_words(&["new", "nuovo"]), &sources).expect("a flow is born");

        let text = fs::read_to_string(home.0.join("nuovo.flow.json")).expect("the file is on disk");
        let document: serde_json::Value = serde_json::from_str(&text).expect("it is JSON");
        let flow = flow::system::flow_of_document(&document).expect("the engine reloads it");
        assert_eq!(flow.id, "nuovo");
        assert!(flow.graph.steps().is_empty(), "{text}");
        assert!(said.contains("nuovo") && said.contains("add-step"), "{said}");
        assert!(said.contains(&home.0.display().to_string()), "{said}");
        assert!(said.contains("yours"), "the origin is said: {said}");
    }

    #[test]
    fn new_on_a_name_already_in_sight_refuses_and_leaves_the_file_as_it_was() {
        let (home, sources) = a_home_with_a_flow();
        let before = fs::read(home.0.join("da-cambiare.flow.json")).expect("the flow is on disk");

        let refused = new_flow(&sources, "da-cambiare").expect_err("a namesake is refused");

        let after = fs::read(home.0.join("da-cambiare.flow.json")).expect("the flow is on disk");
        assert_eq!(before, after, "the existing file was touched");
        assert!(refused.contains("already exists"), "{refused}");
        assert!(refused.contains("yours"), "the origin is said: {refused}");
    }

    #[test]
    fn new_with_the_name_of_a_shipped_flow_refuses_instead_of_shadowing_it() {
        let (home, sources) = a_home_with_a_flow();
        let (shipped, _) = flow::system::FLOWS.first().expect("a flow ships inside the binary");

        let refused = new_flow(&sources, shipped).expect_err("the shipped one is not shadowed");

        assert!(refused.contains("already exists"), "{refused}");
        assert!(refused.contains("built in"), "the origin is said: {refused}");
        assert_eq!(files_in(&home), vec!["da-cambiare.flow.json".to_owned()]);
    }

    #[test]
    fn delete_removes_the_file_and_a_second_delete_refuses() {
        let (home, sources) = a_home_with_a_flow();

        let said = dispatch(&flow_words(&["delete", "da-cambiare"]), &sources).expect("deleted");

        assert!(!home.0.join("da-cambiare.flow.json").exists());
        assert!(said.contains("da-cambiare") && said.contains("deleted"), "{said}");
        assert!(said.contains(&home.0.display().to_string()), "{said}");
        let refused = delete_flow(&sources, "da-cambiare").expect_err("nothing left to delete");
        assert!(refused.contains("no flow is called da-cambiare"), "{refused}");
    }

    #[test]
    fn delete_on_a_flow_that_ships_inside_the_binary_refuses_and_changes_nothing() {
        let (home, sources) = a_home_with_a_flow();
        let (shipped, _) = flow::system::FLOWS.first().expect("a flow ships inside the binary");

        let refused = delete_flow(&sources, shipped).expect_err("the binary is not a file");

        assert!(refused.contains("ships inside the binary"), "{refused}");
        assert_eq!(files_in(&home), vec!["da-cambiare.flow.json".to_owned()]);
    }

    #[test]
    fn new_with_a_name_that_climbs_out_of_the_directory_refuses_from_the_command_line() {
        let (home, sources) = a_home_with_a_flow();

        for name in ["a/b", "../fuori"] {
            let refused = dispatch(&flow_words(&["new", name]), &sources)
                .expect_err("a path is not a flow name");
            assert!(refused.contains("not a safe flow name"), "«{name}»: {refused}");
        }
        assert_eq!(files_in(&home), vec!["da-cambiare.flow.json".to_owned()]);
    }

    #[test]
    fn new_with_only_the_binary_in_sight_has_nowhere_to_write() {
        let refused = new_flow(&[FlowSource::builtin()], "nuovo").expect_err("no writable source");

        assert!(refused.contains("no place to write a flow"), "{refused}");
    }
}
