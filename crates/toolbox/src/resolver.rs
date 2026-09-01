//! From a tool identifier to the executable that is that tool here.
//!
//! **WHY IT LIVES HERE AND NOT IN THE ACTIONS CRATE.** What knows which tools
//! exist is this list of descriptors; what executes is the actions crate, which
//! must know neither where to look nor what exists. The link is a trait declared
//! there and implemented here, joined at one visible point by the registry.

use crate::descriptor::CapabilityState;
use crate::session::SessionAbilities;
use crate::{default_sources, probe_one, Catalog, Machine, Presence};

/// The detector, loaded once and asked many times.
///
/// **IT NEVER ASKS FOR A VERSION**, and that is not an optimisation: asking for
/// a version means *running* a binary, and resolving a name must run nothing. A
/// flow naming three tools would start three processes before starting its work.
pub struct Tools {
    catalog: Catalog,
    machine: Machine,
    /// Who can resume a session, and with which options. It travels in a file of
    /// its own, separate from the descriptors: see the head of `session.rs`.
    sessions: SessionAbilities,
}

impl Tools {
    /// This machine's tools, with the shipped descriptors plus the user's.
    pub fn current() -> Self {
        let mut machine = Machine::current();
        machine.version_probes = false;
        let catalog = Catalog::load(&default_sources(&machine));
        Self {
            catalog,
            machine,
            sessions: SessionAbilities::current(),
        }
    }

    /// A catalog and a world decided by the caller: that is how a test checks
    /// resolution without depending on what is installed on the test runner.
    pub fn new(catalog: Catalog, mut machine: Machine) -> Self {
        machine.version_probes = false;
        Self {
            catalog,
            machine,
            sessions: SessionAbilities::shipped(),
        }
    }

    /// The same, with the session abilities decided by the caller.
    pub fn with_sessions(mut self, sessions: SessionAbilities) -> Self {
        self.sessions = sessions;
        self
    }

    /// Is there a descriptor with this identifier?
    ///
    /// **NOT "IS IT ON THE MACHINE", AND THE DIFFERENCE IS EVERYTHING.** A flow
    /// asking `docker` where docker is missing is not broken, it runs elsewhere;
    /// one asking a name no catalog declares is broken everywhere, with nothing
    /// to install. Only the second is certain without running, and only it errs.
    pub fn declares(&self, id: &str) -> bool {
        self.catalog
            .live()
            .into_iter()
            .any(|loaded| loaded.descriptor.id == id)
    }

    /// How a tool stands against a capability a step asked for.
    ///
    /// **`None` IS NOT "IT DOES NOT HAVE IT".** It is "no descriptor declares
    /// this tool", a third question again, and whoever checks a flow already
    /// says it in their own words: answering with an absence here would hide it
    /// behind a warning about the wrong capability.
    pub fn capability(&self, id: &str, name: &str) -> Option<CapabilityState> {
        self.catalog
            .live()
            .into_iter()
            .find(|loaded| loaded.descriptor.id == id)
            .map(|loaded| loaded.descriptor.capability(name))
    }

    /// Where this machine's descriptors say two different things about one fact.
    ///
    /// **IT LOOKS AT THE RUNNER'S DESCRIPTORS TOO.** A test can demand the
    /// **shipped** ones hold together; one written in `~/.config/sailor/tools.d/`,
    /// the intended way to add an engine without recompiling, sits outside every
    /// check — and fault 32, a capability with no line behind it, is reborn there.
    pub fn contradictions(&self) -> Vec<crate::descriptor::Contradiction> {
        self.catalog.contradictions()
    }

    /// Why this tool cannot serve as a fallback, when it cannot.
    ///
    /// **`None` SAYS TWO DIFFERENT THINGS, AND THE CALLER TELLS THEM APART.**
    /// Either it can serve as a fallback, or no descriptor declares it — the
    /// second is what `declares` says. A reason for a name that does not exist
    /// would send someone hunting a fault in a descriptor that is not there.
    pub fn cannot_be_a_fallback(&self, id: &str) -> Option<String> {
        self.catalog
            .live()
            .into_iter()
            .find(|loaded| loaded.descriptor.id == id)?
            .descriptor
            .cannot_be_a_fallback()
    }

