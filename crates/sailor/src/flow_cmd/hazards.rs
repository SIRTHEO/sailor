//! What a flow file can carry that the check refuses before the run: absolute
//! paths, text mounted into a command, sweeping commits, pointers that cannot
//! match, and steps that contradict themselves.

use flow::reference;
use flow::{ActionRegistry, FlowFile};
use serde_json::Value;

use super::check::engines_of;

/// I campi che dicono **dove** un passo lavora o **quale** binario esegue.
///
/// Un percorso assoluto qui non è un dettaglio del testo: è la posizione in cui
/// il passo lavorerà davvero, ed è il guasto 25 parola per parola — sette passi
/// con la casa di chi scriveva scritta in chiaro, e un flusso che lanciato da un
/// clone commetteva nel repository principale senza dirlo.
pub(super) const POSITION_FIELDS: [&str; 2] = ["workdir", "bin"];

/// I prefissi che fanno di un pezzo di testo un percorso assoluto.
///
/// **È UN ELENCO DICHIARATO, NON UN ANALIZZATORE, E IL PREZZO È DICHIARATO.**
/// È la stessa scelta già pagata da `identifiers_are_in_english`: un elenco non
/// ha falsi positivi e lascia passare ciò che non conosce, mentre un
/// analizzatore di percorsi dentro il testo libero di un prompt chiamerebbe
/// percorso ogni `/` — a cominciare dai puntatori JSON — e verrebbe spento
/// entro un giorno. Chi incontra un percorso che questo elenco non vede lo
/// aggiunge qui.
const ABSOLUTE_PREFIXES: [&str; 4] = ["/Users/", "/home/", "/private/", "~/"];

/// Un percorso assoluto trovato scritto a mano dentro un flusso.
pub(super) struct HardcodedPath {
    pub(super) step: String,
    pub(super) field: String,
    pub(super) value: String,
    /// Vero quando sta in un campo di posizione: il flusso **non gira** altrove,
    /// quindi è un errore. Falso quando sta dentro un testo: lì il flusso gira
    /// lo stesso e il percorso è un'istruzione a chi legge, quindi è un avviso.
    pub(super) fatal: bool,
}

/// I percorsi assoluti scritti a mano in un flusso.
///
/// **DUE ESITI, PERCHÉ SONO DUE GUASTI DIVERSI.** Un `workdir` assoluto decide
/// dove il passo lavora: il flusso si può eseguire in un posto solo, e altrove
/// fa danno invece di fallire. Un percorso dentro il testo di un prompt non
/// impedisce al flusso di girare — è un'istruzione che diventa sbagliata
/// altrove, e chi la riscrive sta riscrivendo un'istruzione, non un campo.
/// Perciò il primo è un errore e il secondo un avviso: chiamarli allo stesso
/// modo vorrebbe dire o bloccare flussi sani, o lasciar passare il guasto 25.
///
/// **I PUNTATORI NON SONO PERCORSI.** `{"$from": "/answer/verdict"}` comincia
/// per `/` e non è un percorso: è un puntatore JSON, e compare in quattro
/// flussi su cinque. Il valore di `$from` e quello di `$json` si saltano. Il
/// testo letterale dentro un `$join` invece si guarda: l'elenco di prefissi non
/// può scambiare `/answer/verdict` per un percorso, quindi saltarlo non
/// comprerebbe niente e perderebbe i prompt composti a pezzi — che è dove i due
/// percorsi di `sviluppa-sailor` stanno davvero.
/// Un rinvio montato dentro un campo che viene **eseguito**.
#[derive(Debug)]
pub(super) struct OutsideTextInCommand {
    pub(super) step: String,
    pub(super) field: String,
}

/// I campi il cui contenuto non viene letto: viene eseguito. Oggi ne esiste
/// uno solo — `command` di `shell_check` — ed è un elenco perché il giorno che
/// ne nasce un secondo, la regola deve valere anche per quello senza che
/// nessuno se ne ricordi.
const EXECUTED_FIELDS: &[&str] = &["command"];

/// **CIÒ CHE VIENE DA FUORI VA IN `env`, MAI IN `command`.** La regola era
/// scritta sopra `ShellCheckAction` e non la applicava nessun codice: un
/// augurio, non una regola.
///
/// Il comando è testo di shell e viene eseguito. Un titolo di richiesta di
/// modifica — che su un remoto condiviso lo scrive chiunque — montato dentro
/// `command` è un comando scritto da chi ha aperto la richiesta. Dentro una
/// variabile d'ambiente resta un dato, e il comando la legge fra virgolette.
///
/// **VALE PER I COMANDI QUANTO PER I MOTORI, E QUESTO È IL PUNTO.** Finché
/// l'unico testo che entrava veniva da un modello, chi scriveva flussi stava
/// attento. L'uscita di un `git` sembra innocua proprio perché non viene da un
/// modello, ed è esattamente per questo che va trattata uguale.
///
/// **UN `$join` DI SOLE LETTERE NON È UNA SEGNALAZIONE.** Comporre un comando
/// da pezzi scritti a mano è sano; ciò che si guarda è se dentro quel campo
/// compare un rinvio — `$from` o `$json` — cioè un valore che questo flusso non
/// ha scritto. Segnalare qualunque composizione renderebbe rosso ogni flusso
/// sano, e un controllo così viene spento entro un giorno.
pub(super) fn outside_text_in_command(flow: &FlowFile) -> Vec<OutsideTextInCommand> {
    let mut found = Vec::new();
    for step in flow.graph.steps() {
        let Some(with) = step.with.as_ref() else {
            continue;
        };
        let Value::Object(fields) = with else {
            continue;
        };
        for name in EXECUTED_FIELDS {
            if let Some(value) = fields.get(*name) {
                if holds_a_reference(value) {
                    found.push(OutsideTextInCommand {
                        step: step.id.clone(),
                        field: (*name).to_owned(),
                    });
                }
            }
        }
    }
    found
}

