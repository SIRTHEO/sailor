//! `sailor flow cost`: what a run spent, which checks refused what, and which
//! models the price list cannot price.

use flow::StepRecord;
use models::pricing::{Known, PriceList};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use super::{default_ledger_dir, now_secs};

/// Quanto ha consumato l'ultima corsa di un flusso.
///
/// **PERCHÉ ESISTE, E PERCHÉ ADESSO.** Il consumo entra nel deposito dal
/// 30/08/2026, e fino al 31 l'unico modo di leggerlo era aprire SQLite a mano —
/// cioè aggirare Sailor, che è il guasto 15: uno strumento aggirato non
/// registra niente di ciò che gli succede intorno. Serve a rispondere alla
/// domanda che decide come si scrivono i flussi: **un flusso consuma più o meno
/// di un prompt solo?** Senza un modo di chiederlo, quella domanda si risponde a
/// impressione.
///
/// **I TOKEN VENGONO PRIMA DEL COSTO, E NON È UNA PREFERENZA DI STILE.** Con una
/// riga di comando locale non si paga a chiamata: si paga un abbonamento, e
/// quello che si consuma è **quota**, che si misura in token. La cifra in valuta
/// è quanto sarebbe costato via API — un metro utile per confrontare, non una
/// fattura. E ci sono motori che i token li dichiarano e il costo no: mostrare
/// solo il costo li renderebbe invisibili.
pub(super) fn cost_of(flow: &str) -> Result<String, String> {
    cost_of_in(&default_ledger_dir()?, flow)
}

/// The same report over a declared ledger directory, so a test can hand in a
/// scratch one and read the whole road from the rows to the sentences.
fn cost_of_in(dir: &Path, flow: &str) -> Result<String, String> {
    let Some(data) = ui::gather::gather(dir).map_err(|error| error.to_string())? else {
        return Err(catalogue::say(
            "cli.flow.no_store_here",
            &[("path", &dir.display().to_string())],
        ));
    };
    // L'ultima per inizio, non l'ultima scritta: una corsa aperta e una chiusa
    // possono arrivare in ordine inverso nella proiezione.
    let run = data
        .runs
        .iter()
        .filter(|run| run.entity == flow)
        .max_by_key(|run| run.started_at)
        .ok_or_else(|| catalogue::say("cli.flow.never_run_here", &[("flow", flow)]))?;
    let steps: &[StepRecord] = data
        .steps_by_run
        .get(&run.run_id)
        .map_or(&[], Vec::as_slice);
    let view = ui::dashboard::summarize_run(
        run,
        steps,
        data.calls_by_run
            .get(&run.run_id)
            .map_or(&[], Vec::as_slice),
        now_secs()?,
    );
    let mut report = spending_report(&view, &actions::current_price_list());
    report.push_str(&refusals_report(steps));
    Ok(report)
}

/// The attempts of one step that one declared check refused, and by which
/// rules: a flow refused thirty times reads which check says no, not only
/// that thirty attempts broke.
struct RefusedAttempts {
    step_id: String,
    check: String,
    attempts: usize,
    rules: BTreeSet<&'static str>,
}

/// One row per (step, check), the most refused first.
fn refused_attempts(steps: &[StepRecord]) -> Vec<RefusedAttempts> {
    let mut by_check: BTreeMap<(String, String), RefusedAttempts> = BTreeMap::new();
    for record in steps {
        let Some(refusal) = &record.refusal else {
            continue;
        };
        let key = (record.step_id.clone(), refusal.check.clone());
        let row = by_check.entry(key).or_insert_with(|| RefusedAttempts {
            step_id: record.step_id.clone(),
            check: refusal.check.clone(),
            attempts: 0,
            rules: BTreeSet::new(),
        });
        row.attempts += 1;
        row.rules.insert(refusal.rule.name());
    }
    let mut rows: Vec<RefusedAttempts> = by_check.into_values().collect();
    rows.sort_by(|left, right| {
        right
            .attempts
            .cmp(&left.attempts)
            .then_with(|| left.step_id.cmp(&right.step_id))
            .then_with(|| left.check.cmp(&right.check))
    });
    rows
}

/// Empty when no check refused anything: a heading over nothing would read as
/// a count of zero, and there is nothing to count.
fn refusals_report(steps: &[StepRecord]) -> String {
    let rows = refused_attempts(steps);
    if rows.is_empty() {
        return String::new();
    }
    let mut report = format!("\n{}", catalogue::say("cli.flow.refusals_heading", &[]));
    for row in rows {
        let rules = row.rules.iter().copied().collect::<Vec<_>>().join(", ");
        let _ = write!(
            report,
            "\n{}",
            catalogue::say(
                "cli.flow.refused_by_check",
                &[
                    ("step", &row.step_id),
                    ("attempts", &row.attempts.to_string()),
                    ("check", &row.check),
                    ("rules", &rules),
                ],
            )
        );
    }
    report
}

