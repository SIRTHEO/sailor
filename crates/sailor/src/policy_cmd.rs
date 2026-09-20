//! `sailor policy`: the delivery policy as the trusted trunk commits it. See
//! `docs/the-delivery-loop.md`.
//!
//! **THE READING ITSELF IS NOT HERE**, it is `workspace::delivery`, which the
//! `delivery_policy` action reads through too.

use crate::Form;
use workspace::delivery::{policy_on_the_trunk, Refusal, DECLARED, GOVERNED};

pub const USAGE: &[Form] = &[Form {
    form: "sailor policy",
    says_key: "",
}];

/// **THE ONE FACT THAT CANNOT LIVE IN THE POLICY FILE**, because the file is
/// read *from* the trunk and so the trunk's name has to arrive first.
const TRUNK: &str = "sailor.trunk";

pub fn run(_args: &[String]) -> i32 {
    match dispatch() {
        Ok(said) => {
            println!("{said}");
            0
        }
        Err(why) => {
            eprintln!("sailor policy: {why}");
            1
        }
    }
}

fn dispatch() -> Result<String, String> {
    let root = workspace::root().map_err(|_| no_policy_on_trunk())?;
    let policy = policy_on_the_trunk(&root).map_err(why_it_refused)?;
    let mut lines = Vec::with_capacity(GOVERNED.len() + DECLARED.len() + 1);
    for (field, word) in GOVERNED
        .iter()
        .zip([&policy.merge, &policy.push, &policy.release])
    {
        lines.push(format!("{field}: {word}"));
    }
    for (field, word) in DECLARED.iter().zip([&policy.forge, &policy.remote]) {
        lines.push(match word.is_empty() {
            true => format!("{field}: {}", not_declared()),
            false => format!("{field}: {word}"),
        });
    }
    lines.push(policy.read_from);
    Ok(lines.join("\n"))
}

/// The same four cases, in the words of the catalogue.
fn why_it_refused(refusal: Refusal) -> String {
    match refusal {
        Refusal::NoTrunkDeclared => no_trunk_declared(),
        Refusal::UnknownTrunk(trunk) => unknown_trunk(&trunk),
        Refusal::NoPolicyOnTrunk => no_policy_on_trunk(),
        Refusal::NotAutoNorAsk { field, found } => invalid_field(&field, &found),
    }
}

fn no_policy_on_trunk() -> String {
    catalogue::say("cli.policy.no_policy_on_trunk", &[])
}

fn no_trunk_declared() -> String {
    catalogue::say("cli.policy.no_trunk_declared", &[("key", TRUNK)])
}

fn unknown_trunk(trunk: &str) -> String {
    catalogue::say(
        "cli.policy.unknown_trunk",
        &[("trunk", trunk), ("key", TRUNK)],
    )
}

fn not_declared() -> String {
    catalogue::say("cli.policy.not_declared", &[])
}

fn invalid_field(field: &str, value: &str) -> String {
    catalogue::say("cli.policy.invalid_field", &[("field", field), ("value", value)])
}
