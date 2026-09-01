//! `sailor step`: prendere in carico e chiudere un passo che un flusso ha
//! consegnato all'agente già vivo.
//!
//! **DUE COMANDI E NON UNO, PERCHÉ FANNO DUE COSE DIVERSE.** `open` dichiara che
//! qualcuno prende il lavoro; `close` dichiara com'è andato. In mezzo c'è il
//! lavoro vero, che Sailor non esegue e non deve eseguire: è tutto il punto
//! della consegna.
//!
//! ─────────────────────────────────────────────────────────────────────────
//! **LA PRIMA DEBOLEZZA, DICHIARATA INVECE CHE NASCOSTA: `--as <chi>` È UN NOME
//! CHE SE LO SCEGLIE CHI LO SCRIVE.**
//!
//! Il rifiuto qui sotto applica «chi crea non giudica»: chi ha chiuso una
//! dipendenza non può aprire il passo che la giudica. Ma applicare quella regola
//! a un nome scelto liberamente da chi si vuole escludere è **una serratura con
//! la chiave in tasca all'escluso**: basta scrivere `--as qualcun-altro` e il
//! rifiuto non scatta.
//!
//! Non è una svista da chiudere qui: Sailor non ha nessun identificativo di
//! sessione da leggere — non sa chi sta digitando, e nessun campo del deposito
//! glielo può dire. Finché non ce l'ha, questo controllo vale contro la
//! distrazione, non contro chi vuole aggirarlo. Chi ci costruisce sopra una
//! garanzia si sta fidando di una cosa che non regge.
//! ─────────────────────────────────────────────────────────────────────────

use actions::handoff::{holder_key, HOLDER_COLLECTION};
use flow::{
    Completion, Decision, FlowFile, InProcessExecutor, Outcome, StepRecord,
};
use ledger::{EngineIdentity, Ledger, ModelCallRecord, StoreRecord};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(message) => {
            eprintln!("sailor step: {message}");
            1
        }
    }
}

fn dispatch(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("open") => open_step(&flags(&args[1..])?),
        Some("close") => close_step(&flags(&args[1..])?),
        _ => Err(usage()),
    }
}

/// Le forme di `sailor step`, una per riga. Vedi `flow_cmd::USAGE`.
pub const USAGE: &[&str] = &[
    "sailor step open --run <run> --step <step> --as <who>",
    "sailor step close --run <run> --step <step> --as <who> --outcome <went|broke> \
     [--output-file <file>] [--turns <n>] [--said <text>]",
];

fn usage() -> String {
    format!("usage:\n  {}", USAGE.join("\n  "))
}

/// Le opzioni scritte sulla riga, in coppie `--nome valore`.
///
/// **UN'OPZIONE SENZA VALORE È UN ERRORE, NON UN VUOTO.** `--step --as chi`
/// prenderebbe `--as` come identificativo del passo e cercherebbe un passo che
/// non esiste, con un messaggio che manda a guardare il flusso invece della riga
/// di comando.
fn flags(args: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut found = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(name) = rest.next() {
        let Some(name) = name.strip_prefix("--") else {
            return Err(format!("non capisco «{name}»; {}", usage()));
        };
        let value = rest
            .next()
            .ok_or_else(|| format!("«--{name}» vuole un valore dopo di sé"))?;
        if let Some(other) = value.strip_prefix("--") {
            return Err(format!(
                "«--{name}» ha ricevuto «--{other}» come valore: manca il valore vero"
            ));
        }
        found.insert(name.to_owned(), value.clone());
    }
    Ok(found)
}

fn required<'a>(found: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str, String> {
    found
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| format!("manca «--{name}»; {}", usage()))
}

// ── prendere in carico ───────────────────────────────────────────────────

/// Apre un tentativo nuovo su un passo consegnato, a nome di chi lo prende.
///
/// **L'INGRESSO SI COPIA TALE E QUALE, E NON È PIGRIZIA.** L'impronta
/// `input_digest` si calcola dall'ingresso: copiarlo identico fa combaciare le
/// due impronte, e allora `flow::attempt_relation` dichiara `SameInput` — cioè
/// «questo è lo stesso lavoro di prima, ripreso», che è la verità. Ricostruire
/// l'ingresso, o metterci dentro chi lo prende, darebbe `DifferentInput`: chi
/// rilegge la corsa vedrebbe due lavori diversi dove ce n'è uno solo, e il
/// mandato — che vive lì dentro — cambierebbe impronta senza essere cambiato.
fn open_step(found: &BTreeMap<String, String>) -> Result<String, String> {
    let ledger = open_ledger()?;
    open_step_in(&ledger, found)
}