/// Vero se da qualche parte qui dentro c'è un valore che il flusso non ha
/// scritto: un rinvio all'uscita di un altro passo.
fn holds_a_reference(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields.iter().any(|(key, inner)| {
            key == reference::FROM_KEY || key == reference::JSON_KEY || holds_a_reference(inner)
        }),
        Value::Array(items) => items.iter().any(holds_a_reference),
        _ => false,
    }
}

pub(super) fn hardcoded_paths(flow: &FlowFile) -> Vec<HardcodedPath> {
    let mut found = Vec::new();
    for step in flow.graph.steps() {
        if let Some(with) = step.with.as_ref() {
            walk_for_paths(&step.id, "", with, &mut found);
        }
    }
    // L'ingresso dichiarato è scritto a mano quanto il `with`, ed è dove sta il
    // testo dell'innesco.
    for (name, declared) in &flow.inputs {
        walk_for_paths(name, "", declared, &mut found);
    }
    found
}

fn walk_for_paths(step: &str, field: &str, value: &Value, found: &mut Vec<HardcodedPath>) {
    match value {
        Value::Object(fields) => {
            for (key, inner) in fields {
                // Il valore di un puntatore non è un percorso, e guardarci
                // dentro riempirebbe il rapporto di falsi positivi.
                if key == reference::FROM_KEY || key == reference::JSON_KEY {
                    continue;
                }
                let trail = if field.is_empty() {
                    key.clone()
                } else {
                    format!("{field}.{key}")
                };
                walk_for_paths(step, &trail, inner, found);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_for_paths(step, field, item, found);
            }
        }
        Value::String(text) => {
            let is_position = field
                .rsplit('.')
                .next()
                .is_some_and(|last| POSITION_FIELDS.contains(&last));
            if is_position && (text.starts_with('/') || text.starts_with("~/")) {
                found.push(HardcodedPath {
                    step: step.to_owned(),
                    field: field.to_owned(),
                    value: text.clone(),
                    fatal: true,
                });
            } else if let Some(prefix) = ABSOLUTE_PREFIXES
                .iter()
                .find(|prefix| text.contains(**prefix))
            {
                found.push(HardcodedPath {
                    step: step.to_owned(),
                    field: field.to_owned(),
                    value: (*prefix).to_owned(),
                    fatal: false,
                });
            }
        }
        _ => {}
    }
}

// ── a `git commit` that does not say what it commits ──────────────────────

/// A step that runs `git commit` without delimiting what it commits.
#[derive(Debug)]
pub(super) struct UndelimitedCommit {
    pub(super) step: String,
    pub(super) field: String,
}

/// `git` options that sit before the subcommand and eat the value after them.
/// `git -C repo commit` is the same gesture as `git commit`, and looking only
/// at the first argument misses it.
///
/// A declared list, not a parser. An unlisted option makes the check stop
/// looking for the subcommand rather than get it wrong: it stays quiet.
const GIT_GLOBAL_OPTIONS_WITH_VALUE: [&str; 7] = [
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--exec-path",
    "--config-env",
];

/// `git commit` options that eat the value after them.
///
/// Without this list the value would read as the delimiting path, and
/// `-C <commit>` would be confused with git's global `-C <dir>`: same letter,
/// told apart only by position. These change where the message comes from,
/// never what lands in the commit, so they do not delimit.
const COMMIT_OPTIONS_WITH_VALUE: [&str; 16] = [
    "-m",
    "--message",
    "-F",
    "--file",
    "-C",
    "--reuse-message",
    "-c",
    "--reedit-message",
    "--author",
    "--date",
    "-t",
    "--template",
    "--fixup",
    "--squash",
    "--trailer",
    "--cleanup",
];

/// Options with which a `git commit` says it commits paths only. Alone they
/// are not enough: `--only` with no paths delimits nothing. The one exception
/// is with `--amend`, which the git manual states explicitly.
const DELIMITING_OPTIONS: [&str; 4] = ["--only", "-o", "--include", "-i"];

