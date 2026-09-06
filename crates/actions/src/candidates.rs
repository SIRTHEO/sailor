//! Which engines a step may try here, in which order, and why the others are
//! set aside before anything is spent.

use crate::cost::now_secs;
use crate::engine::ExternalEngineAction;
use crate::equipment::current_equipment_for;
use crate::recipe::{
    command_line_naming_model_and_ceiling, mentions_any, says_it_cannot_work, PromptVia,
    SessionRecipe, ToolResolver,
};
use crate::session::session_lines;
use crate::spec::{ceiling_of, DataClass, EngineSpec};
use crate::{budget, cooldown, reserve, Declared, EXTERNAL_ENGINE_ACTION};
use flow::ActionError;
use std::path::PathBuf;

impl ExternalEngineAction {
    /// The chain for this step: the strengths table's engines for its kind
    /// first, then the chain as the flow wrote it; a kind without a row, or
    /// a step without a kind, is the chain as written. Then, under
    /// `prefer: fuel`, the engine whose window expires unused soonest moves
    /// to the front, with the why.
    fn ordered(
        &self,
        tools: &dyn ToolResolver,
        spec: &EngineSpec,
        chain: &[String],
    ) -> (Vec<String>, Option<models::fuel::Preference>) {
        let mut ordered: Vec<String> = match spec.kind.as_deref() {
            Some(kind) => self.strengths_table().first_for(kind).to_vec(),
            None => Vec::new(),
        };
        for id in chain {
            if !ordered.contains(id) {
                ordered.push(id.clone());
            }
        }
        if spec.prefer.as_deref() != Some("fuel") {
            return (ordered, None);
        }
        let fuels: Vec<models::fuel::Fuel> = ordered.iter().flat_map(|id| tools.fuel(id)).collect();
        let preferred = models::fuel::prefer(&fuels);
        if let Some(preference) = &preferred {
            if let Some(at) = ordered.iter().position(|id| *id == preference.engine) {
                let first = ordered.remove(at);
                ordered.insert(0, first);
            }
        }
        (ordered, preferred)
    }

    /// The engines the strengths table puts first for this step's kind that
    /// are not usable here: what the step falls back from. Empty without a
    /// declared kind, without a row for it, or when they are all usable.
    ///
    /// **AN ABSENT ENGINE IS NOT A SILENT FAILURE**: without this the ledger
    /// cannot tell a step that never wanted the local engine from one denied it.
    pub(crate) fn fell_back_from(&self, spec: &EngineSpec, usable: &[Candidate]) -> Vec<String> {
        let Some(kind) = spec.kind.as_deref() else {
            return Vec::new();
        };
        self.strengths_table()
            .first_for(kind)
            .iter()
            .filter(|first| !usable.iter().any(|one| one.id.as_deref() == Some(first.as_str())))
            .cloned()
            .collect()
    }