/// Il corpo di `open`, col deposito dichiarato invece che dedotto da `HOME`.
///
/// **SEPARATO PERCHÉ ALTRIMENTI NON SI PROVA.** `ledger::default_directory`
/// legge una variabile d'ambiente, che è globale al processo: una prova che la
/// scrivesse rovinerebbe le altre a caso, e chi vede il rosso guarderebbe il
/// modulo sbagliato.
fn open_step_in(ledger: &Ledger, found: &BTreeMap<String, String>) -> Result<String, String> {
    let run_id = required(found, "run")?;
    let step_id = required(found, "step")?;
    let holder = required(found, "as")?;

    let records = ledger
        .steps(run_id)
        .map_err(|error| format!("non riesco a leggere la corsa {run_id}: {error}"))?;
    let latest = last_attempt(&records, step_id).ok_or_else(|| {
        format!("la corsa {run_id} non ha nessun passo che si chiama {step_id}")
    })?;

    // **SI APRE SOLO CIÒ CHE È IN ATTESA.** Un passo andato, rotto o ancora
    // aperto non è stato consegnato a nessuno: aprirlo di nuovo vorrebbe dire
    // rifare un lavoro che il motore sta già facendo, o disfare un esito.
    match latest.outcome {
        Some(Outcome::Waiting) => {}
        None => {
            return Err(format!(
                "il passo {step_id} è aperto: qualcuno lo sta già eseguendo, e non è stato \
                 consegnato. Chiudilo prima, o riprendi la corsa con `sailor flow resume {run_id}`"
            ))
        }
        Some(other) => {
            return Err(format!(
                "il passo {step_id} non è in attesa ma {other:?}: non c'è nessuna consegna \
                 da prendere in carico"
            ))
        }
    }

    refuse_the_author_as_judge(ledger, run_id, step_id, holder, latest)?;

    let now = now_secs()?;
    let mut started = StepRecord::started(
        run_id,
        step_id,
        latest.attempt + 1,
        latest.epoch + 1,
        latest.deps.clone(),
        latest.input.clone(),
        latest.gates.clone(),
        now,
    );
    started.attempt_relation = flow::attempt_relation(&records, &started);
    // **NESSUN PID, E IL VUOTO È UN'AFFERMAZIONE.** Questo campo dice «il
    // processo che tiene il passo», e qui nessun processo lo tiene: a tenerlo è
    // un agente in un terminale, che non è figlio di niente e che il kernel non
    // sa distinguere da chiunque altro. Scriverci il pid di *questo* comando
    // sarebbe una bugia comoda: il comando esce subito, e alla ripresa quel pid
    // risulterebbe morto — cioè la consegna verrebbe chiusa sotto i piedi di chi
    // ci sta lavorando. Chi tiene un passo consegnato è una scadenza scritta nel
    // record, non un processo.
    started.held_by_pid = None;
    // La specie resta quella congelata alla consegna: un'azione riscritta nel
    // frattempo non deve cambiare il giudizio su un passo già offerto.
    started.species = latest.species;
    ledger
        .append_step_started(&started)
        .map_err(|error| format!("non riesco ad aprire il passo {step_id}: {error}"))?;

    // **IL MANDATO SI LEGGE DALL'INGRESSO, MAI DA `said`.** In `said` c'è la
    // riga corta, tagliata a 16 KB; il lavoro per esteso sta nell'ingresso, che
    // il deposito conserva intero. Leggerlo dalla parte sbagliata darebbe a chi
    // prende il lavoro un mandato troncato a metà frase, senza dirglielo.
    let mandate = started
        .input
        .get("mandate")
        .and_then(Value::as_str)
        .unwrap_or("<questo passo non porta nessun mandato scritto>");
    Ok(format!(
        "passo {step_id} preso in carico da «{holder}» — corsa {run_id}, tentativo {}\n\
         ── mandato ──\n{mandate}\n──\n\
         quando hai finito: sailor step close --run {run_id} --step {step_id} --as {holder} \
         --outcome went [--output-file <file>] [--turns <n>]",
        started.attempt
    ))
}

/// **CHI CREA NON GIUDICA, APPLICATO ALLE DIPENDENZE.**
///
/// Chi ha chiuso un passo da cui questo dipende ne è l'autore: lasciargli anche
/// il passo che ne giudica il lavoro è farsi la propria revisione. Il vincolo
/// permanente non dice «di solito».
///
/// **SI CHIEDE DUE VOLTE, ALL'APERTURA E ALLA CHIUSURA, E NON È UNA RIPETIZIONE
/// INUTILE.** Solo all'apertura resterebbe una porta aperta larga come la prima:
/// aprire con un nome qualunque e **chiudere** con quello dell'autore. Il gesto
/// che conta è la chiusura — è lì che si scrive un verdetto — quindi è lì che il
/// rifiuto deve valere. All'apertura vale perché fermare presto costa meno che
/// fermare dopo il lavoro.
///
/// **LA NEGAZIONE È IL PREDEFINITO.** Un elenco di permessi dimenticato lascia
/// passare tutto; una negazione dimenticata al massimo ferma un lavoro, e lo si
/// vede subito. Il permesso lo dichiara il flusso, passo per passo, con
/// `same_holder_ok`.
fn refuse_the_author_as_judge(
    ledger: &Ledger,
    run_id: &str,
    step_id: &str,
    holder: &str,
    record: &StepRecord,
) -> Result<(), String> {
    let same_holder_ok = record
        .input
        .get("same_holder_ok")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if same_holder_ok {
        return Ok(());
    }
    for dependency in &record.deps {
        let written = ledger
            .read_record(HOLDER_COLLECTION, &holder_key(run_id, dependency))
            .map_err(|error| format!("non riesco a leggere chi ha chiuso {dependency}: {error}"))?;
        if written.is_some_and(|found| found.written_by == holder) {
            return Err(format!(
                "«{holder}» ha già chiuso {dependency}, da cui {step_id} dipende: chi crea \
                 non giudica. Usa un altro nome, oppure dichiara `\"same_holder_ok\": true` \
                 nel passo se il flusso vuole davvero che sia la stessa mano"
            ));
        }
    }
    Ok(())
}

// ── chiudere ─────────────────────────────────────────────────────────────