/// Takes the paths from a file instead of the line: it delimits on its own,
/// because the paths exist — they are just written elsewhere.
///
/// It passes without the file being opened. The danger here is committing the
/// whole index; an empty path list commits the opposite, nothing. A check that
/// is red on a correct form gets switched off within a day.
const PATHSPEC_FILE_OPTION: &str = "--pathspec-from-file";

/// Characters that end a command inside shell text.
///
/// A split, not a shell parser, and the direction of the error is declared: it
/// does not honour quotes, so a `git commit` written inside a string counts as
/// if it ran. It errs by flagging, never by staying quiet.
const SHELL_SEPARATORS: [char; 7] = [';', '&', '|', '\n', '(', ')', '`'];

/// An argument the flow assembles at run time — a `$join`, a `$from` — which
/// cannot be read here. It does not start with `-`, so it is consumed as an
/// option's value and counts as a path after `--`: the two readings the check
/// needs, neither claiming to know what will be written there.
const MOUNTED_ARG: &str = "<mounted>";

/// The handed steps that offer the person no closed choice: a question in
/// free text is not a step, and is refused before it can be asked.
/// Steps that declare themselves blind and ask to continue a session: a flow
/// saying two opposite things about the same step, refused before it runs.
pub(super) fn blind_steps_asking_for_a_session(flow: &FlowFile) -> Vec<String> {
    flow.graph
        .steps()
        .iter()
        .filter(|step| {
            let Some(with) = step.with.as_ref() else {
                return false;
            };
            with.get(actions::BLIND).and_then(Value::as_bool) == Some(true)
                && with.get("session").is_some()
        })
        .map(|step| step.id.clone())
        .collect()
}

/// Steps whose action cannot deliver the verdict `decides_done` promises.
///
/// **A MODEL MUST NOT BE THE ONE THAT DECIDES DONE.** That is the approval step
/// this declaration exists to remove, and a flow that hangs it on an engine
/// would spend a call to be told what it wanted to hear. The registry answers,
/// not a list of names written here beside it.
pub(super) fn deciders_that_are_not_checks(flow: &FlowFile, registry: &ActionRegistry) -> Vec<String> {
    flow.graph
        .steps()
        .iter()
        .filter(|step| step.decides_done)
        .filter(|step| !registry.get(&step.action).is_some_and(|action| action.is_a_check()))
        .map(|step| step.id.clone())
        .collect()
}

pub(super) fn handed_without_choices(flow: &FlowFile) -> Vec<String> {
    flow.graph
        .steps()
        .iter()
        .filter(|step| step.action == actions::handoff::HANDED_TO_AGENT_ACTION)
        .filter(|step| {
            step.with
                .as_ref()
                .map(|with| actions::handoff::choices_of(with).is_empty())
                .unwrap_or(true)
        })
        .map(|step| step.id.clone())
        .collect()
}

/// A pointer that cannot match the shape the step's input will have.
#[derive(Debug)]
pub(super) struct DeadPointer {
    pub(super) step: String,
    pub(super) field: String,
    pub(super) pointer: String,
}

/// **A POINTER THAT CANNOT MATCH IS A STEP THAT NEVER RUNS.** With several
/// dependencies, or one declared skippable, the input is an object keyed by
/// dependency name with `with` laid over: a first segment that is neither is
/// dead before the run starts.
pub(super) fn pointers_that_cannot_match(flow: &FlowFile) -> Vec<DeadPointer> {
    let mut found = Vec::new();
    for step in flow.graph.steps() {
        // Only where the shape is certain. One dependency that cannot be
        // skipped hands its own output over, and what is in it is the other
        // step's business — judging that here would invent.
        let named_by_dependency = match step.deps.as_slice() {
            [] => false,
            [only] => flow.graph.dependency_is_skippable(&step.id, only),
            _ => true,
        };
        if !named_by_dependency {
            continue;
        }
        let mut reachable: Vec<String> = step.deps.clone();
        if let Some(Value::Object(with)) = step.with.as_ref() {
            reachable.extend(with.keys().cloned());
        }
        if let Some(condition) = step.when.as_ref() {
            if let Some(pointer) = pointer_of(condition) {
                collect_dead(&step.id, "when", pointer, &reachable, &mut found);
            }
        }
        if let Some(with) = step.with.as_ref() {
            collect_dead_references(&step.id, "", with, &reachable, &mut found);
        }
    }
    found
}

fn pointer_of(condition: &flow::Condition) -> Option<&str> {
    match condition {
        flow::Condition::PointerEquals { pointer, .. } => Some(pointer),
        flow::Condition::PointerExists { pointer } => Some(pointer),
        flow::Condition::Equals { .. } => None,
    }
}

