//! Resuming instead of rediscovering: how a step opens, resumes or forks the
//! session another step left, and starts over when it cannot.

use crate::candidates::Candidate;
use crate::cost::Recording;
use crate::process::{LiveSink, Pipe};
use crate::recipe::{command_line_with, AskRecipe, SessionRecipe, SESSION_PLACEHOLDER};
use crate::spec::SessionUse;
use crate::{read_text, Pointer, Reading, Reports};
use ledger::SessionMode;

// ── riprendere invece di riscoprire ──────────────────────────────────────

/// Le opzioni di sessione dichiarate dal motore, montate col resto della sua
/// ricetta. Quello che il motore non dichiara resta `None` fin qui.
pub(crate) fn session_lines(recipe: &AskRecipe, declared: Option<SessionRecipe>) -> SessionRecipe {
    let Some(declared) = declared else {
        return SessionRecipe::default();
    };
    let line = |args: Option<Vec<String>>| args.map(|args| command_line_with(recipe, &args));
    SessionRecipe {
        open: line(declared.open),
        resume: line(declared.resume),
        fork: line(declared.fork),
        id_from: declared.id_from,
    }
}

/// Le opzioni col segnaposto sostituito dall'identificativo vero.
///
/// La sostituzione è **dentro** l'opzione, non al posto suo: `codex` vuole
/// l'identificativo come argomento a sé, `claude` pure, ma niente vieta a un
/// motore futuro di volerlo attaccato a un `--session=`.
fn with_session_id(args: &[String], id: &str) -> Vec<String> {
    args.iter()
        .map(|arg| arg.replace(SESSION_PLACEHOLDER, id))
        .collect()
}

/// Un identificativo di sessione nuovo, nella forma che le righe di comando
/// chiedono (un UUID).
///
/// **NON SERVE CHE SIA IMPREVEDIBILE, SERVE CHE SIA UNICO.** Non protegge
/// niente: nomina una conversazione sul disco di chi la esegue. Dentro un
/// processo il contatore basta da solo; fra processi diversi il seme casuale di
/// `RandomState` — che il sistema operativo dà a ogni processo — separa le
/// serie. Tirarsi dentro una dipendenza per questo violerebbe la scelta scritta
/// nel `Cargo.toml` del workspace, che di dipendenze ne tiene tre.
fn fresh_session_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    static MINTED_SO_FAR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seed = std::collections::hash_map::RandomState::new();
    let mut halves = [0u64; 2];
    for (round, half) in halves.iter_mut().enumerate() {
        let mut hasher = seed.build_hasher();
        hasher.write_u64(MINTED_SO_FAR.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        hasher.write_u64(round as u64);
        hasher.write_u32(std::process::id());
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default(),
        );
        *half = hasher.finish();
    }
    let [high, low] = halves;
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (high >> 32) as u32,
        (high >> 16) as u16,
        (high & 0x0fff) as u16,
        // La variante che un UUID deve dichiarare: due bit fissi in cima.
        ((low >> 48) as u16 & 0x3fff) | 0x8000,
        low & 0xffff_ffff_ffff
    )
}

/// Con quali opzioni girare, e sotto quale identificativo di sessione questa
/// chiamata risulta essere girata.
pub(crate) struct SessionPlan {
    /// La riga di comando della sessione. `None` vuol dire «quella di sempre»,
    /// cioè si riparte da zero.
    pub(crate) args: Option<Vec<String>>,
    /// Cosa scrivere nella colonna `session_id` del deposito **se il motore non
    /// dice il proprio**. Vedi il commento su `ModelCallRecord::session_id`.
    recorded: Option<String>,
    /// Dove leggere, in ciò che il motore dirà, l'identificativo vero. Quando
    /// c'è **vince su `recorded`**: la parola del motore su quale sessione ha
    /// usato batte la nostra su quale gli avevamo chiesto.
    read_id_from: Option<Pointer>,
    /// What the ledger will say this call did with the session. `None` is the
    /// step that asked for nothing at all.
    pub(crate) mode: Option<SessionMode>,
}

impl SessionPlan {
    /// Da zero, come è sempre stato.
    fn from_scratch() -> Self {
        Self {
            args: None,
            recorded: None,
            read_id_from: None,
            mode: None,
        }
    }

    /// From nothing, on a step that had asked for something else.
    ///
    /// **THE ROW SAYS SO, NOT ONLY THE LIVE TEXT.** The line on the terminal is
    /// gone by morning; the bill is not, and a run of cold calls keeping no
    /// trace of having asked reads afterwards like a run that resumed.
    fn fell_back() -> Self {
        Self {
            mode: Some(SessionMode::ColdFallback),
            ..Self::from_scratch()
        }
    }

    /// L'identificativo da registrare, dopo che il motore ha parlato.
    pub(crate) fn session_id(&self, said: &str) -> Option<String> {
        match &self.read_id_from {
            Some(pointer) => read_text(said, pointer),
            None => self.recorded.clone(),
        }
    }
}