    /// The tool that, on this machine, **is** that executable: its identifier and
    /// the path it resolved to. **THE LINK IS THE EXECUTABLE, NOT A LOOKUP
    /// TABLE**: these descriptors call `claude-code` what the profiles table
    /// calls `claude`, two lists answering two different questions, and pairing
    /// them with a third would mean keeping three aligned by hand. What reads
    /// `CLAUDE_CONFIG_DIR` is the **binary**, whatever name its namer gives it.
    pub fn declared_as_executable(&self, executable: &str) -> Option<(String, String)> {
        use actions::ToolResolver;
        // The same choice as `profiles::cli_for_executable`, taken from the
        // opposite side. AND IT RESOLVES FOR REAL, which is the point: not what
        // the descriptor *says* it will look for, but what resolves here, the
        // only thing a step will run. A descriptor that declared `codex` and
        // found another binary would answer here what it answers the run.
        for loaded in self.catalog.live() {
            let id = &loaded.descriptor.id;
            let Ok(path) = self.resolve(id) else {
                continue;
            };
            if std::path::Path::new(&path)
                .file_name()
                .and_then(|name| name.to_str())
                == Some(executable)
            {
                return Some((id.clone(), path));
            }
        }
        None
    }

    /// The declared identifiers, in order: this is what gets shown to someone
    /// who wrote one that does not exist.
    pub fn declared_ids(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .catalog
            .live()
            .into_iter()
            .map(|loaded| loaded.descriptor.id.clone())
            .collect();
        names.sort_unstable();
        names
    }
}

/// How many identifiers are listed to someone who asked for one that is unknown.
const NAMES_SHOWN: usize = 20;

impl actions::ToolResolver for Tools {
    /// **THE ERROR MESSAGE IS THE PRODUCT.** A flow written on another machine
    /// meets this function: if the tool is not there, what this writes is all
    /// the reader will have to work out what to install. Hence it keeps "not
    /// here" apart from "could not look", and reports the descriptor's note,
    /// which is the place where it is written where the thing is installed from.
    fn resolve(&self, id: &str) -> Result<String, String> {
        let Some(loaded) = self
            .catalog
            .live()
            .into_iter()
            .find(|loaded| loaded.descriptor.id == id)
        else {
            let mut names: Vec<&str> = self
                .catalog
                .live()
                .into_iter()
                .map(|loaded| loaded.descriptor.id.as_str())
                .collect();
            names.sort_unstable();
            let shown = if names.len() > NAMES_SHOWN {
                format!(
                    "{}, and {} more",
                    names[..NAMES_SHOWN].join(", "),
                    names.len() - NAMES_SHOWN
                )
            } else {
                names.join(", ")
            };
            return Err(format!(
                "no descriptor declares the tool «{id}»; the declared ones are: {shown}. \
                 You add one by writing a JSON file in ~/.config/sailor/tools.d/, with no recompile"
            ));
        };
        let descriptor = &loaded.descriptor;
        if descriptor.detect.is_none() {
            return Err(format!(
                "«{id}» is declared as a configuration entry to read, not as something to run: a step cannot invoke it"
            ));
        }
        let finding = probe_one(loaded, &self.machine);
        let note = if descriptor.note.is_empty() {
            String::new()
        } else {
            format!(" Descriptor note: {}", descriptor.note)
        };
        match (&finding.presence, &finding.executable) {
            (Presence::Present(_), Some(executable)) => Ok(executable.clone()),
            // Found through a path that exists — an application, a configuration
            // file — but with no executable to talk to.
            (Presence::Present(evidence), None) => Err(format!(
                "«{id}» is present ({evidence}), but its descriptor does not say which executable to invoke: add a `command` probe to be able to use it in a step.{note}"
            )),
            (Presence::Absent(reason), _) => Err(format!(
                "the tool «{id}» ({}) is not on this machine: {reason}.{note}",
                if descriptor.label.is_empty() { &descriptor.id } else { &descriptor.label }
            )),
            // The difference this crate exists to hold: nothing here says
            // "install it", because nothing could be looked at.
            (Presence::Undetermined(reason), _) => Err(format!(
                "could not check whether «{id}» is on this machine: {reason}.{note}"
            )),
        }
    }

    /// The recipe the descriptor declares, translated into the shape the actions
    /// know. No interpretation: what is not written is not there, and an engine
    /// that does not declare one does not become usable by guesswork.
    fn ask_recipe(&self, id: &str) -> Option<actions::AskRecipe> {
        let loaded = self
            .catalog
            .live()
            .into_iter()
            .find(|loaded| loaded.descriptor.id == id)?;
        let ask = loaded.descriptor.ask.as_ref()?;
        Some(actions::AskRecipe {
            args: ask.args.clone(),
            prompt: match ask.prompt {
                crate::descriptor::PromptPlace::Stdin => actions::PromptVia::Stdin,
                crate::descriptor::PromptPlace::LastArg => actions::PromptVia::LastArg,
            },
            args_before_prompt: ask.args_before_prompt.clone(),
            unusable_when: ask.unusable_when.clone(),
            refuses_without_prompt: ask.refuses_without_prompt.clone(),
            usage: loaded.descriptor.usage.as_ref().map(usage_recipe),
        })
    }

