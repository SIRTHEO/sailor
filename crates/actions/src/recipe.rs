//! How a tool is resolved and asked: the resolver an action registry hands
//! in, and the recipes a descriptor declares for asking, measuring and
//! carrying a session.

use crate::probe::LoginRecipe;
use crate::{Declared, Pointer};

/// How «I want *this* tool» becomes the executable that is it here.
///
/// **WHY A TRAIT AND NOT A CALL.** `toolbox` knows which tools a machine has,
/// and it depends on this crate: calling it from here would close a loop. The
/// deeper reason comes first: a flow must not know *how* a tool is looked for.
/// Whoever composes the action registry chooses — descriptors read where Sailor
/// runs, an answer without the disk in a test — and the flow stays one file.
pub trait ToolResolver: Send + Sync {
    /// The path of the executable that is `id` on this machine, or why it
    /// cannot be used, written for a person: that text lands inside the red
    /// step, and is all the reader will have.
    fn resolve(&self, id: &str) -> Result<String, String>;

    /// How a one-shot question is put to `id`, when its descriptor says.
    ///
    /// **WHY THE STEP MUST NOT KNOW.** While an engine's options are written
    /// inside a step — `-p` for one, `--mode plan --print` for another — that
    /// step is bound to that engine, and a «model-independent» flow is one in
    /// name only. Six steps out of six naming the same engine is what killed a
    /// flow the day that engine ran out of quota, with another engine,
    /// installed and live, never tried.
    ///
    /// Whoever does not declare it gets `None`, and the step must say the
    /// options itself: it works worse, but never silently.
    fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
        None
    }

    /// Whether what is sent to `id` trains its provider's next model. The
    /// default is what nobody measured, and a private step reads it as a no.
    fn data_pact(&self, _id: &str) -> models::pact::DataPact {
        models::pact::DataPact::Unknown
    }

    /// The subscription windows of `id` as fuel, read now; empty when it
    /// declares no channel or the reading failed.
    fn fuel(&self, _id: &str) -> Vec<models::fuel::Fuel> {
        Vec::new()
    }

    /// How **this** engine opens, resumes and forks a session, when it can.
    ///
    /// **THE DEFAULT IS `None`, AND THAT `None` IS THE PERMANENT CONSTRAINT.**
    /// An engine that cannot resume becomes neither an error nor an `if` branch
    /// written for it: it gets the usual command line, starts over, and pays
    /// more. It is the shape «model independence» takes here — a capability is
    /// a fact about whoever declares it, not a constant beside the code.
    fn session_recipe(&self, _id: &str) -> Option<SessionRecipe> {
        None
    }

    /// How `id` is asked whether the home it starts from is authenticated.
    ///
    /// **`None` MEANS «NOBODY LOOKED», NEVER «IT IS AUTHENTICATED».** Whoever
    /// does not declare it raises no warning, and no reassurance either: the
    /// check is silent on that engine, and the reader sees it is silent. Same
    /// rule as `refuses_without_prompt`, and the direction matters — a default
    /// saying yes would silence the condition this channel exists to show.
    fn login_recipe(&self, _id: &str) -> Option<LoginRecipe> {
        None
    }

    /// The options a model's name is written after, when `id`'s descriptor
    /// says how it is told one.
    ///
    /// **`None` IS A REFUSAL, NOT A DEFAULT**, the other way round from
    /// `session_recipe`: there the step asked for nothing, here it asked, and
    /// answering from a model nobody chose would say so to nobody.
    fn model_option(&self, _id: &str) -> Option<Vec<String>> {
        None
    }

    /// How `id` is told the most one call may spend, when its descriptor says.
    ///
    /// **`None` MEANS NO CEILING CAN BE IMPOSED ON IT**, and that is what turns
    /// a cap over this engine into a stop threshold. It is not a reason to
    /// refuse the engine: it is a reason to stop calling the cap a guarantee.
    fn spend_ceiling_option(&self, _id: &str) -> Option<crate::reserve::CeilingOption> {
        None
    }
}