/// Chiude il passo aperto, validando l'uscita contro lo schema del passo.
///
/// **LA VALIDAZIONE STA QUI PERCHÉ `RecordStore::close` NON LA FA.** Il motore
/// valida in `run_one`, prima di chiudere; questo comando scavalca il motore, e
/// senza il controllo un'uscita malformata entrerebbe nel deposito come buona.
/// Il danno non si vedrebbe adesso: si vedrebbe tre passi dopo, quando il passo
/// che dipende da questo riceve un ingresso che il suo schema rifiuta — e la
/// colpa cadrebbe su di lui.
fn close_step(found: &BTreeMap<String, String>) -> Result<String, String> {
    // Gli esiti si controllano **prima** di aprire il deposito: un refuso sulla
    // riga di comando non deve costare un file aperto, e il messaggio che ne
    // esce parla della riga, non della macchina.
    let _ = declared_outcome(found)?;
    let ledger = open_ledger()?;
    let run_id = required(found, "run")?;
    let flow = flow_of_run(&ledger, run_id)?;
    close_step_in(&ledger, &flow, found)
}

/// L'esito che chi chiude dichiara.
///
/// **SOLO DUE, E NON È UNA SEMPLIFICAZIONE.** `Waiting`, `Skipped` e `Stopped`
/// li scrive il motore per descrivere cose che sono successe a lui: un passo
/// consegnato che «salta» o «aspetta» sarebbe una frase senza referente, e
/// lascerebbe la corsa in uno stato che nessuna ripresa sa sbloccare.
fn declared_outcome(found: &BTreeMap<String, String>) -> Result<Outcome, String> {
    match required(found, "outcome")? {
        "went" => Ok(Outcome::Went),
        "broke" => Ok(Outcome::Broke),
        other => Err(format!(
            "«--outcome {other}» non è un esito che una persona possa dichiarare: \
             i valori sono `went` e `broke`"
        )),
    }
}

/// Il corpo di `close`, col deposito e il flusso dichiarati invece che dedotti.
fn close_step_in(
    ledger: &Ledger,
    flow: &FlowFile,
    found: &BTreeMap<String, String>,
) -> Result<String, String> {
    let run_id = required(found, "run")?;
    let step_id = required(found, "step")?;
    let holder = required(found, "as")?;
    let outcome = declared_outcome(found)?;

    let records = ledger
        .steps(run_id)
        .map_err(|error| format!("non riesco a leggere la corsa {run_id}: {error}"))?;
    // Il record aperto lo trova da sé: chi chiude non deve sapere a che
    // tentativo è arrivato, e chiederglielo sarebbe un numero da sbagliare.
    let open = records
        .iter()
        .filter(|record| record.step_id == step_id && record.outcome.is_none())
        .max_by_key(|record| (record.attempt, record.epoch))
        .ok_or_else(|| {
            format!(
                "la corsa {run_id} non ha nessun passo {step_id} aperto: \
                 prendilo in carico con `sailor step open` prima di chiuderlo"
            )
        })?;

    // **SI CHIUDE SOLO CIÒ CHE `sailor step open` HA APERTO.** Un record con un
    // pid l'ha aperto l'esecutore in processo, e quel processo sta girando
    // adesso: chiuderlo da qui gli toglie il passo di sotto, e la sua chiusura
    // fallirà con «già chiuso» — un guasto della corsa per un gesto fatto in un
    // altro terminale. Un passo consegnato non porta pid, quindi la regola
    // separa esattamente i due casi senza doverli indovinare.
    if open.held_by_pid.is_some() {
        return Err(format!(
            "il passo {step_id} è tenuto dal processo {}: non è stato consegnato a nessuno, \
             lo sta eseguendo il motore. Se quel processo è morto, usa \
             `sailor flow resume {run_id}`",
            open.held_by_pid.unwrap_or_default()
        ));
    }

    // Il rifiuto vale soprattutto qui: la chiusura è il gesto che scrive un
    // verdetto, e un verdetto sul proprio lavoro non vale.
    refuse_the_author_as_judge(ledger, run_id, step_id, holder, open)?;

    let step = flow.graph.step(step_id).ok_or_else(|| {
        format!(
            "il flusso {} non dichiara nessun passo {step_id}: il deposito e il file \
             si sono separati",
            flow.id
        )
    })?;

    let output = match outcome {
        Outcome::Went => match found.get("output-file") {
            // **SENZA UN FILE NON SI SCRIVE UN'USCITA VUOTA, SI RIFIUTA SE
            // QUALCUNO LA ASPETTA.** Il deposito non sa distinguere «uscita
            // nulla» da «nessuna uscita»: nel registro degli eventi tutte e due
            // diventano `"output": null`, e rileggendo tornano `None` (guasto
            // 31, misurato sulla prima corsa vera del 31/08/2026). Chiudere
            // `went` senza uscita mentre un altro passo dipende da questo fa
            // fallire quel passo con «non ha uscita tipata» — cioè il difetto
            // ricompare più tardi, su un passo innocente, che è esattamente ciò
            // che questa funzione esiste per impedire.
            None => {
                let waiting_on_it = dependents_of(flow, step_id);
                if !waiting_on_it.is_empty() {
                    return Err(format!(
                        "il passo {step_id} si chiuderebbe senza uscita, ma {} dipende da lui \
                         e ne pretende una tipata: la corsa si fermerebbe lì. Dichiarala con \
                         `--output-file <file>`",
                        waiting_on_it.join(", ")
                    ));
                }
                None
            }
            Some(path) => {
                let text = std::fs::read_to_string(path)
                    .map_err(|error| format!("non riesco a leggere {path}: {error}"))?;
                let value: Value = serde_json::from_str(&text)
                    .map_err(|error| format!("{path} non è JSON valido: {error}"))?;
                step.output_schema.validate(&value).map_err(|error| {
                    format!(
                        "l'uscita dichiarata non rispetta lo schema del passo {step_id}: {error}. \
                         Il passo non è stato chiuso — un'uscita malformata accettata qui \
                         ucciderebbe la corsa più avanti, e la colpa cadrebbe su un altro passo"
                    )
                })?;
                Some(value)
            }
        },
        // Un passo rotto non ha uscita da validare: non ne ha prodotta nessuna.
        _ => None,
    };

    let now = now_secs()?;
    let attempt = open.attempt;
    let epoch = open.epoch;
    ledger
        .close_step(
            run_id,
            step_id,
            attempt,
            epoch,
            Completion {
                outcome,
                output,
                said: found.get("said").cloned(),
                failure_class: match outcome {
                    Outcome::Broke => Some("handed_back".to_owned()),
                    _ => None,
                },
                ended_at: now,
                bytes_seen: None,
                bytes_discarded: None,
            },
        )
        .map_err(|error| format!("non riesco a chiudere il passo {step_id}: {error}"))?;

    // Chi ha chiuso resta scritto: lo rilegge `open` sul passo che dipende da
    // questo, per rifiutare un giudice che è anche autore.
    ledger
        .put_record(&StoreRecord {
            collection: HOLDER_COLLECTION.to_owned(),
            key: holder_key(run_id, step_id),
            value: serde_json::json!({"outcome": format!("{outcome:?}").to_lowercase()}),
            written_by: holder.to_owned(),
            written_at: now,
        })
        .map_err(|error| format!("non riesco a registrare chi ha chiuso {step_id}: {error}"))?;

    let mut report = format!(
        "passo {step_id} chiuso da «{holder}» come {}",
        match outcome {
            Outcome::Went => "andato",
            _ => "rotto",
        }
    );

    if let Some(turns) = found.get("turns") {
        let turns: u64 = turns
            .parse()
            .map_err(|_| format!("«--turns {turns}» non è un numero"))?;
        write_self_declared_turns(&ledger, run_id, step_id, holder, turns, now)?;
        let _ = write!(
            report,
            "\n{turns} turni autodichiarati, senza costo: entrano fra le chiamate \
             che il totale non conosce, e da adesso la spesa di questa corsa è un «almeno»"
        );
    }

    // **COSA È PRONTO ADESSO, E LA RIGA PER RIPRENDERE.** Chi chiude un passo a
    // mano non ha davanti il grafo: senza questo dovrebbe aprirlo per sapere se
    // ha sbloccato qualcosa, cioè uscire da Sailor per interrogare Sailor.
    let decision = InProcessExecutor
        .decision(&flow.graph, run_id, ledger)
        .map_err(|error| format!("non riesco a calcolare cosa è pronto: {error}"))?;
    let _ = write!(report, "\n{}", what_comes_next(&decision, run_id));
    Ok(report)
}