    fn strengths_table(&self) -> models::strengths::Strengths {
        self.strengths
            .as_deref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| models::strengths::Strengths::parse(&text).ok())
            .unwrap_or_else(models::strengths::Strengths::shipped)
    }

    /// Who to run, in order of preference. `bin` and `tool` do not live
    /// together: two answers to one question would want a precedence, and a
    /// precedence between «the name I wrote» and «what is on the machine» is a
    /// rule nobody remembers at the right moment.
    ///
    /// It also returns the engines that **cannot** be used here, with the
    /// reason: if none is left, that reason is all the reader will have.
    pub(crate) fn candidates(&self, spec: &EngineSpec) -> Result<(Vec<Candidate>, Vec<Refused>), ActionError> {
        // Whoever wrote the options wrote which model in them: a second answer
        // to one question would want a precedence, as `bin` and `tool` would.
        if !spec.model.is_empty() && !spec.args.is_empty() {
            return Err(ActionError::new(
                "invalid_input",
                "the step declares both `args` and `model`: the command line it wrote already \
                 says which model to ask for",
            ));
        }
        match (spec.bin.as_deref(), spec.tool.as_ref()) {
            (Some(bin), None) => Ok((
                vec![Candidate {
                    id: None,
                    bin: bin.to_owned(),
                    args: spec.args.clone(),
                    prompt: PromptVia::Stdin,
                    unusable_when: Vec::new(),
                    exhausted_when: Vec::new(),
                    cooldown_secs: None,
                    waits_for_a_person_when: Vec::new(),
                    declared_usage: None,
                    // A hand-written command has no descriptor: nothing
                    // declares it an engine, and the line is already withheld
                    // by the absent `id`.
                    can_be_asked: false,
                    why: None,
                    session: SessionRecipe::default(),
                    ceiling: None,
                    no_ceiling_because: "the step names a command and not a tool, so no \
                                         descriptor says how a ceiling is written on it"
                        .to_owned(),
                }],
                Vec::new(),
            )),
            (None, Some(choice)) => {
                let Some(tools) = &self.tools else {
                    let first = choice.ids().first().map(String::as_str).unwrap_or("");
                    return Err(ActionError::new(
                        "no_tool_resolver",
                        format!(
                            "il passo chiede lo strumento «{first}», ma questo motore è stato \
                             registrato senza un modo per risolverlo: chi costruisce il registro \
                             deve registrare `{EXTERNAL_ENGINE_ACTION}` con \
                             `ExternalEngineAction::resolving_with(...)`"
                        ),
                    ));
                };
                if let Some(other) = spec.prefer.as_deref().filter(|word| *word != "fuel") {
                    return Err(ActionError::new(
                        "invalid_input",
                        format!("`prefer` knows only «fuel», not «{other}»"),
                    ));
                }
                let (ids, preferred) = self.ordered(tools.as_ref(), spec, choice.ids());
                if ids.is_empty() {
                    return Err(ActionError::new(
                        "invalid_input",
                        "il passo dichiara una catena di motori vuota: serve almeno un \
                         identificativo, o `tool` va tolto del tutto",
                    ));
                }
                let step_said_args = !spec.args.is_empty();
                let mut usable = Vec::new();
                let mut refused = Vec::new();
                for id in &ids {
                    let bin = match tools.resolve(id) {
                        Ok(bin) => bin,
                        Err(reason) => {
                            refused.push(Refused {
                                id: id.clone(),
                                reason,
                                unresolved: true,
                            });
                            continue;
                        }
                    };
                    // An engine set aside for a spent quota is not knocked on
                    // again before its time: the refusal says until when, and
                    // what it said, so the chain goes on with the others.
                    if let Some(aside) = self
                        .cooldowns
                        .as_deref()
                        .and_then(|path| cooldown::set_aside_until(path, id, now_secs()))
                    {
                        refused.push(Refused {
                            id: id.clone(),
                            reason: format!(
                                "set aside until {} after saying its quota was spent: «{}»",
                                aside.until, aside.said
                            ),
                            unresolved: false,
                        });
                        continue;
                    }
                    // The pact first: it is permanent, and a cap that would
                    // be named instead suggests raising it would help.
                    let pact = tools.data_pact(id);
                    if spec.data == Some(DataClass::Private) && !pact.accepts_private() {
                        refused.push(Refused {
                            id: id.clone(),
                            reason: format!(
                                "a private step does not go to an engine whose data pact is «{pact}»"
                            ),
                            unresolved: false,
                        });
                        continue;
                    }
                    // A cap on a window excludes, and never reorders: the sum
                    // is the ledger's, over every run of this engine.
                    if let Some(why) = self.over_budget(id) {
                        refused.push(Refused {
                            id: id.clone(),
                            reason: why,
                            unresolved: false,
                        });
                        continue;
                    }
                    if let Some(why) = current_equipment_for(&bin, &spec.env).refused {
                        refused.push(Refused {
                            id: id.clone(),
                            reason: why,
                            unresolved: false,
                        });
                        continue;
                    }
                    // Options written in the step win over the recipe: whoever
                    // wrote them is saying something precise about *this* call,
                    // and overriding them would decide in their place.
                    if step_said_args {
                        let declared = tools.ask_recipe(id);
                        usable.push(Candidate {
                            id: Some(id.clone()),
                            bin,
                            args: spec.args.clone(),
                            prompt: PromptVia::Stdin,
                            // The descriptor says whether this tool is an
                            // engine, even when the options come from
                            // elsewhere: `git` and `cargo` declare no `ask`,
                            // and their runs are not model calls.
                            can_be_asked: declared.is_some(),
                            why: preferred.as_ref().filter(|p| p.engine == *id).map(|p| p.why.clone()),
                            exhausted_when: declared
                                .as_ref()
                                .map(|recipe| recipe.exhausted_when.clone())
                                .unwrap_or_default(),
                            cooldown_secs: declared.as_ref().and_then(|recipe| recipe.cooldown_secs),
                            waits_for_a_person_when: declared
                                .as_ref()
                                .map(|recipe| recipe.waits_for_a_person_when.clone())
                                .unwrap_or_default(),
                            unusable_when: declared
                                .map(|recipe| recipe.unusable_when)
                                .unwrap_or_default(),
                            // **NO USAGE WHEN THE STEP WRITES THE OPTIONS**,
                            // the same rule as two lines above applied to the
                            // new datum: usage options append to the recipe's,
                            // and here the recipe dictates nothing. Appending
                            // them anyway would extend, behind the back of
                            // whoever wrote that command line, a question they
                            // never asked. Usage stays unknown — the ledger
                            // line is written all the same, and says exactly
                            // that.
                            declared_usage: None,
                            session: SessionRecipe::default(),
                            // Nor a ceiling, for the same reason: a person who
                            // wrote their own line did not ask for an option on
                            // it, and this one binds what they may be charged.
                            ceiling: None,
                            no_ceiling_because: "the step writes its own command line, and \
                                                 Sailor adds no option to one"
                                .to_owned(),
                        });
                        continue;
                    }
                    // The model asked of this engine, and the options its
                    // descriptor names one with. An engine declaring none is not
                    // run on its own default: nobody chose that model.
                    let wanted = spec.model.get(id);
                    let option = match wanted {
                        Some(model) => match tools.model_option(id) {
                            Some(option) => Some((option, model)),
                            None => {
                                refused.push(Refused {
                                    id: id.clone(),
                                    reason: format!(
                                        "il passo gli chiede il modello «{model}», e il suo \
                                         descrittore non dichiara come glielo si nomina \
                                         (`capabilities.choose_model`)"
                                    ),
                                    unresolved: false,
                                });
                                continue;
                            }
                        },
                        None => None,
                    };
                    // The ceiling this call is held to, and the reserve it
                    // makes. Both come from the descriptor and the step: an
                    // engine that takes none leaves the run a stop threshold.
                    let held_to = tools.spend_ceiling_option(id);
                    let ceiling = held_to
                        .as_ref()
                        .and_then(|option| reserve::ceiling_for(option, &ceiling_of(spec)));
                    let written = ceiling.as_ref().and_then(reserve::Ceiling::as_written);
                    match tools.ask_recipe(id) {
                        Some(recipe) => usable.push(Candidate {
                            id: Some(id.clone()),
                            bin,
                            args: command_line_naming_model_and_ceiling(
                                &recipe,
                                option
                                    .as_ref()
                                    .map(|(option, model)| (option.as_slice(), model.as_str())),
                                held_to
                                    .as_ref()
                                    .zip(written.as_deref())
                                    .map(|(option, value)| (option.args.as_slice(), value)),
                            ),
                            ceiling,
                            no_ceiling_because: reserve::why_no_ceiling(
                                held_to.as_ref(),
                                &ceiling_of(spec),
                            ),
                            prompt: recipe.prompt,
                            session: session_lines(&recipe, tools.session_recipe(id)),
                            unusable_when: recipe.unusable_when,
                            exhausted_when: recipe.exhausted_when,
                            cooldown_secs: recipe.cooldown_secs,
                            waits_for_a_person_when: recipe.waits_for_a_person_when,
                            declared_usage: recipe.usage.map(|usage| usage.declared),
                            // We are inside the branch that found an `ask`
                            // recipe: this tool is an engine by definition.
                            can_be_asked: true,
                            why: preferred.as_ref().filter(|p| p.engine == *id).map(|p| p.why.clone()),
                        }),
                        None => refused.push(Refused {
                            id: id.clone(),
                            reason: "il passo non dice con quali opzioni interrogarlo e il suo \
                                     descrittore non dichiara come gli si fa una domanda (`ask`)"
                                .to_owned(),
                            unresolved: false,
                        }),
                    }
                }
                Ok((usable, refused))
            }
            (Some(_), Some(_)) => Err(ActionError::new(
                "invalid_input",
                "the step declares both `bin` and `tool`: only one of the two says what to run",
            )),
            (None, None) => Err(ActionError::new(
                "invalid_input",
                "il passo non dice chi eseguire: serve `tool` (l'identificativo di uno strumento, \
                 o una catena di identificativi) oppure `bin` (un comando così com'è)",
            )),
        }
    }
}

