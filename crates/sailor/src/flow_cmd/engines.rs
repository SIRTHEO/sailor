//! The two questions `sailor flow check` asks each engine without spending:
//! whether the line it assembles is sound, and whether its home is logged in.

use flow::Graph;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use super::check::{engines_of, EngineWorld};

// ── the credential homes, asked of the engine ────────────────────────────

/// **A HOME DECLARED AND EMPTY APPLIES IN SILENCE.** A profile pointing at a
/// directory with no credentials makes every call go out unauthenticated, and
/// the dry check cannot see it: it takes the question away, so the engine stops
/// on «you gave me nothing to do» before any of that. So the engine is asked,
/// the way its descriptor says; declaring nothing raises nothing, because empty
/// means «nobody looked» and never «authenticated». It does not fail the check —
/// stopping a flow over a profile would punish whoever did not set it — and it
/// costs nothing: these read a local file, no model and no money.
pub(super) fn login_states_into(
    report: &mut String,
    graph: &Graph,
    tools: &toolbox::Tools,
    world: &EngineWorld,
) {
    use actions::{LoginVerdict, ToolResolver};

    let mut unauthenticated = Vec::new();
    let mut authenticated = Vec::new();
    let mut unknown = Vec::new();

    let mut asked: BTreeSet<String> = BTreeSet::new();
    for wanted in engines_wanted(graph) {
        // AN ENGINE IS ASKED ONCE even when six steps name it: the home comes
        // from the active profile, not from the step, so six questions would
        // give the same answer six times. The report names the engine and the
        // profile, which is what the reader has to change.
        if !tools.declares(&wanted.tool) || !asked.insert(wanted.tool.clone()) {
            continue;
        }
        // An engine not invocable here is already named by the line section:
        // repeating it would send a person hunting two defects, not one.
        let Ok(bin) = tools.resolve(&wanted.tool) else {
            continue;
        };
        // **ONLY WHERE A PROFILE IS IN FORCE.** With no active profile the
        // engine starts in the home of whoever opened the terminal, the usual
        // home: there is no Sailor choice to make visible, and a warning here
        // would speak of something this command does not govern.
        let equipment = actions::equipment_for(world.profiles, &bin, &BTreeMap::new());
        let ledger::EngineIdentity::ProfileInForce {
            cli_id,
            profile_name,
            ..
        } = &equipment.identity
        else {
            continue;
        };
        // The home is shown as the engine receives it — variable and value —
        // not recomputed elsewhere: two roads assembling the same thing
        // diverge at the first one that changes.
        let home = equipment
            .env
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join(" ");
        let who = format!(
            "{} (profilo «{cli_id}/{profile_name}», {home})",
            wanted.tool
        );

        let Some(recipe) = tools.login_recipe(&wanted.tool) else {
            unknown.push(catalogue::say(
                "cli.flow.engine_login_not_declared",
                &[("who", &who)],
            ));
            continue;
        };
        match actions::probe_login_status(world.probe, &bin, &equipment.env, &recipe) {
            LoginVerdict::LoggedIn { .. } => authenticated.push(who),
            // THE ENGINE'S OWN WORDS, as for a broken line: «not logged in»
            // said by us omits which credential is missing; its sentence says.
            LoginVerdict::LoggedOut { said } => unauthenticated.push(format!("{who}: «{said}»")),
            LoginVerdict::NotDeclared => unknown.push(catalogue::say(
                "cli.flow.engine_login_half_declared",
                &[("who", &who)],
            )),
            LoginVerdict::Unrecognised { said } => unknown.push(catalogue::say(
                "cli.flow.engine_login_unrecognised",
                &[("who", &who), ("said", &said)],
            )),
            LoginVerdict::NoAnswer { why } => {
                unknown.push(format!("{who}: nessuna risposta — {why}"))
            }
        }
    }

    if !unauthenticated.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.homes_without_credentials",
            &[("engines", &unauthenticated.join("; "))],
        ));
    }
    if !authenticated.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.homes_authenticated",
            &[("engines", &authenticated.join("; "))],
        ));
    }
    if !unknown.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.homes_unknown",
            &[("engines", &unknown.join("; "))],
        ));
    }
}

// ── the command lines, assembled and tried with no question ─────────────