/// Lo dice a chi guarda mentre succede, non solo al deposito dopo.
///
/// **UN RIPIEGO MUTO È LA PEGGIORE DELLE DUE COSE**: si paga il prezzo della
/// riscoperta e non si sa di averlo pagato, e chi legge il flusso continuerà a
/// credere che quel passo riprenda. È il vincolo «chiarezza per chi guarda»
/// applicato al caso in cui l'ottimizzazione **non** scatta.
fn say_it_starts_over(live: Option<&dyn LiveSink>, named: &str, why: &str) {
    if let Some(live) = live {
        live.chunk(
            Pipe::Stderr,
            format!("[sailor] {named} riparte da zero: {why}\n").as_bytes(),
        );
    }
}

/// Decide se questa chiamata apre, riprende, ramifica, o riparte da zero.
///
/// **NON FALLISCE MAI, E LA SCELTA È IL VINCOLO.** Ogni impedimento — il motore
/// non sa riprendere, il passo prima non ha lasciato nessuna sessione, non c'è
/// un deposito dove cercarla — porta alla riga di comando di sempre. Un flusso
/// scritto su una macchina dove `claude-code` c'è deve girare su una macchina
/// dove c'è solo un motore che non sa riprendere: gira peggio, non gira meno.
pub(crate) fn session_plan(
    candidate: &Candidate,
    asked: Option<&SessionUse>,
    blind: bool,
    record: Option<&Recording<'_>>,
    live: Option<&dyn LiveSink>,
    named: &str,
) -> SessionPlan {
    let Some(asked) = asked else {
        return SessionPlan::from_scratch();
    };
    // Declared by whoever wrote the step, never inferred from what the step
    // looks like: a session carried in would hand it what it asked not to see.
    if blind {
        say_it_starts_over(live, named, "the step is declared blind");
        return SessionPlan::fell_back();
    }
    let Some(record) = record else {
        // Il deposito è il posto dove una sessione si posa e si ritrova: senza,
        // non c'è niente da aprire perché non ci sarebbe niente da riprendere.
        say_it_starts_over(
            live,
            named,
            &format!(
                "the step asks to {}, and this run has no store to put it in",
                asked.word()
            ),
        );
        return SessionPlan::fell_back();
    };
    match asked {
        SessionUse::Open => {
            let Some(line) = &candidate.session.open else {
                say_it_starts_over(live, named, "cannot open a session that can be found again");
                return SessionPlan::fell_back();
            };
            // **SI CONIA UN IDENTIFICATIVO SOLO SE SI HA DOVE METTERLO.** Una
            // riga senza segnaposto è quella di un motore che il nome se lo dà
            // da sé: registrare lì il nostro scriverebbe nel deposito una
            // sessione che su quella macchina non esiste, e il passo dopo
            // andrebbe a riprendere il nulla — dopo aver speso.
            let ours = line
                .iter()
                .any(|arg| arg.contains(SESSION_PLACEHOLDER))
                .then(fresh_session_id);
            SessionPlan {
                args: Some(match &ours {
                    Some(id) => with_session_id(line, id),
                    None => line.clone(),
                }),
                recorded: ours,
                read_id_from: candidate.session.id_from.clone(),
                mode: Some(SessionMode::Opened),
            }
        }
        SessionUse::Resume(step) | SessionUse::Fork(step) => {
            let forking = matches!(asked, SessionUse::Fork(_));
            let line = if forking {
                &candidate.session.fork
            } else {
                &candidate.session.resume
            };
            let Some(line) = line else {
                say_it_starts_over(live, named, &format!("cannot {}", asked.word()));
                return SessionPlan::fell_back();
            };
            // Senza identificativo di strumento non c'è nessun motore a cui
            // attribuire una sessione: è un `bin` scritto a mano nel passo.
            let Some(cli) = candidate.id.as_deref() else {
                return SessionPlan::from_scratch();
            };
            let found = record
                .ledger
                .session_opened_by(&record.run_id, step, cli)
                .ok()
                .flatten();
            let Some(id) = found else {
                say_it_starts_over(
                    live,
                    named,
                    &format!("step «{step}» left no session of «{cli}» to continue"),
                );
                return SessionPlan::fell_back();
            };
            SessionPlan {
                args: Some(with_session_id(line, &id)),
                // Ramificare conia un identificativo nuovo: se il motore non lo
                // dice, questo ramo resta senza nome, e nessuno potrà
                // continuarlo. Se lo dice, `read_id_from` lo raccoglie e il
                // ramo diventa continuabile come il tronco.
                recorded: if forking { None } else { Some(id) },
                read_id_from: candidate.session.id_from.clone(),
                mode: Some(if forking {
                    SessionMode::Forked
                } else {
                    SessionMode::Resumed
                }),
            }
        }
    }
}