/// An engine the step asked for that cannot even be tried here.
pub(crate) struct Refused {
    pub(crate) id: String,
    pub(crate) reason: String,
    /// True when the resolver could not say which executable it is — the
    /// distinction matters: a step asking for **one** engine and not finding
    /// it must give `tool_unavailable` with the resolver's reason, as it always
    /// has. The chain must not make the commonest case worse.
    pub(crate) unresolved: bool,
}

impl Refused {
    pub(crate) fn line(&self) -> String {
        format!("«{}»: {}", self.id, self.reason)
    }
}

/// An engine that can be tried: already resolved to an executable, with the
/// options to ask it with and the words it declares it cannot work with.
pub(crate) struct Candidate {
    /// The identifier, when it was asked for by identifier. `None` when the
    /// step wrote a command as it is.
    pub(crate) id: Option<String>,
    pub(crate) bin: String,
    pub(crate) args: Vec<String>,
    pub(crate) prompt: PromptVia,
    pub(crate) unusable_when: Vec<String>,
    /// The words that mean the quota is spent, and how long to set the engine
    /// aside when they appear; the descriptor's, or empty.
    pub(crate) exhausted_when: Vec<String>,
    pub(crate) cooldown_secs: Option<u64>,
    /// The words after which this engine only waits for a person and is
    /// stopped; the descriptor's, or empty.
    pub(crate) waits_for_a_person_when: Vec<String>,
    /// Where to read the usage in this engine's output. `None` when the
    /// descriptor declares none, or when the step wrote the options.
    pub(crate) declared_usage: Option<Declared>,
    /// **THIS TOOL IS AN ENGINE**: its descriptor declares how a question is
    /// put to it (`ask`).
    ///
    /// It decides whether the invocation goes into `model_calls`. `git` and
    /// `cargo` sit in the catalogue and run from a step like anything else, but
    /// are never asked: they spend no subscription's quota, and counting them
    /// among model calls falsifies every total that sums them. **The criterion
    /// belongs to the descriptor, not to a list of names written here**: a hand
    /// list ages on the first new tool, with no check to say so.
    ///
    /// It stays true when the step writes the options: an engine asked in its
    /// own way is still an engine, and its line is written — with the usage
    /// unknown, which is the right information.
    pub(crate) can_be_asked: bool,
    /// Why this engine was moved to the front, when the fuel said so.
    pub(crate) why: Option<String>,
    /// The ceiling written on this line, when one could be. `None` is what
    /// makes a run's cap a stop threshold rather than a cap.
    pub(crate) ceiling: Option<reserve::Ceiling>,
    /// Why there is no ceiling, for whoever reads a suspended run. It is
    /// written even when there **is** one: composing it costs nothing, and a
    /// reason built only on the unhappy path is a reason nobody ever tested.
    pub(crate) no_ceiling_because: String,
    /// The alternative command lines this engine opens, resumes or forks a
    /// session with — already assembled with the rest of the recipe, and still
    /// carrying the placeholder in place of the identifier.
    ///
    /// Wholly empty for an engine that cannot do it, and for a step that wrote
    /// its own options: whoever writes their own command line is deciding it,
    /// and slipping in an option they did not ask for would decide in their
    /// place — the rule that already holds for the usage options.
    pub(crate) session: SessionRecipe,
}

/// The person's strengths table: `SAILOR_STRENGTHS`, or `strengths.json` in the home.
pub(crate) fn strengths_path() -> Option<PathBuf> {
    match std::env::var_os("SAILOR_STRENGTHS").filter(|value| !value.is_empty()) {
        Some(declared) => Some(PathBuf::from(declared)),
        None => ledger::sailor_home().map(|home| home.join("strengths.json")),
    }
}