    /// What this tool can do with its own sessions.
    ///
    /// **IT DOES NOT ASK THE CATALOG WHETHER THE TOOL EXISTS**, and that is not
    /// an oversight: whoever gets here has already resolved an executable, and
    /// looking again would add a scan to answer a question already answered.
    fn session_recipe(&self, id: &str) -> Option<actions::SessionRecipe> {
        self.sessions.for_tool(id)
    }

    /// How to ask this engine whether the home it starts from is authenticated.
    /// No interpretation, as above: an engine that does not declare it goes
    /// without, and the check stays silent about it instead of reassuring.
    fn login_recipe(&self, id: &str) -> Option<actions::LoginRecipe> {
        let loaded = self
            .catalog
            .live()
            .into_iter()
            .find(|loaded| loaded.descriptor.id == id)?;
        let login = loaded.descriptor.login_status.as_ref()?;
        Some(actions::LoginRecipe {
            args: login.args.clone(),
            answer: login.answer.as_ref().map(pointer),
            logged_in_when: login.logged_in_when.clone(),
            logged_out_when: login.logged_out_when.clone(),
        })
    }
}

/// The descriptor's `usage` block translated into the shape the actions know.
/// No interpretation in here: what is written is copied, and what is not written
/// stays `None` all the way down.
fn usage_recipe(usage: &crate::descriptor::Usage) -> actions::UsageRecipe {
    actions::UsageRecipe {
        args: usage.args.clone(),
        declared: actions::Declared {
            read: match usage.read {
                crate::descriptor::ReadAs::Json => actions::Shape::Json,
                crate::descriptor::ReadAs::Text => actions::Shape::Text,
            },
            input_tokens: usage.input_tokens.as_ref().map(pointer),
            output_tokens: usage.output_tokens.as_ref().map(pointer),
            cached_tokens: usage.cached_tokens.as_ref().map(pointer),
            cache_write_tokens: usage.cache_write_tokens.as_ref().map(pointer),
            cache_write_long_tokens: usage.cache_write_long_tokens.as_ref().map(pointer),
            total_tokens: usage.total_tokens.as_ref().map(pointer),
            turns: usage.turns.as_ref().map(pointer),
            cost: usage.cost.as_ref().map(pointer),
            model: usage.model.as_ref().map(pointer),
            answer: usage.answer.as_ref().map(pointer),
        },
    }
}

