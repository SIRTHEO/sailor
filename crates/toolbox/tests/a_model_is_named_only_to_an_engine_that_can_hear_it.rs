//! How a descriptor says which model an engine is to answer with, and what
//! answers when it does not say.

use toolbox::{CapabilityState, Catalog, Source, CHOOSE_MODEL};

fn scratch(name: &str) -> std::path::PathBuf {
    let directory =
        std::env::temp_dir().join(format!("sailor-un-modello-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("working directory");
    directory
}

fn loaded(name: &str, text: &str) -> Catalog {
    let file = scratch(name).join("descriptors.json");
    std::fs::write(&file, text).expect("write the descriptors");
    Catalog::load(&[Source::File(file)])
}

/// **DECLARING THE CAPABILITY IS NOT SAYING HOW TO USE IT.** A `true`, and a
/// form without a value, leave nowhere to write the name: whoever composes the
/// command line must read that, not a yes.
#[test]
fn a_capability_without_a_place_for_the_name_cannot_carry_a_model() {
    let catalog = loaded(
        "how-a-model-is-named",
        r#"[
          { "id": "col-valore", "family": "ai_cli", "detect": { "command": "col-valore" },
            "capabilities": { "choose_model": { "args": ["--model"], "takes_value": true } } },
          { "id": "senza-valore", "family": "ai_cli", "detect": { "command": "senza-valore" },
            "capabilities": { "choose_model": { "args": ["--model"] } } },
          { "id": "solo-un-si", "family": "ai_cli", "detect": { "command": "solo-un-si" },
            "capabilities": { "choose_model": true } },
          { "id": "muto", "family": "ai_cli", "detect": { "command": "muto" } }
        ]"#,
    );
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    let option = |id: &str| {
        catalog
            .descriptors
            .iter()
            .find(|loaded| loaded.descriptor.id == id)
            .expect("the descriptor is in the catalog")
            .descriptor
            .model_option()
    };

    assert_eq!(option("col-valore"), Some(vec!["--model".to_owned()]));
    assert_eq!(option("senza-valore"), None, "nowhere to put the name");
    assert_eq!(option("solo-un-si"), None, "a yes is not an instruction");
    assert_eq!(option("muto"), None);
}

/// The shipped engines that can be asked also say how a model is named to
/// them: without that, the flow that exists to take a question to a strong
/// model could ask it of nobody.
#[test]
fn the_shipped_engines_that_can_be_asked_say_how_a_model_is_named_to_them() {
    let catalog = Catalog::load(&[Source::Builtin]);
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    let mut asked = 0;
    for loaded in &catalog.descriptors {
        let descriptor = &loaded.descriptor;
        if descriptor.ask.is_none()
            || descriptor.capability(CHOOSE_MODEL) == CapabilityState::NotLookedAt
        {
            continue;
        }
        asked += 1;
        assert!(
            descriptor.model_option().is_some(),
            "«{}» says it can choose a model and not with which option",
            descriptor.id
        );
    }
    assert!(asked >= 4, "measured {asked} engines, there were four");
}