impl ExternalEngineAction {
    /// Why `id` is over the cap the person declared for it, if it is: no
    /// file, no cap for it, or no ledger to sum from means it fits. A caps
    /// file that does not read, or a sum that fails, refuses with the reason:
    /// a cap the person wrote is never lifted by a typo.
    fn over_budget(&self, id: &str) -> Option<String> {
        let budgets = match budget::declared(self.budgets.as_deref()?) {
            Ok(budgets) => budgets,
            Err(why) => return Some(format!("its caps cannot be read: {why}")),
        };
        let declared = budgets.get(id)?;
        let now = now_secs();
        let spent = match self.ledger.as_ref()?.spent_by_cli_since(id, now - declared.window_secs) {
            Ok(spent) => spent,
            Err(error) => return Some(format!("its spend cannot be summed: {error}")),
        };
        budget::over(declared, &spent)
    }
}

impl Candidate {
    fn says_it_cannot_work(&self, stdout: &str, stderr: &str) -> bool {
        says_it_cannot_work(&self.unusable_when, stdout)
            || says_it_cannot_work(&self.unusable_when, stderr)
    }

    /// The class of a failure this engine declared: a spent quota is its own
    /// class, anything else it cannot work with is `exhausted` as before, and
    /// an output that says neither is `None`.
    pub(crate) fn declared_class(&self, stdout: &str, stderr: &str) -> Option<&'static str> {
        if mentions_any(&self.exhausted_when, stdout) || mentions_any(&self.exhausted_when, stderr) {
            return Some("quota_exhausted");
        }
        self.says_it_cannot_work(stdout, stderr).then_some("exhausted")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::AskRecipe;
    use crate::tests::with_references_resolved;
    use flow::{Action, ActionOutcome, SharedState};
    use serde_json::json;

    // ── asking for a tool by identifier ───────────────────────────────

    struct FixedTools(&'static str);

    impl ToolResolver for FixedTools {
        fn resolve(&self, id: &str) -> Result<String, String> {
            if id == "il-motore" {
                Ok(self.0.to_owned())
            } else {
                Err(format!("«{id}» non è dichiarato da nessun descrittore"))
            }
        }
    }

    /// The step names a tool; which executable it is, the machine decides.
    #[test]
    fn a_tool_id_becomes_the_executable_the_resolver_names() {
        let action = ExternalEngineAction::resolving_with(FixedTools("echo"));
        let input = json!({"tool": "il-motore", "args": ["risolto"], "timeout_secs": 5});

        let ActionOutcome::Went(output) = action
            .execute(&input, &SharedState::new())
            .expect("lo strumento si risolve")
        else {
            panic!("un motore che risponde è sempre Went")
        };

        assert_eq!(output["stdout"], "risolto\n");
    }

    /// A tool that is not here: the step stops **before** spending anything,
    /// and carries the reason from whoever looked at the machine.
    #[test]
    fn a_tool_that_is_not_here_stops_the_step_with_the_resolvers_reason() {
        let action = ExternalEngineAction::resolving_with(FixedTools("echo"));
        let input = json!({"tool": "un-altro", "timeout_secs": 5});

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("lo strumento non c'è");

        assert_eq!(error.class, "tool_unavailable");
        assert!(error.said.contains("un-altro"), "{}", error.said);
    }

    /// An engine registered without a resolver guesses no binary from the
    /// tool's name: it says how to repair the registry.
    #[test]
    fn without_a_resolver_a_tool_step_says_how_to_repair_the_registry() {
        let action = ExternalEngineAction::new();
        let input = json!({"tool": "claude-code", "timeout_secs": 5});

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("nessuno sa risolvere gli strumenti");

        assert_eq!(error.class, "no_tool_resolver");
        assert!(error.said.contains("resolving_with"), "{}", error.said);
    }

    // ── the engine chain ──────────────────────────────────────────────

    /// A make-believe machine with three engines: one declaring itself spent,
    /// one that answers, one that is not installed.
    struct Chain;

    impl ToolResolver for Chain {
        fn resolve(&self, id: &str) -> Result<String, String> {
            match id {
                // Prints a spent engine's message and exits 1.
                "esaurito" => Ok("false-dopo-aver-parlato".to_owned()),
                "vivo" => Ok("echo".to_owned()),
                "rotto" => Ok("false".to_owned()),
                "senza-ricetta" => Ok("echo".to_owned()),
                _ => Err(format!("«{id}» non è su questa macchina")),
            }
        }

        fn ask_recipe(&self, id: &str) -> Option<AskRecipe> {
            match id {
                "esaurito" => Some(AskRecipe {
                    args: Vec::new(),
                    prompt: PromptVia::Stdin,
                    args_before_prompt: Vec::new(),
                    unusable_when: vec!["weekly limit".to_owned()],
                    silent_without_prompt: false,
                    refuses_without_prompt: Vec::new(),
                    exhausted_when: Vec::new(),
                    cooldown_secs: None,
                    waits_for_a_person_when: Vec::new(),
                    usage: None,
                }),
                "vivo" => Some(AskRecipe {
                    args: vec!["ha-risposto-il-secondo".to_owned()],
                    prompt: PromptVia::LastArg,
                    args_before_prompt: Vec::new(),
                    unusable_when: vec!["weekly limit".to_owned()],
                    silent_without_prompt: false,
                    refuses_without_prompt: Vec::new(),
                    exhausted_when: Vec::new(),
                    cooldown_secs: None,
                    waits_for_a_person_when: Vec::new(),
                    usage: None,
                }),
                "rotto" => Some(AskRecipe {
                    args: Vec::new(),
                    prompt: PromptVia::Stdin,
                    args_before_prompt: Vec::new(),
                    unusable_when: vec!["weekly limit".to_owned()],
                    silent_without_prompt: false,
                    refuses_without_prompt: Vec::new(),
                    exhausted_when: Vec::new(),
                    cooldown_secs: None,
                    waits_for_a_person_when: Vec::new(),
                    usage: None,
                }),
                // Resolvable but with no recipe: a step that writes no options
                // has no way to ask it.
                _ => None,
            }
        }
    }

    /// A make-believe executable that says it is spent and exits in error.
    fn engine_that_says_it_is_out(dir: &std::path::Path) -> String {
        let path = dir.join("false-dopo-aver-parlato");
        std::fs::write(
            &path,
            "#!/bin/sh\necho \"You've hit your weekly limit · resets 7am\"\nexit 1\n",
        )
        .expect("scrivere il finto motore");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("renderlo eseguibile");
        }
        path.to_string_lossy().into_owned()
    }