/// Il consumo di una corsa, per una persona.
///
/// **IL LISTINO ARRIVA DA FUORI, E NON È PIGNOLERIA:** così questa funzione si
/// interroga con un listino scritto nella prova, invece di dipendere da quale
/// file esista sulla macchina che esegue la batteria.
fn spending_report(view: &ui::dashboard::ExecutionView, prices: &PriceList) -> String {
    let tokens = &view.tokens;
    let mut report = catalogue::say(
        "cli.flow.run_heading",
        &[
            ("run_id", &view.run_id),
            ("flow", &view.entity),
            ("status", &view.status.to_string()),
            ("total", &view.steps_total.to_string()),
            ("went", &view.steps_went.to_string()),
            ("broke", &view.steps_broke.to_string()),
            ("calls", &tokens.calls.to_string()),
        ],
    );
    // **I TURNI ACCANTO ALLE CHIAMATE, E NON È UN DETTAGLIO.** Una chiamata a un
    // motore agentico non è un giro: ne sono decine, e il conto lo fa quel
    // numero. Misurato il 31/08/2026: una catena di quattro passi legge per
    // turno l'8% in più di una sessione sola, e consuma il doppio — perché di
    // turni ne fa il doppio. Chi legge «7 chiamate» senza sapere quanti turni
    // sono non ha in mano la quantità su cui si interviene.
    //
    // **IL CONTEGGIO GREZZO E BASTA: QUI NON SI STAMPA `cache letta ÷ turni`.**
    // Sembra «il contesto di una richiesta» e non lo è: sui quattro passi di una
    // corsa vera il quoziente dà 21.165 / 13.566 / 48.984 / 50.885 contro i
    // 46.702 / 31.651 / 63.266 / 71.173 che le richieste leggono davvero — sbaglia
    // da 1,29 a 2,33 volte, e il fattore cambia **fra i passi della stessa
    // corsa**. È la media di una rampa, non il prefisso, e non è confrontabile
    // né fra passi né fra un flusso e una sessione sola. Un numero stampato
    // viene usato per decidere: questo manderebbe a intervenire nel posto
    // sbagliato con l'aria di una misura.
    if tokens.turns > 0 {
        let _ = write!(
            report,
            " {}",
            catalogue::say("cli.flow.in_turns", &[("turns", &tokens.turns.to_string())])
        );
        // A handed step's turns are a number the agent gave, not one anybody
        // counted: the qualifier sits in the line of the number, or the reader
        // keeps the number and drops the qualifier.
        let declared = self_declared_turns(&view.calls);
        if declared > 0 {
            report.push_str(&catalogue::say(
                "cli.flow.turns_self_declared",
                &[("declared", &declared.to_string())],
            ));
        }
    }
    let _ = write!(
        report,
        "\n{}",
        catalogue::say(
            "cli.flow.tokens_line",
            &[
                ("in", &tokens.input_tokens.to_string()),
                ("out", &tokens.output_tokens.to_string()),
                ("read", &tokens.cached_tokens.to_string()),
                ("written", &tokens.cache_write_tokens.to_string()),
            ],
        )
    );
    if tokens.total_tokens_only > 0 {
        let _ = write!(
            report,
            "\n{}",
            catalogue::say(
                "cli.flow.totals_not_split",
                &[("count", &tokens.total_tokens_only.to_string())],
            )
        );
    }
    // **LA CIFRA NON SI COMPONE QUI.** Se la scrivesse questa funzione,
    // rifarebbe la regola dei tre casi in un `format!` — e la prima volta che
    // qualcuno tocca uno dei due posti le due versioni divergono in silenzio.
    // Chi decide quanti passi aprire e chi legge il consumo devono leggere la
    // stessa frase.
    let _ = write!(
        report,
        "\n{}",
        ui::dashboard::how_the_cost_reads(&tokens.cost_reading())
    );
    // **WHAT THE ENGINES SAID IT COST THEM, BESIDE THE SUM AND NEVER IN IT.**
    // A figure an engine declares is its own word, not a reading of a price
    // list, and adding the two would make a number nobody can take apart.
    // Leaving it out was worse: a run that really cost money read as unknown.
    report.push_str(&declared_report(&view.calls));
    // The floor and the names travel together: «at least» without the steps
    // that made it a floor sends the reader to redo the sum by hand and land
    // on the wrong total again.
    report.push_str(&unmeasured_report(&view.calls, prices));
    // **QUELLO CHE MANCA SI DICE, O IL TOTALE SI LEGGE COME COMPLETO.** È la
    // stessa regola della finestra: una somma che tace su ciò che non ha
    // contato è una rassicurazione, non una misura. Resta anche adesso che il
    // costo lo dice da sé: i token mancanti sono un'altra lacuna, e una corsa
    // può avere quella e non l'altra.
    if tokens.calls_without_tokens > 0 || tokens.calls_without_cost > 0 {
        let _ = write!(
            report,
            "\n{}",
            catalogue::say(
                "cli.flow.partial_counts",
                &[
                    ("without_tokens", &tokens.calls_without_tokens.to_string()),
                    ("without_cost", &tokens.calls_without_cost.to_string()),
                ],
            )
        );
    }
    // **E SI DICE QUALE MODELLO, NON SOLO QUANTE CHIAMATE.** «Tre senza costo
    // noto» è un numero su cui non si può agire; il nome del modello scoperto è
    // una riga da scrivere nel listino. È la seconda metà della cura del guasto
    // 35: chi non ha un prezzo per un modello deve saperlo, non dedurlo da uno
    // zero. I nomi vengono da `tokens_by_model`, cioè da chi ha davvero
    // risposto in questa corsa.
    let not_declared = ui::dashboard::model_not_declared();
    let unpriced = cannot_be_priced(
        prices,
        &view
            .tokens_by_model
            .keys()
            .filter(|name| !name.trim().is_empty())
            .filter(|name| **name != not_declared)
            .cloned()
            .collect(),
    );
    if !unpriced.is_empty() {
        let _ = write!(
            report,
            "\n{}",
            catalogue::say(
                "cli.flow.models_the_price_list_cannot_price",
                &[("models", &unpriced.join(", "))]
            )
        );
    }
    // **CON QUALE IDENTITÀ SONO PARTITI I PROCESSI DI QUESTA CORSA.**
    //
    // «Se un processo AI si avvia deve esserci un profilo associato»: fino al
    // 01/09/2026 quel dato era scritto nel deposito, riletto dentro una
    // struttura, e non arrivava a **nessuna** schermata né a nessun comando.
    // Un dato raccolto e mai guardato è a un passo dal diventare un dato
    // sbagliato che nessuno nota, e questo è il posto dove una persona guarda
    // quando qualcosa è andato storto.
    //
    // **QUI NON COMPARE NESSUN GETTONE**, e non è una svista da riparare: si
    // dice quale casa e come è stata scelta, che è ciò su cui si va a guardare.
    // Cosa c'è dentro quella casa non è affare di un rapporto sul consumo.
    let identities = ui::dashboard::identities_of(&view.calls);
    if !identities.is_empty() {
        report.push_str(&catalogue::say("cli.flow.identity_heading", &[]));
        for (identity, how_many) in identities {
            report.push_str(&identity_line(&identity, how_many));
        }
    }
    report
}

/// One identity and how many calls started under it. The plural is the
/// catalogue's: two keys, and the count picks between them. Both keys are
/// written out where they are asked for, so the judge that reads the keys the
/// code asks for sees both.
fn identity_line(identity: &ledger::EngineIdentity, how_many: usize) -> String {
    let identity = identity.to_string();
    let count = how_many.to_string();
    let values = [("identity", identity.as_str()), ("how_many", count.as_str())];
    if how_many == 1 {
        catalogue::say("cli.flow.identity_calls_one", &values)
    } else {
        catalogue::say("cli.flow.identity_calls_many", &values)
    }
}

/// The turns of a run that an agent declared of itself and nobody measured.
fn self_declared_turns(calls: &[ui::dashboard::CallView]) -> u64 {
    calls
        .iter()
        .filter(|call| call.engine_identity == ledger::EngineIdentity::DeclaredByAnAgent)
        .filter_map(|call| call.turns)
        .sum()
}