/// Cosa si può fare adesso, detto a chi ha appena chiuso.
fn what_comes_next(decision: &Decision, run_id: &str) -> String {
    match decision {
        Decision::Ready(steps) => format!(
            "pronti adesso: {}. Riprendi con: sailor flow resume {run_id}",
            steps.join(", ")
        ),
        Decision::Waiting(steps) => format!(
            "in attesa di qualcuno: {}. Prendi il lavoro con: sailor step open --run {run_id} \
             --step <passo> --as <chi>",
            steps.join(", ")
        ),
        Decision::Running(steps) => format!("ancora in corso: {}", steps.join(", ")),
        Decision::Stopped(steps) => format!("fermi nel deposito: {}", steps.join(", ")),
        Decision::Failed(steps) => format!("rotti oltre i tentativi: {}", steps.join(", ")),
        Decision::CapReached(stop) => registry::why_it_stopped(stop),
        Decision::Complete => "la corsa è completa: non resta niente da fare".to_owned(),
    }
}

/// Scrive una riga di consumo **autodichiarata**, e marcata come tale.
///
/// ─────────────────────────────────────────────────────────────────────────
/// **LA SECONDA DEBOLEZZA: SU UN FLUSSO CON CONSEGNE IL TETTO DI SPESA SMETTE DI
/// ESSERE UNA GARANZIA.**
///
/// Il tetto (`FlowFile::spend_cap_micros`) si misura sulle righe di
/// `model_calls`, e fino a oggi quelle righe le scriveva il motore leggendo ciò
/// che il fornitore dichiara. Questa la scrive **chi ha fatto il lavoro**, su un
/// numero che si è contato da solo. Non è verificabile da nessuna parte.
///
/// Per questo `cost_micros` resta `None` e non un numero inventato: così la riga
/// entra in `Spend::calls_without_cost`, `Spend::is_complete()` diventa falso, e
/// ogni posto che mostra il tetto dice già oggi «la spesa vera è più alta». Un
/// costo stimato qui sarebbe peggio del vuoto: renderebbe *completa* una somma
/// che non lo è, e il tetto scatterebbe su una cifra inventata senza che nessuno
/// possa più accorgersene.
/// ─────────────────────────────────────────────────────────────────────────
fn write_self_declared_turns(
    ledger: &Ledger,
    run_id: &str,
    step_id: &str,
    holder: &str,
    turns: u64,
    now: i64,
) -> Result<(), String> {
    ledger
        .record_model_call(&ModelCallRecord {
            call_id: format!("{run_id}:{step_id}:{now}:handed"),
            run_id: run_id.to_owned(),
            step_id: Some(step_id.to_owned()),
            // Nessuna sessione: questo passo non apre nessun processo — il
            // lavoro va a un agente che è già vivo nel terminale, e la sua
            // conversazione non è di Sailor e non si può riprendere da qui.
            session_id: None,
            // Il perché della riga sta nel `purpose`: chi somma le chiamate di
            // una corsa deve poter separare ciò che il motore ha misurato da ciò
            // che qualcuno ha dichiarato di sé.
            purpose: "handed_to_agent:self_declared".to_owned(),
            cli: holder.to_owned(),
            requested_model: String::new(),
            actual_model: String::new(),
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: Some(turns),
            // Vedi il commento sopra: il vuoto è la parte onesta di questa riga.
            cost_micros: None,
            declared_cost_micros: None,
            price_currency: None,
            input_price_micros_per_million: None,
            output_price_micros_per_million: None,
            cached_price_micros_per_million: None,
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            // **DICHIARATA DA UN AGENTE, E NON «NESSUNA».** Qui Sailor non ha
            // avviato niente: a lavorare è stato l'agente già vivo nel
            // terminale, e con quale identità l'abbia fatto Sailor non lo sa e
            // non può saperlo. Lasciare il vuoto confonderebbe questo fatto con
            // «nessun profilo in forza», che è un'altra cosa: lì la casa è
            // quella del processo, e si può andare a guardarla.
            engine_identity: EngineIdentity::DeclaredByAnAgent,
            retry_chain: Vec::new(),
            error_type: None,
            started_at: now,
            ended_at: Some(now),
        })
        .map_err(|error| format!("non riesco a registrare i turni dichiarati: {error}"))
}