    struct ChainIn(String);

    impl ToolResolver for ChainIn {
        fn resolve(&self, id: &str) -> Result<String, String> {
            match id {
                "esaurito" => Ok(self.0.clone()),
                other => Chain.resolve(other),
            }
        }
        fn ask_recipe(&self, id: &str) -> Option<AskRecipe> {
            Chain.ask_recipe(id)
        }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sailor-catena-{name}"));
        std::fs::create_dir_all(&dir).expect("cartella di lavoro");
        dir
    }

    /// **The case the chain exists for.** The first engine declares itself
    /// spent; the work does not die, it goes to the second, and that answers.
    #[test]
    fn an_engine_that_says_it_is_out_hands_the_work_to_the_next_one() {
        let dir = scratch("passa-al-secondo");
        let action =
            ExternalEngineAction::resolving_with(ChainIn(engine_that_says_it_is_out(&dir)));
        let input = json!({"tool": ["esaurito", "vivo"], "timeout_secs": 10});

        let ActionOutcome::Went(output) = action
            .execute(&input, &SharedState::new())
            .expect("il secondo motore risponde")
        else {
            panic!("un motore che risponde è sempre Went")
        };

        assert_eq!(output["status"], "ok");
        assert_eq!(output["stdout"], "ha-risposto-il-secondo\n");
    }

