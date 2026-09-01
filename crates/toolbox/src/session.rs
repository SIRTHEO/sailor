//! Which engines can resume a session, and with which options.
//!
//! **WHY A FILE OF ITS OWN AND NOT A DESCRIPTOR FIELD.** Two hands on the same
//! JSON file do not give a conflict you can read: they give a file that loads,
//! does not say what one of the two believed, and no compiler shows it. A
//! separate file costs one extra read at startup and can collide with nothing.

use serde::Deserialize;
use std::path::PathBuf;

/// The abilities shipped inside the binary, for the same reason the system flows
/// are: a file next to the program can be missing, and then the product behaves
/// differently on different machines with no visible cause.
const BUILT_IN: &str = include_str!("../descriptors/sessions.json");

/// Where a Sailor user puts their own, without recompiling anything.
const USER_FILE: &str = "sessions.json";

/// What an engine can do with its own sessions, as the file states it.
///
/// Each mode is the **whole** command line — what takes the place of the
/// question's options — with `{session}` where the identifier goes. Resuming is
/// not always one extra flag: two of the four engines measured change
/// *subcommand* (`exec resume <id>`), which an "add these options" model loses.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionAbility {
    /// The tool's identifier, the same one the descriptors use.
    pub tool: String,
    /// How a session is opened **with an identifier we choose**. An engine that
    /// does not let us choose has no entry here: there is no way to find a
    /// session again when you do not know its name.
    #[serde(default)]
    pub open: Option<Vec<String>>,
    #[serde(default)]
    pub resume: Option<Vec<String>>,
    #[serde(default)]
    pub fork: Option<Vec<String>>,
    /// Where the engine says **which session** it has just spoken about. Needed
    /// by the ones that mint the identifier themselves, which is most of them.
    #[serde(default)]
    pub id_from: Option<IdPlace>,
}

/// Where the identifier is written inside what the engine said.
///
/// **TWO SHAPES AND NO MORE, AND THE COPY IS DELIBERATE.**
/// `toolbox::descriptor` already has a type saying the same thing for the usage
/// numbers; using it here would tie this file to that one. Two lines of copy
/// cost less than a contended file.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdPlace {
    /// A regular expression with the identifier in the first group, for engines
    /// that speak in plain text. `codex` needs it: it mints its own identifier,
    /// and the only way `codex exec --help` offers to read one back is `--json`,
    /// which changes the output format — that would break the usage reading its
    /// own descriptor already declares. Hence a pattern over what it printed.
    Pattern(String),
    /// A path of keys, for the ones that answer in JSON.
    Path(Vec<String>),
}

impl IdPlace {
    fn pointer(&self) -> actions::Pointer {
        match self {
            IdPlace::Pattern(pattern) => actions::Pointer::Pattern(pattern.clone()),
            IdPlace::Path(keys) => actions::Pointer::Path(keys.clone()),
        }
    }
}

/// The abilities this machine knows: the shipped ones plus the user's.
#[derive(Debug, Clone, Default)]
pub struct SessionAbilities {
    entries: Vec<SessionAbility>,
}

impl SessionAbilities {
    /// The shipped ones, plus the user's file when there is one.
    ///
    /// **A BADLY WRITTEN USER FILE STOPS NOTHING**, and that is not indulgence:
    /// the only consequence of ignoring it is that those engines start from
    /// scratch, which is exactly how things worked before. Breaking Sailor's
    /// startup over crooked JSON in an optional file would cost far more.
    pub fn current() -> Self {
        let mut abilities = Self::shipped();
        if let Some(path) = user_file() {
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Ok(theirs) = serde_json::from_str::<Vec<SessionAbility>>(&text) {
                    abilities.absorb(theirs);
                }
            }
        }
        abilities
    }

    /// Only the ones shipped inside the binary.
    pub fn shipped() -> Self {
        Self {
            entries: serde_json::from_str(BUILT_IN)
                .expect("the shipped abilities are data of this repository"),
        }
    }

    /// A list decided by the caller: that is how a test checks the translation
    /// without depending on what is shipped.
    pub fn of(entries: Vec<SessionAbility>) -> Self {
        Self { entries }
    }

    /// **THE USER WINS, ENTRY BY ENTRY.** Same rule as the descriptors: writing
    /// an entry at home for a tool we ship says that on their engine the options
    /// are different — maybe they have another version — and theirs is right
    /// there.
    fn absorb(&mut self, theirs: Vec<SessionAbility>) {
        for ability in theirs {
            self.entries.retain(|mine| mine.tool != ability.tool);
            self.entries.push(ability);
        }
    }

    /// What the tool with this identifier can do. `None` for one nobody declared,
    /// nearly all of them. **AND IT STAYS DATA, NOT AN `if` BRANCH:** no engine
    /// name appears in this module, whoever adds one writes an entry, and whoever
    /// has none keeps working by starting from scratch. It is the permanent
    /// constraint "model independence" — a capability that holds on one engine
    /// only is declared as that engine's capability.
    pub fn for_tool(&self, id: &str) -> Option<actions::SessionRecipe> {
        let ability = self.entries.iter().find(|entry| entry.tool == id)?;
        Some(actions::SessionRecipe {
            open: ability.open.clone(),
            resume: ability.resume.clone(),
            fork: ability.fork.clone(),
            id_from: ability.id_from.as_ref().map(IdPlace::pointer),
        })
    }
}

