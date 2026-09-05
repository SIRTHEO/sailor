//! Which engines a step may try here, in which order, and why the others are
//! set aside before anything is spent.

use crate::cost::now_secs;
use crate::engine::ExternalEngineAction;
use crate::equipment::current_equipment_for;
use crate::recipe::{
    command_line, mentions_any, says_it_cannot_work, PromptVia, SessionRecipe, ToolResolver,
};
use crate::session::session_lines;
use crate::spec::{DataClass, EngineSpec};
use crate::{budget, cooldown, Declared, EXTERNAL_ENGINE_ACTION};
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

    /// Chi eseguire, in ordine di preferenza. `bin` e `tool` non convivono: due
    /// risposte alla stessa domanda vorrebbero una precedenza, e una precedenza
    /// fra «il nome che ho scritto» e «quello che c'è sulla macchina» sarebbe una
    /// regola che nessuno ricorda al momento giusto.
    ///
    /// Restituisce anche i motori che **non** si possono usare qui, col motivo:
    /// se nessuno resta, quel motivo è tutto ciò che chi legge avrà.
    pub(crate) fn candidates(&self, spec: &EngineSpec) -> Result<(Vec<Candidate>, Vec<Refused>), ActionError> {
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
                    // Un comando scritto a mano non ha un descrittore: non c'è
                    // niente che dichiari che sia un motore, e infatti la riga
                    // non si scrive già per via dell'`id` assente.
                    can_be_asked: false,
                    why: None,
                    session: SessionRecipe::default(),
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
                    // Le opzioni scritte nel passo vincono sulla ricetta: chi le
                    // ha scritte sta dicendo qualcosa di preciso su *questa*
                    // chiamata, e sovrascriverle sarebbe decidere al posto suo.
                    if step_said_args {
                        let declared = tools.ask_recipe(id);
                        usable.push(Candidate {
                            id: Some(id.clone()),
                            bin,
                            args: spec.args.clone(),
                            prompt: PromptVia::Stdin,
                            // Il descrittore dice se questo strumento è un
                            // motore, anche quando le opzioni non vengono da
                            // lui: `git` e `cargo` non dichiarano `ask`, e le
                            // loro esecuzioni non sono chiamate a un modello.
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
                            // **NIENTE CONSUMO QUANDO LE OPZIONI LE SCRIVE IL
                            // PASSO**, ed è la stessa regola di due righe più
                            // su applicata al dato nuovo: le opzioni del
                            // consumo si accodano a quelle della ricetta, e qui
                            // la ricetta non detta niente. Accodarle lo stesso
                            // vorrebbe dire allungare alle spalle di chi ha
                            // scritto quella riga di comando una domanda che
                            // non ha fatto. Il consumo resta sconosciuto — la
                            // riga nel deposito si scrive comunque, e dice
                            // proprio questo.
                            declared_usage: None,
                            session: SessionRecipe::default(),
                        });
                        continue;
                    }
                    match tools.ask_recipe(id) {
                        Some(recipe) => usable.push(Candidate {
                            id: Some(id.clone()),
                            bin,
                            args: command_line(&recipe),
                            prompt: recipe.prompt,
                            session: session_lines(&recipe, tools.session_recipe(id)),
                            unusable_when: recipe.unusable_when,
                            exhausted_when: recipe.exhausted_when,
                            cooldown_secs: recipe.cooldown_secs,
                            waits_for_a_person_when: recipe.waits_for_a_person_when,
                            declared_usage: recipe.usage.map(|usage| usage.declared),
                            // Siamo dentro il ramo che ha trovato una ricetta
                            // `ask`: questo strumento è per definizione un
                            // motore.
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

/// Un motore chiesto dal passo che qui non si può nemmeno provare.
pub(crate) struct Refused {
    pub(crate) id: String,
    pub(crate) reason: String,
    /// Vero quando il risolutore non ha saputo dire quale eseguibile sia — la
    /// distinzione conta: un passo che chiede **un** motore solo e non lo trova
    /// deve dare `tool_unavailable` col motivo del risolutore, come ha sempre
    /// fatto. La catena non deve peggiorare il caso più comune.
    pub(crate) unresolved: bool,
}

impl Refused {
    pub(crate) fn line(&self) -> String {
        format!("«{}»: {}", self.id, self.reason)
    }
}

/// Un motore che si può provare: già risolto in un eseguibile, con le opzioni
/// con cui interrogarlo e le parole con cui dichiara di non poter lavorare.
pub(crate) struct Candidate {
    /// L'identificativo, se è stato chiesto per identificativo. `None` quando il
    /// passo ha scritto un comando così com'è.
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
    /// Dove leggere il consumo nell'uscita di questo motore. `None` quando il
    /// descrittore non lo dichiara, o quando le opzioni le ha scritte il passo.
    pub(crate) declared_usage: Option<Declared>,
    /// **QUESTO STRUMENTO È UN MOTORE**, cioè il suo descrittore dichiara come
    /// gli si fa una domanda (`ask`).
    ///
    /// Serve a decidere se la sua invocazione va in `model_calls`. `git` e
    /// `cargo` stanno nel catalogo e si eseguono da un passo come tutti gli
    /// altri, ma non si interrogano: non consumano quota di nessun
    /// abbonamento, e contarli fra le chiamate ai modelli falsa ogni totale che
    /// le somma. **Il criterio è del descrittore e non di un elenco di nomi
    /// scritto qui**: un elenco a mano invecchia al primo strumento nuovo, e
    /// nessun controllo lo direbbe.
    ///
    /// Resta vero anche quando le opzioni le scrive il passo: un motore
    /// interrogato a modo proprio è sempre un motore, e la sua riga si scrive —
    /// col consumo sconosciuto, che è l'informazione giusta.
    pub(crate) can_be_asked: bool,
    /// Why this engine was moved to the front, when the fuel said so.
    pub(crate) why: Option<String>,
    /// Le righe di comando alternative con cui questo motore apre, riprende o
    /// ramifica una sessione — già montate col resto della ricetta, e ancora
    /// col segnaposto al posto dell'identificativo.
    ///
    /// Tutta vuota per chi non lo sa fare, e per chi si è scritto le opzioni
    /// nel passo: chi scrive la propria riga di comando la sta decidendo lui, e
    /// infilarci dentro un'opzione che non ha chiesto sarebbe decidere al posto
    /// suo — la stessa regola che vale già per le opzioni del consumo.
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
    use flow::{Action, ActionOutcome, SharedState};
    use serde_json::json;

    // ── chiedere uno strumento per identificativo ─────────────────────

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

    /// Il passo nomina uno strumento; chi eseguirlo lo decide la macchina.
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

    /// Lo strumento che qui non c'è: il passo si ferma **prima** di spendere
    /// qualunque cosa, e porta con sé il motivo di chi ha guardato la macchina.
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

    /// Un motore registrato senza risolutore non indovina un binario dal nome
    /// dello strumento: dice come si ripara il registro.
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

    // ── la catena di motori ───────────────────────────────────────────

    /// Una macchina finta con tre motori: uno che dichiara di essere esaurito,
    /// uno che risponde, uno che non è installato.
    struct Chain;

    impl ToolResolver for Chain {
        fn resolve(&self, id: &str) -> Result<String, String> {
            match id {
                // Stampa il messaggio di un motore esaurito ed esce 1.
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
                // Risolvibile ma senza ricetta: un passo che non scrive le
                // opzioni non sa come interrogarlo.
                _ => None,
            }
        }
    }

    /// Un eseguibile finto che dice di essere esaurito ed esce in errore.
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

    /// **Il caso del 29/08/2026.** Il primo motore dichiara di essere esaurito;
    /// il lavoro non muore, passa al secondo, e il secondo risponde.
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

    /// Un eseguibile finto che dice di essere esaurito **ed esce zero**.
    ///
    /// **PERCHÉ SERVE UN SECONDO FINTO MOTORE.** Il gemello qui sopra esce 1, e
    /// tutte le prove ermetiche su questa catena facevano così: il motore
    /// esaurito usciva **sempre** in errore, quindi nessuna di esse guardava mai
    /// il ramo riuscito. Un difetto che vive solo di là non poteva diventare
    /// rosso.
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

    /// **DIRLO E USCIRE ZERO.**
    ///
    /// **IL GUASTO NON HA ANCORA UN NUMERO, E NON GLIENE DO UNO.** Sta in
    /// `docs/da-fare.md` in attesa della fusione: due rami stanno numerando
    /// righe nuove nello stesso momento, e il numero che questo lavoro si
    /// aspettava di prendere è già stato preso mentre era in corso. Un numero
    /// sbagliato in un commento manda a leggere il guasto di qualcun altro.
    ///
    /// Un motore che dichiara con le proprie parole di non poter lavorare, e
    /// **esce zero**. Fino al 01/09/2026 `says_it_cannot_work` veniva
    /// interrogato solo dentro il ramo `ExitError`: nel ramo riuscito la
    /// risposta era presa per buona, il ripiego non scattava, e la riga del
    /// deposito nasceva con `error_type: None` — cioè il passo risultava
    /// riuscito, e il motore dopo di lui non partiva mai.
    ///
    /// **NON È IPOTETICO SU QUESTA MACCHINA.** È la forma del guasto 39:
    /// `CODEX_HOME=<cartella vuota> codex exec < /dev/null` risponde «No prompt
    /// provided via stdin» ed esce **zero**. E la sonda a secco la distinzione
    /// ce l'aveva già — `judge_dry_run` è applicata a `Ok` *e* a `ExitError` —
    /// quindi il controllo statico e la corsa vera dicevano cose diverse sullo
    /// stesso motore.
    ///
    /// La coppia con `an_engine_that_says_it_is_out_hands_the_work_to_the_next_one`
    /// è tutta la dimostrazione: le stesse parole, l'unica differenza è il
    /// codice d'uscita, e il ripiego deve scattare in tutti e due i casi.
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

    /// **E DA SOLO LO DICE, INVECE DI FINGERE DI AVER RISPOSTO.**
    ///
    /// Senza nessun ripiego dietro non c'è niente da salvare, ma resta la
    /// diagnosi: chi legge «esaurito» sa che deve aspettare o cambiare profilo,
    /// chi legge un passo **verde** va a cercare la risposta che non c'è. Era la
    /// seconda metà del difetto, e la peggiore: il passo si chiudeva riuscito.
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

    /// **IL GUASTO 31, RESO UN FATTO INVECE DI UNA LETTURA.**
    ///
    /// Lo stesso motore esaurito di qui sopra, con la sola differenza che
    /// conta: il suo descrittore **non dichiara nessuna parola** di
    /// `unusable_when`. `says_it_cannot_work` su un elenco vuoto è `false`,
    /// quindi il suo esaurirsi passa per un fallimento qualunque, il passo
    /// muore lì, e il motore successivo **non parte mai**. È il descrittore di
    /// `agy` così com'è spedito il 31/08/2026, ed è la ragione per cui nella
    /// catena `claude-code → agy → codex` un `agy` esaurito uccide il passo e
    /// `codex` non viene nemmeno provato.
    ///
    /// **PERCHÉ NON BASTA LA GEMELLA SUI FRAMMENTI VUOTI.** Quella prova un
    /// descrittore scritto male; questa prova un descrittore che **tace**, che
    /// è il caso vero e quello che nessuno legge come un difetto: un campo
    /// assente sembra una scelta, un campo pieno di stringhe vuote sembra un
    /// errore. Il comportamento è lo stesso, e la differenza è che al primo
    /// nessuno guarda.
    ///
    /// La coppia con la prova qui sopra è tutta la dimostrazione: elenco
    /// popolato, il secondo parte; elenco vuoto, il secondo non parte.
    #[test]
    fn an_engine_that_declares_no_exhaustion_words_kills_the_chain() {
        /// Come `ChainIn`, ma al primo motore si toglie ciò che `agy` non ha.
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

    /// Un eseguibile finto che fallisce **parlando**, ma non con le parole con
    /// cui quel motore dichiarerebbe di essere esaurito.
    ///
    /// **PERCHÉ NON BASTA UN COMANDO CHE FALLISCE MUTO.** La prima versione
    /// della prova qui sotto usava `false`, che esce 1 senza dire niente, e un
    /// mutante che faceva scattare il ripiego su *qualunque* uscita le è
    /// passato sotto: con l'uscita vuota, «qualunque uscita» e «quelle parole»
    /// si comportano uguale. Un fallimento vero parla, ed è quello il caso che
    /// questa prova deve tenere.
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

    /// **La metà che conta di più.** Un fallimento qualunque NON scende la
    /// catena: un mandato scritto male deve fermarsi lì, non trovare più in
    /// basso un motore che risponde comunque — quella sarebbe una risposta
    /// sbagliata con la faccia di una buona.
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

    /// Un descrittore scritto a mano con un frammento **vuoto** fra le parole
    /// di `unusable_when`: quel frammento è contenuto in qualunque testo, e
    /// senza una guardia farebbe scendere la catena a **ogni** fallimento —
    /// cioè esattamente il guasto che la catena esiste per non introdurre. Chi
    /// ha scritto quel descrittore non se ne accorgerebbe: funzionerebbe, e
    /// darebbe risposte sbagliate.
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

    /// Quando ogni motore della catena dichiara di non poter lavorare, il passo
    /// è rosso con il motivo di **ognuno**: chi legge deve vedere l'intera
    /// catena, non solo l'ultimo anello.
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

    /// Il descrittore decide dove va il testo della domanda. Senza questo, un
    /// flusso dovrebbe conoscere le opzioni di ogni motore — ed è la ragione per
    /// cui i flussi erano legati a uno solo.
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

        // `echo` stampa i propri argomenti: se la domanda fosse finita
        // sull'ingresso invece che in coda agli argomenti, qui non ci sarebbe.
        assert_eq!(output["stdout"], "ha-risposto-il-secondo la-domanda\n");
    }

    /// Un motore che c'è ma non dichiara come lo si interroga non viene
    /// indovinato: si mette da parte col motivo, e si prova il prossimo.
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

    /// Le opzioni scritte nel passo vincono sulla ricetta: chi le ha scritte sta
    /// dicendo qualcosa di preciso su questa chiamata.
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