/// An engine whose line a step leaves to the descriptor to assemble.
struct WantedEngine {
    step: String,
    tool: String,
    /// The model **this step** wants of it, if it names one: the line to try
    /// is that one, not the one without.
    model: Option<String>,
}

/// The engines whose line `flow check` must try, step by step and **engine by
/// engine of the chain**: fault 16 came of six steps naming one engine, and
/// fault 27 sat in the *second* — no flow puts `agy` first, so that branch had
/// never run. **AND ONLY THE STEPS THAT DO NOT WRITE THEIR OWN LINE.** A step
/// declaring its own `args` wins over the recipe — `ExternalEngineAction`
/// decides that, and here the same rule is read, never copied a second time.
fn engines_wanted(graph: &Graph) -> Vec<WantedEngine> {
    let mut wanted = Vec::new();
    for step in graph.steps() {
        let Some(with) = step.with.as_ref() else {
            continue;
        };
        if with.get("args").is_some() {
            continue;
        }
        for tool in engines_of(with) {
            let model = with
                .get("model")
                .and_then(|named| named.get(&tool))
                .and_then(Value::as_str)
                .map(str::to_owned);
            wanted.push(WantedEngine {
                step: step.id.clone(),
                tool,
                model,
            });
        }
    }
    wanted
}

/// What could be learned about an engine's line.
enum EngineOutcome {
    /// The engine is not invocable here, and the detector says why.
    NotHere(String),
    /// No `ask` block: the line does not assemble at all, and there is nothing
    /// to try. An absence in the descriptor, not a defect of the line.
    NotAssemblable,
    /// The line assembled and was tried: how it came out, and what it said.
    Tried {
        line: String,
        verdict: actions::ProbeVerdict,
    },
}