/// The part of a reading that belongs to **this** step.
///
/// **AN ENGINE THAT COUNTS PER CALL IS ALREADY ANSWERED**, and its reading is
/// returned untouched: nothing is subtracted from a number that was never a
/// running total. Only `per_session` takes the other road, against what this
/// run has already attributed to that session.
pub(crate) fn this_step_share(
    record: &Recording<'_>,
    candidate: &Candidate,
    session_id: Option<&str>,
    reading: Reading,
) -> Reading {
    let cumulative = candidate
        .declared_usage
        .as_ref()
        .is_some_and(|declared| declared.reports == Reports::PerSession);
    if !cumulative {
        return reading;
    }
    // **NO SESSION, NO BASELINE, NO SHARE.** A cumulative engine called outside
    // a session we can name states what the session has spent, and there is no
    // honest way to cut this call out of it: the row says unknown.
    let before = session_id
        .zip(candidate.id.as_deref())
        .and_then(|(session, cli)| {
            record
                .ledger
                .attributed_to_session(&record.run_id, session, cli)
                .ok()
        })
        .map(what_the_session_carried)
        .unwrap_or_default();
    models::usage::share_after(reading, &before)
}

/// The ledger's totals in the shape the subtraction speaks.
fn what_the_session_carried(so_far: ledger::SessionSoFar) -> Reading {
    Reading {
        input_tokens: so_far.input_tokens,
        output_tokens: so_far.output_tokens,
        cached_tokens: so_far.cached_tokens,
        cache_write_tokens: so_far.cache_write_tokens,
        cache_write_long_tokens: so_far.cache_write_long_tokens,
        total_tokens: so_far.total_tokens,
        turns: so_far.turns,
        declared_cost: so_far
            .declared_cost_micros
            .map(|micros| micros as f64 / 1_000_000.0),
        ..Reading::default()
    }
}

#[cfg(test)]
mod resuming_instead_of_rediscovering {
    //! Le prove della ripresa: un passo continua la sessione di un altro invece
    //! di riaprire un processo che non sa niente.
    //!
    //! **NESSUN MOTORE VERO.** I motori qui dentro sono script di shell che
    //! scrivono la propria riga di comando su un file: quello che si prova è
    //! **cosa arriva al motore** e **cosa resta nel deposito**, che sono le due
    //! cose su cui questo lavoro sta o cade. Quanto si risparmi in token lo
    //! dice una corsa vera, non una prova: qui non si può misurare e non si
    //! finge di farlo.

    use super::*;
    use crate::engine::ExternalEngineAction;
    use crate::recipe::{PromptVia, ToolResolver};
    use crate::EXTERNAL_ENGINE_ACTION;
    use flow::{Action, ActionOutcome, SharedState};
    use ledger::Ledger;
    use serde_json::{json, Value};
    use std::os::unix::fs::PermissionsExt;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sailor-sessione-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("cartella di lavoro");
        dir
    }

    /// A step declared blind is handed what the flow hands it and nothing else.
    /// The word is the flow's, not ours: no kind of work is read as a judge.
    #[test]
    fn a_step_declared_blind_carries_no_option_that_would_continue_a_session() {
        let dir = scratch("cieco");
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        ledger
            .record_model_call(&a_call_that_opened("s-1"))
            .expect("una sessione lasciata dal passo di prima");
        let candidate = a_candidate_that_can_resume();
        let record = Recording {
            ledger: &ledger,
            run_id: "corsa".to_owned(),
            step_id: "verifica".to_owned(),
        };
        let asked = SessionUse::Resume("implementa".to_owned());

        // The control: without the declaration the step resumes, which is what
        // makes the blind case a difference and not a coincidence.
        let seeing = session_plan(&candidate, Some(&asked), false, Some(&record), None, "«motore»");
        assert_eq!(
            seeing.args,
            Some(vec!["--resume".to_owned(), "s-1".to_owned()]),
            "a step that did not ask to be blind continues the session it named"
        );

        let blind = session_plan(&candidate, Some(&asked), true, Some(&record), None, "«motore»");
        assert!(
            blind.args.is_none() && blind.recorded.is_none(),
            "a blind step starts from scratch: {:?}",
            blind.args
        );
    }

    fn a_candidate_that_can_resume() -> Candidate {
        Candidate {
            id: Some("motore".to_owned()),
            bin: "eco".to_owned(),
            args: vec!["ask".to_owned()],
            prompt: PromptVia::Stdin,
            unusable_when: Vec::new(),
            exhausted_when: Vec::new(),
            cooldown_secs: None,
            waits_for_a_person_when: Vec::new(),
            declared_usage: None,
            can_be_asked: true,
            why: None,
            session: SessionRecipe {
                open: Some(vec!["--session".to_owned(), SESSION_PLACEHOLDER.to_owned()]),
                resume: Some(vec!["--resume".to_owned(), SESSION_PLACEHOLDER.to_owned()]),
                fork: None,
                id_from: None,
            },
        }
    }

    fn a_call_that_opened(session: &str) -> ledger::ModelCallRecord {
        ledger::ModelCallRecord {
            call_id: format!("corsa:implementa:{session}"),
            run_id: "corsa".to_owned(),
            step_id: Some("implementa".to_owned()),
            purpose: EXTERNAL_ENGINE_ACTION.to_owned(),
            cli: "motore".to_owned(),
            requested_model: String::new(),
            actual_model: String::new(),
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: None,
            cost_micros: None,
            declared_cost_micros: None,
            price_currency: None,
            input_price_micros_per_million: None,
            output_price_micros_per_million: None,
            cached_price_micros_per_million: None,
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            engine_identity: ledger::EngineIdentity::NotAKnownEngine,
            retry_chain: Vec::new(),
            error_type: None,
            started_at: 1,
            ended_at: Some(2),
            session_id: Some(session.to_owned()),
            work_kind: None,
            fell_back_from: Vec::new(),
            session_mode: Some(SessionMode::Opened),
        }
    }

    /// Un motore che scrive **in coda** la riga di comando con cui è stato
    /// invocato: in coda perché una prova sola lo chiama quattro volte, e
    /// sovrascrivere terrebbe solo l'ultima.
    const LOGS_ITS_ARGUMENTS: &str = r#"cat > /dev/null
