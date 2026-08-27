//! La parte pura della ripresa: dato un passo rimasto aperto e la sua specie,
//! decide cosa farne senza toccare disco né processi. La trappola che questo
//! crate esiste per non ripetere — un passo "in corso" che nessuna
//! riconciliazione chiude mai — si previene con un giudice testabile qui, non
//! con logica sparsa nella parte che tocca il deposito (vedi `reconcile`).

pub mod reconcile;

pub use reconcile::{
    default_classify, default_ledger_dir, ledger_dir_from_env, reconcile_ledger,
    reconcile_suspended_steps, Applied, Classify, ReconcileReport, LEDGER_DIR_ENV,
};

/// Le tre specie di passo, dal capitolo sulla ripresa del prior art: un'azione
/// o si può rifare, o si può disfare e rifare, o va lasciata a una persona.
/// Non esiste una quarta strada implicita: un passo la cui specie non è nota
/// si tratta come `HandToHuman`, mai come ripetibile per difetto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepSpecies {
    /// Si può rilanciare tale e quale: nessun effetto residuo da disfare.
    Repeatable,
    /// L'effetto già prodotto si può disfare, poi il passo si rifà.
    Compensable,
    /// Né l'uno né l'altro: nessuna azione automatica è sicura.
    HandToHuman,
}

/// Cosa fare di un passo rimasto aperto — `outcome: None` con `ended_at:
/// None`, cioè un'intenzione scritta senza il suo esito.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Il pid è morto e il passo è ripetibile: si rifà lo stesso passo.
    Resume,
    /// Il pid è morto e il passo è compensabile: si disfa l'effetto, poi si rifà.
    Redo,
    /// Il pid è vivo, o il numero potrebbe essere stato riciclato: aspettare
    /// costa meno che troncare un lavoro che potrebbe essere ancora in corso.
    StillRunning,
    /// Troppo vecchio, o già ritentato oltre il tetto di questo crate: si
    /// chiude come abbandonato invece di ritentare all'infinito.
    AbandonMarked,
    /// Non ripetibile e non compensabile: nessuna azione automatica è sicura,
    /// va segnalato a una persona.
    HandToHuman,
}

/// Un passo trovato aperto, con tutto ciò che serve al giudizio. L'identità è
/// quella del deposito: corsa, passo, tentativo, epoca.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenStep {
    pub run_id: String,
    pub step_id: String,
    pub attempt: u32,
    pub epoch: u64,
    /// Il pid del processo che lo teneva, se noto.
    pub held_by_pid: Option<u32>,
    pub started_at: i64,
    pub species: StepSpecies,
}

/// Soglie sopra le quali un passo si abbandona invece di ritentare. È una
/// rete di sicurezza propria di questo crate, indipendente dal tetto di
/// tentativi che il grafo del flusso applica altrove (`Step::max_attempts`,
/// che qui non è visibile: la ripresa non ha il grafo, solo il deposito).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    pub max_age_seconds: i64,
    pub max_attempts: u32,
}

impl Default for Thresholds {
    /// Valori di comodo, non misurati: un giorno di attesa, cinque tentativi.
    /// Chi chiama può sempre passare le proprie soglie.
    fn default() -> Self {
        Self {
            max_age_seconds: 24 * 60 * 60,
            max_attempts: 5,
        }
    }
}

/// Il verdetto per un passo, dato l'istante presente e chi sa dire se un pid
/// è vivo.
///
/// L'ordine dei controlli È la regola, non un dettaglio d'implementazione: un
/// pid vivo vince su tutto, perché troncare un lavoro che potrebbe essere
/// ancora in corso costa più che aspettare. Poi vengono i tetti di età e
/// tentativi, che si applicano a prescindere dalla specie — ritentare
/// all'infinito non è mai la risposta giusta. Solo alla fine la specie decide
/// come si ripara.
pub fn verdict_for(
    step: &OpenStep,
    now: i64,
    thresholds: &Thresholds,
    pid_is_alive: &dyn Fn(u32) -> bool,
) -> Verdict {
    if step.held_by_pid.is_some_and(|pid| pid_is_alive(pid)) {
        return Verdict::StillRunning;
    }
    if step.attempt >= thresholds.max_attempts {
        return Verdict::AbandonMarked;
    }
    if now.saturating_sub(step.started_at) > thresholds.max_age_seconds {
        return Verdict::AbandonMarked;
    }
    match step.species {
        StepSpecies::Repeatable => Verdict::Resume,
        StepSpecies::Compensable => Verdict::Redo,
        StepSpecies::HandToHuman => Verdict::HandToHuman,
    }
}

