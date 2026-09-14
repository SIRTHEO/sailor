//! The catalogue in the window: the entries to start a flow from, and the one
//! road that makes a flow of one — the command line's own, not a copy.

use std::collections::BTreeMap;

use flow::starters::{self, Destination, Kind, Teaches};
use serde::Serialize;
use ui::gather::FlowSource;

#[derive(Serialize)]
pub(crate) struct CatalogueInput {
    name: String,
    means: String,
    required: bool,
}

#[derive(Serialize)]
pub(crate) struct CatalogueStep {
    id: String,
    action: String,
    deps: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct CatalogueEntry {
    name: String,
    /// `template` or `example`; absent on an entry the judge refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'static str>,
    purpose: String,
    inputs: Vec<CatalogueInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    teaches: Option<Teaches>,
    steps: Vec<CatalogueStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refused: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CatalogueReading {
    entries: Vec<CatalogueEntry>,
    /// Where each destination would write, or `None` when none is in sight.
    workspace: Option<String>,
    home: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MadeFlow {
    flow: String,
    origin: &'static str,
    directory: String,
}

/// Sentences come in the language this process speaks, the window's own.
#[tauri::command]
pub(crate) fn flow_catalogue() -> CatalogueReading {
    reading_of(&ui::gather::flow_sources(), catalogue::language())
}

#[tauri::command]
pub(crate) fn flow_from_catalogue(
    entry: String,
    name: String,
    inputs: BTreeMap<String, String>,
    place: String,
) -> Result<MadeFlow, String> {
    make_in(&ui::gather::flow_sources(), &entry, &name, &inputs, &place)
}

fn reading_of(sources: &[FlowSource], language: &str) -> CatalogueReading {
    let place = |destination| {
        starters::place(sources, destination)
            .ok()
            .map(|source| source.dir.display().to_string())
    };
    CatalogueReading {
        entries: starters::all(&sailor::flow_cmd::from_catalogue::full_judge)
            .into_iter()
            .map(|(name, judged)| entry_of(name, judged, language))
            .collect(),
        workspace: place(Destination::Workspace),
        home: place(Destination::Home),
    }
}

fn entry_of(name: &str, judged: Result<starters::Listed, Vec<String>>, language: &str) -> CatalogueEntry {
    let listed = match judged {
        Ok(listed) => listed,
        Err(refused) => {
            return CatalogueEntry {
                name: name.to_owned(),
                kind: None,
                purpose: String::new(),
                inputs: Vec::new(),
                teaches: None,
                steps: Vec::new(),
                refused: Some(refused.join("\n")),
            }
        }
    };
    let entry = listed.entry;
    let steps = entry
        .flow
        .pointer("/graph/steps")
        .and_then(serde_json::Value::as_array)
        .map(|steps| {
            steps
                .iter()
                .map(|step| CatalogueStep {
                    id: text_at(step, "id"),
                    action: text_at(step, "action"),
                    deps: step
                        .get("deps")
                        .and_then(serde_json::Value::as_array)
                        .map(|deps| deps.iter().filter_map(|dep| dep.as_str().map(str::to_owned)).collect())
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    CatalogueEntry {
        name: entry.entry,
        kind: Some(match entry.kind {
            Kind::Template => "template",
            Kind::Example => "example",
        }),
        purpose: starters::said(language, &entry.purpose),
        inputs: entry
            .inputs
            .into_iter()
            .map(|(name, input)| CatalogueInput {
                name,
                means: starters::said(language, &input.means),
                required: input.required,
            })
            .collect(),
        teaches: entry.teaches.map(|teaches| Teaches {
            capability: starters::said(language, &teaches.capability),
            result: starters::said(language, &teaches.result),
        }),
        steps,
        refused: None,
    }
}

fn text_at(step: &serde_json::Value, field: &str) -> String {
    step.get(field).and_then(serde_json::Value::as_str).unwrap_or_default().to_owned()
}

fn make_in(
    sources: &[FlowSource],
    entry: &str,
    name: &str,
    inputs: &BTreeMap<String, String>,
    place: &str,
) -> Result<MadeFlow, String> {
    let destination = match place {
        "workspace" => Destination::Workspace,
        "home" => Destination::Home,
        other => return Err(catalogue::say("window.catalogue.unknown_place", &[("place", other)])),
    };
    let made = sailor::flow_cmd::from_catalogue::made_from_catalogue(sources, entry, name, inputs, destination)?;
    Ok(MadeFlow {
        flow: made.flow,
        origin: made.origin,
        directory: made.dir.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(line: u32) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sailor-window-catalogue-{}-{line}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    #[test]
    fn the_reading_names_every_entry_with_its_inputs_and_its_steps() {
        let home = scratch(line!());
        let sources = vec![
            FlowSource::builtin(),
            FlowSource { origin: flow::system::YOUR_ORIGIN, dir: home.clone() },
        ];

        let reading = reading_of(&sources, "en");

        assert_eq!(reading.entries.len(), starters::ENTRIES.len());
        assert!(reading.entries.iter().all(|entry| entry.refused.is_none() && !entry.steps.is_empty()));
        let template = reading.entries.iter().find(|entry| entry.kind == Some("template")).expect("a template");
        assert!(template.inputs.iter().any(|input| input.required));
        assert_eq!(reading.workspace, None, "no project in sight");
        assert_eq!(reading.home, Some(home.display().to_string()));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_reading_in_italian_carries_the_italian_sentences_of_every_entry() {
        let sources = vec![FlowSource::builtin()];

        let italian = reading_of(&sources, "it");
        let english = reading_of(&sources, "en");

        for (read, other) in italian.entries.iter().zip(&english.entries) {
            let (_, text) = starters::ENTRIES.iter().find(|(name, _)| *name == read.name).expect("an entry");
            let entry: starters::Entry = serde_json::from_str(text).expect("it reads");
            assert_eq!(read.purpose, starters::said("it", &entry.purpose));
            assert_ne!(read.purpose, other.purpose, "«{}» is not said in Italian", read.name);
            for (input, declared) in read.inputs.iter().zip(entry.inputs.values()) {
                assert_eq!(input.means, starters::said("it", &declared.means));
            }
            if let (Some(said), Some(declared)) = (&read.teaches, &entry.teaches) {
                assert_eq!(said.capability, starters::said("it", &declared.capability));
                assert_eq!(said.result, starters::said("it", &declared.result));
            }
        }
    }

    #[test]
    fn a_flow_is_made_where_the_window_says_and_an_unknown_place_is_refused() {
        let home = scratch(line!());
        let sources = vec![
            FlowSource::builtin(),
            FlowSource { origin: flow::system::YOUR_ORIGIN, dir: home.clone() },
        ];

        let made = make_in(&sources, "a-check-that-stops-the-run", "checked-here", &BTreeMap::new(), "home")
            .expect("made at home");
        let nowhere = make_in(&sources, "a-check-that-stops-the-run", "checked-there", &BTreeMap::new(), "workspace")
            .expect_err("no project in sight");
        let unknown = make_in(&sources, "a-check-that-stops-the-run", "checked-there", &BTreeMap::new(), "moon")
            .expect_err("no such place");

        assert_eq!(made.origin, flow::system::YOUR_ORIGIN);
        assert!(home.join("checked-here.flow.json").exists());
        assert!(nowhere.contains("no workspace"), "{nowhere}");
        assert!(unknown.contains("moon"), "{unknown}");
        let _ = std::fs::remove_dir_all(&home);
    }
}