fn collect_dead_references(
    step: &str,
    field: &str,
    value: &Value,
    reachable: &[String],
    found: &mut Vec<DeadPointer>,
) {
    match value {
        Value::Object(fields) => {
            if let Some(Value::String(pointer)) = fields.get(reference::FROM_KEY) {
                collect_dead(step, field, pointer, reachable, found);
                return;
            }
            for (key, inner) in fields {
                let trail = if field.is_empty() {
                    key.clone()
                } else {
                    format!("{field}.{key}")
                };
                collect_dead_references(step, &trail, inner, reachable, found);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_dead_references(step, field, item, reachable, found);
            }
        }
        _ => {}
    }
}

fn collect_dead(
    step: &str,
    field: &str,
    pointer: &str,
    reachable: &[String],
    found: &mut Vec<DeadPointer>,
) {
    // An empty pointer is the whole input, and reaches it by definition.
    let Some(first) = pointer.trim_start_matches('/').split('/').next() else {
        return;
    };
    if first.is_empty() || reachable.iter().any(|name| name == first) {
        return;
    }
    found.push(DeadPointer {
        step: step.to_owned(),
        field: field.to_owned(),
        pointer: pointer.to_owned(),
    });
}

/// Steps that run `git commit` without delimiting what they commit, which in a
/// shared work tree means committing another session's staged work. `--amend`
/// and `-a` are stopped too: both sweep what is staged. A bare path without
/// `--` does not count, because a path cannot be told from the value of an
/// unknown option. Blind to: `git` behind a variable or alias, `xargs git
/// commit`, and quotes `SHELL_SEPARATORS` ignores.
pub(super) fn undelimited_commits(flow: &FlowFile) -> Vec<UndelimitedCommit> {
    let mut found = Vec::new();
    for step in flow.graph.steps() {
        let Some(with) = step.with.as_ref() else {
            continue;
        };
        let Value::Object(fields) = with else {
            continue;
        };
        // The engine door. `engines_of` reads both `"tool": "git"` and the
        // chain that names it, in one place: a second copy here would drop
        // half the steps. `bin` is the same door under another name.
        let names_git = engines_of(with).iter().any(|id| is_git(id))
            || fields
                .get("bin")
                .and_then(Value::as_str)
                .is_some_and(is_git);
        if names_git {
            if let Some(Value::Array(args)) = fields.get("args") {
                let argv: Vec<&str> = args.iter().map(mounted_or_written).collect();
                if commits_without_paths(&argv) {
                    found.push(UndelimitedCommit {
                        step: step.id.clone(),
                        field: "args".to_owned(),
                    });
                }
            }
        }
        // The shell door. The same field `outside_text_in_command` watches,
        // for the same reason: that text is not read, it is run.
        for name in EXECUTED_FIELDS {
            let Some(value) = fields.get(*name) else {
                continue;
            };
            let mut command = String::new();
            executed_text(value, &mut command);
            if shell_commits_without_paths(&command) {
                found.push(UndelimitedCommit {
                    step: step.id.clone(),
                    field: (*name).to_owned(),
                });
            }
        }
    }
    found
}

/// True if this name is `git`, however it was written.
fn is_git(name: &str) -> bool {
    name == "git" || name.ends_with("/git")
}

/// An argument as the check sees it: the text the flow wrote, or the
/// placeholder for what the flow assembles at run time.
fn mounted_or_written(arg: &Value) -> &str {
    arg.as_str().unwrap_or(MOUNTED_ARG)
}

/// The text that will really land in the executed field, in written order. A
/// reference becomes the placeholder, because skipping it would glue together
/// the two pieces around it.
fn executed_text(value: &Value, into: &mut String) {
    match value {
        Value::String(text) => into.push_str(text),
        Value::Array(items) => items.iter().for_each(|item| executed_text(item, into)),
        Value::Object(fields) => {
            if fields.contains_key(reference::FROM_KEY) || fields.contains_key(reference::JSON_KEY)
            {
                into.push_str(MOUNTED_ARG);
            } else {
                fields.values().for_each(|inner| executed_text(inner, into));
            }
        }
        _ => {}
    }
}

/// True if this shell text holds a `git commit` that does not say what it
/// commits. Same rule as the engine door; only where the arguments come from
/// changes.
fn shell_commits_without_paths(command: &str) -> bool {
    command
        .split(|letter| SHELL_SEPARATORS.contains(&letter))
        .any(|segment| {
            let tokens: Vec<&str> = segment.split_whitespace().collect();
            tokens
                .iter()
                .position(|token| is_git(token))
                .is_some_and(|at| commits_without_paths(&tokens[at + 1..]))
        })
}