/// Assembles the line of every engine of every chain, tries it **without
/// giving the question**, and writes into the report how it stands.
///
/// **HERE `flow check` STARTS PROCESSES** — no network, no money, a time cap —
/// which is why the cure beside fault 1 exists at all; `resolver.rs` still runs
/// nothing. **ON BY DEFAULT**: a check behind a flag is one nobody asks, and
/// fault 27 stayed invisible exactly so. `--no-engines` makes the report **stay
/// silent** rather than call unlooked-at lines sound. **AND A SOUND LINE IS NOT
/// «IT WAS REALLY CALLED»**: the ledger knows that, and mixing them is fault 32.
pub(super) fn engine_lines_into(
    report: &mut String,
    graph: &Graph,
    tools: &toolbox::Tools,
    probe: &dyn actions::DryProbe,
) {
    use actions::{ProbeVerdict, ToolResolver};

    // AN ENGINE IS TRIED ONCE even when six steps name it: six trials would
    // start six processes to learn one thing six times, and the report stays
    // step by step anyway. THE KEY CARRIES THE MODEL TOO: two steps asking one
    // engine for two models assemble two lines, and probing one would call
    // sound a line nobody looked at.
    let mut judged: BTreeMap<(String, Option<String>), EngineOutcome> = BTreeMap::new();

    let mut sound = Vec::new();
    let mut broken = Vec::new();
    let mut untried = Vec::new();
    let mut unassemblable = Vec::new();
    let mut exhausted = Vec::new();

    for wanted in engines_wanted(graph) {
        // A tool no descriptor declares is already named above: repeating it
        // here would send a person hunting two defects where there is one.
        if !tools.declares(&wanted.tool) {
            continue;
        }
        let asked = (wanted.tool.clone(), wanted.model.clone());
        if !judged.contains_key(&asked) {
            let outcome = match tools.resolve(&wanted.tool) {
                Err(reason) => EngineOutcome::NotHere(reason),
                Ok(bin) => match (tools.ask_recipe(&wanted.tool), &wanted.model) {
                    (None, _) => EngineOutcome::NotAssemblable,
                    // A model named to an engine that cannot receive one: the
                    // run would refuse it, so there is no line to try, and
                    // saying so before spending is this check's whole trade.
                    (Some(_), Some(_)) if tools.model_option(&wanted.tool).is_none() => {
                        EngineOutcome::NotAssemblable
                    }
                    (Some(recipe), model) => {
                        let args = match (model, tools.model_option(&wanted.tool)) {
                            (Some(model), Some(option)) => {
                                actions::command_line_naming_model(&recipe, &option, model)
                            }
                            _ => actions::command_line(&recipe),
                        };
                        let line = std::iter::once(bin.clone())
                            .chain(args.iter().cloned())
                            .collect::<Vec<_>>()
                            .join(" ");
                        EngineOutcome::Tried {
                            verdict: actions::probe_dry_run_with(probe, &bin, &recipe, &args),
                            line,
                        }
                    }
                },
            };
            judged.insert(asked.clone(), outcome);
        }

        let who = format!("{} → {}", wanted.step, wanted.tool);
        match judged.get(&asked).expect("appena inserito") {
            EngineOutcome::NotHere(reason) => untried.push(catalogue::say(
                "cli.flow.engine_not_invocable_here",
                &[("who", &who), ("reason", reason)],
            )),
            EngineOutcome::NotAssemblable => unassemblable.push(catalogue::say(
                "cli.flow.engine_line_not_assemblable",
                &[("who", &who)],
            )),
            EngineOutcome::Tried { line, verdict } => match verdict {
                ProbeVerdict::Sound => sound.push(who),
                // THE ENGINE'S WORDS IN FULL, AND THE LINE THAT PRODUCED
                // THEM. On fault 27 `agy`'s sentence said which flag had
                // eaten which argument: a diagnosis no word of ours could
                // replace. Cutting it, or summarising it, sends the reader
                // back to guessing.
                ProbeVerdict::Broken { said } => broken.push(catalogue::say(
                    "cli.flow.engine_line_broken",
                    &[("who", &who), ("line", line), ("said", said)],
                )),
                ProbeVerdict::CannotWork { said } => exhausted.push(format!("{who}: «{said}»")),
                ProbeVerdict::NotDeclared => untried.push(catalogue::say(
                    "cli.flow.engine_refusal_not_declared",
                    &[("who", &who), ("line", line)],
                )),
                ProbeVerdict::TimedOut { why } => untried.push(catalogue::say(
                    "cli.flow.engine_no_answer_to_line",
                    &[("who", &who), ("line", line), ("why", why)],
                )),
            },
        }
    }

    if !sound.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.command_lines_sound",
            &[("engines", &sound.join("; "))],
        ));
    }
    if !broken.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.command_lines_broken",
            &[("engines", &broken.join("; "))],
        ));
    }
    if !exhausted.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.engines_that_cannot_work_now",
            &[("engines", &exhausted.join("; "))],
        ));
    }
    if !untried.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.command_lines_untried",
            &[("engines", &untried.join("; "))],
        ));
    }
    if !unassemblable.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.command_lines_not_assemblable",
            &[("engines", &unassemblable.join("; "))],
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::super::check::check_report;
    use super::*;
    use flow::FlowFile;
    use registry::{registry_in, House};
    use std::path::{Path, PathBuf};

    // ── the credential homes ──────────────────────────────────────────

    /// A fake `codex` behaving like the real one **on this one question**: it
    /// answers on stderr, says «Not logged in» when the home holds no
    /// `auth.json`, and «Logged in using ChatGPT» when it does. The two exit
    /// codes are the measured ones — 1 and 0 — on purpose: were the verdict
    /// ever made to depend on the status, this test would stay green, and the
    /// twin test in `crates/actions/tests` says why that is not enough.
    ///
    /// **IT IS CALLED `codex` BECAUSE THE BOND IS THE EXECUTABLE**: on that
    /// name `profiles::cli_for_executable` decides which variable moves the home.
    fn a_fake_codex_that_answers_about_its_home(dir: &Path) -> String {
        let path = dir.join("codex");
        std::fs::write(
            &path,
            "#!/bin/sh\n\
             if [ \"$1\" = login ] && [ \"$2\" = status ]; then\n\
             \x20 if [ -f \"$CODEX_HOME/auth.json\" ]; then\n\
             \x20   echo 'Logged in using ChatGPT' >&2; exit 0\n\
             \x20 fi\n\
             \x20 echo 'Not logged in' >&2; exit 1\n\
             fi\n\
             echo 'No prompt provided via stdin.' >&2\n\
             exit 1\n",
        )
        .expect("scrivere il finto motore");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("bit di esecuzione");
        }
        path.to_string_lossy().into_owned()
    }

    /// A throwaway directory holding the fake engine and its descriptor.
    fn a_machine_with_a_real_fake_codex(declares_login: bool) -> (PathBuf, toolbox::Tools) {
        static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("prova-case-{}-{serial}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("la cartella di prova");
        a_fake_codex_that_answers_about_its_home(&dir);

        let login = if declares_login {
            r#","login_status":{"args":["login","status"],
               "logged_in_when":["logged in using"],
               "logged_out_when":["not logged in"]}"#
        } else {
            ""
        };
        let file = dir.join("tools.json");
        std::fs::write(
            &file,
            format!(
                r#"{{"tools":[{{"id":"codex","family":"ai_cli","label":"codex",
                   "detect":{{"command":"codex"}},
                   "ask":{{"args":["exec"],"prompt":"stdin",
                           "refuses_without_prompt":["no prompt provided via stdin"]}}
                   {login}}}]}}"#
            ),
        )
        .expect("scrivere i descrittori");
        let catalog = toolbox::Catalog::load(&[toolbox::Source::File(file)]);
        let tools = toolbox::Tools::new(
            catalog,
            toolbox::Machine {
                path_dirs: vec![dir.clone()],
                home: dir.clone(),
                env: BTreeMap::new(),
                version_probes: false,
            },
        );
        (dir, tools)
    }

    /// A profile store declaring one home, and active.
    fn a_store_pointing_at(home: &Path) -> profiles::ProfileStore {
        profiles::ProfileStore {
            profiles: vec![profiles::Profile {
                name: "prove".to_owned(),
                cli_id: "codex".to_owned(),
                home_dir: home.to_path_buf(),
                endpoint: None,
            }],
            active: [("codex".to_owned(), "prove".to_owned())]
                .into_iter()
                .collect(),
        }
    }

    /// **FAULT 39, THE OTHER HALF, AGAINST A REAL PROCESS.**
    ///
    /// The dry sift keeps saying «sound line» in both arms — that is its job,
    /// it takes the question away on purpose — and beside it appears what
    /// nobody said: which home this engine starts from, and whether that home
    /// holds credentials.
    ///
    /// **TWO ARMS, AND BOTH ARE NEEDED.** The first alone would stay green
    /// under a check that always shouted; the second alone would stay green
    /// under a check that looks at nothing. Together they say the answer comes
    /// from the home.
    ///
    /// **AND THE PROBE IS THE REAL ONE.** `RealDryProbe` starts a process: a
    /// fake would answer what we tell it, proving only that we can write an
    /// answer. Here the engine reads it off the disk.
    ///
    /// *Mutants run*: (a) reading `logged_in_when` before `logged_out_when` in
    /// `judge_login_status` — the first arm goes red, i.e. the original silence
    /// comes back; (b) removing `login_status` from the descriptor — see the
    /// test below.
    #[test]
    fn a_flow_check_says_which_home_the_engine_starts_from_and_whether_it_has_credentials() {
        let (dir, tools) = a_machine_with_a_real_fake_codex(true);
        let flow = flow_with_chain(r#""codex""#);
        let real = actions::RealDryProbe;

        let empty = dir.join("casa-vuota");
        std::fs::create_dir_all(&empty).expect("la casa senza credenziali");
        let store = a_store_pointing_at(&empty);
        let (report, unknown) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld {
                probe: &real,
                profiles: &store,
            }),
        );
        assert!(
            report.contains("HOMES WITHOUT CREDENTIALS"),
            "una casa senza credenziali si applica in silenzio: {report}"
        );
        assert!(
            report.contains(&empty.display().to_string()) && report.contains("codex/prove"),
            "chi legge deve sapere QUALE profilo e QUALE casa, o non sa cosa cambiare: {report}"
        );
        assert!(
            report.contains("Not logged in"),
            "le parole del motore sono la diagnosi: {report}"
        );
        assert!(
            report.contains("sound command lines"),
            "il vaglio a secco continua a dire la sua, e continua a dire il vero: {report}"
        );
        assert!(
            unknown.is_empty(),
            "un profilo senza credenziali NON fa fallire il controllo: punire chi non \
             c'entra è la cura sbagliata"
        );

        let full = dir.join("casa-piena");
        std::fs::create_dir_all(&full).expect("la casa autenticata");
        std::fs::write(full.join("auth.json"), "{}").expect("le credenziali");
        let store = a_store_pointing_at(&full);
        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld {
                probe: &real,
                profiles: &store,
            }),
        );
        assert!(
            report.contains("authenticated homes"),
            "una casa piena deve risultare piena: {report}"
        );
        assert!(
            !report.contains("HOMES WITHOUT CREDENTIALS"),
            "e non deve comparire fra quelle vuote: {report}"
        );
    }

    /// **A DESCRIPTOR WITHOUT THE BLOCK TRIPS NOTHING — AND DOES NOT SAY
    /// «AUTHENTICATED».**
    ///
    /// Mutant (b) written once and for all instead of run once: the home is
    /// empty, identical to the first arm above, and the sole change is that the
    /// descriptor does not say how to ask. The report must say **nobody
    /// looked**, never stay quiet and never reassure. A convenient default here
    /// would put the defect back for every engine without such a block yet —
    /// that is, for all the ones still to come.
    #[test]
    fn a_descriptor_without_the_block_makes_the_check_say_nobody_looked() {
        let (dir, tools) = a_machine_with_a_real_fake_codex(false);
        let flow = flow_with_chain(r#""codex""#);
        let real = actions::RealDryProbe;
        let empty = dir.join("casa-vuota");
        std::fs::create_dir_all(&empty).expect("la casa senza credenziali");
        let store = a_store_pointing_at(&empty);

        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld {
                probe: &real,
                profiles: &store,
            }),
        );

        assert!(
            report.contains("homes whose authentication nobody could read")
                && report.contains("nobody looked"),
            "un'assenza deve dirsi: {report}"
        );
        assert!(
            !report.contains("authenticated homes"),
            "«nessuno ha guardato» non è «è autenticato»: {report}"
        );
        assert!(
            !report.contains("HOMES WITHOUT CREDENTIALS"),
            "e non è nemmeno «non è autenticato»: inventare un no dove non si è \
             guardato manderebbe a riparare una casa sana: {report}"
        );
    }

    // ── the command lines tried dry ───────────────────────────────────

    /// A fake machine with engines inside it, and their descriptors.
    ///
    /// Nothing depends on what is installed on whoever runs: the path is a
    /// temporary directory, and the engines are empty files with the execute
    /// bit — never started, since the probe of these tests is a fake.
    fn tools_with_engines(entries: &[(&str, &str)]) -> toolbox::Tools {
        static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("prova-motori-{}-{serial}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("la cartella di prova");
        let mut declared = Vec::new();
        for (id, ask) in entries {
            let path = dir.join(id);
            std::fs::write(&path, "").expect("il finto eseguibile");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                    .expect("bit di esecuzione");
            }
            declared.push(format!(
                r#"{{"id":"{id}","family":"ai_cli","label":"{id}","detect":{{"command":"{id}"}}{ask}}}"#
            ));
        }
        let file = dir.join("tools.json");
        std::fs::write(&file, format!(r#"{{"tools":[{}]}}"#, declared.join(",")))
            .expect("scrivere");
        let catalog = toolbox::Catalog::load(&[toolbox::Source::File(file)]);
        toolbox::Tools::new(
            catalog,
            toolbox::Machine {
                path_dirs: vec![dir.clone()],
                home: dir,
                env: BTreeMap::new(),
                version_probes: false,
            },
        )
    }

    /// A probe that runs nothing and answers what we tell it, keyed on the
    /// name of the executable handed to it.
    struct ScriptedProbe(Vec<(&'static str, &'static str)>);

    impl actions::DryProbe for ScriptedProbe {
        fn run(&self, bin: &str, _args: &[String], _stdin: Option<Vec<u8>>) -> actions::DryRun {
            let said = self
                .0
                .iter()
                .find(|(name, _)| bin.ends_with(name))
                .map(|(_, said)| *said)
                .unwrap_or("");
            actions::DryRun::Answered {
                stdout: String::new(),
                stderr: said.to_owned(),
            }
        }
    }

    /// It answers nothing about credentials: these tests are about command
    /// lines, and with no active profile the question is not even asked. A fake
    /// that answered would be claiming something about that world, and there
    /// are tests for it on purpose.
    impl actions::LoginProbe for ScriptedProbe {
        fn ask(
            &self,
            _bin: &str,
            _args: &[String],
            _env: &BTreeMap<String, String>,
        ) -> actions::DryRun {
            actions::DryRun::NoAnswer {
                why: "questa sonda non risponde alla domanda sulle credenziali".to_owned(),
            }
        }
    }

    fn flow_with_chain(chain: &str) -> FlowFile {
        let json = format!(
            r#"{{
                "id": "prova",
                "description": "flusso di prova",
                "graph": {{
                    "steps": [{{
                        "id": "chiedi",
                        "deps": [],
                        "action": "external_engine",
                        "max_attempts": 1,
                        "when": null,
                        "with": {{"tool": {chain}, "stdin": "ciao", "timeout_secs": 10}},
                        "input_schema": {{"type": "any"}},
                        "output_schema": {{"type": "any"}}
                    }}],
                    "skippable_dependencies": []
                }},
                "inputs": {{}}
            }}"#
        );
        serde_json::from_str(&json).expect("caricare il flusso")
    }

    /// Like `flow_with_chain`, with the step naming which model it wants of
    /// each engine.
    fn flow_asking_model(chain: &str, model: &str) -> FlowFile {
        let json = format!(
            r#"{{
                "id": "prova",
                "description": "flusso di prova",
                "graph": {{
                    "steps": [{{
                        "id": "chiedi",
                        "deps": [],
                        "action": "external_engine",
                        "max_attempts": 1,
                        "when": null,
                        "with": {{"tool": {chain}, "model": {model}, "stdin": "ciao", "timeout_secs": 10}},
                        "input_schema": {{"type": "any"}},
                        "output_schema": {{"type": "any"}}
                    }}],
                    "skippable_dependencies": []
                }},
                "inputs": {{}}
            }}"#
        );
        serde_json::from_str(&json).expect("caricare il flusso")
    }

    /// A probe that remembers **the line it was given**. The line written in
    /// the report and the line tried are two things, and a test reading only
    /// the first would pass a check that shows one and tries another.
    #[derive(Default)]
    struct RecordingProbe(std::sync::Mutex<Vec<Vec<String>>>);

    impl actions::DryProbe for RecordingProbe {
        fn run(&self, _bin: &str, args: &[String], _stdin: Option<Vec<u8>>) -> actions::DryRun {
            self.0.lock().expect("la sonda").push(args.to_vec());
            actions::DryRun::Answered {
                stdout: String::new(),
                stderr: "input must be provided".to_owned(),
            }
        }
    }

    impl actions::LoginProbe for RecordingProbe {
        fn ask(
            &self,
            _bin: &str,
            _args: &[String],
            _env: &BTreeMap<String, String>,
        ) -> actions::DryRun {
            actions::DryRun::NoAnswer {
                why: "questa sonda non risponde alla domanda sulle credenziali".to_owned(),
            }
        }
    }

    /// **THE LINE TRIED IS THE ONE THAT WOULD RUN.** Assembled from the
    /// descriptor alone it would carry no model and still be called sound:
    /// the two doors of fault 1, held together here.
    #[test]
    fn the_line_tried_without_spending_carries_the_model_the_step_asked_for() {
        let flow = flow_asking_model(r#""motore""#, r#"{"motore": "il-modello-forte"}"#);
        let tools = tools_with_engines(&[("motore", REFUSES_AND_TAKES_A_MODEL)]);
        let probe = RecordingProbe::default();

        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld::without_profiles(&probe)),
        );

        let tried = probe.0.lock().expect("la sonda").clone();
        assert_eq!(
            tried,
            vec![vec![
                "-p".to_owned(),
                "--model".to_owned(),
                "il-modello-forte".to_owned()
            ]],
            "il rapporto dice: {report}"
        );
    }

    /// **AND ONE ASKED OF AN ENGINE THAT CANNOT HEAR IT IS NOT TRIED.** The run
    /// would refuse it: assembling the line without the model and calling it
    /// sound would send a person to spend on a chain that will stop.
    #[test]
    fn an_engine_that_cannot_be_told_a_model_has_no_line_to_try() {
        let flow = flow_asking_model(r#""motore""#, r#"{"motore": "il-modello-forte"}"#);
        let tools = tools_with_engines(&[("motore", REFUSES)]);
        let probe = RecordingProbe::default();

        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld::without_profiles(&probe)),
        );

        assert!(
            probe.0.lock().expect("la sonda").is_empty(),
            "non c'era nessuna riga da provare: {report}"
        );
        assert!(!report.contains("sound command lines"), "{report}");
    }

    const REFUSES: &str = r#","ask":{"args":["-p"],"prompt":"stdin","refuses_without_prompt":["input must be provided"]}"#;
    const REFUSES_AND_TAKES_A_MODEL: &str = r#","ask":{"args":["-p"],"prompt":"stdin","refuses_without_prompt":["input must be provided"]},"capabilities":{"choose_model":{"args":["--model"],"takes_value":true}}"#;
    const SAYS_NOTHING: &str = r#","ask":{"args":["-p"],"prompt":"stdin"}"#;
    const NO_ASK: &str = "";

    /// **A SOUND LINE CAN BE SEEN, AND IT COSTS ZERO.** It is the check fault 1
    /// asked for and nobody wrote, because it looked like it meant spending.
    #[test]
    fn a_line_the_engine_only_complains_about_the_missing_prompt_is_called_sound() {
        let flow = flow_with_chain(r#""motore""#);
        let tools = tools_with_engines(&[("motore", REFUSES)]);
        let probe = ScriptedProbe(vec![("motore", "Input must be provided through stdin")]);

        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld::without_profiles(&probe)),
        );

        assert!(report.contains("sound command lines"), "{report}");
        assert!(report.contains("chiedi → motore"), "{report}");
    }

    /// **THE ENGINE'S WORDS ARE THE DIAGNOSIS, AND GO IN WHOLE.** On fault 27
    /// `agy`'s sentence said which flag had eaten which argument; a report
    /// saying only «broken» sends the reader back to guessing, and is worth no
    /// more than its own absence.
    #[test]
    fn a_broken_line_is_reported_with_the_engines_own_words_and_the_line_that_produced_it() {
        let flow = flow_with_chain(r#""motore""#);
        let tools = tools_with_engines(&[("motore", REFUSES)]);
        let probe = ScriptedProbe(vec![(
            "motore",
            "--print took \"--output-format\" as its prompt",
        )]);

        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld::without_profiles(&probe)),
        );

        assert!(report.contains("BROKEN command lines"), "{report}");
        assert!(
            report.contains("--print took \"--output-format\" as its prompt"),
            "senza le parole del motore la riga rossa non dice cosa correggere: {report}"
        );
        assert!(
            report.contains("assembled line «") && report.contains("-p»"),
            "e senza la riga montata non si sa nemmeno cosa è stato provato: {report}"
        );
    }

    /// **THE WHOLE CHAIN IS LOOKED AT, NOT THE FIRST.** Fault 27 sat in the
    /// **second** engine of every chain, and lived undisturbed precisely
    /// because no flow put it first. A check reading the first engine alone is
    /// a check that does not look where the defect was.
    #[test]
    fn every_engine_of_the_chain_is_tried_not_only_the_first() {
        let flow = flow_with_chain(r#"["primo", "secondo", "terzo"]"#);
        let tools =
            tools_with_engines(&[("primo", REFUSES), ("secondo", REFUSES), ("terzo", REFUSES)]);
        let probe = ScriptedProbe(vec![
            ("primo", "Input must be provided through stdin"),
            ("secondo", "took --output-format as its prompt"),
            ("terzo", "Input must be provided through stdin"),
        ]);

        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld::without_profiles(&probe)),
        );

        assert!(
            report.contains("chiedi → secondo"),
            "il secondo della catena non è stato guardato: {report}"
        );
        assert!(
            report.contains("took --output-format as its prompt"),
            "{report}"
        );
        assert!(report.contains("chiedi → terzo"), "né il terzo: {report}");
    }

    /// **«NOT TRIED» AND «NOT ASSEMBLABLE» ARE TWO DIFFERENT FACTS.** An engine
    /// with no `ask` block has no line to try — cured by writing the
    /// descriptor; one that has the line but does not declare how it refuses
    /// has a line nobody looked at — cured by running it. Under one word they
    /// would send a person to do the wrong work, and fault 32 lives in the
    /// first of the two.
    #[test]
    fn a_missing_ask_block_is_not_confused_with_a_line_nobody_looked_at() {
        let flow = flow_with_chain(r#"["senza-ask", "senza-rifiuto"]"#);
        let tools = tools_with_engines(&[("senza-ask", NO_ASK), ("senza-rifiuto", SAYS_NOTHING)]);
        let probe = ScriptedProbe(vec![("senza-rifiuto", "un errore qualunque")]);

        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld::without_profiles(&probe)),
        );

        let untried = report
            .lines()
            .find(|line| line.starts_with("command lines not tried"))
            .unwrap_or_else(|| panic!("manca la riga «non provate»: {report}"));
        let unassemblable = report
            .lines()
            .find(|line| line.starts_with("command lines that cannot be assembled"))
            .unwrap_or_else(|| panic!("manca la riga «non montabili»: {report}"));

        assert!(untried.contains("senza-rifiuto"), "{untried}");
        assert!(
            !untried.contains("senza-ask"),
            "un motore senza `ask` non è una riga non provata: {untried}"
        );
        assert!(unassemblable.contains("senza-ask"), "{unassemblable}");
        assert!(!unassemblable.contains("senza-rifiuto"), "{unassemblable}");
    }

    /// **AN EXHAUSTED ENGINE IS NOT A BROKEN LINE**, and its sentence is the
    /// fourth. Confusing them would send a person to correct a healthy
    /// descriptor when waiting was enough.
    #[test]
    fn an_engine_that_cannot_work_now_gets_its_own_sentence() {
        let flow = flow_with_chain(r#""motore""#);
        let tools = tools_with_engines(&[(
            "motore",
            r#","ask":{"args":["-p"],"prompt":"stdin","unusable_when":["weekly limit"],"refuses_without_prompt":["input must be provided"]}"#,
        )]);
        let probe = ScriptedProbe(vec![("motore", "You've hit your weekly limit")]);

        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld::without_profiles(&probe)),
        );

        assert!(
            report.contains("engines that cannot work right now"),
            "{report}"
        );
        assert!(
            !report.contains("BROKEN command lines"),
            "la riga è sana, è la quota che è finita: {report}"
        );
    }

    /// **WITH NO PROBE THE REPORT STAYS SILENT**, it does not call sound lines
    /// it never looked at: the same rule as the absent detector, and without it
    /// `--no-engines` would become a way to make the check state something it
    /// never verified.
    #[test]
    fn with_no_engines_the_report_says_nothing_about_command_lines() {
        let flow = flow_with_chain(r#""motore""#);
        let tools = tools_with_engines(&[("motore", REFUSES)]);

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert!(!report.contains("command lines"), "{report}");
    }

    /// Steps that write their own `args` assemble no line from the descriptor:
    /// they are the ones invoking `cargo` or `git` through the same action, and
    /// calling them «not assemblable» would be an alarm on a healthy step —
    /// noise that teaches people to stop reading the report.
    #[test]
    fn a_step_that_writes_its_own_arguments_is_not_reported_as_unassemblable() {
        let json = r#"{
            "id": "prova",
            "description": "flusso di prova",
            "graph": {
                "steps": [{
                    "id": "prove",
                    "deps": [],
                    "action": "external_engine",
                    "max_attempts": 1,
                    "when": null,
                    "with": {"tool": "cargo", "args": ["test"], "timeout_secs": 10},
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"}
                }],
                "skippable_dependencies": []
            },
            "inputs": {}
        }"#;
        let flow: FlowFile = serde_json::from_str(json).expect("caricare il flusso");
        let tools = tools_with_engines(&[("cargo", NO_ASK)]);
        let probe = ScriptedProbe(vec![]);

        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld::without_profiles(&probe)),
        );

        assert!(!report.contains("command lines"), "{report}");
    }
}