/// The placeholder standing in for the session identifier inside a session
/// recipe's options.
///
/// It lives here and not in `toolbox` because it is **the contract between the
/// two**: a capability file and the code assembling the command line must name
/// the same thing, and twin constants in two crates diverge on first edit.
pub const SESSION_PLACEHOLDER: &str = "{session}";

/// What an engine can do with its own sessions, as options already written.
///
/// **EVERY MODE CARRIES THE WHOLE LINE, NOT THE EXTRA OPTIONS.** It looks like
/// a duplicate of `AskRecipe::args` and is not: on `codex`, resuming is not an
/// added option but **a different subcommand** — `codex exec resume <id>`
/// against `codex exec` — and forking is a third one, `codex exec fork <id>`.
/// An «add these options» model could express neither, and would rule both out
/// for good. Verified with `codex exec --help` on this machine.
///
/// What is shared with the ask recipe stays shared: the usage options, and the
/// ones that must stay glued to the question, are appended here as they are
/// appended there, because **measuring must not stop working on a resume** —
/// that would be the most elegant way to lose the very numbers saying whether
/// resuming pays.
///
/// `None` on a mode means the engine cannot do it: it starts over, and pays
/// more.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionRecipe {
    /// Opens a session. If the line holds the placeholder we choose the
    /// identifier; if it does not, the engine mints it and `id_from` reads it
    /// back.
    pub open: Option<Vec<String>>,
    /// Resumes an existing session, which stays the same one.
    pub resume: Option<Vec<String>>,
    /// Forks an existing session: the trunk stays where it is, and this step's
    /// work does not touch it.
    pub fork: Option<Vec<String>>,
    /// Where, in what the engine said, the identifier of the session it
    /// **just used** sits.
    ///
    /// **IT EXISTS BECAUSE MOST ENGINES DO NOT LET THE NAME BE CHOSEN.**
    /// `codex` has no option to impose an identifier, but it **prints** one —
    /// `session id: <uuid>` — in the same text its descriptor already reads
    /// tokens from. Without this route, engines that mint their own would be
    /// shut out for good from a capability they have.
    ///
    /// **AND IT HOLDS AFTER A FORK TOO**, where it pays most: a branch is born
    /// with a new identifier nobody asked for, and reading it is the way a
    /// later step can carry on **that branch** instead of the trunk.
    pub id_from: Option<Pointer>,
}

/// Where the question's text ends up when an engine is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptVia {
    /// On standard input.
    Stdin,
    /// As the last argument of the command line.
    LastArg,
}

/// A recipe's command line, **without** the question's text.
///
/// The order is: the question's options, the ones that ask for the usage, and
/// last the ones that must stay glued to the question.
///
/// **IT SITS OUTSIDE THE PLACE THAT USES IT SO IT CAN BE READ WITHOUT RUNNING
/// ANYTHING.** A wrong order here breaks no compile and no unit test: it shows
/// up by launching the right engine and no other way, which is how fault 1
/// reached production, and how it came back through another door.
pub fn command_line(recipe: &AskRecipe) -> Vec<String> {
    command_line_with(recipe, &recipe.args)
}

/// The same line, with the question's options replaced by others.
///
/// Sessions need it: `codex exec resume <id>` is not `codex exec` with
/// something appended, it is another line. What sits **after** the question's
/// options — the usage, and what must stay glued to the text — does not change,
/// and that is why this function exists instead of letting the caller assemble
/// the line: a resumed engine must go on saying what it spends.
pub fn command_line_with(recipe: &AskRecipe, ask_args: &[String]) -> Vec<String> {
    let mut args = ask_args.to_vec();
    if let Some(usage) = &recipe.usage {
        args.extend(usage.args.iter().cloned());
    }
    args.extend(recipe.args_before_prompt.iter().cloned());
    args
}

/// The same line, with the model the step asked for written on it.
///
/// The model's options follow the question's and stay **before** the ones glued
/// to the text, for the reason `args_before_prompt` exists: a name slipped
/// between the flag introducing the question and the question is read as it.
pub fn command_line_naming_model(
    recipe: &AskRecipe,
    option: &[String],
    model: &str,
) -> Vec<String> {
    command_line_naming_model_and_ceiling(recipe, Some((option, model)), None)
}