fn user_file() -> Option<PathBuf> {
    Some(ledger::sailor_home()?.join(USER_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **WHAT IS SHIPPED MUST BE TRUE ON THE MACHINE IT RUNS ON.** `claude-code`
    /// is shipped because it is the only one of the four that closes the loop
    /// with an identifier we choose. This test cannot check the help of a binary
    /// that may not be there, but it holds the shape: drop `--fork-session` from
    /// the "fork" entry and forking becomes a resume, so three parallel steps
    /// would write over each other.
    #[test]
    fn the_shipped_engine_declares_all_three_moves() {
        let shipped = SessionAbilities::shipped();

        let recipe = shipped.for_tool("claude-code").expect("it is shipped");

        assert_eq!(
            recipe.open.as_deref(),
            Some(["-p", "--session-id", "{session}"].map(str::to_owned).as_slice())
        );
        assert_eq!(
            recipe.resume.as_deref(),
            Some(["-p", "--resume", "{session}"].map(str::to_owned).as_slice())
        );
        assert_eq!(
            recipe.fork.as_deref(),
            Some(
                ["-p", "--resume", "{session}", "--fork-session"]
                    .map(str::to_owned)
                    .as_slice()
            )
        );
    }

    /// An engine nobody declared does not become capable by guesswork.
    ///
    /// `agy` is absent although it *can* resume — `--conversation <id>` — because
    /// it mints the identifier itself with no way to read it back, and cannot
    /// fork. `gemini` is absent for the mirror reason: its `--resume` wants
    /// "latest" or an **index**. Both start from scratch, and pay more for it.
    #[test]
    fn an_engine_nobody_declared_can_do_nothing() {
        assert!(SessionAbilities::shipped().for_tool("agy").is_none());
    }

    /// **AN ENGINE THAT DOES NOT LET US NAME A SESSION MUST SAY WHERE IT WRITES
    /// THE NAME.** An entry that opens a session with no placeholder and no
    /// `id_from` opens a conversation nobody can find again: the next step would
    /// resume nothing, and would only notice after spending. Checked against
    /// `codex exec --help` and against a real run: `codex` has no option at all
    /// for imposing an identifier, and it prints the one it minted.
    #[test]
    fn an_engine_that_mints_its_own_name_declares_where_it_writes_it() {
        for ability in SessionAbilities::shipped().entries {
            let Some(open) = &ability.open else { continue };
            let ours = open
                .iter()
                .any(|arg| arg.contains(actions::SESSION_PLACEHOLDER));
            assert!(
                ours || ability.id_from.is_some(),
                "«{}» opens a session nobody will be able to find again",
                ability.tool
            );
        }
    }

    /// The `codex` pattern must recognise its real output, not one that looks
    /// like it: written badly, the identifier would stay unknown for ever and
    /// nobody would notice. The text below is copied from a real run.
    #[test]
    fn the_shipped_pattern_finds_the_identifier_in_the_real_output() {
        let recipe = SessionAbilities::shipped()
            .for_tool("codex")
            .expect("codex is shipped");
        let pointer = recipe
            .id_from
            .expect("and declares where it writes its own name");
        let said = "model: gpt-5.6-sol\nprovider: openai\napproval: never\n\
                    session id: 01a057e8-f849-79c1-84f8-9de1f4f758b8\n--------\nuser\n";

        assert_eq!(
            actions::read_text(said, &pointer).as_deref(),
            Some("01a057e8-f849-79c1-84f8-9de1f4f758b8")
        );
    }

    /// A mode that is not declared stays absent **while the other two work**:
    /// the case of an engine that can resume and cannot fork, which without this
    /// asymmetry would have to give up what it can do as well.
    #[test]
    fn a_move_that_is_not_declared_stays_absent_without_taking_the_others_with_it() {
        let abilities = SessionAbilities::of(vec![SessionAbility {
            tool: "resume-only".to_owned(),
            open: None,
            resume: Some(vec!["--conversation".to_owned(), "{session}".to_owned()]),
            fork: None,
            id_from: None,
        }]);

        let recipe = abilities.for_tool("resume-only").expect("it is declared");

        assert!(recipe.open.is_none());
        assert!(recipe.fork.is_none());
        assert_eq!(recipe.resume.expect("this one is there").len(), 2);
    }
}