    /// A make-believe executable that says it is spent **and exits zero**.
    ///
    /// **WHY A SECOND MAKE-BELIEVE ENGINE.** The twin above exits 1, as every
    /// hermetic test on this chain had it do: with the spent engine **always**
    /// failing, none of them ever looked at the successful branch, and a defect
    /// living there alone could never turn red.
    fn engine_that_says_it_is_out_and_exits_zero(dir: &std::path::Path) -> String {
        let path = dir.join("zero-dopo-aver-parlato");
        std::fs::write(
            &path,
            "#!/bin/sh\necho \"You've hit your weekly limit · resets 7am\"\nexit 0\n",
        )
        .expect("scrivere il finto motore");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("renderlo eseguibile");
        }
        path.to_string_lossy().into_owned()
    }

    /// **SAYING IT AND EXITING ZERO.**
    ///
    /// An engine that declares in its own words that it cannot work, and
    /// **exits zero**. Ask `says_it_cannot_work` inside the `ExitError` branch
    /// alone and the successful branch takes the answer on trust: no fallback
    /// fires, the ledger line is born with `error_type: None`, so the step
    /// reads as a success and the engine behind it never starts.
    ///
    /// **NOT HYPOTHETICAL ON THIS MACHINE.** It is the shape of fault 39:
    /// `CODEX_HOME=<empty folder> codex exec < /dev/null` answers «No prompt
    /// provided via stdin» and exits **zero**. The dry probe already had the
    /// distinction — `judge_dry_run` is applied to `Ok` *and* to `ExitError` —
    /// so the static check and the real run said different things about the
    /// same engine.
    ///
    /// Paired with `an_engine_that_says_it_is_out_hands_the_work_to_the_next_one`
    /// it is the whole demonstration: the same words, the exit code the one
    /// difference, and the fallback must fire in both cases.
    #[test]
    fn an_engine_that_says_it_is_out_while_exiting_zero_still_hands_the_work_over() {
        let dir = scratch("esaurito-a-uscita-zero");
        let action = ExternalEngineAction::resolving_with(ChainIn(
            engine_that_says_it_is_out_and_exits_zero(&dir),
        ));
        let input = json!({"tool": ["esaurito", "vivo"], "timeout_secs": 10});

        let ActionOutcome::Went(output) = action
            .execute(&input, &SharedState::new())
            .expect("il secondo motore risponde")
        else {
            panic!("un motore che risponde è sempre Went")
        };

        assert_eq!(
            output["stdout"], "ha-risposto-il-secondo\n",
            "il primo motore ha detto di non poter lavorare ed è uscito zero: il \
             lavoro doveva passare al secondo, non fermarsi sulla sua non-risposta"
        );
    }

    /// **AND ALONE IT SAYS SO, INSTEAD OF PRETENDING TO HAVE ANSWERED.**
    ///
    /// With no fallback behind it there is nothing to save, but the diagnosis
    /// remains: whoever reads «spent» knows to wait or change profile, whoever
    /// reads a **green** step goes hunting for an answer that is not there —
    /// the second half of the defect, and the worse one.
    #[test]
    fn alone_an_engine_that_says_it_is_out_while_exiting_zero_does_not_pass_for_answered() {
        let dir = scratch("esaurito-a-uscita-zero-da-solo");
        let action = ExternalEngineAction::resolving_with(ChainIn(
            engine_that_says_it_is_out_and_exits_zero(&dir),
        ));
        let input = json!({"tool": ["esaurito"], "timeout_secs": 10});

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("un motore che dice di non poter lavorare non ha risposto");

        assert_eq!(
            error.class, "engine_exhausted",
            "esaurito non è rotto, e a uscita zero non è nemmeno «riuscito»: {}",
            error.said
        );
        assert!(
            error.said.contains("weekly limit"),
            "il motivo deve portare le parole con cui il motore l'ha detto: {}",
            error.said
        );
    }

    /// A make-believe engine that answers **in the declared shape** and then
    /// exits in error saying the words of its own refusal.
    fn engine_that_answers_in_shape_then_exits_in_error(dir: &std::path::Path) -> String {
        let path = dir.join("risponde-e-poi-esce-male");
        std::fs::write(
            &path,
            "#!/bin/sh\necho '{\"answer\":\"fatto\"}'\n\
             echo \"You've hit your weekly limit · resets 7am\" >&2\nexit 1\n",
        )
        .expect("scrivere il finto motore");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("renderlo eseguibile");
        }
        path.to_string_lossy().into_owned()
    }

    /// **THE SHAPE IS READ BEFORE THE WORDS, AND THE EXIT CODE HAS A VETO.**
    /// The engine answers in shape and then exits in error saying the words
    /// its descriptor declares. Two questions, two answers: the step did not
    /// succeed, and the engine is not one to set aside either — reading those
    /// words inside an answer that is there would hand the work on and throw
    /// away an answer already paid for. Fault 95, in the other branch.
    #[test]
    fn an_answer_in_shape_with_a_bad_exit_code_stops_the_step_instead_of_being_thrown_away() {
        let dir = scratch("risposta-conforme-uscita-anomala");
        let action = ExternalEngineAction::resolving_with(ChainIn(
            engine_that_answers_in_shape_then_exits_in_error(&dir),
        ));
        let input = json!({
            "tool": ["esaurito", "vivo"],
            "answer_shape": {
                "type": "object",
                "properties": {"answer": {"type": "string"}},
                "required": ["answer"],
                "allow_extra": false
            },
            "stdin": {"$json": "/answer_shape"},
            "timeout_secs": 10
        });

        let error = action
            .execute(&with_references_resolved(input), &SharedState::new())
            .expect_err("an exit code out of the ordinary does not close as a success");

        assert_eq!(
            error.class, "engine_exit_error",
            "it answered: the failure is the process's, not a spent quota: {}",
            error.said
        );
        assert!(
            error.said.contains("fatto"),
            "the answer it collected must not be thrown away: {}",
            error.said
        );
        assert!(
            !error.said.contains("ha-risposto-il-secondo"),
            "the second engine had no business starting over an answer already given: {}",
            error.said
        );
    }

    /// **FAULT 31, MADE A FACT INSTEAD OF A READING.**
    ///
    /// The spent engine from above, with the difference that counts: its
    /// descriptor **declares no word** of `unusable_when`. `says_it_cannot_work`
    /// over an empty list is `false`, so running out passes for an ordinary
    /// failure, the step dies there, and the next engine **never starts**. That
    /// is `agy`'s descriptor as shipped, and the reason a spent `agy` in the
    /// chain `claude-code → agy → codex` kills the step with `codex` untried.
    ///
    /// **WHY THE TWIN ON EMPTY FRAGMENTS IS NOT ENOUGH.** That one tests a
    /// badly written descriptor; this one tests a descriptor that **says
    /// nothing**, the real case and the one nobody reads as a defect: an absent
    /// field looks like a choice, a field full of empty strings looks like a
    /// mistake. The behaviour is the same; the difference is nobody looks at
    /// the first.
    ///
    /// Paired with the test above it is the whole demonstration: list
    /// populated, the second starts; list empty, it does not.
    #[test]
    fn an_engine_that_declares_no_exhaustion_words_kills_the_chain() {
        /// Like `ChainIn`, but the first engine loses what `agy` lacks.
        struct NoMarks(String);
        impl ToolResolver for NoMarks {
            fn resolve(&self, id: &str) -> Result<String, String> {
                match id {
                    "esaurito" => Ok(self.0.clone()),
                    other => Chain.resolve(other),
                }
            }
            fn ask_recipe(&self, id: &str) -> Option<AskRecipe> {
                let recipe = Chain.ask_recipe(id)?;
                if id == "esaurito" {
                    return Some(AskRecipe {
                        unusable_when: Vec::new(),
                        ..recipe
                    });
                }
                Some(recipe)
            }
        }

        let dir = scratch("catena-senza-parole");
        let action =
            ExternalEngineAction::resolving_with(NoMarks(engine_that_says_it_is_out(&dir)));
        let input = json!({"tool": ["esaurito", "vivo"], "timeout_secs": 10});

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("il passo muore sul primo motore");

        assert_eq!(
            error.class, "engine_exit_error",
            "un esaurimento non dichiarato passa per un fallimento qualunque"
        );
        assert!(
            !error.said.contains("ha-risposto-il-secondo"),
            "il secondo motore non doveva nemmeno partire: {}",
            error.said
        );
    }

    /// A make-believe executable that fails **out loud**, but not with the
    /// words that engine would declare itself spent with.
    ///
    /// **WHY A MUTE FAILURE IS NOT ENOUGH.** Against `false`, which exits 1
    /// without a word, a mutant firing the fallback on *any* output survives:
    /// with an empty output, «any output» and «those words» behave alike. A
    /// real failure speaks, and that is the case this test must hold.
    fn engine_that_fails_loudly(dir: &std::path::Path) -> String {
        let path = dir.join("fallisce-parlando");
        std::fs::write(
            &path,
            "#!/bin/sh\necho 'errore: il mandato non ha senso' >&2\nexit 1\n",
        )
        .expect("scrivere il finto motore");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("renderlo eseguibile");
        }
        path.to_string_lossy().into_owned()
    }

    struct LoudFailure(String);

    impl ToolResolver for LoudFailure {
        fn resolve(&self, id: &str) -> Result<String, String> {
            match id {
                "rotto" => Ok(self.0.clone()),
                other => Chain.resolve(other),
            }
        }
        fn ask_recipe(&self, id: &str) -> Option<AskRecipe> {
            Chain.ask_recipe(id)
        }
    }

    /// **The half that counts most.** An ordinary failure does NOT walk down
    /// the chain: a badly written brief must stop there, not find further down
    /// an engine that answers anyway — that would be a wrong answer wearing the
    /// face of a good one.
    #[test]
    fn an_ordinary_failure_does_not_walk_down_the_chain() {
        let dir = scratch("fallimento-qualunque");
        let action =
            ExternalEngineAction::resolving_with(LoudFailure(engine_that_fails_loudly(&dir)));
        let input = json!({"tool": ["rotto", "vivo"], "timeout_secs": 10});

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("il primo è fallito senza dire di non poter lavorare");

        assert_eq!(error.class, "engine_exit_error");
        assert!(
            error.said.contains("il mandato non ha senso"),
            "{}",
            error.said
        );
    }

    /// A hand-written descriptor with an **empty** fragment among the words of
    /// `unusable_when`: that fragment is contained in any text, and without a
    /// guard would walk the chain down on **every** failure — exactly the fault
    /// the chain exists not to introduce. Whoever wrote that descriptor would
    /// never notice: it would work, and give wrong answers.
    #[test]
    fn an_empty_mark_in_a_descriptor_does_not_make_everything_a_fallback() {
        struct EmptyMark(String);
        impl ToolResolver for EmptyMark {
            fn resolve(&self, id: &str) -> Result<String, String> {
                match id {
                    "rotto" => Ok(self.0.clone()),
                    other => Chain.resolve(other),
                }
            }
            fn ask_recipe(&self, id: &str) -> Option<AskRecipe> {
                match id {
                    "rotto" => Some(AskRecipe {
                        args: Vec::new(),
                        prompt: PromptVia::Stdin,
                        args_before_prompt: Vec::new(),
                        unusable_when: vec![String::new(), "   ".to_owned()],
                        silent_without_prompt: false,
                        refuses_without_prompt: Vec::new(),
                    exhausted_when: Vec::new(),
                    cooldown_secs: None,
                    waits_for_a_person_when: Vec::new(),
                        usage: None,
                    }),
                    other => Chain.ask_recipe(other),
                }
            }
        }

        let dir = scratch("frammento-vuoto");
        let action =
            ExternalEngineAction::resolving_with(EmptyMark(engine_that_fails_loudly(&dir)));
        let input = json!({"tool": ["rotto", "vivo"], "timeout_secs": 10});

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("un frammento vuoto non è una dichiarazione di esaurimento");

        assert_eq!(error.class, "engine_exit_error");
    }

    /// When every engine in the chain declares it cannot work, the step is red
    /// with **each** reason: the reader must see the whole chain, not just the
    /// last link.
    #[test]
    fn a_chain_that_is_entirely_out_names_every_engine() {
        let dir = scratch("tutti-esauriti");
        let action =
            ExternalEngineAction::resolving_with(ChainIn(engine_that_says_it_is_out(&dir)));
        let input = json!({"tool": ["esaurito", "non-installato"], "timeout_secs": 10});

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("nessuno dei due può lavorare");

        assert_eq!(error.class, "no_usable_engine");
        assert!(error.said.contains("esaurito"), "{}", error.said);
        assert!(error.said.contains("non-installato"), "{}", error.said);
    }

    /// The descriptor decides where the question's text goes. Without it a flow
    /// would have to know every engine's options — which is what binds a flow
    /// to a single one.
    #[test]
    fn the_descriptor_decides_where_the_question_goes() {
        let action = ExternalEngineAction::resolving_with(Chain);
        let input = json!({"tool": "vivo", "stdin": "la-domanda", "timeout_secs": 10});

        let ActionOutcome::Went(output) = action
            .execute(&input, &SharedState::new())
            .expect("risponde")
        else {
            panic!("un motore che risponde è sempre Went")
        };

        // `echo` prints its own arguments: had the question gone to the input
        // rather than to the end of the arguments, it would not be here.
        assert_eq!(output["stdout"], "ha-risposto-il-secondo la-domanda\n");
    }

    /// An engine that is here but declares no way of asking it is not guessed
    /// at: it is set aside with the reason, and the next is tried.
    #[test]
    fn an_engine_without_a_recipe_is_set_aside_with_the_reason() {
        let action = ExternalEngineAction::resolving_with(Chain);
        let input = json!({"tool": ["senza-ricetta"], "timeout_secs": 10});

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("non si sa come interrogarlo");

        assert_eq!(error.class, "no_usable_engine");
        assert!(error.said.contains("ask"), "{}", error.said);
    }

    /// Options written in the step win over the recipe: whoever wrote them is
    /// saying something precise about this call.
    #[test]
    fn options_written_in_the_step_win_over_the_recipe() {
        let action = ExternalEngineAction::resolving_with(Chain);
        let input = json!({"tool": "vivo", "args": ["scritte-nel-passo"], "timeout_secs": 10});

        let ActionOutcome::Went(output) = action
            .execute(&input, &SharedState::new())
            .expect("risponde")
        else {
            panic!("un motore che risponde è sempre Went")
        };

        assert_eq!(output["stdout"], "scritte-nel-passo\n");
    }

    // ── which model is wanted ─────────────────────────────────────────

    /// Two engines alike but for two things: the first says how a model is
    /// named to it and the second does not, and each **signs its own line** —
    /// unsigned they echo identically and no chain test could say who
    /// answered. One of the question's options must stay glued to the text:
    /// that is where the model's place on the line shows.
    struct Models;

    impl ToolResolver for Models {
        fn resolve(&self, id: &str) -> Result<String, String> {
            match id {
                "sa-il-modello" | "non-sa-il-modello" => Ok("echo".to_owned()),
                _ => Err(format!("«{id}» non è su questa macchina")),
            }
        }

        fn ask_recipe(&self, id: &str) -> Option<AskRecipe> {
            let signature = format!("--ha-risposto-{}", self.resolve(id).map(|_| id).ok()?);
            Some(AskRecipe {
                args: vec![signature, "--mode".to_owned(), "plan".to_owned()],
                prompt: PromptVia::LastArg,
                args_before_prompt: vec!["--print".to_owned()],
                unusable_when: Vec::new(),
                silent_without_prompt: false,
                refuses_without_prompt: Vec::new(),
                exhausted_when: Vec::new(),
                cooldown_secs: None,
                waits_for_a_person_when: Vec::new(),
                usage: None,
            })
        }

        fn model_option(&self, id: &str) -> Option<Vec<String>> {
            (id == "sa-il-modello").then(|| vec!["--model".to_owned()])
        }
    }

    /// The step names a model for an engine that can receive one: the line
    /// carries it **after** the recipe's options and **before** the one glued
    /// to the question, which would otherwise read the name as the question.
    #[test]
    fn a_named_model_lands_before_the_option_glued_to_the_question() {
        let action = ExternalEngineAction::resolving_with(Models);
        let input = json!({
            "tool": "sa-il-modello",
            "model": {"sa-il-modello": "il-modello-forte"},
            "stdin": "la-domanda",
            "timeout_secs": 10
        });

        let ActionOutcome::Went(output) = action
            .execute(&input, &SharedState::new())
            .expect("risponde")
        else {
            panic!("un motore che risponde è sempre Went")
        };

        assert_eq!(
            output["stdout"],
            "--ha-risposto-sa-il-modello --mode plan --model il-modello-forte --print la-domanda\n",
            "il nome del modello sta dopo le opzioni della ricetta e prima di `--print`"
        );
    }

    /// An engine that cannot be told a model is **not** run on its own: it is
    /// set aside and the chain goes on. The second, named by no entry, runs on
    /// its default — that is, with no model option on the line.
    #[test]
    fn an_engine_that_cannot_be_told_a_model_is_set_aside_and_the_chain_goes_on() {
        let action = ExternalEngineAction::resolving_with(Models);
        let input = json!({
            "tool": ["non-sa-il-modello", "sa-il-modello"],
            "model": {"non-sa-il-modello": "il-modello-forte"},
            "stdin": "la-domanda",
            "timeout_secs": 10
        });

        let ActionOutcome::Went(output) = action
            .execute(&input, &SharedState::new())
            .expect("il secondo motore risponde")
        else {
            panic!("un motore che risponde è sempre Went")
        };

        assert_eq!(
            output["stdout"], "--ha-risposto-sa-il-modello --mode plan --print la-domanda\n",
            "il secondo non è nominato da nessuna voce: gira col suo predefinito"
        );
    }

    /// **AND ALONE IT SAYS SO.** With nobody behind it there is nothing to
    /// save, but the reason remains: the reader learns that model cannot be
    /// asked of it, instead of reading an answer from a model nothing names.
    #[test]
    fn alone_it_says_why_instead_of_answering_from_a_model_nobody_chose() {
        let action = ExternalEngineAction::resolving_with(Models);
        let input = json!({
            "tool": ["non-sa-il-modello"],
            "model": {"non-sa-il-modello": "il-modello-forte"},
            "stdin": "la-domanda",
            "timeout_secs": 10
        });

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("non gli si può nominare un modello");

        assert_eq!(error.class, "no_usable_engine");
        assert!(error.said.contains("choose_model"), "{}", error.said);
        assert!(error.said.contains("il-modello-forte"), "{}", error.said);
    }

    /// Whoever wrote the options wrote which model in them: two answers to one
    /// question do not live together, as `bin` and `tool` do not.
    #[test]
    fn a_step_cannot_write_its_own_options_and_name_a_model() {
        let action = ExternalEngineAction::resolving_with(Models);
        let input = json!({
            "tool": "sa-il-modello",
            "args": ["--model", "un-altro"],
            "model": {"sa-il-modello": "il-modello-forte"},
            "timeout_secs": 10
        });

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("due risposte alla stessa domanda");

        assert_eq!(error.class, "invalid_input");
        assert!(error.said.contains("model"), "{}", error.said);
    }

    #[test]
    fn a_step_cannot_declare_both_a_binary_and_a_tool() {
        let action = ExternalEngineAction::resolving_with(FixedTools("echo"));
        let input = json!({"bin": "sh", "tool": "il-motore", "timeout_secs": 5});

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("due risposte alla stessa domanda");

        assert_eq!(error.class, "invalid_input");
    }

    #[test]
    fn the_external_engine_action_rejects_an_input_without_a_binary() {
        let action = ExternalEngineAction::new();
        let input = json!({"timeout_secs": 5});
        let shared = SharedState::new();
        assert!(action.execute(&input, &shared).is_err());
    }
}
