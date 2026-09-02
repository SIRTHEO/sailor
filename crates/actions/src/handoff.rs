//! Un passo consegnato all'**agente già vivo**, invece che a un processo nuovo.
//!
//! **SUPERFICIE: `gate`. POTERI PRETESI: nessuno.** Non legge il mondo, non lo
//! tocca, non scrive nel deposito di suo: offre un mandato e si mette in
//! attesa. La dichiarazione è scritta qui perché le quattro superfici di
//! `docs/2026-08-31-le-quattro-superfici.md` non esistono ancora nel codice, e
//! un'azione nuova che tace mentre il criterio nasce diventa la prima eccezione
//! non scritta — che è il modo in cui la finestra è arrivata a otto tipi di
//! passo contro tre eseguiti. Chi porta le superfici nel registro trova questa
//! riga già scritta e non deve indovinarla.
//!
//! **PERCHÉ ESISTE.** Misurato il 31/08/2026: un flusso di quattro passi costa
//! **2,79 volte** un singolo prompt sullo stesso compito, e il rapporto dei
//! consumi è il rapporto dei turni — 62 contro 30. Ogni passo avvia un processo
//! che riscopre il repository da zero. L'alternativa è che il flusso
//! **descriva** il lavoro e a eseguirlo sia l'agente già vivo nel terminale, che
//! il contesto ce l'ha già. Ma un agente che lavora fuori dal motore non scrive
//! niente nel deposito, e allora sparisce la metà per cui Sailor esiste. Questa
//! azione tiene insieme le due metà: il passo resta un record, con la sua
//! intenzione scritta prima e il suo esito scritto dopo; a eseguirlo è qualcun
//! altro.
//!
//! **NON AVVIA NIENTE, E NON È UNA MANCANZA.** Il valore di questo passo è
//! esattamente che nessun processo nasce: se ne avviasse uno saremmo tornati al
//! costo che l'azione esiste per togliere.

