//! Gradino 3 of the space-lifecycle design note: what is declared, what it
//! weighs, and what would be proposable — read only. Nothing here removes a
//! byte. `sailor machine free` already acts on its own, on a different rule
//! (whose the ledger says a build directory is); this shows what a person
//! wrote down instead, and waits.

use crate::Form;
use inventory::place::{declared_places, Place};
use std::path::PathBuf;

pub const USAGE: &[Form] = &[Form {
    form: "sailor space",
    says_key: "cli.space.says",
}];

pub fn run(_args: &[String]) -> i32 {
    match reading() {
        Ok(said) => {
            println!("{said}");
            0
        }
        Err(why) => {
            eprintln!("sailor space: {why}");
            2
        }
    }
}

fn places_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/sailor/places.d"))
}

fn reading() -> Result<String, String> {
    let Some(dir) = places_dir() else {
        return Err(catalogue::say("cli.no_home", &[]));
    };
    let places = declared_places(&dir)?;
    if places.is_empty() {
        return Ok(catalogue::say("cli.space.nothing_declared", &[]));
    }
    let total: u64 = places.iter().filter_map(|place| place.weight).sum();
    let proposable = places.iter().filter(|place| place.is_proposable()).count();
    let mut said = catalogue::say(
        "cli.space.summary",
        &[
            ("count", &places.len().to_string()),
            ("gigabytes", &crate::machine_cmd::gigabytes(total)),
            ("proposable", &proposable.to_string()),
        ],
    );
    for place in &places {
        said.push('\n');
        said.push_str(&one_line(place));
    }
    Ok(said)
}

fn one_line(place: &Place) -> String {
    let weight = place
        .weight
        .map_or_else(|| catalogue::say("cli.space.could_not_weigh", &[]), |bytes| {
            format!("{} GB", crate::machine_cmd::gigabytes(bytes))
        });
    let state = if place.is_proposable() {
        catalogue::say("cli.space.state.proposable", &[])
    } else if place.declaration.is_none() {
        catalogue::say("cli.space.state.undeclared", &[])
    } else {
        catalogue::say("cli.space.state.unweighed", &[])
    };
    catalogue::say(
        "cli.space.place",
        &[("name", &place.name), ("weight", &weight), ("state", &state)],
    )
}
