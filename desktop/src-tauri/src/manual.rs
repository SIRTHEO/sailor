//! The command line manual, read from the binary instead of copied out.
//!
//! **WHY THIS FILE HOLDS NO COMMAND.** Sailor has ten commands and some thirty
//! forms; writing them in TypeScript would have been half an hour of work and a
//! page that diverges from the binary at the first option added. It is fault 10
//! — the same truth in more than one place — which in this repo has already
//! come back five times, the last on the vocabulary of the actions, where the
//! window offered six names the engine did not know and refused five it did
//! execute.
//!
//! So `crates/sailor` became lib+bin, exposes `COMMANDS`, and here the shape
//! alone is translated: from `&'static str` to JSON. When a command is born, or
//! a usage line changes, this page says so with nobody touching it — and when a
//! command disappears, it disappears from here too.

use serde::Serialize;

/// A command as the window reads it: without the pointer to the function, the
/// single thing in `sailor::Command` that does not cross the bridge.
#[derive(Serialize)]
pub struct CommandDoc {
    /// The name that gets typed: `flow`, `step`, `release`.
    pub name: &'static str,
    /// One line saying what it is for, the same one `sailor --help` prints and
    /// from the same place: the table of commands carries the key rather than
    /// the sentence, so the window shows it in the language of whoever is
    /// looking instead of the one it was written in.
    pub description: String,
    /// The forms, each already split in two: the window lays them out as a
    /// table, and a table wants its columns apart.
    pub usage: Vec<FormDoc>,
}

/// One form as the window reads it.
///
/// **SPLIT IN TWO BECAUSE THE HALVES ARE NOT THE SAME THING.** Joined in one
/// line, as they were until today, the window would have had to cut them apart
/// on a double space — a second idea of where a form ends.
#[derive(Serialize)]
pub struct FormDoc {
    /// What is typed: `sailor flow run <name> [mandate]`. Never translated.
    pub form: &'static str,
    /// What it does, in the language of whoever is reading. Empty when the
    /// shape says it all on its own.
    pub says: String,
}

/// The commands this Sailor can execute, in the order the binary lists them.
/// The order is not alphabetical and that is deliberate: it is the order of the
/// table, which puts the commands actually used daily at the top.
#[tauri::command]
pub(crate) fn manual() -> Vec<CommandDoc> {
    sailor::COMMANDS
        .iter()
        .map(|command| CommandDoc {
            name: command.name,
            description: catalogue::say(command.description_key, &[]),
            usage: command
                .usage
                .iter()
                .map(|form| FormDoc {
                    form: form.form,
                    says: if form.says_key.is_empty() {
                        String::new()
                    } else {
                        catalogue::say(form.says_key, &[])
                    },
                })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **THE PAGE CANNOT BE SHORTER THAN THE BINARY.**
    ///
    /// The strong guarantee is by construction — this function maps `COMMANDS`
    /// and has nowhere to write a name — but a `filter` added someday to "hide
    /// the internal commands" would break it in silence, and the window would
    /// show an incomplete manual without saying so.
    #[test]
    fn the_manual_carries_every_command_the_binary_has() {
        let manual = manual();
        assert_eq!(
            manual.len(),
            sailor::COMMANDS.len(),
            "the window's manual has {} commands, the binary has {}",
            manual.len(),
            sailor::COMMANDS.len()
        );
        for (page, command) in manual.iter().zip(sailor::COMMANDS) {
            assert_eq!(page.name, command.name);
            assert!(
                !page.usage.is_empty(),
                "'{}' reaches the window without saying how it is written",
                page.name
            );
        }
    }

    /// What crosses the bridge is JSON, and a field that fails to serialize is
    /// found here instead of in front of an empty page.
    ///
    /// **THE USAGE FORM IS NO LONGER COPIED BY HAND, and the reason is a real
    /// fault.** This line used to look for `sailor flow run <nome> [mandato]`,
    /// written here by hand. When the command line moved to English the string
    /// turned false and the test red — but **nobody saw it**, because
    /// `desktop/src-tauri` declares a `[workspace]` of its own and
    /// `cargo test --workspace` will not compile it: the tests of this shell sit
    /// outside the gate. An invisible red is worse than a green.
    ///
    /// It looks for no phrase any more: it **counts**. What this test must
    /// defend is that the fields cross the bridge, not which words they hold —
    /// the words are guarded where they are born, and asking `COMMANDS` for
    /// them would compare a source with itself, since that is where `manual()`
    /// takes them. *Mutant*: `#[serde(skip)]` on `usage` — "the binary declares
    /// 34 usage forms and 0 of them cross the bridge".
    #[test]
    fn the_manual_crosses_the_bridge_as_json() {
        let json = serde_json::to_string(&manual()).expect("the manual serializes");
        assert!(json.contains("\"flow\""), "the flow command is missing");

        // **IT COUNTS, IT DOES NOT LOOK FOR A PHRASE.** `manual()` is born of
        // `COMMANDS`, so looking for a word here would compare a source with
        // itself: it would stay green regardless, a copy confirming itself.
        // What this test can really lose is **a field that does not cross the
        // bridge** — a serde `skip`, a type that will not serialize — and that
        // shows by counting: were `usage` to stop passing, the count falls to
        // zero while the binary declares some thirty.
        let declared: usize = sailor::COMMANDS
            .iter()
            .map(|command| command.usage.len())
            .sum();
        let arrived = json.matches("\"form\":").count();
        assert_eq!(
            arrived, declared,
            "the binary declares {declared} usage forms and {arrived} of them cross the bridge: {json}"
        );
        assert!(
            json.contains("\"description\"") && json.contains("\"usage\""),
            "a field does not cross the bridge: {json}"
        );
    }
}