fn pointer(place: &crate::descriptor::Where) -> actions::Pointer {
    match place {
        crate::descriptor::Where::Path(keys) => actions::Pointer::Path(keys.clone()),
        crate::descriptor::Where::Pattern(pattern) => {
            actions::Pointer::Pattern(pattern.clone())
        }
        crate::descriptor::Where::FirstKeyOf { first_key_of } => {
            actions::Pointer::FirstKey(first_key_of.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::Source;
    use actions::ToolResolver;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// A made-up machine: one directory on the path, no variables. Tests must not
    /// depend on what is installed on whoever runs them.
    fn machine(dir: &std::path::Path) -> Machine {
        Machine {
            path_dirs: vec![dir.to_path_buf()],
            home: dir.to_path_buf(),
            env: BTreeMap::new(),
            version_probes: false,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sailor-resolver-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create the test directory");
        dir
    }

    fn catalog_of(text: &str, dir: &std::path::Path) -> Catalog {
        let file = dir.join("descriptors.json");
        std::fs::write(&file, text).expect("write the test descriptors");
        Catalog::load(&[Source::File(file)])
    }

    fn fake_executable(dir: &std::path::Path, name: &str) {
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").expect("write the fake binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("make it executable");
        }
    }

    /// **THE MEASUREMENT THAT COULD HAVE COME OUT OTHERWISE**: the same
    /// identifier, the same two test lines, and the only difference is whether
    /// the binary is there. Either one alone would prove nothing.
    #[test]
    fn an_id_becomes_the_path_of_the_executable_that_is_here() {
        let dir = temp_dir("present");
        fake_executable(&dir, "un-motore");
        let catalog = catalog_of(
            r#"[{"id": "il-motore", "family": "ai_cli", "label": "The engine",
                 "detect": {"command": "un-motore"}}]"#,
            &dir,
        );
        let tools = Tools::new(catalog, machine(&dir));

        let path = tools
            .resolve("il-motore")
            .expect("the binary is on the path");

        assert_eq!(path, dir.join("un-motore").to_string_lossy());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tool_that_is_not_here_says_what_it_looked_for_and_where_it_comes_from() {
        let dir = temp_dir("absent");
        let catalog = catalog_of(
            r#"[{"id": "il-motore", "family": "ai_cli", "label": "The engine",
                 "detect": {"command": "un-motore"},
                 "note": "install it with `npm i -g un-motore`"}]"#,
            &dir,
        );
        let tools = Tools::new(catalog, machine(&dir));

        let reason = tools
            .resolve("il-motore")
            .expect_err("there is no such binary");

        assert!(reason.contains("il-motore"), "{reason}");
        assert!(reason.contains("un-motore"), "{reason}");
        assert!(reason.contains("npm i -g"), "{reason}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_id_nobody_declared_lists_the_ones_that_exist() {
        let dir = temp_dir("unknown");
        let catalog = catalog_of(
            r#"[{"id": "il-motore", "family": "ai_cli", "detect": {"command": "un-motore"}}]"#,
            &dir,
        );
        let tools = Tools::new(catalog, machine(&dir));

        let reason = tools.resolve("un-altro").expect_err("nobody declares it");

        assert!(reason.contains("un-altro"), "{reason}");
        assert!(reason.contains("il-motore"), "{reason}");
        assert!(reason.contains("tools.d"), "{reason}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MCP servers are discovered by reading a file: they are entries, not
    /// binaries, and asking for one in order to run it is an error of whoever
    /// wrote the step.
    #[test]
    fn a_configuration_entry_is_not_something_to_invoke() {
        let dir = temp_dir("entry");
        let catalog = catalog_of(
            r#"[{"id": "i-server", "family": "mcp_server",
                 "enumerate": {"json_keys": {"files": ["~/x.json"], "pointer": ["mcpServers"]}}}]"#,
            &dir,
        );
        let tools = Tools::new(catalog, machine(&dir));

        let reason = tools
            .resolve("i-server")
            .expect_err("it is not an executable");

        assert!(reason.contains("not as something to run"), "{reason}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **THE NEW DATA MUST REACH THE POINT OF INVOCATION.** A `usage` block
    /// written in a descriptor and never translated into an `AskRecipe` is a
    /// field that loads and does nothing: the measurement would stay unknown for
    /// ever, and no engine test would notice, because no recipe declaring it
    /// would ever reach the engine. This is the stretch of road no other test
    /// walks.
    #[test]
    fn the_usage_block_travels_from_the_descriptor_into_the_recipe() {
        let dir = temp_dir("usage-in-recipe");
        fake_executable(&dir, "misurabile");
        let catalog = catalog_of(
            r#"[{
              "id": "misurabile", "family": "ai_cli",
              "detect": { "command": "misurabile" },
              "ask": { "args": ["-p"], "prompt": "stdin" },
              "usage": {
                "args": ["--output-format", "json"],
                "read": "json",
                "input_tokens": ["usage", "input_tokens"],
                "cached_tokens": ["usage", "cache_read_input_tokens"],
                "model": ["model"],
                "answer": ["result"]
              }
            }]"#,
            &dir,
        );
        let tools = Tools::new(catalog, machine(&dir));

        let recipe = tools.ask_recipe("misurabile").expect("the recipe is there");
        let usage = recipe.usage.expect("and it carries the usage block");
        assert_eq!(usage.args, vec!["--output-format", "json"]);
        assert_eq!(usage.declared.read, actions::Shape::Json);
        assert_eq!(
            usage.declared.input_tokens,
            Some(actions::Pointer::Path(vec![
                "usage".to_owned(),
                "input_tokens".to_owned()
            ]))
        );
        assert_eq!(
            usage.declared.cached_tokens,
            Some(actions::Pointer::Path(vec![
                "usage".to_owned(),
                "cache_read_input_tokens".to_owned()
            ])),
            "the cache arrives with a pointer of its own, or criterion 3 falls here"
        );
        assert_eq!(
            usage.declared.answer,
            Some(actions::Pointer::Path(vec!["result".to_owned()]))
        );
        assert_eq!(
            usage.declared.output_tokens, None,
            "and what is not written stays absent"
        );
    }

    /// **AND SO DOES THE REFUSAL WITHOUT A PROMPT**, for the same reason and on
    /// another stretch of road. The test on the shipped descriptors demands the
    /// field be *written*; this one demands it *arrive*. Drop this line from
    /// `ask_recipe` and the descriptors stay full, the shipped test stays green,
    /// and the probe judges every command line "not declared" — it stops
    /// checking without turning red. That is the mutant separating the two tests.
    #[test]
    fn the_refusal_without_a_prompt_travels_from_the_descriptor_into_the_recipe() {
        let dir = temp_dir("refusal-in-recipe");
        fake_executable(&dir, "schizzinoso");
        let catalog = catalog_of(
            r#"[{
              "id": "schizzinoso", "family": "ai_cli",
              "detect": { "command": "schizzinoso" },
              "ask": {
                "args": ["-p"],
                "prompt": "stdin",
                "unusable_when": ["quota"],
                "refuses_without_prompt": ["input must be provided"]
              }
            }]"#,
            &dir,
        );
        // No session abilities: this test looks at how a question is put to an
        // engine, not at how a conversation is resumed with it.
        let tools = Tools {
            catalog,
            machine: machine(&dir),
            sessions: SessionAbilities::default(),
        };

        let recipe = tools.ask_recipe("schizzinoso").expect("the recipe is there");
        assert_eq!(
            recipe.refuses_without_prompt,
            vec!["input must be provided".to_owned()],
            "the field reaches the recipe, or the probe has nothing to judge with"
        );
        // The two lists stay distinct this far: confusing them would call an
        // exhausted engine "broken", which is the opposite case.
        assert_eq!(recipe.unusable_when, vec!["quota".to_owned()]);
    }

    /// An engine that declares nothing reaches the recipe with an empty list,
    /// which the probe reads as "nobody looked" — never as "the line is sound".
    #[test]
    fn a_descriptor_that_says_nothing_about_refusing_gives_an_empty_list() {
        let dir = temp_dir("refusal-unsaid");
        fake_executable(&dir, "silenzioso");
        let catalog = catalog_of(
            r#"[{
              "id": "silenzioso", "family": "ai_cli",
              "detect": { "command": "silenzioso" },
              "ask": { "args": ["-p"], "prompt": "stdin" }
            }]"#,
            &dir,
        );
        // No session abilities: this test looks at how a question is put to an
        // engine, not at how a conversation is resumed with it.
        let tools = Tools {
            catalog,
            machine: machine(&dir),
            sessions: SessionAbilities::default(),
        };

        let recipe = tools.ask_recipe("silenzioso").expect("the recipe is there");
        assert!(recipe.refuses_without_prompt.is_empty());
    }

    /// A descriptor without `usage` gives a recipe without it: that engine is
    /// invoked as before, and its tokens stay unknown.
    #[test]
    fn a_descriptor_without_usage_gives_a_recipe_without_it() {
        let dir = temp_dir("usage-absent");
        fake_executable(&dir, "muto");
        let catalog = catalog_of(
            r#"[{
              "id": "muto", "family": "ai_cli",
              "detect": { "command": "muto" },
              "ask": { "args": ["-p"], "prompt": "stdin" }
            }]"#,
            &dir,
        );
        let tools = Tools::new(catalog, machine(&dir));

        let recipe = tools.ask_recipe("muto").expect("the recipe is there");
        assert!(recipe.usage.is_none());
        assert_eq!(recipe.args, vec!["-p"], "the rest of the recipe is intact");
    }

    /// The shipped `codex` descriptor reaches the recipe with its pattern and no
    /// extra options: codex's command line does not change because it is now
    /// being measured.
    #[test]
    fn the_shipped_codex_recipe_carries_its_pattern_and_adds_no_arguments() {
        let dir = temp_dir("codex-shipped");
        fake_executable(&dir, "codex");
        let catalog = Catalog::load(&[Source::Builtin]);
        let tools = Tools::new(catalog, machine(&dir));

        let recipe = tools.ask_recipe("codex").expect("codex has a recipe");
        let usage = recipe
            .usage
            .expect("and declares how its usage is read");
        assert!(usage.args.is_empty(), "no options added to codex");
        assert_eq!(usage.declared.read, actions::Shape::Text);
        let Some(actions::Pointer::Pattern(pattern)) = usage.declared.total_tokens else {
            panic!("codex declares the total with a pattern")
        };
        // And the pattern really does recognise codex's output, not one that
        // looks like it: written badly, the usage would stay unknown for ever
        // with nobody noticing.
        let read = models_read(&pattern, "stuff\ntokens used\n13.910\nmore");
        assert_eq!(read, Some(13_910));
    }

    /// Reads a total with the given pattern, going through the same function the
    /// engine will use.
    fn models_read(pattern: &str, said: &str) -> Option<u64> {
        let declared = actions::Declared {
            read: actions::Shape::Text,
            total_tokens: Some(actions::Pointer::Pattern(pattern.to_owned())),
            ..actions::Declared::default()
        };
        actions::read_declared(said, &declared).total_tokens
    }
}