use crate::{sink_for_step, Pipe, StepSinks};
use flow::{
    Action, ActionError, ActionOutcome, EffectStatus, SharedState, StepRecord, StepSpecies,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Il nome sotto cui questa azione si registra in un `flow::ActionRegistry`.
pub const HANDED_TO_AGENT_ACTION: &str = "handed_to_agent";

/// La collezione del deposito dove `sailor step close` scrive chi ha chiuso un
/// passo consegnato.
///
/// **STA QUI E NON NEL COMANDO PERCHÉ LA LEGGONO IN DUE.** Chi chiude scrive,
/// chi apre il passo dopo legge per rifiutare un giudice che è anche autore. Un
/// nome ricopiato nei due punti diverge al primo refuso, e il rifiuto
/// smetterebbe di scattare **in silenzio** — cioè proprio la serratura si
/// aprirebbe da sola senza che nessuna prova diventi rossa.
pub const HOLDER_COLLECTION: &str = "handoff_holders";

/// L'indirizzo, dentro `HOLDER_COLLECTION`, di chi ha chiuso un passo.
pub fn holder_key(run_id: &str, step_id: &str) -> String {
    format!("{run_id}/{step_id}")
}

/// Che cosa un passo consegnato dichiara.
///
/// **DUE CAMPI SI DICHIARANO QUI E SI LEGGONO ALTROVE, ED È VOLUTO.**
/// `handoff_timeout_secs` lo legge `inspect_effect` e `same_holder_ok` lo legge
/// `sailor step open`, tutti e due direttamente da `record.input` — perché
/// quando servono la struttura non c'è più: c'è solo un record nel deposito.
/// Stanno comunque in questa struttura per due ragioni misurabili: senza,
/// `unknown_fields` li chiamerebbe refusi e `flow check` respingerebbe un passo
/// scritto bene; e una scadenza mancante o scritta come testo romperebbe il
/// passo alla ripresa invece che quando si esegue.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct HandoffSpec {
    /// Il lavoro, per esteso. **Resta intero nell'`input` del record**, che il
    /// deposito conserva senza troncare; in `said` finisce solo la riga corta,
    /// perché `said` è tagliato a 16 KB (`flow::MAX_SAID_BYTES`) e un mandato
    /// lungo ci si perderebbe dentro a metà frase.
    mandate: String,
    /// A chi è offerto. È un'etichetta per chi guarda e il valore predefinito
    /// che `sailor step open` suggerisce, **non** una credenziale: vedi la
    /// debolezza dichiarata in `crates/sailor/src/step_cmd.rs`.
    holder: String,
    /// Entro quanti secondi qualcuno deve prenderlo in carico. Oltre, il passo
    /// risulta non applicato e una ripresa lo rimette fra i pronti.
    handoff_timeout_secs: u64,
    /// Se chi ha chiuso una dipendenza può aprire anche questo passo.
    ///
    /// **NEGAZIONE PREDEFINITA, NON LISTA DI PERMESSI.** `false` vuol dire che
    /// chi ha prodotto il lavoro non lo giudica — il vincolo permanente «chi
    /// crea non giudica». Il predefinito è la negazione perché una lista di
    /// permessi dimenticata lascia passare tutto, mentre una negazione
    /// dimenticata al massimo ferma un lavoro e lo si vede subito.
    #[serde(default)]
    same_holder_ok: bool,
    /// Ciò che questa azione non riconosce, per la stessa ragione di
    /// `EngineSpec::extra`: un refuso nel `with` si nomina a `flow check`, cioè
    /// prima di spendere.
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

/// L'azione che consegna un passo a chi è già vivo.
pub struct HandoffAction {
    watcher: Option<Arc<dyn StepSinks>>,
    /// Che ora è, per `inspect_effect`.
    ///
    /// **INIETTABILE PERCHÉ LA SCADENZA SI DEVE POTER PROVARE.** `inspect_effect`
    /// non riceve un orologio dal tratto — lo riceve `reconcile`, che però non
    /// glielo passa — e una prova sulla scadenza con l'ora vera dovrebbe
    /// aspettare davvero, cioè sarebbe un'attesa fissa: il difetto per cui una
    /// prova di questa casa è già stata rotta nel punto sbagliato.
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl Default for HandoffAction {
    fn default() -> Self {
        Self::new()
    }
}

impl HandoffAction {
    pub fn new() -> Self {
        Self {
            watcher: None,
            now: Arc::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |elapsed| elapsed.as_secs() as i64)
            }),
        }
    }

    /// Con qualcuno che guarda: il mandato compare **mentre** il passo lo
    /// offre, non quando la corsa è finita. È il punto di tutto: una persona
    /// deve vedere cosa è stato chiesto nel momento in cui viene chiesto.
    pub fn watched_by(mut self, watcher: Option<Arc<dyn StepSinks>>) -> Self {
        self.watcher = watcher;
        self
    }

    /// Con un orologio dichiarato, per le prove sulla scadenza.
    pub fn at_time(mut self, now: Arc<dyn Fn() -> i64 + Send + Sync>) -> Self {
        self.now = now;
        self
    }
}