/// What the engines of this run declared it cost them, when any did.
fn declared_report(calls: &[ui::dashboard::CallView]) -> String {
    let declared: Vec<i64> = calls
        .iter()
        .filter_map(|call| call.declared_cost_micros)
        .collect();
    if declared.is_empty() {
        return String::new();
    }
    let total: i64 = declared.iter().sum();
    format!(
        "\n{}",
        catalogue::say(
            "cli.flow.declared_by_the_engines",
            &[
                ("units", &in_units(total)),
                ("calls", &declared.len().to_string()),
                ("of", &calls.len().to_string()),
            ],
        )
    )
}

/// One line per unmeasured (step, reason), in order of appearance; a step
/// retried three times without a cost is one line, the count is in the floor.
fn unmeasured_report(calls: &[ui::dashboard::CallView], prices: &PriceList) -> String {
    let mut lines: Vec<String> = Vec::new();
    for call in calls.iter().filter(|call| call.cost_micros.is_none()) {
        let line = why_unmeasured(call, prices);
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut report = format!("\n{}", catalogue::say("cli.flow.unmeasured_heading", &[]));
    for line in lines {
        let _ = write!(report, "\n{line}");
    }
    report
}

/// A handed step is told apart by the identity its row declares, not by a
/// word in its purpose. The other two reasons are repaired in other places: a
/// model the list cannot price by writing the entry, a call that declared no
/// cost by the engine that wrote it.
fn why_unmeasured(call: &ui::dashboard::CallView, prices: &PriceList) -> String {
    let step = call
        .step_id
        .clone()
        .unwrap_or_else(|| catalogue::say("cli.flow.call_outside_any_step", &[]));
    if call.engine_identity == ledger::EngineIdentity::DeclaredByAnAgent {
        return catalogue::say("cli.flow.unmeasured_handed_step", &[("step", &step)]);
    }
    let model = call.actual_model.trim();
    let unpriced = if model.is_empty() {
        None
    } else {
        cannot_be_priced(prices, &BTreeSet::from([model.to_owned()])).pop()
    };
    match unpriced {
        Some(model) => catalogue::say(
            "cli.flow.unmeasured_model_unpriced",
            &[("step", &step), ("model", &model)],
        ),
        None => catalogue::say("cli.flow.unmeasured_no_cost_declared", &[("step", &step)]),
    }
}

/// Quanto costa una unità di valuta in micro. Un milione: `1_000_000` è un
/// dollaro.
const MICROS_IN_A_UNIT: f64 = 1_000_000.0;

/// I modelli che le corse passate di questo flusso hanno davvero usato.
///
/// **DAL DEPOSITO E NON DAL FLUSSO, PERCHÉ IL FLUSSO NON LO SA.** Un passo nomina
/// lo strumento — `claude-code`, `codex` — non il modello: quale modello risponda
/// lo decide quella riga di comando, e lo si scopre solo dopo, da ciò che ha
/// dichiarato. Un elenco dedotto dal file del flusso sarebbe indovinato.
///
/// **`None` NON È UN INSIEME VUOTO, ED È LA DISTINZIONE CHE CONTA.** Un deposito
/// che non si apre — non c'è, non si legge, i permessi lo negano — non dice
/// «questo flusso non ha mai usato nessun modello»: dice che nessuno ha potuto
/// guardare. Confonderli farebbe stampare «mai girato qui» a un flusso girato
/// cento volte, ed è la stessa regola per cui il rilevatore assente fa tacere il
/// rapporto invece di dichiarare sano ciò che non ha visto.
pub(super) fn models_seen_by(flow_id: &str) -> Option<BTreeSet<String>> {
    let dir = default_ledger_dir().ok()?;
    let data = ui::gather::gather(&dir).ok()??;
    Some(
        data.runs
            .iter()
            .filter(|run| run.entity == flow_id)
            .filter_map(|run| data.calls_by_run.get(&run.run_id))
            .flatten()
            // Una chiamata senza modello dichiarato non è un modello senza
            // prezzo: è un motore che non dice chi ha risposto, e nominare la
            // stringa vuota fra i modelli scoperti manderebbe a cercare una
            // voce di listino per un nome che non esiste.
            .filter(|call| !call.actual_model.trim().is_empty())
            .map(|call| call.actual_model.clone())
            .collect(),
    )
}

/// Le micro-unità come le legge una persona.
pub(super) fn in_units(micros: i64) -> String {
    format!("{:.2}", micros as f64 / MICROS_IN_A_UNIT)
}

// ── quello che il listino non sa prezzare ────────────────────────────────

/// I nomi che questo listino non sa prezzare, ciascuno col perché.
///
/// **IL PERCHÉ STA ACCANTO AL NOME PERCHÉ SONO DUE RIPARAZIONI DIVERSE.** Un
/// nome che il listino non conosce si ripara aggiungendo una voce — o un alias,
/// se è lo stesso modello con un altro nome; una voce che c'è ma non ha i prezzi
/// si ripara scrivendo i prezzi. Un elenco di soli nomi manderebbe a riscrivere
/// una voce che esiste già.
fn cannot_be_priced(prices: &PriceList, seen: &BTreeSet<String>) -> Vec<String> {
    seen.iter()
        .filter_map(|name| match prices.knows(name) {
            Known::Priced => None,
            Known::Absent => Some(catalogue::say(
                "cli.flow.model_absent_from_the_list",
                &[("model", name)],
            )),
            Known::ListedWithoutPrice => Some(catalogue::say(
                "cli.flow.model_listed_without_price",
                &[("model", name)],
            )),
        })
        .collect()
}

/// Che cosa il listino sa dire di questo flusso, **prima** di lanciarlo.
///
/// **PERCHÉ UN FRENO CHE NON FRENA SI DEVE VEDERE PRIMA.** È la seconda metà
/// della cura del guasto 35, e senza di lei la prima non basta: adesso il
/// listino viaggia col prodotto, ma i modelli che nessuno ha prezzato — quelli di
/// OpenAI e di Google, tenuti fuori di proposito perché nessuno ne ha verificato
/// i prezzi — continuano a lasciare il costo sconosciuto. Chi non ha un prezzo
/// per un modello deve **saperlo**, non scoprirlo con uno zero.
///
/// **I MODELLI ARRIVANO DAL DEPOSITO, NON DAL FLUSSO.** Un passo nomina lo
/// strumento, non il modello: nessuno *chiede* un modello, e indovinarne uno
/// sarebbe inventarlo. L'unica fonte onesta è chi ha già risposto, cioè le corse
/// passate — per questo un flusso mai girato qui non riceve un elenco vuoto ma
/// una frase che dice che non si sa, come per il rilevatore assente.
pub(super) fn what_is_priced(prices: &PriceList, seen: Option<&BTreeSet<String>>, cap: Option<i64>) -> String {
    let mut said = format!(
        "\n{}",
        catalogue::say(
            "cli.flow.price_list_size",
            &[("count", &prices.entries.len().to_string())],
        )
    );
    let Some(seen) = seen else {
        said.push_str(&catalogue::say("cli.flow.models_store_unreadable", &[]));
        return said;
    };
    if seen.is_empty() {
        said.push_str(&catalogue::say("cli.flow.models_never_run_here", &[]));
        return said;
    }
    let unpriced = cannot_be_priced(prices, seen);
    if unpriced.is_empty() {
        let _ = write!(
            said,
            "\n{}",
            catalogue::say(
                "cli.flow.past_models_all_priced",
                &[(
                    "models",
                    &seen.iter().cloned().collect::<Vec<_>>().join(", ")
                )],
            )
        );
        return said;
    }
    let _ = write!(
        said,
        "\n{}",
        catalogue::say(
            "cli.flow.past_models_unpriced",
            &[("models", &unpriced.join(", "))]
        )
    );
    // **LA RIGA DEL TETTO SOLO QUANDO LE DUE COSE COINCIDONO.** Un tetto senza
    // modelli scoperti non ha niente da dichiarare, e modelli scoperti senza
    // tetto non fermano niente: è la coincidenza a essere pericolosa, ed è la
    // frase per cui il guasto 35 è stato scritto — un freno che non frena si
    // deve vedere prima di lanciare, non a fattura arrivata.
    if cap.is_some() {
        said.push_str(&catalogue::say("cli.flow.cap_will_not_count_them", &[]));
    }
    said
}

#[cfg(test)]
mod tests {
    use super::super::run_and_resume::record_run;
    use super::super::test_support::*;
    use super::*;
    use flow::FlowFile;
    use ledger::Ledger;

    // ── quello che il listino non sa prezzare ────────────────────────────

    /// Un listino che conosce un modello solo, con i suoi prezzi, e una voce
    /// dichiarata a metà: bastano a distinguere le tre risposte.
    fn a_small_price_list() -> PriceList {
        PriceList::parse(
            r#"{"currency":"USD","models":[
                {"id":"prezzato","input_per_million":5.0,"output_per_million":25.0},
                {"id":"a-meta","input_per_million":5.0}
            ]}"#,
        )
        .expect("il listino di prova si legge")
    }

    /// **CHI NON HA UN PREZZO PER UN MODELLO DEVE SAPERLO, E SAPERE QUALE.**
    ///
    /// È la seconda metà della cura del guasto 35. Il primo modello è prezzato e
    /// non deve comparire; gli altri due no, e devono comparire **col perché**,
    /// perché si riparano in due modi diversi.
    ///
    /// *Mutante eseguito*: far restituire a `cannot_be_priced` un elenco vuoto.
    /// Il rapporto torna a tacere e questa prova diventa rossa — che è
    /// esattamente il difetto: uno zero al posto di una risposta.
    #[test]
    fn a_model_without_a_price_is_named_and_the_reason_with_it() {
        let said = what_is_priced(
            &a_small_price_list(),
            Some(&names(&["prezzato", "a-meta", "mai-visto"])),
            None,
        );

        assert!(
            !said.contains("prezzato ("),
            "a priced model is not flagged: {said}"
        );
        assert!(
            said.contains("mai-visto (no entry in the price list)"),
            "{said}"
        );
        assert!(said.contains("a-meta (an entry with no prices)"), "{said}");
        assert!(
            said.contains("stays unknown"),
            "and it says what happens to it: {said}"
        );
    }

    /// **QUANDO SONO TUTTI PREZZATI LO DICE LO STESSO.** Un rapporto che tace
    /// lascia chi legge a chiedersi se il controllo abbia guardato — è la stessa
    /// regola per cui la riga del tetto c'è anche quando il tetto non c'è.
    #[test]
    fn when_everything_is_priced_the_report_says_so_instead_of_falling_silent() {
        let said = what_is_priced(&a_small_price_list(), Some(&names(&["prezzato"])), None);

        assert!(said.contains("all priced"), "{said}");
        assert!(!said.contains("no entry"), "{said}");
    }

    /// **«MAI GIRATO QUI» E «NON HO POTUTO GUARDARE» SONO DUE FRASI DIVERSE.**
    ///
    /// Un deposito che non si apre — non c'è, i permessi lo negano — non dice
    /// che il flusso non è mai girato: dice che nessuno ha potuto guardare.
    /// Confonderli fa stampare «mai girato qui» a un flusso girato cento volte,
    /// ed è la stessa regola per cui il rilevatore assente fa tacere il rapporto
    /// invece di dichiarare sano ciò che non ha visto.
    ///
    /// *Mutante eseguito*: far collassare il ramo `None` su quello dell'insieme
    /// vuoto. Le due frasi diventano una e questa prova diventa rossa.
    #[test]
    fn a_ledger_that_could_not_be_read_is_not_a_flow_that_never_ran() {
        let unreadable = what_is_priced(&a_small_price_list(), None, None);
        let never_ran = what_is_priced(&a_small_price_list(), Some(&BTreeSet::new()), None);

        assert_ne!(unreadable, never_ran);
        assert!(unreadable.contains("could not be read"), "{unreadable}");
        assert!(!unreadable.contains("mai girato"), "{unreadable}");
    }

    /// **UN FLUSSO MAI GIRATO QUI NON RICEVE UN ELENCO VUOTO, MA UNA FRASE.**
    ///
    /// I modelli si sanno solo da chi ha già risposto: un passo nomina lo
    /// strumento, non il modello. Dire «tutti prezzati» senza aver visto niente
    /// sarebbe una rassicurazione costruita sul nulla — è la stessa distinzione
    /// che il rilevatore tiene fra «non c'è» e «non ho potuto guardare».
    #[test]
    fn a_flow_that_never_ran_here_is_told_that_nothing_is_known_yet() {
        let said = what_is_priced(&a_small_price_list(), Some(&BTreeSet::new()), None);

        assert!(!said.contains("all priced"), "{said}");
        assert!(said.contains("has never run here"), "{said}");
    }

    /// **UN TETTO CHE NON PUÒ SCATTARE SI DEVE VEDERE PRIMA DI LANCIARE.**
    ///
    /// È la frase per cui il guasto 35 è stato scritto: il tetto si misura sui
    /// costi noti, quindi un modello senza prezzo lo rende più largo di quanto
    /// dice — e chi lancia lo scopre a fattura arrivata. La riga compare solo
    /// quando tutte e due le condizioni ci sono, perché è la loro coincidenza a
    /// essere pericolosa.
    ///
    /// *Mutante eseguito*: togliere il ramo che guarda `cap` e stampare la frase
    /// sempre. Il terzo braccio — flusso senza tetto — diventa rosso.
    #[test]
    fn a_cap_that_cannot_fire_is_declared_before_the_run_not_after() {
        let unpriced = names(&["mai-visto"]);
        let priced = names(&["prezzato"]);

        let with_cap = what_is_priced(&a_small_price_list(), Some(&unpriced), Some(5_000_000));
        assert!(with_cap.contains("the spend cap"), "{with_cap}");

        let all_priced = what_is_priced(&a_small_price_list(), Some(&priced), Some(5_000_000));
        assert!(
            !all_priced.contains("the spend cap"),
            "senza modelli scoperti il tetto non ha niente da dichiarare: {all_priced}"
        );

        let no_cap = what_is_priced(&a_small_price_list(), Some(&unpriced), None);
        assert!(
            !no_cap.contains("the spend cap"),
            "un flusso senza tetto non ha un tetto da avvisare: {no_cap}"
        );
    }

    /// Una chiamata registrata: il modello che ha risposto, i suoi token, e se
    /// un costo è stato calcolato o no.
    fn a_call(actual_model: &str, cost: Option<i64>) -> ledger::ModelCallRecord {
        ledger::ModelCallRecord {
            call_id: format!("call-{actual_model}"),
            run_id: "run-1".to_owned(),
            step_id: None,
            purpose: "external_engine".to_owned(),
            cli: "claude-code".to_owned(),
            requested_model: String::new(),
            actual_model: actual_model.to_owned(),
            input_tokens: Some(100),
            output_tokens: Some(100),
            cached_tokens: None,
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: Some(1),
            cost_micros: cost,
            declared_cost_micros: None,
            price_currency: cost.map(|_| "USD".to_owned()),
            input_price_micros_per_million: None,
            output_price_micros_per_million: None,
            cached_price_micros_per_million: None,
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            engine_identity: ledger::EngineIdentity::default(),
            retry_chain: vec![],
            error_type: None,
            started_at: 0,
            ended_at: Some(1),
            session_id: None,
            work_kind: None,
            fell_back_from: Vec::new(),
        }
    }

    fn a_finished_run() -> ledger::RunRecord {
        ledger::RunRecord {
            run_id: "run-1".to_owned(),
            kind: "flow".to_owned(),
            entity: "prova".to_owned(),
            parent_run_id: None,
            started_by: "prova".to_owned(),
            status: "went".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: 0,
            ended_at: Some(10),
            worktree: None,
            stop_reason: None,
        }
    }

    /// **`flow cost` NOMINA IL MODELLO SCOPERTO, NON SOLO QUANTE CHIAMATE.**
    ///
    /// «Una chiamata senza costo noto» è un numero su cui non si può agire; il
    /// nome del modello è una riga da scrivere nel listino. La corsa ha due
    /// chiamate — una prezzata e una no — e solo la seconda deve comparire:
    /// nominarle tutte e due manderebbe a correggere una voce che c'è già.
    ///
    /// *Mutante eseguito*: far restituire a `cannot_be_priced` un elenco vuoto.
    /// Il rapporto torna a dire solo «1 senza costo noto» e questa diventa rossa.
    #[test]
    fn the_cost_report_names_the_model_that_has_no_price() {
        let calls = vec![a_call("prezzato", Some(1_000)), a_call("mai-visto", None)];
        let view = ui::dashboard::summarize_run(&a_finished_run(), &[], &calls, 100);

        let said = spending_report(&view, &a_small_price_list());

        assert!(
            said.contains("mai-visto (no entry in the price list)"),
            "{said}"
        );
        assert!(
            !said.contains("prezzato ("),
            "un modello prezzato non si segnala: {said}"
        );
        assert!(said.contains("lower than the real one"), "{said}");
    }

    /// **WHAT AN ENGINE SAID IT COST IS SAID, BESIDE THE SUM AND NOT IN IT.**
    /// A run whose only priced call is unpriceable read as costing nothing
    /// while the engine had declared its own figure in the same row.
    #[test]
    fn the_cost_report_says_what_the_engines_declared_it_cost_them() {
        let mut declaring = a_call("mai-visto", None);
        declaring.declared_cost_micros = Some(2_555_965);
        let calls = vec![a_call("prezzato", Some(1_000)), declaring];
        let view = ui::dashboard::summarize_run(&a_finished_run(), &[], &calls, 100);

        let said = spending_report(&view, &a_small_price_list());

        assert!(
            said.contains("2.55") || said.contains("2.56"),
            "the figure the engine declared is in the report: {said}"
        );
        assert!(
            said.contains("1 of the 2 calls"),
            "and over how many calls of how many: {said}"
        );
    }

    /// Nobody declaring anything says nothing: a line about a figure that does
    /// not exist would be one more thing to read on every healthy run.
    #[test]
    fn a_run_where_no_engine_declared_a_cost_says_nothing_about_it() {
        let calls = vec![a_call("prezzato", Some(1_000))];
        let view = ui::dashboard::summarize_run(&a_finished_run(), &[], &calls, 100);

        let said = spending_report(&view, &a_small_price_list());

        assert!(
            !said.contains("their own word"),
            "no engine declared a cost, so no line about it: {said}"
        );
    }

    /// **IL RAPPORTO DICE CON QUALE IDENTITÀ OGNI PROCESSO È PARTITO.**
    ///
    /// Fino al 01/09/2026 quel dato era scritto nel deposito, riletto dentro
    /// `CallView`, e non arrivava a **nessuna** schermata né a nessun comando —
    /// cercato in `crates/ui`, `desktop/src` e `crates/sailor`. Un dato raccolto
    /// e mai guardato è a un passo dal diventare un dato sbagliato che nessuno
    /// nota; questo è il posto dove una persona guarda quando qualcosa è andato
    /// storto.
    ///
    /// **E IL PERCORSO DELLA CASA CI DEVE STARE**, perché è il fondo su cui una
    /// diagnostica si appoggia: un nome di profilo dice sotto quale etichetta si
    /// è girato, un percorso dice dove andare a guardare.
    ///
    /// *Mutante eseguito*: togliere da `spending_report` il blocco che scrive
    /// «identità:». Questa diventa rossa e nessun'altra.
    #[test]
    fn the_cost_report_says_which_identity_each_process_started_with() {
        let mut in_force = a_call("prezzato", Some(1_000));
        in_force.engine_identity = ledger::EngineIdentity::ProfileInForce {
            cli_id: "codex".to_owned(),
            profile_name: "lavoro".to_owned(),
            home_dir: "/case/codex/lavoro".into(),
            endpoint: None,
        };
        let mut again = in_force.clone();
        again.call_id = "call-due".to_owned();
        let mut by_the_step = a_call("prezzato", Some(1_000));
        by_the_step.call_id = "call-passo".to_owned();
        by_the_step.engine_identity = ledger::EngineIdentity::ChosenByTheStep {
            cli_id: "codex".to_owned(),
            home_dir: "/una/casa/scritta/nel/passo".into(),
        };

        let calls = vec![in_force, again, by_the_step];
        let view = ui::dashboard::summarize_run(&a_finished_run(), &[], &calls, 100);

        let said = spending_report(&view, &a_small_price_list());

        assert!(
            said.contains(&catalogue::say("cli.flow.identity_heading", &[])),
            "{said}"
        );
        assert!(
            said.contains(&catalogue::say(
                "cli.flow.identity_calls_many",
                &[
                    ("identity", "profile codex/lavoro — home /case/codex/lavoro"),
                    ("how_many", "2")
                ]
            )),
            "{said}"
        );
        assert!(
            said.contains(&catalogue::say(
                "cli.flow.identity_calls_one",
                &[
                    (
                        "identity",
                        "home chosen by the step (codex) — home /una/casa/scritta/nel/passo"
                    ),
                    ("how_many", "1")
                ]
            )),
            "il caso in cui l'identità è stata cambiata apposta è quello che deve vedersi: {said}"
        );
    }

    /// The count picks the key, and the two keys differ: a line that always
    /// said «calls» would pass a test that only looked for the number.
    #[test]
    fn the_identity_line_picks_its_plural_by_count() {
        let identity = ledger::EngineIdentity::ChosenByTheStep {
            cli_id: "codex".to_owned(),
            home_dir: "/una/casa".into(),
        };
        let named = identity.to_string();
        let one = catalogue::say(
            "cli.flow.identity_calls_one",
            &[("identity", named.as_str()), ("how_many", "1")],
        );
        let many = catalogue::say(
            "cli.flow.identity_calls_many",
            &[("identity", named.as_str()), ("how_many", "2")],
        );
        let one_in_the_plural = catalogue::say(
            "cli.flow.identity_calls_many",
            &[("identity", named.as_str()), ("how_many", "1")],
        );
        assert_ne!(one, one_in_the_plural, "the two keys say the same thing, and the count decides nothing");

        assert_eq!(identity_line(&identity, 1), one);
        assert_eq!(identity_line(&identity, 2), many);
        assert_ne!(identity_line(&identity, 1), one_in_the_plural, "one call was given the plural");
    }

    /// La gemella: quando tutto è prezzato la riga non compare. Senza di lei un
    /// mutante che la stampasse sempre passerebbe la prova qui sopra.
    #[test]
    fn a_run_where_everything_is_priced_gets_no_such_line() {
        let calls = vec![a_call("prezzato", Some(1_000))];
        let view = ui::dashboard::summarize_run(&a_finished_run(), &[], &calls, 100);

        let said = spending_report(&view, &a_small_price_list());

        assert!(!said.contains("cannot price"), "{said}");
    }

    fn a_refused_attempt(
        step_id: &str,
        attempt: u32,
        check: &str,
        rule: flow::RefusalRule,
    ) -> StepRecord {
        let mut record = StepRecord::started(
            "run-1",
            step_id,
            attempt,
            1,
            vec![],
            serde_json::json!(null),
            vec![],
            attempt as i64,
        );
        record.outcome = Some(flow::Outcome::Broke);
        record.failure_class = Some("answer_off_shape".to_owned());
        record.refusal = Some(flow::Refusal::new(check, "$.verdict", rule, "\"remvoe\""));
        record.ended_at = Some(attempt as i64 + 1);
        record
    }

    /// One line per (step, check) with how many attempts that check refused and
    /// by which rules, the most refused first; a run nobody refused adds nothing.
    #[test]
    fn the_cost_report_counts_the_attempts_each_check_refused_per_step() {
        use flow::RefusalRule::{MissingField, NotAllowed, WrongType};
        let mut went = StepRecord::started("run-1", "judge", 4, 1, vec![], serde_json::json!(null), vec![], 4);
        went.outcome = Some(flow::Outcome::Went);
        let steps = vec![
            a_refused_attempt("judge", 1, "answer_shape", NotAllowed),
            a_refused_attempt("judge", 2, "answer_shape", MissingField),
            a_refused_attempt("judge", 3, "answer_shape", NotAllowed),
            went,
            a_refused_attempt("judge", 5, "output_schema", WrongType),
            a_refused_attempt("draft", 1, "answer_shape", WrongType),
        ];

        let said = refusals_report(&steps);

        let lines: Vec<&str> = said.lines().filter(|line| !line.is_empty()).collect();
        assert_eq!(lines.len(), 4, "{said}");
        assert_eq!(lines[0], catalogue::say("cli.flow.refusals_heading", &[]));
        assert!(lines[1].contains("judge") && lines[1].contains("3 ") && lines[1].contains("answer_shape"), "{said}");
        assert!(lines[1].contains("missing_field, not_allowed"), "{said}");
        assert!(lines[2].contains("draft") && lines[2].contains("1 ") && lines[2].contains("answer_shape"), "{said}");
        assert!(lines[3].contains("judge") && lines[3].contains("1 ") && lines[3].contains("output_schema"), "{said}");

        assert_eq!(refusals_report(&steps[3..4]), "");
    }

    /// **IL TOTALE DI UNA CORSA È QUELLO DELLE SUE CHIAMATE, NON UNO ZERO.**
    /// Il difetto che questa prova esiste per prendere ha vissuto in silenzio
    /// finché il costo delle chiamate non è diventato vero: `record_run`
    /// scriveva `total_cost_micros: 0` a mano, e la finestra mostrava quello
    /// zero accanto alla somma giusta calcolata altrove.
    #[test]
    fn a_runs_total_is_what_its_calls_cost() {
        let directory = TestDirectory::new();
        let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
        let flow: FlowFile = serde_json::from_str(&flow_json("shell_check", "[]", "{}"))
            .expect("caricare il flusso");
        for (call_id, cost) in [("prima", 96_310), ("seconda", 3_690)] {
            ledger
                .record_model_call(&spent_call(call_id, "corsa-costosa", cost))
                .expect("registrare la chiamata");
        }

        record_run(
            &ledger,
            &flow,
            "corsa-costosa",
            "complete",
            100,
            Some(110),
            None,
            None,
        )
        .expect("registrare la corsa");

        let dump = ledger.projection_dump().expect("leggere la proiezione");
        let run = dump["runs"]
            .as_array()
            .expect("l'elenco delle corse c'è")
            .iter()
            .find(|row| row[0] == "corsa-costosa")
            .expect("la corsa registrata si ritrova");
        assert_eq!(
            run[6],
            serde_json::json!(100_000),
            "il totale è la somma delle due chiamate, non uno zero scritto a mano"
        );
    }

    /// Una chiamata già costata, per misurare il totale di una corsa.
    fn spent_call(call_id: &str, run_id: &str, cost: i64) -> ledger::ModelCallRecord {
        ledger::ModelCallRecord {
            call_id: call_id.to_owned(),
            run_id: run_id.to_owned(),
            step_id: Some("chiedi".to_owned()),
            purpose: "external_engine".to_owned(),
            cli: "claude-code".to_owned(),
            requested_model: String::new(),
            actual_model: String::new(),
            input_tokens: Some(2),
            output_tokens: Some(4),
            cached_tokens: None,
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: None,
            cost_micros: Some(cost),
            declared_cost_micros: None,
            price_currency: None,
            input_price_micros_per_million: None,
            output_price_micros_per_million: None,
            cached_price_micros_per_million: None,
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            engine_identity: ledger::EngineIdentity::default(),
            retry_chain: vec![],
            error_type: None,
            started_at: 100,
            ended_at: Some(105),
            session_id: None,
            work_kind: None,
            fell_back_from: Vec::new(),
        }
    }

    // ── il totale che contiene un'incognita ──────────────────────────────

    /// Una chiamata come il deposito la conserva, coi soli campi che questo
    /// conto guarda. `cost` a `None` è una chiamata **non misurata**: è la
    /// forma che `sailor step close --turns` scrive per un passo consegnato.
    ///
    /// **IL NOME DICE DA COSA SI RICONOSCE**, e non è pignoleria: fino al
    /// 01/09/2026 questa e la sorella qui sopra si chiamavano tutte e due
    /// `a_call`, nate su due rami diversi lo stesso giorno. Git le ha fuse senza
    /// segnalare niente — nessuna riga in comune — e a rifiutare l'albero è
    /// stato `cargo`. È il guasto 36 di `docs/guasti-incontrati.md` che si
    /// ripete: il confine sui file non vede i nomi che vivono nello stesso
    /// modulo.
    fn a_call_named(call_id: &str, cost: Option<i64>) -> ledger::ModelCallRecord {
        ledger::ModelCallRecord {
            call_id: call_id.to_owned(),
            run_id: "run-1".to_owned(),
            step_id: None,
            purpose: "prova".to_owned(),
            cli: "claude".to_owned(),
            requested_model: "m".to_owned(),
            actual_model: "m".to_owned(),
            input_tokens: Some(10),
            output_tokens: Some(2),
            cached_tokens: Some(1),
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: Some(3),
            cost_micros: cost,
            declared_cost_micros: None,
            price_currency: None,
            input_price_micros_per_million: None,
            output_price_micros_per_million: None,
            cached_price_micros_per_million: None,
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            engine_identity: ledger::EngineIdentity::default(),
            retry_chain: vec![],
            error_type: None,
            started_at: 0,
            ended_at: Some(1),
            session_id: None,
            work_kind: None,
            fell_back_from: Vec::new(),
        }
    }

    fn a_run() -> ledger::RunRecord {
        ledger::RunRecord {
            run_id: "run-1".to_owned(),
            kind: "flow".to_owned(),
            entity: "prova".to_owned(),
            parent_run_id: None,
            started_by: "prova".to_owned(),
            status: "succeeded".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: 0,
            ended_at: Some(10),
            worktree: None,
            stop_reason: None,
        }
    }

    /// **IL LISTINO CONOSCE IL MODELLO DI QUESTE PROVE, ED È DELIBERATO.** Qui
    /// si guarda una cosa sola: la forma del totale quando una chiamata non è
    /// misurata. Con un listino che non conoscesse `m`, ogni rapporto porterebbe
    /// anche la riga dei modelli scoperti — un secondo motivo di lacuna,
    /// scritto sopra al primo — e una prova rossa non direbbe più quale delle
    /// due cose si è rotta.
    fn report_for(calls: &[ledger::ModelCallRecord]) -> String {
        let prices = PriceList::parse(
            r#"{"currency":"USD","models":[
                {"id":"m","input_per_million":5.0,"output_per_million":25.0}
            ]}"#,
        )
        .expect("il listino di prova si legge");
        let view = ui::dashboard::summarize_run(&a_run(), &[], calls, 100);
        spending_report(&view, &prices)
    }

    /// La cifra secca, come la scriverebbe un totale completo. Se compare in un
    /// rapporto parziale, chi legge ha in mano un numero che non è il totale.
    fn bare_total(micros: i64) -> String {
        catalogue::say("ui.cost.exact", &[("units", &units(micros))])
    }

    /// The floor, as the catalogue writes it: the known part, and how many of
    /// the calls are outside it.
    fn floored_total(micros: i64, calls: usize, without_cost: usize) -> String {
        catalogue::say(
            "ui.cost.at_least",
            &[
                ("units", &units(micros)),
                ("calls", &calls.to_string()),
                ("calls_without_cost", &without_cost.to_string()),
            ],
        )
    }

    fn units(micros: i64) -> String {
        format!("{:.4}", micros as f64 / 1_000_000.0)
    }

    /// **UN TOTALE CHE CONTIENE UN'INCOGNITA NON È UN TOTALE.**
    ///
    /// È il guasto 37 misurato: la corsa consegnata dell'A/B del 31/08/2026 ha
    /// stampato `1,6674` mentre era costata `7,2080` — 4,3 volte — perché tre
    /// chiamate su quattro non avevano un costo e la nota che lo diceva stava
    /// **sotto** il numero. Chi legge un totale legge il numero, non la nota.
    /// Qui la nota prende il posto del numero: la cifra secca non deve esistere
    /// da nessuna parte nel rapporto.
    #[test]
    fn a_total_with_an_unmeasured_call_is_never_a_bare_figure() {
        let report = report_for(&[
            a_call_named("misurata", Some(1_667_400)),
            a_call_named("consegnata-1", None),
            a_call_named("consegnata-2", None),
            a_call_named("consegnata-3", None),
        ]);

        assert!(
            !report.contains(&bare_total(1_667_400)),
            "la cifra secca non deve comparire: si legge come il totale vero.\n{report}"
        );
        assert!(
            report.contains(&floored_total(1_667_400, 4, 3)),
            "il numero va letto come un pavimento, con quanto manca accanto alla cifra.\n{report}"
        );
    }

    /// **E QUANDO SI SA TUTTO, IL NUMERO RESTA SECCO.** Senza questa metà
    /// l'avviso non varrebbe niente: un rapporto che si dichiara incompleto
    /// sempre non distingue più i due casi, ed è lo stesso difetto al contrario.
    #[test]
    fn a_total_where_every_call_is_measured_stays_a_plain_figure() {
        let report = report_for(&[
            a_call_named("una", Some(1_000_000)),
            a_call_named("due", Some(667_400)),
        ]);

        assert!(
            report.contains(&bare_total(1_667_400)),
            "tutto misurato: la somma è la somma.\n{report}"
        );
        assert!(
            !report.contains("at least"),
            "niente pavimenti dove non manca niente.\n{report}"
        );
        assert!(
            !report.contains(&catalogue::say("cli.flow.unmeasured_heading", &[])),
            "and nobody is named as unmeasured.\n{report}"
        );
    }

    /// The row `step close --turns` writes for a handed step: no cost, no
    /// tokens, the turns the agent counted, and the identity that says so.
    fn a_handed_call(step_id: &str, turns: u64) -> ledger::ModelCallRecord {
        let mut call = a_call_named(&format!("handed-{step_id}"), None);
        call.step_id = Some(step_id.to_owned());
        call.purpose = "handed_to_agent:self_declared".to_owned();
        call.requested_model = String::new();
        call.actual_model = String::new();
        call.input_tokens = None;
        call.output_tokens = None;
        call.cached_tokens = None;
        call.turns = Some(turns);
        call.engine_identity = ledger::EngineIdentity::DeclaredByAnAgent;
        call
    }

    fn a_measured_call(step_id: &str, cost: i64) -> ledger::ModelCallRecord {
        let mut call = a_call_named(&format!("measured-{step_id}"), Some(cost));
        call.step_id = Some(step_id.to_owned());
        call
    }

    fn the_line_naming(report: &str, step: &str) -> String {
        report
            .lines()
            .find(|line| line.trim_start().starts_with(&format!("{step}:")))
            .unwrap_or_default()
            .to_owned()
    }

    /// The floor and the name travel together: whoever reads «at least» sees
    /// which step kept the total from being one, and that its number was
    /// declared by the agent rather than measured — in the line of the turns
    /// too, not beside it.
    #[test]
    fn a_handed_step_is_named_as_self_declared_where_its_cost_would_be() {
        let report = report_for(&[
            a_measured_call("ask", 1_667_400),
            a_handed_call("build", 33),
        ]);

        assert!(report.contains(&floored_total(1_667_400, 2, 1)), "{report}");
        assert!(!report.contains(&bare_total(1_667_400)), "{report}");
        assert!(
            the_line_naming(&report, "build").contains("self-declared"),
            "the handed step is named with its reason where its cost would be.\n{report}"
        );
        assert!(
            the_line_naming(&report, "ask").is_empty(),
            "the measured step is not accused.\n{report}"
        );
        assert!(
            report.contains("in 36 turns, of which 33 self-declared"),
            "the declared turns are qualified in the line of the number.\n{report}"
        );
        assert!(
            !report.contains(&format!("{} (", ui::dashboard::model_not_declared())),
            "a step no engine served is not a model missing from the price list.\n{report}"
        );
    }

    /// The other reason a call has no cost, told apart from a handed step:
    /// the two are repaired in different places.
    #[test]
    fn a_call_the_price_list_cannot_price_is_named_with_the_model_not_as_handed() {
        let mut judged = a_call_named("judged", None);
        judged.step_id = Some("judge".to_owned());
        judged.actual_model = "mai-visto".to_owned();
        let report = report_for(&[a_measured_call("ask", 1_000_000), judged]);

        let named = the_line_naming(&report, "judge");
        assert!(
            named.contains("mai-visto (no entry in the price list)"),
            "{report}"
        );
        assert!(!named.contains("self-declared"), "{report}");
    }

    /// The whole road on a scratch ledger: the rows the engine and `step close
    /// --turns` write, gathered and reported as the command prints them.
    #[test]
    fn on_a_scratch_ledger_the_report_floors_the_total_and_names_the_handed_step() {
        let directory = TestDirectory::new();
        let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
        let flow: FlowFile = serde_json::from_str(&flow_json("shell_check", "[]", "{}"))
            .expect("caricare il flusso");
        let mut measured = a_measured_call("ask", 1_667_400);
        measured.run_id = "corsa-consegnata".to_owned();
        let mut handed = a_handed_call("build", 33);
        handed.run_id = "corsa-consegnata".to_owned();
        for call in [&measured, &handed] {
            ledger
                .record_model_call(call)
                .expect("registrare la chiamata");
        }
        record_run(
            &ledger,
            &flow,
            "corsa-consegnata",
            "complete",
            100,
            Some(110),
            None,
            None,
        )
        .expect("registrare la corsa");

        let report = cost_of_in(&directory.0, "prova").expect("il rapporto si scrive");

        assert!(report.contains(&floored_total(1_667_400, 2, 1)), "{report}");
        assert!(!report.contains(&bare_total(1_667_400)), "{report}");
        assert!(
            the_line_naming(&report, "build").contains("self-declared"),
            "{report}"
        );
    }

    /// **NESSUNA CHIAMATA MISURATA NON È «ALMENO ZERO».** È il terzo caso di
    /// `Spend`, quello che un `Option` collasserebbe: «almeno 0,0000» è vero e
    /// non dice niente, e chi lo legge crede di aver visto una spesa piccola.
    #[test]
    fn a_run_where_nothing_is_measured_says_unknown_instead_of_at_least_zero() {
        let report = report_for(&[a_call_named("consegnata", None)]);

        assert!(
            report.contains(&catalogue::say("ui.cost.unknown", &[("calls", "1")])),
            "senza nemmeno una misura non c'è un pavimento da dichiarare.\n{report}"
        );
        assert!(
            !report.contains(&bare_total(0)),
            "e soprattutto non c'è uno zero.\n{report}"
        );
    }
}