// ── gli attrezzi comuni ──────────────────────────────────────────────────

/// I passi che pretendono l'uscita tipata di questo.
///
/// Una dipendenza dichiarata saltabile non conta: quel passo sa già andare
/// avanti senza, ed è il motivo per cui la si dichiara.
fn dependents_of(flow: &FlowFile, step_id: &str) -> Vec<String> {
    flow.graph
        .steps()
        .iter()
        .filter(|other| {
            other.deps.iter().any(|dependency| dependency == step_id)
                && !flow.graph.dependency_is_skippable(&other.id, step_id)
        })
        .map(|other| other.id.clone())
        .collect()
}

/// L'ultimo tentativo su un passo, comunque sia andato.
fn last_attempt<'a>(records: &'a [StepRecord], step_id: &str) -> Option<&'a StepRecord> {
    records
        .iter()
        .filter(|record| record.step_id == step_id)
        .max_by_key(|record| (record.attempt, record.epoch))
}

/// Il flusso su cui gira una corsa, ritrovato dal deposito.
///
/// **IL GRAFO NON SI CHIEDE A CHI DIGITA.** Un `--flow` sulla riga di comando
/// lascerebbe passare il nome sbagliato, e allora l'uscita di un passo verrebbe
/// validata contro lo schema di un altro — cioè il controllo direbbe di sì
/// guardando la cosa sbagliata. La corsa sa da sola da dove viene: sta scritto
/// in `runs.entity`.
pub(crate) fn flow_of_run(ledger: &Ledger, run_id: &str) -> Result<FlowFile, String> {
    let header = ledger
        .run_header(run_id)
        .map_err(|error| format!("non riesco a leggere la corsa {run_id}: {error}"))?
        .ok_or_else(|| format!("nessuna corsa si chiama {run_id} in questo deposito"))?;
    if header.entity.is_empty() {
        return Err(format!(
            "la corsa {run_id} non dichiara da quale flusso viene: senza non so quale \
             grafo caricare"
        ));
    }
    let sources = ui::gather::flow_sources();
    let known = ui::gather::load_all_flows(&sources);
    match known.iter().find(|(name, _, _)| *name == header.entity) {
        Some((_, _, Ok(flow))) => Ok(flow.clone()),
        Some((_, origin, Err(reason))) => Err(format!(
            "il flusso {} ({origin}) della corsa {run_id} non si carica: {reason}",
            header.entity
        )),
        None => Err(format!(
            "il flusso {} della corsa {run_id} non si trova più: era su questa macchina \
             quando la corsa è partita",
            header.entity
        )),
    }
}

pub(crate) fn open_ledger() -> Result<Ledger, String> {
    let dir = ledger::default_directory()
        .ok_or_else(|| "HOME non è definita: non so dove aprire il deposito".to_owned())?;
    Ledger::open(&dir).map_err(|error| {
        format!(
            "non riesco ad aprire il deposito {}: {error}",
            dir.display()
        )
    })
}