impl Action for HandoffAction {
    /// Dalla struttura vera, come le due azioni gemelle: un elenco scritto a
    /// mano qui accanto sarebbe una seconda copia della stessa verità.
    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match serde_json::from_value::<HandoffSpec>(declared.clone()) {
            Ok(spec) => spec.extra.into_keys().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// **RIFARE QUESTO PASSO È SICURO, E LA RAGIONE È COSA FA DAVVERO.**
    /// L'effetto dell'azione è *offrire* un mandato, non eseguirlo. Offrirlo due
    /// volte non duplica niente sul mondo: duplica una riga di testo. Il lavoro
    /// vero lo fa una persona o un agente, e quello è protetto dal fatto che
    /// `sailor step open` rifiuta di aprire un passo che non è in attesa.
    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    /// **NON CHIEDE NIENTE AL SISTEMA OPERATIVO, ED È IL PUNTO.** Chi tiene un
    /// passo consegnato non è un processo: è una scadenza scritta nel record.
    /// Interrogare il kernel qui sarebbe il guasto 12 rifatto — dentro il
    /// perimetro `pgrep` risponde vuoto *senza errore*, e un sensore cieco che
    /// risponde «nessuno» è peggio di un sensore assente, perché chi sta a valle
    /// si fida.
    ///
    /// Niente colonna nuova: la scadenza sta in `input`, l'istante di partenza
    /// in `started_at`, e tutti e due sono già nel record.
    ///
    /// Prima della scadenza `Unknown` — *non so se qualcuno ci sta lavorando,
    /// quindi non dichiaro niente*. Dopo, `NotApplied`: nessuno l'ha preso in
    /// carico nel tempo che il passo si dava.
    fn inspect_effect(
        &self,
        record: &StepRecord,
        _shared: &SharedState,
    ) -> Result<EffectStatus, ActionError> {
        let Some(limit) = record
            .input
            .get("handoff_timeout_secs")
            .and_then(Value::as_i64)
        else {
            // Un record senza scadenza leggibile non si dichiara scaduto: una
            // consegna senza tetto è ambigua, e l'ambiguità si conserva.
            return Ok(EffectStatus::Unknown(
                "the handover declares no readable deadline".to_owned(),
            ));
        };
        let deadline = record.started_at.saturating_add(limit);
        if (self.now)() < deadline {
            Ok(EffectStatus::Unknown(format!(
                "handed over, and the deadline has not passed: {} seconds to go",
                deadline - (self.now)()
            )))
        } else {
            Ok(EffectStatus::NotApplied)
        }
    }

    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let live = sink_for_step(&self.watcher, shared);
        // **I RINVII ARRIVANO GIÀ SCIOLTI, E QUI SERVONO PIÙ CHE ALTROVE.** Il
        // mandato di un passo consegnato è quasi sempre il lavoro deciso dal
        // passo prima: senza `$from` resterebbe una costante scritta il giorno
        // in cui il flusso è nato, e la consegna non servirebbe a niente. A
        // scioglierli è `step_input`, per ogni azione e una volta sola.
        let spec: HandoffSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        if spec.mandate.trim().is_empty() {
            return Err(ActionError::new(
                "invalid_input",
                "an empty brief is not a job: nobody would know what they were asked for",
            ));
        }
        let run_id = shared
            .get(flow::CURRENT_RUN)
            .and_then(Value::as_str)
            .unwrap_or("<an unknown run>");
        let step_id = shared
            .get(flow::CURRENT_STEP)
            .and_then(Value::as_str)
            .unwrap_or("<an unknown step>");

        // IL MANDATO SI VEDE MENTRE SUCCEDE, non a corsa finita: chi guarda
        // deve poterlo prendere in carico adesso.
        if let Some(live) = live.as_deref() {
            live.chunk(
                Pipe::Stdout,
                format!(
                    "\n── brief handed to «{}» ──\n{}\n──\n",
                    spec.holder, spec.mandate
                )
                .as_bytes(),
            );
        }

        // **LA RIGA È CORTA APPOSTA.** Il mandato intero resta nell'`input` del
        // record, che il deposito conserva senza tagliare; `said` è troncato a
        // 16 KB, e metterlo lì vorrebbe dire perderne la coda proprio quando è
        // lungo, cioè quando serve.
        Ok(ActionOutcome::Waiting(format!(
            "consegnato a «{}»; il mandato è nell'ingresso del passo, non qui. \
             Prendilo con: sailor step open --run {run_id} --step {step_id} --as {}",
            spec.holder, spec.holder
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    fn shared_for(run_id: &str, step_id: &str) -> SharedState {
        let mut shared = SharedState::new();
        shared.insert(flow::CURRENT_RUN.to_owned(), json!(run_id));
        shared.insert(flow::CURRENT_STEP.to_owned(), json!(step_id));
        shared
    }

    /// **NESSUN PROCESSO NASCE, E LA DECISIONE È «IN ATTESA».** È il valore
    /// dell'azione detto come misura: se un giorno qualcuno la facesse avviare
    /// qualcosa, il costo che esiste per togliere tornerebbe.
    #[test]
    fn a_handed_step_waits_instead_of_going() {
        let action = HandoffAction::new();
        let outcome = action
            .execute(
                &json!({
                    "mandate": "ripara il guasto 25",
                    "holder": "claude-vivo",
                    "handoff_timeout_secs": 3600
                }),
                &shared_for("run-1", "implementa"),
            )
            .expect("la consegna non fallisce");
        match outcome {
            ActionOutcome::Waiting(line) => {
                assert!(
                    line.contains("sailor step open --run run-1 --step implementa"),
                    "la riga deve portare il comando da eseguire: {line}"
                );
                assert!(line.contains("claude-vivo"), "{line}");
            }
            ActionOutcome::Went(value) => {
                panic!("una consegna non conosce il proprio risultato: {value}")
            }
            // A handover waits for **a person**, and a person does not come
            // back by themselves on the next beat: `NotYet` would put the step
            // back in play while somebody is holding it.
            ActionOutcome::NotYet(line) => {
                panic!("a handover waits for a person, not for a beat: {line}")
            }
        }
    }

    /// **IL MANDATO LUNGO STA NELL'INGRESSO, NON IN `said`.** La riga corta
    /// resta corta anche con un mandato da 40 KB: è la differenza fra un lavoro
    /// che si legge intero e uno tagliato a metà frase.
    #[test]
    fn a_long_mandate_stays_out_of_the_said_line() {
        let long = "ripara ".repeat(6000);
        assert!(
            long.len() > flow::MAX_SAID_BYTES,
            "la fixture deve superare il tetto di `said`, altrimenti non prova niente"
        );
        let action = HandoffAction::new();
        let outcome = action
            .execute(
                &json!({
                    "mandate": long.clone(),
                    "holder": "claude-vivo",
                    "handoff_timeout_secs": 60
                }),
                &shared_for("run-1", "implementa"),
            )
            .expect("la consegna non fallisce");
        let ActionOutcome::Waiting(line) = outcome else {
            panic!("una consegna si mette in attesa");
        };
        assert!(
            !line.contains(&long),
            "il mandato non deve finire nella riga: sarebbe troncato a {} byte",
            flow::MAX_SAID_BYTES
        );
        assert!(
            line.len() < flow::MAX_SAID_BYTES,
            "la riga deve restare corta: {} byte",
            line.len()
        );
    }

    // **LA PROVA CHE IL MANDATO PUÒ VENIRE DAL PASSO PRIMA NON STA PIÙ QUI.**
    // Chiamava `execute` con `{"$from": …}` dentro, e reggeva perché questa
    // azione risolveva i rinvii da sé. Dal 01/09/2026 li scioglie
    // `flow::step_input` per tutte: riscritta qui, dovrebbe sciogliere il rinvio
    // a mano prima di chiamare — cioè misurare la prova invece del prodotto. La
    // regola si interroga dove vive, in
    // `crates/flow/tests/a_reference_reaches_every_action.rs`, e lì vale per
    // ogni azione registrata invece che per questa sola.

    /// Un `with` con un refuso si nomina a controllo, prima di spendere.
    #[test]
    fn a_misspelled_field_is_named_before_the_run() {
        let action = HandoffAction::new();
        let unknown = action.unknown_fields(&json!({
            "mandate": "x",
            "holder": "chi",
            "handoff_timeout_secs": 1,
            "handoff_timeout_sec": 30
        }));
        assert_eq!(unknown, vec!["handoff_timeout_sec".to_owned()]);
    }

    /// **PRIMA DELLA SCADENZA NON SI DICHIARA NIENTE.** È la differenza fra
    /// «non lo so» e «non è stato fatto», e chiuderla dal lato sbagliato
    /// toglierebbe il lavoro di mano a chi lo sta facendo.
    #[test]
    fn before_the_deadline_the_effect_is_unknown() {
        let clock = Arc::new(Mutex::new(1_000i64));
        let reading = Arc::clone(&clock);
        let action =
            HandoffAction::new().at_time(Arc::new(move || *reading.lock().expect("orologio sano")));
        let mut record = StepRecord::started(
            "run-1",
            "implementa",
            1,
            1,
            vec![],
            json!(null),
            vec![],
            1_000,
        );
        record.input = json!({"handoff_timeout_secs": 600});
        record.started_at = 1_000;

        let inspected = action
            .inspect_effect(&record, &SharedState::new())
            .expect("la sonda risponde");
        assert!(
            matches!(inspected, EffectStatus::Unknown(_)),
            "dentro la finestra non si dichiara: {inspected:?}"
        );

        *clock.lock().expect("orologio sano") = 1_601;
        let inspected = action
            .inspect_effect(&record, &SharedState::new())
            .expect("la sonda risponde");
        assert_eq!(
            inspected,
            EffectStatus::NotApplied,
            "passata la scadenza nessuno l'ha preso in carico"
        );
    }

    /// Un mandato vuoto non è un lavoro.
    #[test]
    fn an_empty_mandate_is_refused() {
        let action = HandoffAction::new();
        let error = action
            .execute(
                &json!({"mandate": "   ", "holder": "chi", "handoff_timeout_secs": 1}),
                &shared_for("run-1", "implementa"),
            )
            .expect_err("un mandato vuoto si rifiuta");
        assert_eq!(error.class, "invalid_input");
    }

    /// Rifare una consegna è sicuro: quel che duplica è una riga di testo.
    #[test]
    fn handing_a_step_over_twice_is_safe() {
        assert_eq!(HandoffAction::new().species(), StepSpecies::Repeatable);
    }
}
