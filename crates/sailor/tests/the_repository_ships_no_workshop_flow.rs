//! What a person downloads is the product, not the workshop that builds it.
//!
//! `flows/` in the repository is the *project* source, and a project's flow
//! wins over the same name in someone's home. So a development flow left here
//! does not merely travel with the product: it takes precedence over the one
//! its owner wrote.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The flows the product is meant to hand out from `flows/`.
///
/// Empty on purpose. Shipped flows live in `flow::system`, compiled into the
/// binary; this directory is for example templates, and none is written yet.
/// Adding a name here is a decision about what the product hands out, so it is
/// made once, in a place a reader can find.
const TEMPLATES_THE_PRODUCT_HANDS_OUT: &[&str] = &[];

/// Where a flow that is not a template belongs: a person's own home, which has
/// no remote and travels with nobody.
const WHERE_THEY_BELONG: &str = "~/.config/sailor/flows/";

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("the crate lives in <root>/crates/sailor")
        .to_path_buf()
}

fn flow_files(directory: &Path) -> BTreeSet<String> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        // No directory is the honest end state, not a failure to report.
        return BTreeSet::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|name| name.strip_suffix(".flow.json").map(str::to_owned))
        .collect()
}

/// Nothing of ours ships in `flows/`.
///
/// Born red with nine: the development cycle, the publishing flow, the research
/// flows and two fixtures, all of them ours. They were taken out once and came
/// back inside a merge, which is why this is a check and not a note.
#[test]
fn the_repository_ships_no_flow_of_ours() {
    let here = flow_files(&repository().join("flows"));
    let allowed: BTreeSet<String> = TEMPLATES_THE_PRODUCT_HANDS_OUT
        .iter()
        .map(|name| (*name).to_owned())
        .collect();

    let ours: Vec<&String> = here.difference(&allowed).collect();

    assert!(
        ours.is_empty(),
        "«flows/» carries {} flow(s) the product does not hand out: {:?}. A flow \
         in the repository is the project's, and it wins over the same name in \
         its owner's home - so this is not only shipping our workshop, it is \
         overriding theirs. Move them to {WHERE_THEY_BELONG}, or name them in \
         TEMPLATES_THE_PRODUCT_HANDS_OUT if they really are examples to hand out",
        ours.len(),
        ours
    );
}

/// A name listed as a template has to exist, or the list becomes a place where
/// a deleted flow keeps its permission to come back.
#[test]
fn every_template_named_is_really_there() {
    let here = flow_files(&repository().join("flows"));
    for name in TEMPLATES_THE_PRODUCT_HANDS_OUT {
        assert!(
            here.contains(*name),
            "«{name}» is named as a template the product hands out, and there is \
             no «flows/{name}.flow.json»"
        );
    }
}
