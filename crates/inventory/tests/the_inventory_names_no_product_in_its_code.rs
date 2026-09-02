//! The inventory looks for what a descriptor declares, and knows no name.
//!
//! **THE FAULT THIS CLOSES.** Every path this crate searched had one command
//! line's home compiled in, so a machine holding a different one got "you have
//! nothing" with no check failing — the worst shape a wrong answer takes.

use std::path::{Path, PathBuf};

/// **THE GATE ARMS ITSELF FROM THE DATA.** The names it forbids are the ones the
/// descriptor declares, so this file publishes no list of its own — a ban by
/// list has to write down what it means to keep out.
fn names_that_belong_to_a_product() -> Vec<String> {
    let mut names = Vec::new();
    for product in inventory::extensions::declared(None) {
        names.push(product.home.clone());
        names.push(product.project.clone());
        names.extend(product.settings.clone());
        if !product.installed_plugins.is_empty() {
            names.push(product.installed_plugins.clone());
        }
        if !product.plugin_manifest.is_empty() {
            names.push(product.plugin_manifest.clone());
        }
        for place in product.skills.iter().chain(product.agents.iter()) {
            names.push(place.under.clone());
        }
    }
    names.sort();
    names.dedup();
    names
}

fn sources() -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    found
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, found);
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            found.push(path);
        }
    }
}

/// A comment may say the name — it is prose, and prose is a label. Code may not.
fn is_prose(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//") || trimmed.starts_with('*')
}

#[test]
fn no_line_of_code_carries_a_name_the_descriptor_should_own() {
    let forbidden = names_that_belong_to_a_product();
    assert!(
        !forbidden.is_empty(),
        "the descriptor declares nothing, so this gate would pass by looking at \
         no name at all"
    );
    let mut caught = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (at, line) in text.lines().enumerate() {
            if is_prose(line) {
                continue;
            }
            for name in &forbidden {
                if line.contains(name.as_str()) {
                    caught.push(format!("{}:{}  {}", path.display(), at + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        caught.is_empty(),
        "these lines decide with a product's name instead of asking the \
         descriptor: {caught:#?}"
    );
}

/// **THE SCAN MUST STILL BE READING SOMETHING.** A walk that found no file would
/// pass the test above by having nothing to look at, which is the green of
/// whoever did not look.
#[test]
fn the_scan_still_opens_the_crate_it_is_guarding() {
    let opened = sources();
    assert!(
        opened.len() >= 3,
        "only {} source files found under this crate",
        opened.len()
    );
    assert!(
        opened.iter().any(|path| path.ends_with("discovery.rs")),
        "the file that used to hold the paths is not among the ones opened"
    );
}

/// The survey must be able to say **how many** command lines it looked at, so a
/// report never reads as "you have nothing" when the truth is "nobody said where
/// to look" — the distinction this crate exists to keep.
#[test]
fn the_inventory_can_say_how_many_command_lines_it_looked_at() {
    assert!(inventory::extensions::how_many_declared(None) >= 1);
}