/// The arguments of `git commit`, past the global options that sit before the
/// subcommand. `None` when this line is not a commit, including when the
/// subcommand could not be found.
fn commit_arguments<'a>(argv: &'a [&'a str]) -> Option<&'a [&'a str]> {
    let mut rest = argv;
    while rest.first().is_some_and(|token| token.starts_with('-')) {
        let eats_the_next = GIT_GLOBAL_OPTIONS_WITH_VALUE.contains(&rest[0]);
        rest = rest.get(if eats_the_next { 2 } else { 1 }..)?;
    }
    match rest.split_first() {
        Some((&"commit", after)) => Some(after),
        _ => None,
    }
}

/// The rule, in one place: both doors ask it, neither copies it.
fn commits_without_paths(argv: &[&str]) -> bool {
    let Some(mut rest) = commit_arguments(argv) else {
        return false;
    };
    let mut amends = false;
    let mut says_only = false;
    let mut names_a_path = false;
    while let Some(token) = rest.first() {
        rest = &rest[1..];
        if *token == "--" {
            // After `--` git reads no more options: what follows are paths,
            // and one is enough.
            return rest.is_empty();
        }
        // `--option=value` is a single argument: the value is not consumed.
        let (name, attached) = match token.split_once('=') {
            Some((name, _)) if name.starts_with("--") => (name, true),
            _ => (*token, false),
        };
        if name == PATHSPEC_FILE_OPTION {
            return false;
        }
        if name == "--amend" {
            amends = true;
        }
        if DELIMITING_OPTIONS.contains(&name) {
            says_only = true;
        }
        if COMMIT_OPTIONS_WITH_VALUE.contains(&name) {
            if !attached {
                rest = rest.get(1..).unwrap_or_default();
            }
            continue;
        }
        if !token.starts_with('-') {
            names_a_path = true;
        }
    }
    !(says_only && (names_a_path || amends))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A handed step must offer closed choices: without them the check names
    /// the step; with a labelled option it passes.
    #[test]
    fn a_handed_step_without_closed_choices_is_named_by_the_check() {
        let flow_with = |with: &str| -> FlowFile {
            serde_json::from_str(&format!(
                r#"{{
                    "id": "consegna",
                    "description": "a handed step",
                    "graph": {{"steps": [{{
                        "id": "decide",
                        "deps": [],
                        "input_schema": {{"type": "any"}},
                        "output_schema": {{"type": "any"}},
                        "when": null,
                        "action": "handed_to_agent",
                        "max_attempts": 1,
                        "with": {with}
                    }}]}},
                    "inputs": {{}}
                }}"#
            ))
            .expect("the flow is valid")
        };
        let base = r#"{"mandate": "choose", "holder": "you", "handoff_timeout_secs": 60"#;
        assert_eq!(handed_without_choices(&flow_with(&format!("{base}}}"))), vec!["decide".to_owned()]);
        assert_eq!(handed_without_choices(&flow_with(&format!("{base}, \"options\": []}}"))), vec!["decide".to_owned()]);
        let asked = format!("{base}, \"options\": [{{\"label\": \"yes\", \"facts\": \"it fits\"}}]}}");
        assert!(handed_without_choices(&flow_with(&asked)).is_empty());
    }

    /// A step cannot be blind and continue somebody's session: the flow would
    /// say two opposite things and the run would honour the first.
    #[test]
    fn a_blind_step_that_asks_to_continue_a_session_is_named_by_the_check() {
        let flow_with = |with: &str| -> FlowFile {
            serde_json::from_str(&format!(
                r#"{{
                    "id": "verifica",
                    "description": "a step that must not see",
                    "graph": {{"steps": [{{
                        "id": "judge",
                        "deps": [],
                        "input_schema": {{"type": "any"}},
                        "output_schema": {{"type": "any"}},
                        "when": null,
                        "action": "external_engine",
                        "max_attempts": 1,
                        "with": {with}
                    }}]}},
                    "inputs": {{}}
                }}"#
            ))
            .expect("the flow is valid")
        };
        let base = r#"{"tool": "un-motore", "stdin": "judge this", "timeout_secs": 60"#;
        let both = format!("{base}, \"blind\": true, \"session\": {{\"resume\": \"implement\"}}}}");
        assert_eq!(
            blind_steps_asking_for_a_session(&flow_with(&both)),
            vec!["judge".to_owned()]
        );
        let blind_alone = format!("{base}, \"blind\": true}}");
        assert!(blind_steps_asking_for_a_session(&flow_with(&blind_alone)).is_empty());
        let resuming_alone = format!("{base}, \"session\": {{\"resume\": \"implement\"}}}}");
        assert!(
            blind_steps_asking_for_a_session(&flow_with(&resuming_alone)).is_empty(),
            "a step that never said it was blind is free to continue a session"
        );
    }

    // ── il guasto 25: i percorsi assoluti scritti dentro un flusso ────

    /// Un flusso con un solo passo, il cui `with` è quello che le si passa.
    fn flow_with(with: &str) -> FlowFile {
        let json = format!(
            r#"{{
                "id": "prova", "description": "un passo solo",
                "graph": {{"steps": [{{
                    "id": "unico", "deps": [], "action": "external_engine",
                    "max_attempts": 1, "when": null,
                    "input_schema": {{"type": "any"}},
                    "output_schema": {{"type": "any"}},
                    "with": {with}
                }}]}},
                "inputs": {{}}
            }}"#
        );
        serde_json::from_str(&json).expect("caricare il flusso")
    }

    /// IL CASO DEL GUASTO 25. Un `workdir` assoluto decide dove il passo
    /// lavora: il flusso si può eseguire in un posto solo, e altrove non
    /// fallisce — fa danno nel posto sbagliato.
    #[test]
    fn an_absolute_workdir_is_an_error() {
        let flow = flow_with(r#"{"workdir": "/work/sailor"}"#);

        let found = hardcoded_paths(&flow);

        assert_eq!(found.len(), 1, "uno solo: {:?}", found.len());
        assert!(found[0].fatal, "un campo di posizione è un errore");
        assert_eq!(found[0].step, "unico");
        assert_eq!(found[0].field, "workdir");
    }

    /// Un `workdir` relativo è esattamente ciò che si vuole ottenere: si
    /// risolve sulla radice di chi lancia, e non ha niente da segnalare.
    #[test]
    fn a_relative_workdir_is_clean() {
        let flow = flow_with(r#"{"workdir": "crates/flow"}"#);

        assert!(hardcoded_paths(&flow).is_empty());
    }

    /// A flow of one step that declares it decides the run is done.
    fn deciding_flow(action: &str) -> FlowFile {
        let json = format!(
            r#"{{
                "id": "prova", "description": "un passo solo",
                "graph": {{"steps": [{{
                    "id": "unico", "deps": [], "action": "{action}",
                    "max_attempts": 1, "when": null, "decides_done": true,
                    "input_schema": {{"type": "any"}},
                    "output_schema": {{"type": "any"}}
                }}]}},
                "inputs": {{}}
            }}"#
        );
        serde_json::from_str(&json).expect("caricare il flusso")
    }

    fn one_of_each() -> ActionRegistry {
        let mut registry = ActionRegistry::default();
        registry.register("shell_check", actions::ShellCheckAction::new());
        registry.register("external_engine", actions::ExternalEngineAction::new());
        registry
    }

    /// **A MODEL MUST NOT BE THE ONE THAT DECIDES DONE.** An engine step
    /// carrying `decides_done` would spend a call to be told what it wanted to
    /// hear, which is the approval step this declaration exists to remove. An
    /// action nobody registered cannot decide either: it never answers.
    #[test]
    fn only_an_action_that_returns_a_verdict_may_decide_the_run_is_done() {
        let registry = one_of_each();

        assert!(deciders_that_are_not_checks(&deciding_flow("shell_check"), &registry).is_empty());
        assert_eq!(
            deciders_that_are_not_checks(&deciding_flow("external_engine"), &registry),
            vec!["unico".to_owned()]
        );
        assert_eq!(
            deciders_that_are_not_checks(&deciding_flow("nessuna"), &registry),
            vec!["unico".to_owned()]
        );
    }

    /// **UN PUNTATORE NON È UN PERCORSO.** `{"$from": "/answer/verdict"}`
    /// comincia per `/` e compare in quattro flussi su cinque: se il controllo
    /// lo chiamasse percorso nascerebbe pieno di falsi positivi e verrebbe
    /// spento entro un giorno.
    #[test]
    fn a_json_pointer_is_not_a_path() {
        let flow = flow_with(
            r#"{"stdin": {"$from": "/answer/verdict"}, "env": {"X": {"$json": "/shape"}}}"#,
        );

        assert!(hardcoded_paths(&flow).is_empty());
    }

    /// Un passo `shell_check`, che `flow_with` non sa costruire perché monta
    /// sempre un motore.
    fn shell_flow_with(with: &str) -> FlowFile {
        let json = format!(
            r#"{{
                "id": "prova", "description": "un comando solo",
                "graph": {{"steps": [{{
                    "id": "unico", "deps": [], "action": "shell_check",
                    "max_attempts": 1, "when": null,
                    "input_schema": {{"type": "any"}},
                    "output_schema": {{"type": "any"}},
                    "with": {with}
                }}]}},
                "inputs": {{}}
            }}"#
        );
        serde_json::from_str(&json).expect("caricare il flusso")
    }

    /// **CIÒ CHE VIENE DA FUORI VA IN `env`, MAI IN `command`.** La regola è
    /// scritta da sempre sopra `ShellCheckAction`, e cercata in tutto
    /// `crates/` non la applica nessun codice e non la copre nessuna prova: è
    /// un augurio, non una regola.
    ///
    /// Il comando è testo di shell e viene eseguito. Un titolo di richiesta di
    /// modifica — che su un remoto condiviso lo scrive chiunque — montato
    /// dentro `command` è un comando scritto da chi ha aperto la richiesta.
    /// Dentro una variabile d'ambiente resta un dato, e il comando la legge
    /// fra virgolette.
    ///
    /// LA MISURA CHE POTEVA VENIRE DIVERSA: la seconda metà. Un controllo che
    /// segnalasse qualunque rinvio, ovunque, sarebbe rosso su ogni flusso sano
    /// e verrebbe spento in un giorno — come sarebbe successo a
    /// `hardcoded_paths` se avesse scambiato un puntatore per un percorso.
    #[test]
    fn outside_text_belongs_in_env_never_in_command() {
        let montato = shell_flow_with(
            r#"{"command": {"$join": ["gh pr view --json title ", {"$from": "/answer/titolo"}]}, "timeout_secs": 5}"#,
        );

        let found = outside_text_in_command(&montato);

        assert_eq!(found.len(), 1, "uno solo: {found:?}");
        assert_eq!(found[0].step, "unico");
        assert_eq!(found[0].field, "command");

        // La forma giusta dello stesso lavoro: il valore passa come dato, e il
        // comando lo legge fra virgolette.
        let passato = shell_flow_with(
            r#"{"command": "gh pr view --json title \"$TITOLO\"", "env": {"TITOLO": {"$from": "/answer/titolo"}}, "timeout_secs": 5}"#,
        );

        assert!(
            outside_text_in_command(&passato).is_empty(),
            "un rinvio in «env» è la forma corretta, non una segnalazione"
        );
    }

    /// Un percorso dentro il testo di un prompt non impedisce al flusso di
    /// girare: è un'istruzione che diventa sbagliata altrove. Avviso, non
    /// errore — e riscriverlo è riscrivere un'istruzione, quindi lo fa una
    /// persona.
    #[test]
    fn an_absolute_path_inside_a_prompt_is_a_warning() {
        let flow =
            flow_with(r#"{"stdin": {"$join": ["Lavora solo dentro /home/someone/sailor.\n"]}}"#);

        let found = hardcoded_paths(&flow);

        assert_eq!(found.len(), 1);
        assert!(!found[0].fatal, "dentro un testo è un avviso");
        assert_eq!(found[0].field, "stdin.$join");
    }

    // ── a `git commit` that does not say what it commits ──────────────

    /// The shipped form of the defect: an `external_engine` step running
    /// `git commit -m …` and nothing else, which commits the whole index —
    /// and in a shared tree the index is not only its own.
    #[test]
    fn a_commit_that_names_no_path_is_an_error() {
        let flow = flow_with(r#"{"tool": "git", "args": ["commit", "-m", "a message"]}"#);

        let found = undelimited_commits(&flow);

        assert_eq!(found.len(), 1, "exactly one: {found:?}");
        assert_eq!(found[0].step, "unico");
        assert_eq!(
            found[0].field, "args",
            "the step commits with no path delimiting what it commits"
        );
    }

    /// `git -C <dir> commit` is the same gesture: global options sit before
    /// the subcommand, so looking at the first argument alone sees `-C` and
    /// closes green on a commit identical to the one above.
    #[test]
    fn a_global_option_does_not_hide_the_commit() {
        let flow =
            flow_with(r#"{"tool": "git", "args": ["-C", "subdir", "commit", "-m", "a message"]}"#);

        assert_eq!(undelimited_commits(&flow).len(), 1);
    }

    /// The second door. `shell_check`'s `command` is executed text, and a
    /// `git commit` fits mid-line. No flow commits this way today: without
    /// this test the same rule would mean two things by door.
    #[test]
    fn a_commit_inside_shell_text_is_an_error() {
        let flow = shell_flow_with(
            r#"{"command": "cargo fmt && git commit -m 'un messaggio'", "timeout_secs": 5}"#,
        );

        let found = undelimited_commits(&flow);

        assert_eq!(found.len(), 1, "uno solo: {found:?}");
        assert_eq!(found[0].step, "unico");
        assert_eq!(found[0].field, "command");
    }

    /// The delimited forms pass. A check that is red on the correct form gets
    /// switched off within a day, so what passes is tested with what stops.
    #[test]
    fn a_delimited_commit_is_clean() {
        for args in [
            r#"["commit", "-m", "un messaggio", "--", "docs/decisioni.md"]"#,
            r#"["commit", "--only", "docs/decisioni.md", "-m", "un messaggio"]"#,
            r#"["commit", "--include", "docs/decisioni.md", "-m", "un messaggio"]"#,
        ] {
            let flow = flow_with(&format!(r#"{{"tool": "git", "args": {args}}}"#));

            assert!(
                undelimited_commits(&flow).is_empty(),
                "this commit says what it commits: {args}"
            );
        }

        let on_one_line = shell_flow_with(
            r#"{"command": "git commit -m 'un messaggio' -- docs/decisioni.md", "timeout_secs": 5}"#,
        );

        assert!(undelimited_commits(&on_one_line).is_empty());
    }

    /// A step that does not name `git` is untouched, and so is a `git` that
    /// does not commit: reading the world is not writing it.
    #[test]
    fn a_step_that_does_not_commit_is_untouched() {
        for with in [
            r#"{"tool": "cargo", "args": ["test", "--workspace"]}"#,
            r#"{"tool": "git", "args": ["add", "-A"]}"#,
        ] {
            assert!(
                undelimited_commits(&flow_with(with)).is_empty(),
                "niente da dire su: {with}"
            );
        }

        let reading =
            shell_flow_with(r#"{"command": "git status --porcelain", "timeout_secs": 5}"#);

        assert!(undelimited_commits(&reading).is_empty());
    }

    /// The two ambiguous forms, decided above the function: `--amend` and `-a`
    /// delimit nothing, so both are stopped. The one exception is
    /// `--amend --only` with no paths, the documented way to rewrite only
    /// the message.
    #[test]
    fn amend_and_all_do_not_say_what_is_committed() {
        for args in [
            r#"["commit", "--amend", "--no-edit"]"#,
            r#"["commit", "-a", "-m", "un messaggio"]"#,
            r#"["commit", "--all", "-m", "un messaggio"]"#,
        ] {
            let flow = flow_with(&format!(r#"{{"tool": "git", "args": {args}}}"#));

            assert_eq!(
                undelimited_commits(&flow).len(),
                1,
                "this takes what is staged without saying so: {args}"
            );
        }

        let only_the_message =
            flow_with(r#"{"tool": "git", "args": ["commit", "--amend", "--only", "-m", "x"]}"#);

        assert!(
            undelimited_commits(&only_the_message).is_empty(),
            "«--amend --only» with no paths is the documented form"
        );
    }

    /// An option's value is not the path that delimits. `-m` and `-F` eat what
    /// follows them, and counting that as a path would let the base case
    /// through as delimited.
    #[test]
    fn the_value_of_an_option_is_not_a_path() {
        let flow = flow_with(
            r#"{"tool": "git", "args": ["commit", "--only", "-F", "message.txt", "-C", "HEAD"]}"#,
        );

        assert_eq!(
            undelimited_commits(&flow).len(),
            1,
            "«--only» with no paths delimits nothing"
        );
    }

    /// **A POINTER THAT CANNOT MATCH IS A STEP THAT NEVER RUNS.** With more
    /// than one dependency the input is keyed by dependency name, so a bare
    /// `/text` reaches nothing: in `when` the step skips and the run closes
    /// green, which is the shape nobody can see from a run.
    #[test]
    fn a_pointer_that_cannot_reach_the_input_is_named_before_the_run() {
        let flow: FlowFile = serde_json::from_value(serde_json::json!({
            "id": "prova", "description": "d",
            "graph": {"steps": [
                {"id": "innesco", "deps": [], "action": "trigger", "max_attempts": 1,
                 "when": null, "with": {"source": "manual"},
                 "input_schema": {"type": "any"}, "output_schema": {"type": "any"}},
                {"id": "altro", "deps": [], "action": "trigger", "max_attempts": 1,
                 "when": null, "with": {"source": "manual"},
                 "input_schema": {"type": "any"}, "output_schema": {"type": "any"}},
                {"id": "morto", "deps": ["innesco", "altro"], "action": "shell_check",
                 "max_attempts": 1,
                 "when": {"kind": "pointer_equals", "pointer": "/status", "value": "ok"},
                 "with": {"command": {"$from": "/text"}},
                 "input_schema": {"type": "any"}, "output_schema": {"type": "any"}},
                {"id": "vivo", "deps": ["innesco", "altro"], "action": "shell_check",
                 "max_attempts": 1,
                 "when": {"kind": "pointer_exists", "pointer": "/innesco/text"},
                 "with": {"command": {"$from": "/altro/text"}, "cap": 3,
                          "quanti": {"$from": "/cap"}},
                 "input_schema": {"type": "any"}, "output_schema": {"type": "any"}}
            ]},
            "inputs": {}
        }))
        .expect("il flusso si legge");

        let dead = pointers_that_cannot_match(&flow);
        let named: Vec<(&str, &str)> = dead
            .iter()
            .map(|found| (found.step.as_str(), found.pointer.as_str()))
            .collect();
        // Both shapes of the same defect, and nothing else: the step whose
        // pointers name a dependency is untouched, and so is the one whose
        // pointer names a key its own `with` lays over.
        assert_eq!(named, vec![("morto", "/status"), ("morto", "/text")], "{dead:?}");
    }

    /// THE CONTROL: one dependency that cannot be skipped hands its own output
    /// over, and what is inside it is the other step's business. Judging there
    /// would call every honest pointer dead.
    #[test]
    fn a_single_dependency_that_cannot_be_skipped_is_not_judged() {
        let flow: FlowFile = serde_json::from_value(serde_json::json!({
            "id": "prova", "description": "d",
            "graph": {"steps": [
                {"id": "innesco", "deps": [], "action": "trigger", "max_attempts": 1,
                 "when": null, "with": {"source": "manual"},
                 "input_schema": {"type": "any"}, "output_schema": {"type": "any"}},
                {"id": "dopo", "deps": ["innesco"], "action": "shell_check", "max_attempts": 1,
                 "when": {"kind": "pointer_exists", "pointer": "/text"},
                 "with": {"command": {"$from": "/text"}},
                 "input_schema": {"type": "any"}, "output_schema": {"type": "any"}}
            ]},
            "inputs": {}
        }))
        .expect("il flusso si legge");
        assert!(pointers_that_cannot_match(&flow).is_empty());
    }
}