/// Il verdetto per ogni passo della lista, nello stesso ordine.
pub fn verdicts_for(
    steps: &[OpenStep],
    now: i64,
    thresholds: &Thresholds,
    pid_is_alive: &dyn Fn(u32) -> bool,
) -> Vec<Verdict> {
    steps
        .iter()
        .map(|step| verdict_for(step, now, thresholds, pid_is_alive))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(species: StepSpecies, held_by_pid: Option<u32>, attempt: u32, started_at: i64) -> OpenStep {
        OpenStep {
            run_id: "run".to_owned(),
            step_id: "step".to_owned(),
            attempt,
            epoch: 1,
            held_by_pid,
            started_at,
            species,
        }
    }

    fn dead(_pid: u32) -> bool {
        false
    }

    fn alive(_pid: u32) -> bool {
        true
    }

    #[test]
    fn dead_pid_and_repeatable_step_resumes() {
        let step = step(StepSpecies::Repeatable, Some(4242), 1, 0);
        let verdict = verdict_for(&step, 100, &Thresholds::default(), &dead);
        assert_eq!(verdict, Verdict::Resume);
    }

    #[test]
    fn alive_pid_is_never_touched_even_if_old_and_retried() {
        // Vecchissimo e già al limite dei tentativi: un pid vivo vince comunque,
        // perché troncare costa più che aspettare un numero riciclato.
        let step = step(StepSpecies::Repeatable, Some(4242), 99, 0);
        let thresholds = Thresholds {
            max_age_seconds: 1,
            max_attempts: 1,
        };
        let verdict = verdict_for(&step, 1_000_000, &thresholds, &alive);
        assert_eq!(verdict, Verdict::StillRunning);
    }

    #[test]
    fn step_too_old_is_abandoned_even_if_repeatable() {
        let step = step(StepSpecies::Repeatable, Some(4242), 1, 0);
        let thresholds = Thresholds {
            max_age_seconds: 10,
            max_attempts: 5,
        };
        let verdict = verdict_for(&step, 100, &thresholds, &dead);
        assert_eq!(verdict, Verdict::AbandonMarked);
    }

    #[test]
    fn step_already_retried_too_many_times_is_abandoned() {
        let step = step(StepSpecies::Repeatable, None, 5, 0);
        let thresholds = Thresholds {
            max_age_seconds: 1_000_000,
            max_attempts: 5,
        };
        let verdict = verdict_for(&step, 100, &thresholds, &dead);
        assert_eq!(verdict, Verdict::AbandonMarked);
    }

    #[test]
    fn step_neither_repeatable_nor_compensable_goes_to_a_human() {
        let step = step(StepSpecies::HandToHuman, None, 1, 0);
        let verdict = verdict_for(&step, 100, &Thresholds::default(), &dead);
        assert_eq!(verdict, Verdict::HandToHuman);
    }

    #[test]
    fn compensable_step_with_a_dead_pid_is_redone() {
        let step = step(StepSpecies::Compensable, Some(4242), 1, 0);
        let verdict = verdict_for(&step, 100, &Thresholds::default(), &dead);
        assert_eq!(verdict, Verdict::Redo);
    }

    #[test]
    fn unknown_pid_behaves_like_a_dead_one() {
        // Nessun detentore noto: non si può concludere "al lavoro", si procede
        // come se il pid fosse morto — mai un'attesa indefinita per un'assenza.
        let step = step(StepSpecies::Repeatable, None, 1, 0);
        let verdict = verdict_for(&step, 100, &Thresholds::default(), &alive);
        assert_eq!(verdict, Verdict::Resume);
    }

    #[test]
    fn verdicts_for_preserves_order() {
        let steps = vec![
            step(StepSpecies::Repeatable, Some(1), 1, 0),
            step(StepSpecies::HandToHuman, None, 1, 0),
        ];
        let verdicts = verdicts_for(&steps, 100, &Thresholds::default(), &dead);
        assert_eq!(verdicts, vec![Verdict::Resume, Verdict::HandToHuman]);
    }
}