printf '%s\n' "$*" >> "$(dirname "$0")/invocations"
printf 'ok'"#;

    /// Un motore che, oltre a registrare la riga, **annuncia** la sessione con
    /// cui sta parlando — e ne annuncia una diversa a ogni invocazione, come fa
    /// un motore vero quando ramifica.
    const ANNOUNCES_ITS_SESSION: &str = r#"cat > /dev/null
here="$(dirname "$0")"
printf '%s\n' "$*" >> "$here/invocations"
n=$(cat "$here/counter" 2>/dev/null || echo 0)
n=$((n + 1))
printf '%s' "$n" > "$here/counter"
printf 'session id: sessione-%s\nok\n' "$n""#;

    fn fake_engine(dir: &std::path::Path) -> String {
        engine_that(dir, LOGS_ITS_ARGUMENTS)
    }

    fn engine_that(dir: &std::path::Path, body: &str) -> String {
        let path = dir.join("engine");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("scrivere il finto motore");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("renderlo eseguibile");
        path.to_string_lossy().into_owned()
    }

    fn invocations(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(dir.join("invocations"))
            .expect("il motore finto ha scritto le proprie invocazioni")
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn shared(run: &str, step: &str) -> SharedState {
        let mut shared = SharedState::new();
        shared.insert(flow::CURRENT_RUN.to_owned(), json!(run));
        shared.insert(flow::CURRENT_STEP.to_owned(), json!(step));
        shared
    }

    const TOOL: &str = "motore-di-prova";

    /// Un risolutore che dichiara la ricetta della domanda e — separatamente —
    /// cosa quel motore sa fare con le proprie sessioni. Le due cose viaggiano
    /// separate anche nella vita vera.
    struct Declares {
        bin: String,
        sessions: Option<SessionRecipe>,
    }

    impl ToolResolver for Declares {
        fn resolve(&self, id: &str) -> Result<String, String> {
            match id {
                TOOL => Ok(self.bin.clone()),
                other => Err(format!("«{other}» non è su questa macchina")),
            }
        }
        fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
            Some(AskRecipe {
                args: vec!["--ask".to_owned()],
                prompt: PromptVia::Stdin,
                args_before_prompt: Vec::new(),
                unusable_when: Vec::new(),
                silent_without_prompt: false,
                refuses_without_prompt: Vec::new(),
                exhausted_when: Vec::new(),
                cooldown_secs: None,
                waits_for_a_person_when: Vec::new(),
                usage: None,
            })
        }
        fn session_recipe(&self, _id: &str) -> Option<SessionRecipe> {
            self.sessions.clone()
        }
    }

    /// Un motore che sa tutti e tre i modi, come `claude-code`.
    fn knows_all_three() -> SessionRecipe {
        SessionRecipe {
            open: Some(vec![
                "--ask".to_owned(),
                "--session-id".to_owned(),
                SESSION_PLACEHOLDER.to_owned(),
            ]),
            resume: Some(vec![
                "--ask".to_owned(),
                "--resume".to_owned(),
                SESSION_PLACEHOLDER.to_owned(),
            ]),
            fork: Some(vec![
                "--ask".to_owned(),
                "--resume".to_owned(),
                SESSION_PLACEHOLDER.to_owned(),
                "--fork-session".to_owned(),
            ]),
            id_from: None,
        }
    }

    /// Un motore che l'identificativo se lo conia da sé e lo **stampa**, come
    /// `codex`: apre con la riga di sempre, e il nome si va a leggere.
    fn mints_its_own() -> SessionRecipe {
        SessionRecipe {
            open: Some(vec!["--ask".to_owned()]),
            resume: Some(vec![
                "--ask".to_owned(),
                "resume".to_owned(),
                SESSION_PLACEHOLDER.to_owned(),
            ]),
            fork: Some(vec![
                "--ask".to_owned(),
                "fork".to_owned(),
                SESSION_PLACEHOLDER.to_owned(),
            ]),
            id_from: Some(Pointer::Pattern("session id: ([0-9a-z-]+)".to_owned())),
        }
    }

    fn step_that(session: Value) -> Value {
        json!({
            "tool": TOOL,
            "stdin": "guarda l'albero",
            "timeout_secs": 20,
            "session": session,
        })
    }

    fn ran(action: &ExternalEngineAction, input: &Value, run: &str, step: &str) {
        match action.execute(input, &shared(run, step)) {
            Ok(ActionOutcome::Went(_)) => {}
            other => panic!("il passo «{step}» doveva andare: {other:?}"),
        }
    }

    /// **CHI APRE POSA L'IDENTIFICATIVO NEL DEPOSITO, O NESSUNO POTRÀ
    /// RIPRENDERLO.** Le due metà si provano insieme di proposito: un
    /// identificativo passato al motore e non registrato è indistinguibile,
    /// dal passo dopo, da una sessione mai aperta.
    #[test]
    fn a_step_that_opens_a_session_hands_it_to_the_engine_and_writes_it_down() {
        let dir = scratch("apre");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(knows_all_three()),
        })
        .recording_to(Some(ledger.clone()));

        ran(&action, &step_that(json!("open")), "corsa-1", "scopri");

        let line = invocations(&dir).remove(0);
        let written = ledger
            .session_opened_by("corsa-1", "scopri", TOOL)
            .expect("il deposito risponde")
            .expect("e ha registrato la sessione");
        assert!(
            line.contains("--session-id") && line.contains(&written),
            "il motore deve ricevere lo stesso identificativo che il deposito conserva: \
             riga «{line}», registrato «{written}»"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **IL CASO CHE RENDE DI PIÙ, ED È IL MOTIVO DI TUTTO IL LAVORO.** Tre
    /// passi indipendenti guardano lo stesso albero nello stesso momento: senza
    /// ramificazione fanno tre scoperte identiche e le pagano tre volte. Qui
    /// devono ricevere tutti e tre lo stesso tronco, e ognuno il proprio ramo.
    ///
    /// **E OGNUNO DEI TRE DEVE REGISTRARE UNA SESSIONE IGNOTA.** Ramificare
    /// conia un identificativo che il motore non ci dice: scrivere lì quello
    /// del padre farebbe riprendere il tronco a chi crede di stare sul proprio
    /// ramo — in silenzio, che è il modo peggiore.
    #[test]
    fn three_independent_steps_fork_one_discovery_instead_of_doing_it_three_times() {
        let dir = scratch("ramifica");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(knows_all_three()),
        })
        .recording_to(Some(ledger.clone()));

        ran(&action, &step_that(json!("open")), "corsa-2", "scopri");
        for step in ["struttura", "rischi", "attrito"] {
            ran(
                &action,
                &step_that(json!({ "fork": "scopri" })),
                "corsa-2",
                step,
            );
        }

        let trunk = ledger
            .session_opened_by("corsa-2", "scopri", TOOL)
            .expect("il deposito risponde")
            .expect("il tronco è registrato");
        let lines = invocations(&dir);
        assert_eq!(lines.len(), 4, "una scoperta e tre rami");
        for line in &lines[1..] {
            assert!(
                line.contains(&trunk) && line.contains("--fork-session"),
                "ogni ramo parte dal tronco senza continuarlo: «{line}»"
            );
        }
        for step in ["struttura", "rischi", "attrito"] {
            assert_eq!(
                ledger
                    .session_opened_by("corsa-2", step, TOOL)
                    .expect("il deposito risponde"),
                None,
                "il ramo di «{step}» ha un identificativo che il motore non ci ha detto"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Chi riprende continua la **stessa** sessione, e la lascia in eredità:
    /// tre passi in fila devono poter continuare l'uno dall'altro.
    #[test]
    fn resuming_keeps_the_same_session_so_the_next_step_can_take_it_too() {
        let dir = scratch("riprende");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(knows_all_three()),
        })
        .recording_to(Some(ledger.clone()));

        ran(&action, &step_that(json!("open")), "corsa-3", "scopri");
        ran(
            &action,
            &step_that(json!({ "resume": "scopri" })),
            "corsa-3",
            "piano",
        );
        ran(
            &action,
            &step_that(json!({ "resume": "piano" })),
            "corsa-3",
            "implementa",
        );

        let trunk = ledger
            .session_opened_by("corsa-3", "scopri", TOOL)
            .expect("il deposito risponde")
            .expect("il tronco è registrato");
        assert_eq!(
            ledger
                .session_opened_by("corsa-3", "piano", TOOL)
                .expect("risponde"),
            Some(trunk.clone()),
            "chi riprende non cambia sessione, e per questo la può passare avanti"
        );
        let lines = invocations(&dir);
        assert!(
            lines[2].contains(&trunk) && !lines[2].contains("--fork-session"),
            "il terzo passo continua lo stesso tronco: «{}»",
            lines[2]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **IL VINCOLO PERMANENTE, PROVATO.** Un motore che non sa ramificare non
    /// diventa rosso e non diventa un caso speciale: riceve la riga di sempre,
    /// riparte da zero e paga di più. Se un giorno qualcuno facesse fallire il
    /// passo «per non nascondere il problema», questa prova lo prenderebbe.
    #[test]
    fn an_engine_that_cannot_fork_starts_over_instead_of_breaking() {
        let dir = scratch("non-sa-ramificare");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(SessionRecipe {
                open: knows_all_three().open,
                resume: None,
                fork: None,
                id_from: None,
            }),
        })
        .recording_to(Some(ledger.clone()));

        ran(&action, &step_that(json!("open")), "corsa-4", "scopri");
        ran(
            &action,
            &step_that(json!({ "fork": "scopri" })),
            "corsa-4",
            "rischi",
        );

        let lines = invocations(&dir);
        assert_eq!(
            lines[1], "--ask",
            "chi non sa ramificare riceve la riga di sempre, non una riga monca"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Un motore che non dichiara **niente** sulle sessioni funziona come
    /// prima, anche quando il passo chiede di aprirne una: è il caso di tre dei
    /// quattro motori installati su questa macchina.
    #[test]
    fn an_engine_that_declares_no_sessions_works_exactly_as_before() {
        let dir = scratch("muto");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: None,
        })
        .recording_to(Some(ledger.clone()));

        ran(&action, &step_that(json!("open")), "corsa-5", "scopri");

        assert_eq!(invocations(&dir), vec!["--ask".to_owned()]);
        assert_eq!(
            ledger
                .session_opened_by("corsa-5", "scopri", TOOL)
                .expect("risponde"),
            None,
            "non c'è nessuna sessione da registrare, e non se ne inventa una"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ramificare da un passo che non ha lasciato niente riparte da zero. È il
    /// caso di chi scrive `{"fork": "un-passo-che-non-c-e"}` — un refuso — e di
    /// chi ramifica da un passo che quel giorno è finito su un altro motore.
    #[test]
    fn forking_from_a_step_that_left_no_session_starts_over() {
        let dir = scratch("nessun-tronco");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(knows_all_three()),
        })
        .recording_to(Some(ledger));

        ran(
            &action,
            &step_that(json!({ "fork": "un-passo-che-non-c-e" })),
            "corsa-6",
            "rischi",
        );

        assert_eq!(invocations(&dir), vec!["--ask".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **UN MOTORE CHE IL NOME SE LO DÀ DA SÉ NON È UN MOTORE ESCLUSO.**
    /// Verificato il 31/08/2026 su `codex`, che non ha nessuna opzione per
    /// imporre un identificativo e lo **stampa**: senza questa via i motori che
    /// coniano da sé sarebbero fuori da una capacità che hanno.
    ///
    /// E il ramo diventa **continuabile a sua volta**: il terzo passo ramifica
    /// dal secondo, non dal primo. Senza leggere l'identificativo del ramo, una
    /// catena di tre passi tornerebbe di colpo alla scoperta iniziale, e nessun
    /// errore lo direbbe — arriverebbe solo un contesto sbagliato.
    #[test]
    fn an_engine_that_names_its_own_session_is_read_and_its_branch_is_continuable() {
        let dir = scratch("si-nomina-da-se");
        let bin = engine_that(&dir, ANNOUNCES_ITS_SESSION);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(mints_its_own()),
        })
        .recording_to(Some(ledger.clone()));

        ran(&action, &step_that(json!("open")), "corsa-7", "scopri");
        ran(
            &action,
            &step_that(json!({ "fork": "scopri" })),
            "corsa-7",
            "rischi",
        );
        ran(
            &action,
            &step_that(json!({ "fork": "rischi" })),
            "corsa-7",
            "dettaglio",
        );

        assert_eq!(
            ledger
                .session_opened_by("corsa-7", "scopri", TOOL)
                .expect("risponde"),
            Some("sessione-1".to_owned()),
            "l'identificativo lo dice il motore, non lo decidiamo noi"
        );
        assert_eq!(
            ledger
                .session_opened_by("corsa-7", "rischi", TOOL)
                .expect("risponde"),
            Some("sessione-2".to_owned()),
            "e il ramo ha il proprio, non quello del tronco"
        );
        let lines = invocations(&dir);
        assert_eq!(
            lines[1], "--ask fork sessione-1",
            "il ramo parte dal tronco"
        );
        assert_eq!(
            lines[2], "--ask fork sessione-2",
            "e il ramo dopo parte dal ramo"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **NON SI CONIA UN NOME CHE NON SI PUÒ CONSEGNARE.** Un motore che apre
    /// con la riga di sempre non riceve nessun identificativo: scriverne uno
    /// nostro nel deposito farebbe riprendere al passo dopo una sessione che su
    /// quella macchina non esiste — e se ne accorgerebbe dopo aver speso.
    #[test]
    fn a_session_we_cannot_name_is_not_named_by_us() {
        let dir = scratch("nome-non-consegnabile");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            // Apre con la riga di sempre e non dice dove scrive il proprio
            // nome: è il caso di chi non ha nessuna delle due vie.
            sessions: Some(SessionRecipe {
                open: Some(vec!["--ask".to_owned()]),
                ..SessionRecipe::default()
            }),
        })
        .recording_to(Some(ledger.clone()));

        ran(&action, &step_that(json!("open")), "corsa-8", "scopri");

        assert_eq!(invocations(&dir), vec!["--ask".to_owned()]);
        assert_eq!(
            ledger
                .session_opened_by("corsa-8", "scopri", TOOL)
                .expect("risponde"),
            None,
            "una sessione che non si sa nominare resta senza nome nel deposito"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Due chiamate non ricevono mai lo stesso identificativo: se lo
    /// ricevessero, due sessioni diverse si scriverebbero addosso sul disco di
    /// chi esegue, e il passo dopo riprenderebbe un miscuglio.
    #[test]
    fn two_sessions_never_get_the_same_identifier() {
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..1000 {
            assert!(
                seen.insert(fresh_session_id()),
                "un identificativo ripetuto"
            );
        }
        // E la forma è quella che le righe di comando chiedono: cinque gruppi
        // separati da trattini, la versione al posto giusto.
        let one = fresh_session_id();
        let groups: Vec<&str> = one.split('-').collect();
        assert_eq!(
            groups.iter().map(|group| group.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12],
            "«{one}» non ha la forma di un UUID"
        );
        assert!(one.starts_with(|c: char| c.is_ascii_hexdigit()));
        assert!(groups[2].starts_with('4'), "la versione: «{one}»");
    }

    // ── what the ledger says a call did with the session ────────────────

    fn calls_in(dir: &std::path::Path) -> Vec<ledger::ModelCallRecord> {
        let ledger = Ledger::open(dir).expect("reopen the store");
        let dump = ledger.projection_dump().expect("read the projection");
        ui::parse::parse_model_calls(&dump)
    }

    fn only_call(dir: &std::path::Path) -> ledger::ModelCallRecord {
        let mut calls = calls_in(dir);
        assert_eq!(calls.len(), 1, "one call only: {calls:?}");
        calls.remove(0)
    }

    /// A step with no `session` key at all: the line the engine gets and the
    /// row the store keeps must be the ones they were before any of this.
    fn plain_step() -> Value {
        json!({ "tool": TOOL, "stdin": "guarda l'albero", "timeout_secs": 20 })
    }

    /// **A FLOW THAT DECLARES NOTHING BEHAVES AS IT DID.** All of this is
    /// opt-in: with no declaration the engine is invoked on its own recipe and
    /// nothing is written in the session columns.
    #[test]
    fn a_step_that_declares_no_session_is_invoked_and_recorded_as_before() {
        let dir = scratch("dichiara-niente");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(knows_all_three()),
        })
        .recording_to(Some(ledger));

        ran(&action, &plain_step(), "corsa-9", "scopri");

        assert_eq!(
            invocations(&dir),
            vec!["--ask".to_owned()],
            "no session option on a line that asked for none"
        );
        let call = only_call(&dir.join("deposito"));
        assert_eq!(call.session_id, None);
        assert_eq!(
            call.session_mode, None,
            "whoever asked for nothing has nothing to confess"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **THE FALLBACK IS ON THE ROW, NOT ONLY ON THE TERMINAL.** An engine that
    /// cannot resume gives the step a cold call; without this column that run's
    /// bill would read exactly like a run where every step resumed.
    #[test]
    fn a_step_that_had_to_start_over_says_so_in_the_ledger() {
        let dir = scratch("ripiego-registrato");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            // Opens, cannot resume: three of the four engines on this machine.
            sessions: Some(SessionRecipe {
                open: knows_all_three().open,
                ..SessionRecipe::default()
            }),
        })
        .recording_to(Some(ledger));

        ran(
            &action,
            &step_that(json!({ "resume": "scopri" })),
            "corsa-10",
            "piano",
        );

        let call = only_call(&dir.join("deposito"));
        assert_eq!(
            call.session_mode,
            Some(SessionMode::ColdFallback),
            "the step had asked to resume, and the call started from nothing"
        );
        assert_eq!(call.session_id, None, "and there is no session to name");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A BLIND STEP IS COLD, AND THE ROW SAYS IT WAS ASKED TO BE OTHERWISE.**
    /// The check refuses the pair before a run; a step reached by a road the
    /// check never walked still leaves a trace of what it asked for.
    #[test]
    fn a_blind_step_gets_a_cold_call_and_the_ledger_records_the_fallback() {
        let dir = scratch("cieco-registrato");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(knows_all_three()),
        })
        .recording_to(Some(ledger));

        // A session really is left behind, or the blind step would start over
        // for want of one and the test would pass without blindness.
        ran(&action, &step_that(json!("open")), "corsa-11", "scopri");
        let mut step = step_that(json!({ "resume": "scopri" }));
        step["blind"] = json!(true);
        ran(&action, &step, "corsa-11", "giudica");

        assert_eq!(
            invocations(&dir)[1],
            "--ask",
            "no option that would carry an earlier context in"
        );
        let judging = calls_in(&dir.join("deposito"))
            .into_iter()
            .find(|call| call.step_id.as_deref() == Some("giudica"))
            .expect("the blind step wrote its row");
        assert_eq!(judging.session_mode, Some(SessionMode::ColdFallback));
        assert_eq!(judging.session_id, None, "and it continued nobody");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// And a call that really did continue says the other thing, so the two are
    /// told apart by the row and not by whoever remembers the run.
    #[test]
    fn a_call_that_really_resumed_is_written_down_as_resumed() {
        let dir = scratch("ripresa-registrata");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(knows_all_three()),
        })
        .recording_to(Some(ledger));

        ran(&action, &step_that(json!("open")), "corsa-12", "scopri");
        ran(
            &action,
            &step_that(json!({ "resume": "scopri" })),
            "corsa-12",
            "piano",
        );

        let modes: Vec<Option<SessionMode>> = calls_in(&dir.join("deposito"))
            .iter()
            .map(|call| call.session_mode)
            .collect();
        assert!(
            modes.contains(&Some(SessionMode::Opened))
                && modes.contains(&Some(SessionMode::Resumed)),
            "one row opens and the other continues: {modes:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── whose numbers those are: this call's, or the session's ──────────

    /// An engine that keeps counting from the moment its session opened: every
    /// answer states a thousand more than the one before it.
    const COUNTS_THE_WHOLE_SESSION: &str = r#"cat > /dev/null
here="$(dirname "$0")"
printf '%s\n' "$*" >> "$here/invocations"
n=$(cat "$here/counter" 2>/dev/null || echo 0)
n=$((n + 1))
printf '%s' "$n" > "$here/counter"
printf '{"result":"ok","usage":{"input_tokens":%s}}' "$((n * 1000))""#;

    /// The same engine, declared one way or the other. Nothing but the
    /// declaration separates the two runs.
    struct Counting {
        bin: String,
        reports: Reports,
    }

    impl ToolResolver for Counting {
        fn resolve(&self, id: &str) -> Result<String, String> {
            match id {
                TOOL => Ok(self.bin.clone()),
                other => Err(format!("«{other}» non è su questa macchina")),
            }
        }
        fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
            Some(AskRecipe {
                args: vec!["--ask".to_owned()],
                prompt: PromptVia::Stdin,
                args_before_prompt: Vec::new(),
                unusable_when: Vec::new(),
                exhausted_when: Vec::new(),
                cooldown_secs: None,
                waits_for_a_person_when: Vec::new(),
                silent_without_prompt: false,
                refuses_without_prompt: Vec::new(),
                usage: Some(crate::recipe::UsageRecipe {
                    args: Vec::new(),
                    declared: crate::Declared {
                        read: crate::Shape::Json,
                        from: models::usage::Heard::Stdout,
                        reports: self.reports,
                        input_tokens: Some(Pointer::Path(vec![
                            "usage".to_owned(),
                            "input_tokens".to_owned(),
                        ])),
                        answer: Some(Pointer::Path(vec!["result".to_owned()])),
                        ..crate::Declared::default()
                    },
                }),
            })
        }
        fn session_recipe(&self, _id: &str) -> Option<SessionRecipe> {
            Some(knows_all_three())
        }
    }

    /// Runs open-then-resume against the counting engine and returns what each
    /// row was charged, in the order the calls were made.
    fn charged_to_each_step(label: &str, reports: Reports) -> Vec<Option<u64>> {
        let dir = scratch(label);
        let bin = engine_that(&dir, COUNTS_THE_WHOLE_SESSION);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Counting { bin, reports })
            .recording_to(Some(ledger));

        ran(&action, &step_that(json!("open")), "corsa-13", "scopri");
        ran(
            &action,
            &step_that(json!({ "resume": "scopri" })),
            "corsa-13",
            "piano",
        );

        let mut calls = calls_in(&dir.join("deposito"));
        // In the order they were made: the counter at the end of `call_id` is
        // the only field that keeps it — the step names sort the other way.
        calls.sort_by_key(|call| {
            call.call_id
                .rsplit(':')
                .next()
                .and_then(|tail| tail.parse::<u64>().ok())
                .expect("every call carries its own sequence number")
        });
        let _ = std::fs::remove_dir_all(&dir);
        calls.iter().map(|call| call.input_tokens).collect()
    }

    /// **THE DIFFERENCE BETWEEN TWO READINGS IS THE SECOND STEP'S SHARE.** The
    /// engine says 1000 then 2000; charging the second step 2000 would count
    /// the first step's thousand twice and make the resumed call look dearer
    /// than the cold one it replaced.
    #[test]
    fn a_cumulative_engine_charges_each_step_only_what_it_added() {
        assert_eq!(
            charged_to_each_step("consumo-cumulativo", Reports::PerSession),
            vec![Some(1_000), Some(1_000)]
        );
    }

    /// **AND WHICH ONE IT IS, IS DECLARED.** The same engine read as per-call
    /// charges the second step the whole 2000: the two readings are identical,
    /// so only the descriptor can tell them apart.
    #[test]
    fn the_same_numbers_read_as_per_call_are_not_touched() {
        assert_eq!(
            charged_to_each_step("consumo-per-chiamata", Reports::PerCall),
            vec![Some(1_000), Some(2_000)]
        );
    }
}