fn now_secs() -> Result<i64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .map_err(|error| format!("l'orologio di sistema precede Unix epoch: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow::{AttemptRelation, StepSpecies};
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn words(given: &[&str]) -> Vec<String> {
        given.iter().map(|word| (*word).to_owned()).collect()
    }

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    /// Una cartella che si cancella da sé: il deposito è un file, e un file
    /// lasciato indietro fa passare la prova dopo per la ragione sbagliata.
    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "sailor-step-{label}-{}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("creare la cartella di prova");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Un flusso di due passi: uno produce, l'altro giudica. È la forma minima
    /// in cui «chi crea non giudica» ha un significato.
    fn a_flow() -> FlowFile {
        serde_json::from_str(
            r#"{
                "id": "consegna-di-prova",
                "description": "un passo consegnato e il suo giudizio",
                "graph": {
                    "steps": [
                        {
                            "id": "implementa",
                            "deps": [],
                            "input_schema": {"type": "any"},
                            "output_schema": {"type": "any"},
                            "when": null,
                            "action": "handed_to_agent",
                            "max_attempts": 3
                        },
                        {
                            "id": "verdetto",
                            "deps": ["implementa"],
                            "input_schema": {"type": "any"},
                            "output_schema": {
                                "type": "object",
                                "properties": {"verdict": {"type": "string"}},
                                "required": ["verdict"],
                                "allow_extra": false
                            },
                            "when": null,
                            "action": "handed_to_agent",
                            "max_attempts": 3
                        }
                    ]
                },
                "inputs": {}
            }"#,
        )
        .expect("il flusso di prova è valido")
    }

    fn handed_input(step_id: &str) -> Value {
        json!({
            "mandate": format!("fai il lavoro di {step_id}"),
            "holder": "claude-vivo",
            "handoff_timeout_secs": 3600
        })
    }

    /// Un deposito con una corsa dentro e un passo già consegnato.
    fn a_handed_run(directory: &TestDirectory, step_id: &str, deps: Vec<String>) -> Ledger {
        let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
        ledger
            .record_run(&ledger::RunRecord {
                run_id: "run-1".to_owned(),
                kind: "flow".to_owned(),
                entity: "consegna-di-prova".to_owned(),
                parent_run_id: None,
                started_by: "prova".to_owned(),
                status: "waiting".to_owned(),
                total_cost_micros: 0,
                error: None,
                started_at: 100,
                ended_at: Some(150),
            })
            .expect("registrare la corsa");
        hand_over(&ledger, step_id, deps);
        ledger
    }

    /// Scrive il passo come lo scriverebbe il motore: aperto e poi chiuso con
    /// esito «in attesa», che è ciò che `handed_to_agent` produce.
    fn hand_over(ledger: &Ledger, step_id: &str, deps: Vec<String>) {
        let mut record = StepRecord::started(
            "run-1",
            step_id,
            1,
            1,
            deps,
            handed_input(step_id),
            vec![],
            110,
        );
        record.species = Some(StepSpecies::Repeatable);
        record.held_by_pid = Some(std::process::id());
        ledger
            .append_step_started(&record)
            .expect("aprire il passo");
        ledger
            .close_step(
                "run-1",
                step_id,
                1,
                1,
                Completion {
                    outcome: Outcome::Waiting,
                    output: None,
                    said: Some("consegnato a «claude-vivo»".to_owned()),
                    failure_class: None,
                    ended_at: 150,
                    bytes_seen: None,
                    bytes_discarded: None,
                },
            )
            .expect("consegnarlo");
    }

    fn options(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    /// **L'APERTURA PORTA LO STESSO INGRESSO.**
    ///
    /// L'impronta si calcola dall'ingresso: copiarlo identico fa dire a
    /// `attempt_relation` che è lo stesso lavoro ripreso, non un lavoro nuovo.
    /// Con un ingresso ricostruito la corsa mostrerebbe due lavori dove ce n'è
    /// uno, e il mandato — che vive lì dentro — cambierebbe impronta senza
    /// essere cambiato.
    #[test]
    fn opening_a_handed_step_carries_the_very_same_input() {
        let directory = TestDirectory::new("stesso-ingresso");
        let ledger = a_handed_run(&directory, "implementa", vec![]);

        let report = open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "implementa"), ("as", "chi-lavora")]),
        )
        .expect("il passo si prende in carico");
        assert!(
            report.contains("fai il lavoro di implementa"),
            "il mandato si legge dall'ingresso: {report}"
        );

        let records = ledger.steps("run-1").expect("rileggere i passi");
        let opened = records
            .iter()
            .find(|record| record.outcome.is_none())
            .expect("c'è un tentativo aperto");
        assert_eq!(opened.attempt, 2);
        assert_eq!(opened.epoch, 2);
        assert_eq!(
            opened.input,
            handed_input("implementa"),
            "l'ingresso si copia tale e quale"
        );
        assert_eq!(
            opened.attempt_relation,
            Some(AttemptRelation::SameInput),
            "stesso ingresso e stessi freni: è lo stesso lavoro ripreso"
        );
        assert_eq!(
            opened.held_by_pid, None,
            "nessun processo tiene un passo consegnato: scriverci un pid lo farebbe \
             dichiarare morto alla prima ripresa"
        );
    }

    /// **UN'USCITA CHE IL PASSO NON DICHIARA SI RESPINGE.**
    ///
    /// `RecordStore::close` non valida niente. Senza questo controllo un'uscita
    /// malformata entrerebbe nel deposito come buona e ucciderebbe la corsa tre
    /// passi dopo, dove la colpa cadrebbe su un passo innocente.
    #[test]
    fn an_output_the_step_does_not_declare_is_refused() {
        let directory = TestDirectory::new("uscita-non-dichiarata");
        let ledger = a_handed_run(&directory, "verdetto", vec!["implementa".to_owned()]);
        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "verdetto"), ("as", "chi-giudica")]),
        )
        .expect("il passo si prende in carico");

        let wrong = directory.0.join("uscita.json");
        std::fs::write(&wrong, r#"{"esito": "va bene"}"#).expect("scrivere l'uscita");
        let error = close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi-giudica"),
                ("outcome", "went"),
                ("output-file", wrong.to_str().expect("percorso leggibile")),
            ]),
        )
        .expect_err("un'uscita fuori schema si respinge");
        assert!(error.contains("non rispetta lo schema"), "{error}");

        let records = ledger.steps("run-1").expect("rileggere i passi");
        assert!(
            records
                .iter()
                .any(|record| record.step_id == "verdetto" && record.outcome.is_none()),
            "il passo resta aperto: respingere e chiudere lo stesso sarebbe peggio di non \
             controllare"
        );
    }

    /// L'uscita che il passo dichiara passa, e il passo si chiude.
    #[test]
    fn the_declared_output_closes_the_step() {
        let directory = TestDirectory::new("uscita-dichiarata");
        let ledger = a_handed_run(&directory, "verdetto", vec!["implementa".to_owned()]);
        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "verdetto"), ("as", "chi-giudica")]),
        )
        .expect("il passo si prende in carico");

        let good = directory.0.join("uscita.json");
        std::fs::write(&good, r#"{"verdict": "va bene"}"#).expect("scrivere l'uscita");
        let report = close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi-giudica"),
                ("outcome", "went"),
                ("output-file", good.to_str().expect("percorso leggibile")),
                ("turns", "12"),
            ]),
        )
        .expect("l'uscita dichiarata si accetta");
        assert!(report.contains("chiuso"), "{report}");

        let spent = ledger.spent_in_run("run-1").expect("la spesa si chiede");
        assert_eq!(spent.calls, 1, "i turni dichiarati scrivono una chiamata");
        assert_eq!(
            spent.calls_without_cost, 1,
            "una chiamata autodichiarata non porta un costo"
        );
        assert!(
            !spent.is_complete(),
            "con una consegna dentro, il totale di una corsa smette di essere completo — \
             ed è ciò che rende il tetto di spesa una garanzia solo su ciò che si sa"
        );
    }

    /// **CHI HA SCRITTO UNA DIPENDENZA NON PUÒ CHIUDERE IL PASSO CHE LA
    /// GIUDICA.**
    ///
    /// Il vincolo permanente «chi crea non giudica», applicato al gesto in cui
    /// il giudizio si scrive davvero. Il rifiuto all'apertura da solo non
    /// basterebbe: si aprirebbe con un nome qualunque e si chiuderebbe con
    /// quello dell'autore.
    #[test]
    fn whoever_wrote_a_dependency_cannot_close_the_step_that_judges_it() {
        let directory = TestDirectory::new("autore-giudice");
        let ledger = a_handed_run(&directory, "implementa", vec![]);
        hand_over(&ledger, "verdetto", vec!["implementa".to_owned()]);

        // «autore» fa il lavoro e lo chiude.
        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "implementa"), ("as", "autore")]),
        )
        .expect("l'autore prende il lavoro");
        let done = directory.0.join("implementa.json");
        std::fs::write(&done, r#"{"fatto": true}"#).expect("scrivere l'uscita");
        close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "implementa"),
                ("as", "autore"),
                ("outcome", "went"),
                ("output-file", done.to_str().expect("percorso leggibile")),
            ]),
        )
        .expect("l'autore chiude il proprio lavoro");

        // E adesso prova a giudicarsi. Aprire già non si può.
        let refused = open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "verdetto"), ("as", "autore")]),
        )
        .expect_err("l'autore non apre il passo che lo giudica");
        assert!(refused.contains("non giudica"), "{refused}");

        // E nemmeno chiudere, entrando da un nome qualunque: è il gesto che
        // conta, ed è quello che la porta lasciata aperta permetterebbe.
        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "verdetto"), ("as", "un-terzo")]),
        )
        .expect("un terzo prende il giudizio");
        let good = directory.0.join("verdetto.json");
        std::fs::write(&good, r#"{"verdict": "va bene"}"#).expect("scrivere l'uscita");
        let refused = close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "autore"),
                ("outcome", "went"),
                ("output-file", good.to_str().expect("percorso leggibile")),
            ]),
        )
        .expect_err("l'autore non chiude il passo che lo giudica");
        assert!(refused.contains("non giudica"), "{refused}");
    }

    /// Il permesso esiste e si dichiara nel passo: una negazione senza scampo
    /// fermerebbe i flussi in cui la stessa mano è la scelta giusta.
    #[test]
    fn the_flow_can_declare_that_the_same_hand_is_allowed() {
        let directory = TestDirectory::new("stessa-mano");
        let ledger = a_handed_run(&directory, "implementa", vec![]);

        let mut record = StepRecord::started(
            "run-1",
            "verdetto",
            1,
            1,
            vec!["implementa".to_owned()],
            json!({
                "mandate": "giudica",
                "holder": "claude-vivo",
                "handoff_timeout_secs": 3600,
                "same_holder_ok": true
            }),
            vec![],
            110,
        );
        record.species = Some(StepSpecies::Repeatable);
        ledger.append_step_started(&record).expect("aprire");
        ledger
            .close_step(
                "run-1",
                "verdetto",
                1,
                1,
                Completion {
                    outcome: Outcome::Waiting,
                    output: None,
                    said: None,
                    failure_class: None,
                    ended_at: 150,
                    bytes_seen: None,
                    bytes_discarded: None,
                },
            )
            .expect("consegnarlo");

        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "implementa"), ("as", "autore")]),
        )
        .expect("l'autore prende il lavoro");
        let done = directory.0.join("implementa.json");
        std::fs::write(&done, r#"{"fatto": true}"#).expect("scrivere l'uscita");
        close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "implementa"),
                ("as", "autore"),
                ("outcome", "went"),
                ("output-file", done.to_str().expect("percorso leggibile")),
            ]),
        )
        .expect("l'autore chiude");

        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "verdetto"), ("as", "autore")]),
        )
        .expect("il passo lo dichiara ammesso, quindi passa");
    }

    /// **CHIUDERE «ANDATO» SENZA USCITA, MENTRE QUALCUNO L'ASPETTA, SI
    /// RIFIUTA.**
    ///
    /// Il deposito non distingue «uscita nulla» da «nessuna uscita»: nel
    /// registro degli eventi diventano tutte e due `"output": null` e tornano
    /// `None` (guasto 31). Senza questo rifiuto la corsa si ferma al passo
    /// **dopo**, con «non ha uscita tipata», e chi guarda va a cercare il difetto
    /// nel passo sbagliato. È la stessa ragione per cui l'uscita si valida qui:
    /// un difetto che si manifesta lontano da dove è nato costa il doppio.
    #[test]
    fn closing_as_went_without_an_output_is_refused_when_a_step_waits_for_it() {
        let directory = TestDirectory::new("uscita-che-manca");
        let ledger = a_handed_run(&directory, "implementa", vec![]);
        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "implementa"), ("as", "chi")]),
        )
        .expect("il passo si prende in carico");

        let error = close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "implementa"),
                ("as", "chi"),
                ("outcome", "went"),
            ]),
        )
        .expect_err("senza uscita il passo dopo non partirebbe");
        assert!(error.contains("verdetto"), "deve nominare chi aspetta: {error}");
        assert!(error.contains("--output-file"), "{error}");
    }

    /// Un passo che non ha nessuno a valle si chiude senza uscita: pretenderla
    /// sarebbe una formalità che ferma un lavoro finito.
    #[test]
    fn a_last_step_closes_without_an_output() {
        let directory = TestDirectory::new("ultimo-passo");
        let ledger = a_handed_run(&directory, "verdetto", vec!["implementa".to_owned()]);
        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "verdetto"), ("as", "chi")]),
        )
        .expect("il passo si prende in carico");
        close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi"),
                ("outcome", "went"),
            ]),
        )
        .expect("nessuno dipende da «verdetto»: si chiude senza uscita");
    }

    /// **UN PASSO CHE STA GIRANDO DAVVERO NON SI CHIUDE A MANO.**
    ///
    /// Un record con un pid l'ha aperto l'esecutore, e quel processo sta
    /// lavorando: chiuderlo da un altro terminale gli toglie il passo di sotto,
    /// e la sua chiusura fallisce con «già chiuso» — cioè un gesto fatto altrove
    /// rompe una corsa sana.
    #[test]
    fn a_step_a_live_executor_holds_cannot_be_closed_by_hand() {
        let directory = TestDirectory::new("tenuto-dal-motore");
        let ledger = a_handed_run(&directory, "implementa", vec![]);
        // Come lo aprirebbe il motore: col proprio pid scritto dentro.
        let mut record = StepRecord::started(
            "run-1",
            "implementa",
            2,
            2,
            vec![],
            handed_input("implementa"),
            vec![],
            200,
        );
        record.held_by_pid = Some(std::process::id());
        ledger.append_step_started(&record).expect("il motore apre");

        let error = close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "implementa"),
                ("as", "chi"),
                ("outcome", "went"),
            ]),
        )
        .expect_err("un passo tenuto dal motore non si chiude a mano");
        assert!(error.contains("lo sta eseguendo il motore"), "{error}");
        assert!(
            ledger
                .steps("run-1")
                .expect("rileggere i passi")
                .iter()
                .any(|found| found.attempt == 2 && found.outcome.is_none()),
            "il tentativo del motore resta aperto: chiuderlo romperebbe la corsa che gira"
        );
    }

    /// Un passo che non è in attesa non si prende in carico: non è stato
    /// consegnato a nessuno.
    #[test]
    fn a_step_that_was_not_handed_over_cannot_be_taken() {
        let directory = TestDirectory::new("non-consegnato");
        let ledger = a_handed_run(&directory, "implementa", vec![]);
        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "implementa"), ("as", "chi")]),
        )
        .expect("il primo lo prende");
        let error = open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "implementa"), ("as", "un-altro")]),
        )
        .expect_err("un passo già aperto non si riprende");
        assert!(error.contains("è aperto"), "{error}");
    }

    #[test]
    fn an_option_without_a_value_is_refused() {
        let error = flags(&words(&["--run", "--step", "implementa"]))
            .expect_err("un'opzione senza valore si rifiuta");
        assert!(error.contains("manca il valore vero"), "{error}");
    }

    #[test]
    fn options_come_back_as_pairs() {
        let found = flags(&words(&["--run", "run-1", "--step", "implementa", "--as", "chi"]))
            .expect("le coppie si leggono");
        assert_eq!(found.get("run").map(String::as_str), Some("run-1"));
        assert_eq!(found.get("step").map(String::as_str), Some("implementa"));
        assert_eq!(found.get("as").map(String::as_str), Some("chi"));
    }

    #[test]
    fn a_missing_option_names_itself() {
        let error = required(&BTreeMap::new(), "run").expect_err("manca");
        assert!(error.contains("--run"), "{error}");
    }

    /// Un esito che una persona non può dichiarare si rifiuta prima di toccare
    /// il deposito: `Waiting` e `Skipped` li scrive il motore, non una mano.
    #[test]
    fn only_went_and_broke_can_be_declared_by_hand() {
        let found: BTreeMap<String, String> = [
            ("run", "run-1"),
            ("step", "implementa"),
            ("as", "chi"),
            ("outcome", "waiting"),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
        let error = close_step(&found).expect_err("«waiting» non si dichiara a mano");
        assert!(error.contains("`went` e `broke`"), "{error}");
    }

    #[test]
    fn what_comes_next_names_the_resume_line() {
        let next = what_comes_next(&Decision::Ready(vec!["verdetto".to_owned()]), "run-1");
        assert!(next.contains("sailor flow resume run-1"), "{next}");
        assert!(next.contains("verdetto"), "{next}");
    }
}