/// The same line, with the ceiling this call is held to written on it too.
///
/// The ceiling sits where the model's name sits, and for the same reason. It is
/// the one option Sailor adds that a person's money depends on: a run that
/// reserves a maximum and does not impose it holds a figure meaning nothing.
pub fn command_line_naming_model_and_ceiling(
    recipe: &AskRecipe,
    model: Option<(&[String], &str)>,
    ceiling: Option<(&[String], &str)>,
) -> Vec<String> {
    let mut ask_args = recipe.args.clone();
    for (option, value) in [model, ceiling].into_iter().flatten() {
        ask_args.extend(option.iter().cloned());
        ask_args.push(value.to_owned());
    }
    command_line_with(recipe, &ask_args)
}

/// How an engine is asked in one shot, and how it says it **cannot work**.
#[derive(Clone, Debug)]
pub struct AskRecipe {
    /// The options that ask for a one-shot question, without its text.
    pub args: Vec<String>,
    /// Where the question's text goes.
    pub prompt: PromptVia,
    /// The options that must stay **glued to the question**, after the usage
    /// ones. Empty for nearly every engine; see `Ask::args_before_prompt`.
    pub args_before_prompt: Vec<String>,
    /// The fragments that, appearing in a failure's output, say **this engine
    /// could not work** — quota spent, credentials missing — and not that the
    /// work itself was wrong.
    ///
    /// **THE DISTINCTION IS EVERYTHING.** Moving to the next engine on every
    /// failure would be the worst thing: a badly written brief would walk down
    /// the chain to a model that answers anyway, and the wrong answer would
    /// arrive with nobody knowing why. The chain moves on **only** when the
    /// engine declared it could not work, and on the words its descriptor
    /// declares: whoever declares none triggers no fallback.
    pub unusable_when: Vec<String>,
    /// The words that mean the quota is spent, and how long to set the engine
    /// aside when they appear. Empty and `None` when the descriptor does not
    /// tell a spent quota from a missing credential.
    pub exhausted_when: Vec<String>,
    pub cooldown_secs: Option<u64>,
    /// The words after which the engine only waits for a person; on seeing
    /// one the step stops it instead of paying the wait. Empty: waited in full.
    pub waits_for_a_person_when: Vec<String>,
    /// Measured: without a question it exits quietly with an empty stdout
    /// instead of refusing in words.
    pub silent_without_prompt: bool,
    /// The fragments this engine refuses a line **assembled without the
    /// question** with: «the line was fine, the text was missing».
    ///
    /// It travels with the recipe and not beside it, because it is needed
    /// exactly where the line is: whoever assembles `command_line` for a dry
    /// run must judge the answer without asking the catalogue again. Empty
    /// means «nobody looked», never «the line is sound».
    pub refuses_without_prompt: Vec<String>,
    /// How **what it spent** is read, when its descriptor declares it.
    ///
    /// It travels the same road as the rest of the recipe: a descriptor
    /// declares it once, and no flow need know it. `None` is the answer from
    /// whoever declares nothing, and is no fault: that engine is invoked as
    /// before, and its tokens stay unknown.
    pub usage: Option<UsageRecipe>,
}

/// The options to add to be told the usage, and where to read it.
#[derive(Clone, Debug)]
pub struct UsageRecipe {
    pub args: Vec<String>,
    pub declared: Declared,
}

/// Whether this output holds one of the declared words. The comparison ignores
/// case: no provider promises not to change it. An empty fragment does not
/// count — it would match everything, turning any output into a hit.
///
/// **IT LIVES HERE IN A SINGLE COPY** because the two lists a descriptor
/// declares — «I cannot work» and «the question was missing» — are read in
/// exactly the same way. Twin functions would diverge on the first detail
/// somebody changes in one of them, and that is fault 10.
pub(crate) fn mentions_any(marks: &[String], output: &str) -> bool {
    let output = output.to_lowercase();
    marks
        .iter()
        .any(|mark| !mark.trim().is_empty() && output.contains(&mark.to_lowercase()))
}

/// Whether this output is how an engine says it cannot work.
pub(crate) fn says_it_cannot_work(marks: &[String], output: &str) -> bool {
    mentions_any(marks, output)
}
