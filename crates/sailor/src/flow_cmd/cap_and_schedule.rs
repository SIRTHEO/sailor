//! `sailor flow cap` and `sailor flow schedule`: the two settings a command may
//! write into a flow file, and the refusals that guard the writing.

use flow::FlowFile;
use std::fmt::Write as _;
use ui::gather::FlowSource;

use super::cost::in_units;
use super::{default_ledger_dir, one_flow};

// ── il tetto di spesa di un flusso ───────────────────────────────────────

/// **QUANTE CORSE COSTATE SERVONO PER SUGGERIRE UN TETTO.**
///
/// Tre, e non è una soglia arrotondata: sotto tre non c'è nessuna dispersione da
/// guardare. Con due campioni il massimo e il minimo sono gli unici due valori,
/// e chiamare «peggiore osservata» il maggiore di due è un dato inventato con la
/// faccia di una misura — che è precisamente il guasto 22, uno zero mai
/// calcolato passato per una misura, in un'altra forma. Sotto la soglia il
/// comando **rifiuta di suggerire** e dice cosa c'è.
const RUNS_BEFORE_SUGGESTING: usize = 3;

/// Che cosa il deposito ha visto spendere a un flusso.
struct Observed {
    /// Le corse di quel flusso registrate, comunque siano andate.
    runs: usize,
    /// Quelle che hanno speso qualcosa di noto: le sole su cui si può contare.
    costed_runs: usize,
    /// La corsa più cara osservata, in micro.
    worst_run_micros: i64,
    /// La chiamata più cara osservata, in micro.
    dearest_call_micros: i64,
    /// Quante chiamate non hanno dichiarato un costo. Non entrano nelle cifre
    /// sopra, e chi legge un suggerimento deve sapere quante ne mancano.
    calls_without_cost: usize,
}

/// Il conto vero e proprio: una corsa per riga, e per ogni corsa il costo di
/// ciascuna sua chiamata — `None` quando quel motore non l'ha dichiarato.
///
/// **PRENDE I COSTI E NON IL DEPOSITO, PER POTER ESSERE PROVATO.** Il deposito
/// predefinito è uno solo per processo, e le prove girano in parallelo dentro lo
/// stesso: una prova che volesse puntarlo altrove dovrebbe scrivere una
/// variabile d'ambiente, cioè rovinare le altre a caso. Qui la regola — cos'è
/// una «corsa costata», quale sia la peggiore, quale la chiamata più cara — si
/// interroga senza aprire niente.
fn observed_from(runs: &[Vec<Option<i64>>]) -> Observed {
    let mut seen = Observed {
        runs: runs.len(),
        costed_runs: 0,
        worst_run_micros: 0,
        dearest_call_micros: 0,
        calls_without_cost: 0,
    };
    for calls in runs {
        let mut spent = 0i64;
        for call in calls {
            match call {
                Some(cost) => {
                    spent += cost;
                    seen.dearest_call_micros = seen.dearest_call_micros.max(*cost);
                }
                None => seen.calls_without_cost += 1,
            }
        }
        // **UNA CORSA COSTATA È UNA CHE HA SPESO, NON UNA CHE HA CHIAMATO.** Le
        // 28 corse su 34 che questo deposito porta a zero il 31/08/2026 sono il
        // guasto 22 — il costo era la costante zero fino al 30/08 — e contarle
        // come campioni farebbe scendere ogni suggerimento verso lo zero, cioè
        // verso un tetto che ferma ogni flusso prima del primo passo.
        if spent > 0 {
            seen.costed_runs += 1;
            seen.worst_run_micros = seen.worst_run_micros.max(spent);
        }
    }
    seen
}

/// Quel che il deposito sa della spesa di un flusso.
///
/// **UN DEPOSITO ASSENTE NON È UN ERRORE**: è una macchina su cui quel flusso
/// non è mai girato, e la risposta giusta è «zero corse», non un guasto. Chi
/// legge deve poter chiedere il tetto di un flusso appena scritto.
fn observed_spending(flow_id: &str) -> Result<Observed, String> {
    let Ok(dir) = default_ledger_dir() else {
        return Ok(observed_from(&[]));
    };
    let Some(data) = ui::gather::gather(&dir).map_err(|error| error.to_string())? else {
        return Ok(observed_from(&[]));
    };
    let runs: Vec<Vec<Option<i64>>> = data
        .runs
        .iter()
        .filter(|run| run.entity == flow_id)
        .map(|run| {
            data.calls_by_run
                .get(&run.run_id)
                .map(|calls| calls.iter().map(|call| call.cost_micros).collect())
                .unwrap_or_default()
        })
        .collect();
    Ok(observed_from(&runs))
}

/// **QUELLO CHE IL TETTO NON PROMETTE**, scritto ogni volta che un tetto c'è.
///
/// Senza queste due righe il tetto si legge come una garanzia sulla spesa, e non
/// lo è. Chi mette un tetto e poi trova una fattura più alta ha ragione di
/// sentirsi tradito: meglio dirglielo quando lo mette.
pub(super) const WHAT_THE_CAP_DOES_NOT_PROMISE_KEY: &str = "cli.flow.what_the_cap_does_not_promise";

/// `sailor flow cap <nome>`: il tetto che c'è, e cosa il deposito ha visto.
pub(super) fn cap_of(sources: &[FlowSource], name: &str) -> Result<String, String> {
    let (flow, origin) = one_flow(sources, name)?;
    let mut report = format!("flow: {} ({origin})", flow.id);
    match flow.spend_cap_micros {
        None => report.push_str(&catalogue::say("cli.flow.cap_none_spends_freely", &[])),
        Some(cap) => {
            let _ = write!(
                report,
                "{}",
                catalogue::say(
                    "cli.flow.cap_is",
                    &[("micros", &cap.to_string()), ("units", &in_units(cap))]
                )
            );
            report.push_str(&catalogue::say(WHAT_THE_CAP_DOES_NOT_PROMISE_KEY, &[]));
        }
    }

    report.push_str(&what_the_ledger_saw(&observed_spending(&flow.id)?));
    Ok(report)
}

/// Cosa il deposito ha visto, e se da lì esce un suggerimento.
///
/// Sta a parte da `cap_of` perché la regola delle tre corse si deve poter
/// interrogare senza un deposito: `cap_of` ne apre uno vero, e un deposito vero
/// in una prova è una variabile d'ambiente globale al processo.
fn what_the_ledger_saw(seen: &Observed) -> String {
    let mut said = format!(
        "\n{}",
        catalogue::say(
            "cli.flow.in_the_store",
            &[
                ("runs", &seen.runs.to_string()),
                ("costed", &seen.costed_runs.to_string()),
            ],
        )
    );
    if seen.calls_without_cost > 0 {
        let _ = write!(
            said,
            "\n{}",
            catalogue::say(
                "cli.flow.calls_without_cost",
                &[("count", &seen.calls_without_cost.to_string())],
            )
        );
    }

    if seen.costed_runs < RUNS_BEFORE_SUGGESTING {
        let _ = write!(
            said,
            "\n{}",
            catalogue::say(
                "cli.flow.no_suggestion_yet",
                &[
                    ("needed", &RUNS_BEFORE_SUGGESTING.to_string()),
                    ("costed", &seen.costed_runs.to_string()),
                ],
            )
        );
        return said;
    }

    // **PERCHÉ SI SOMMA LA CHIAMATA PIÙ CARA, E NON È PRUDENZA.** Il controllo
    // scatta *prima* di aprire un fronte, mai dentro una chiamata: la corsa si
    // ferma con la granularità di una chiamata, non di un micro. Un tetto messo
    // esattamente sulla peggiore osservata è quindi un tetto che taglia le corse
    // di quella taglia in modo imprevedibile — dipende da come la spesa si
    // distribuisce fra i fronti. La somma dice: «la corsa più cara che ho visto,
    // più la grana con cui so fermarmi».
    let suggested = seen.worst_run_micros + seen.dearest_call_micros;
    let _ = write!(
        said,
        "\n{}",
        catalogue::say(
            "cli.flow.suggestion",
            &[
                ("suggested", &suggested.to_string()),
                ("units", &in_units(suggested)),
                ("worst_run", &in_units(seen.worst_run_micros)),
                ("dearest_call", &in_units(seen.dearest_call_micros)),
            ],
        )
    );
    said
}

/// The word that takes the cap off instead of setting one.
///
/// Without it the command could enter a state and not leave it: `0` is not
/// «none», it is «this flow must spend nothing».
const NO_CAP: &str = "none";

/// Words that used to be the only spelling, still accepted and no longer shown.
///
/// **A WORD A PERSON HAS ALREADY TYPED INTO A SCRIPT IS A PROMISE.** Dropping it
/// costs someone a run that fails for a reason the message cannot explain, so it
/// keeps working; leaving it in the help would teach it to whoever comes next.
const RETIRED_WORDS: &[(&str, &str)] =
    &[("nessuno", NO_CAP), ("leggero", LIGHT), ("pesante", HEAVY)];

/// What a typed word means, following [`RETIRED_WORDS`] once.
///
/// One hop and no more: an alias of an alias would make the accepted vocabulary
/// depend on the order of this list.
fn as_written_today(word: &str) -> &str {
    RETIRED_WORDS
        .iter()
        .find(|(retired, _)| *retired == word)
        .map_or(word, |(_, current)| *current)
}

/// `sailor flow cap <name> <micros|none>`: sets the cap or takes it off.
pub(super) fn set_cap(sources: &[FlowSource], name: &str, value: &str) -> Result<String, String> {
    let value = as_written_today(value);
    let wanted = if value == NO_CAP {
        None
    } else {
        let micros: i64 = value.parse().map_err(|_| {
            catalogue::say(
                "cli.flow.cap_not_a_number",
                &[("value", value), ("none", NO_CAP), ("name", name)],
            )
        })?;
        if micros < 0 {
            return Err(catalogue::say(
                "cli.flow.cap_negative",
                &[("micros", &micros.to_string()), ("none", NO_CAP)],
            ));
        }
        Some(micros)
    };

    let (mut flow, source) = a_flow_i_may_rewrite(sources, name)?;

    let before = flow.spend_cap_micros;
    if before == wanted {
        return Ok(catalogue::say(
            "cli.flow.cap_unchanged",
            &[
                ("flow", name),
                ("origin", source.origin),
                ("cap", &said_cap(before)),
            ],
        ));
    }
    flow.spend_cap_micros = wanted;
    flow::system::save_in(&source.dir, &flow)?;
    Ok(catalogue::say(
        "cli.flow.cap_written",
        &[
            ("flow", name),
            ("origin", source.origin),
            ("before", &said_cap(before)),
            ("after", &said_cap(wanted)),
            ("directory", &source.dir.display().to_string()),
        ],
    ))
}

/// Un tetto come lo legge una persona, compreso quando non c'è.
fn said_cap(cap: Option<i64>) -> String {
    match cap {
        None => NO_CAP.to_owned(),
        Some(micros) => format!("{micros} micro ({})", in_units(micros)),
    }
}

/// The flow a command may **rewrite**, and the source it sits in.
///
/// Apart, because both refusals belong to whoever writes, not to the cap: they
/// lived inside `set_cap` while it was the only gesture touching a file, and
/// copying them into the second would have been two copies of one rule. A
/// shipped flow is not rewritten — it is inside the binary, and the way is a
/// namesake at home, created by whoever wants it. And the file is named after
/// the `id`, or a twin appears: the register indexes by file name and writing
/// goes by `id`.
fn a_flow_i_may_rewrite<'a>(
    sources: &'a [FlowSource],
    name: &str,
) -> Result<(FlowFile, &'a FlowSource), String> {
    let (flow, source) = where_it_lives(sources, name)?;
    if source.is_builtin() {
        return Err(catalogue::say(
            "cli.flow.ships_inside_the_binary",
            &[("flow", name)],
        ));
    }
    // **SI CONFRONTANO I DUE NOMI, NON SI CHIEDE SE UN FILE ESISTE.** Qui c'era
    // `target.exists()`, e quel controllo rispondeva alla domanda sbagliata:
    // «esiste un file che si chiama come l'`id`?» invece di «il file che ho
    // letto è quello che sto per riscrivere?». Quando in cartella c'è davvero
    // un `<id>.flow.json` che appartiene a un *altro* flusso, `exists()` dice
    // di sì e la scrittura finisce **nel file di quel flusso**, con il comando
    // che risponde «fatto» e uscita zero. Misurato col binario vero il
    // 01/09/2026, guasto 50 — era il 41 finché questo ramo era da solo, poi il
    // 48, e adesso il 50: `sorgenti` ha preso quei due numeri mentre questo
    // ramo li assegnava, due fusioni di fila. È la terza cicatrice della stessa
    // ferita, e la cura non è rinumerare meglio — è che un numero nuovo non si
    // sceglie leggendo la tabella di un ramo solo.
    //
    // Il registro indicizza per nome di file: `name` **è** il nome del file da
    // cui questo flusso viene, e `save_in` scrive su `<id>.flow.json`. Se i due
    // nomi coincidono è lo stesso file, altrimenti non lo è — e nessun'altra
    // domanda lo può stabilire.
    if name != flow.id {
        return Err(catalogue::say(
            "cli.flow.file_does_not_match_id",
            &[("name", name), ("id", &flow.id)],
        ));
    }
    // Restano i due casi in cui il nome coincide e il file **non** è quello che
    // si riscriverebbe: un `<name>.json` senza `.flow`, che il registro carica
    // e la scrittura non sostituirebbe; e i due insieme, dove quale dei due
    // gira lo decide l'ordine in cui il sistema elenca la cartella.
    let target = source.dir.join(format!("{name}.flow.json"));
    let plain = source.dir.join(format!("{name}.json"));
    if !target.exists() {
        return Err(catalogue::say(
            "cli.flow.file_without_the_suffix",
            &[("name", name), ("file", &plain.display().to_string())],
        ));
    }
    if plain.exists() {
        return Err(catalogue::say(
            "cli.flow.two_files_one_name",
            &[("name", name)],
        ));
    }
    Ok((flow, source))
}

/// The word that takes the trigger off instead of setting one.
///
/// Same reason as [`NO_CAP`]. And a flow with no trigger is not a broken flow —
/// «it runs when somebody asks» is a fact, not a gap to fill.
const NO_SCHEDULE: &str = "none";

/// The two words a flow uses to say what one of its runs weighs.
const LIGHT: &str = "light";
const HEAVY: &str = "heavy";

/// Un innesco come lo legge una persona, compreso quando non c'è.
///
/// **UNA SOLA SCRITTURA PER TUTTI E DUE I COMANDI.** La legge chi chiede
/// `schedule <nome>` e chi lo cambia: due frasi diverse per lo stesso dato
/// farebbero credere a chi le confronta di aver cambiato più di quanto ha
/// cambiato.
fn said_schedule(schedule: Option<&flow::Schedule>) -> String {
    let Some(schedule) = schedule else {
        return catalogue::say(
            "cli.flow.no_schedule_starts_by_hand",
            &[("none", NO_SCHEDULE)],
        );
    };
    let when = match schedule.recurrence {
        flow::Recurrence::EverySeconds { seconds } => catalogue::say(
            "cli.flow.every_so_many_seconds",
            &[("seconds", &seconds.to_string())],
        ),
        flow::Recurrence::DailyAt { hour, minute } => catalogue::say(
            "cli.flow.once_a_day_at",
            &[("time", &format!("{hour:02}:{minute:02}"))],
        ),
    };
    let weight = match schedule.weight {
        flow::Weight::Light => LIGHT,
        flow::Weight::Heavy => HEAVY,
    };
    let perimeter = if schedule.perimeter.is_empty() {
        // Vuoto è «non dichiarato», che non è «nessun limite»: chi legge deve
        // poter distinguere i due, e la parola lo dice.
        "not declared".to_owned()
    } else {
        schedule.perimeter.join(", ")
    };
    format!("{when}; peso {weight}; perimetro: {perimeter}")
}

/// Da una parola alla ricorrenza che nomina, o al perché non la nomina.
///
/// **TRE FORME, RICONOSCIUTE DALLA LORO,** e nessuna bandiera: `nessuno`
/// toglie, `<numero>s` è un intervallo, `HH:MM` è un'ora del giorno. Sono le
/// due forme che `flow::Recurrence` conosce più il modo di uscirne — un
/// vocabolario più largo del tipo che deve riempire inventerebbe casi che il
/// motore non sa eseguire.
fn recurrence_from(value: &str) -> Result<flow::Recurrence, String> {
    if let Some(digits) = value.strip_suffix('s') {
        let seconds: u64 = digits
            .parse()
            .map_err(|_| how_a_schedule_is_written(value))?;
        if seconds == 0 {
            return Err(catalogue::say(
                "cli.flow.every_zero_seconds",
                &[("none", NO_SCHEDULE)],
            ));
        }
        return Ok(flow::Recurrence::EverySeconds { seconds });
    }
    if let Some((hour, minute)) = value.split_once(':') {
        let hour: u32 = hour.parse().map_err(|_| how_a_schedule_is_written(value))?;
        let minute: u32 = minute
            .parse()
            .map_err(|_| how_a_schedule_is_written(value))?;
        if hour > 23 || minute > 59 {
            return Err(catalogue::say(
                "cli.flow.not_a_time_of_day",
                &[("value", value)],
            ));
        }
        return Ok(flow::Recurrence::DailyAt { hour, minute });
    }
    Err(how_a_schedule_is_written(value))
}

/// Le forme ammesse, scritte per intero ogni volta che una non è riconosciuta:
/// un rifiuto che non dice cosa scrivere costringe a leggere il codice.
fn how_a_schedule_is_written(value: &str) -> String {
    catalogue::say(
        "cli.flow.how_a_schedule_is_written",
        &[("value", value), ("none", NO_SCHEDULE)],
    )
}

fn weight_from(word: &str) -> Result<flow::Weight, String> {
    match as_written_today(word) {
        LIGHT => Ok(flow::Weight::Light),
        HEAVY => Ok(flow::Weight::Heavy),
        other => Err(catalogue::say(
            "cli.flow.not_a_weight",
            &[("word", other), ("light", LIGHT), ("heavy", HEAVY)],
        )),
    }
}

/// `sailor flow schedule <nome>`: l'innesco che c'è, e quando è dovuto.
pub(super) fn schedule_of(sources: &[FlowSource], name: &str) -> Result<String, String> {
    let (flow, origin) = one_flow(sources, name)?;
    Ok(format!(
        "flow: {} ({origin})\ntrigger: {}",
        flow.id,
        said_schedule(flow.schedule.as_ref())
    ))
}

/// `sailor flow schedule <name> <every|at|none> [weight]`: sets, changes or
/// takes off the trigger.
///
/// **PERCHÉ QUESTO COMANDO ESISTE, ED È IL GUASTO 15 ALLA LETTERA.** Il
/// 29/08/2026 per cambiare l'innesco di un flusso è stato usato uno script
/// Python che riscriveva il JSON a mano. Uno strumento aggirato non registra
/// niente di ciò che gli succede intorno: nessun rifiuto sui flussi di sistema,
/// nessun controllo che il file si chiami come l'`id`, nessuna validazione del
/// grafo alla riscrittura. Tutte e tre le cose le fa questa strada, e nessuna le
/// faceva `python3`.
///
/// **IL PESO NON SI INVENTA.** Su un flusso che un innesco non ce l'ha ancora,
/// senza la parola il comando **rifiuta** invece di scegliere [`LIGHT`]: un peso
/// comparso da sé è un dato inventato con la faccia di una dichiarazione, ed è
/// il guasto 22 in un'altra forma. Su un flusso che ce l'ha già, tacere vuol
/// dire «lascialo com'è», che è un'altra cosa e si vede dal messaggio.
pub(super) fn set_schedule(
    sources: &[FlowSource],
    name: &str,
    value: &str,
    weight: Option<&str>,
) -> Result<String, String> {
    let (mut flow, source) = a_flow_i_may_rewrite(sources, name)?;
    let before = said_schedule(flow.schedule.as_ref());

    let wanted = if as_written_today(value) == NO_SCHEDULE {
        if let Some(word) = weight {
            return Err(catalogue::say(
                "cli.flow.no_schedule_has_no_weight",
                &[("none", NO_SCHEDULE), ("word", word)],
            ));
        }
        None
    } else {
        let recurrence = recurrence_from(value)?;
        let weight = match (weight, flow.schedule.as_ref()) {
            (Some(word), _) => weight_from(word)?,
            (None, Some(existing)) => existing.weight,
            (None, None) => {
                return Err(catalogue::say(
                    "cli.flow.no_schedule_yet_so_no_weight",
                    &[
                        ("name", name),
                        ("value", value),
                        ("light", LIGHT),
                        ("heavy", HEAVY),
                    ],
                ))
            }
        };
        Some(flow::Schedule {
            recurrence,
            weight,
            // **IL PERIMETRO SI CONSERVA, NON SI RIDICHIARA.** Dice dove quella
            // lavorazione può scrivere: perderlo cambiando l'orario sarebbe un
            // permesso allargato da un comando che parlava d'altro.
            perimeter: flow
                .schedule
                .as_ref()
                .map(|existing| existing.perimeter.clone())
                .unwrap_or_default(),
        })
    };

    if flow.schedule == wanted {
        return Ok(catalogue::say(
            "cli.flow.schedule_unchanged",
            &[
                ("flow", name),
                ("origin", source.origin),
                ("before", &before),
            ],
        ));
    }
    flow.schedule = wanted;
    let after = said_schedule(flow.schedule.as_ref());
    flow::system::save_in(&source.dir, &flow)?;
    Ok(catalogue::say(
        "cli.flow.schedule_written",
        &[
            ("flow", name),
            ("origin", source.origin),
            ("before", &before),
            ("after", &after),
            ("directory", &source.dir.display().to_string()),
        ],
    ))
}

/// Il flusso **e la sorgente da cui viene**: per riscriverlo serve la cartella,
/// non il nome dell'origine.
///
/// Si guarda dalla più specifica alla meno specifica, cioè al contrario
/// dell'ordine in cui le sorgenti sono elencate: a parità di nome vince
/// l'ultima, e chi riscrive deve riscrivere **quella che gira**. Riscrivere la
/// copia meno specifica lascerebbe il comando a dire «fatto» mentre la corsa
/// continua a leggere l'altra.
fn where_it_lives<'a>(
    sources: &'a [FlowSource],
    name: &str,
) -> Result<(FlowFile, &'a FlowSource), String> {
    for source in sources.iter().rev() {
        match flow::system::registry_of(source).remove(name) {
            Some(Ok(flow)) => return Ok((flow, source)),
            Some(Err(reason)) => {
                return Err(catalogue::say(
                    "cli.flow.does_not_load_so_not_rewritten",
                    &[
                        ("flow", name),
                        ("origin", source.origin),
                        ("reason", &reason),
                    ],
                ))
            }
            None => continue,
        }
    }
    match one_flow(sources, name) {
        // Lo stesso messaggio di `one_flow`, con lo stesso elenco di nomi: due
        // parole diverse per lo stesso «non c'è» manderebbero a cercare due
        // difetti dove ce n'è uno.
        Err(reason) => Err(reason),
        // Non può succedere — `one_flow` legge le stesse sorgenti del giro qui
        // sopra — e se succedesse vorrebbe dire che le due strade che cercano un
        // flusso si sono separate. Dirlo vale più che panicare in mano a chi sta
        // usando il comando.
        Ok(_) => Err(catalogue::say(
            "cli.flow.loads_but_no_source_lists_it",
            &[("name", name)],
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::usage;
    use super::*;
    use std::fs;

    /// **SOTTO TRE CORSE COSTATE NON SI SUGGERISCE NIENTE, E SI DICE PERCHÉ.**
    ///
    /// È la regola che tiene fuori il guasto 22: nel deposito di questa macchina
    /// il 31/08/2026 sei corse su trentaquattro hanno un costo diverso da zero,
    /// e nessun flusso ne ha tre. Una mediana su quella colonna darebbe zero per
    /// ogni flusso, cioè un tetto che ferma ogni corsa prima del primo passo.
    #[test]
    fn under_three_costed_runs_no_cap_is_suggested() {
        let two = observed_from(&[vec![Some(100)], vec![Some(200)]]);

        let said = what_the_ledger_saw(&two);

        assert!(said.contains("no suggestion"), "{said}");
        // The suggestion line starts on a new line: looking for «suggestion: »
        // without the newline would also find «no suggestion: ».
        assert!(!said.contains("\nsuggestion: "), "{said}");
        assert!(
            said.contains("there are 2"),
            "and it says what is there: {said}"
        );
    }

    /// **CON TRE, IL SUGGERIMENTO È LA PEGGIORE PIÙ LA CHIAMATA PIÙ CARA.**
    ///
    /// La gemella di quella sopra: senza di lei un comando che non suggerisse
    /// mai passerebbe l'altra e non servirebbe a niente.
    #[test]
    fn with_three_costed_runs_the_suggestion_is_the_worst_plus_the_dearest_call() {
        let three = observed_from(&[
            vec![Some(100), Some(50)],
            vec![Some(400), Some(300)],
            vec![Some(200)],
        ]);

        let said = what_the_ledger_saw(&three);

        // Peggiore corsa 700, chiamata più cara 400: 1100.
        assert!(said.contains("\nsuggestion: 1100 micro"), "{said}");
        assert!(!said.contains("no suggestion"), "{said}");
    }

    /// **UNA CORSA CHE NON HA SPESO NON È UN CAMPIONE.**
    ///
    /// Ventotto delle trentaquattro corse di questo deposito portano zero perché
    /// il costo *era* la costante zero fino al 30/08/2026. Contarle farebbe
    /// scendere ogni suggerimento verso lo zero — cioè verso un tetto che ferma
    /// tutto — con l'aria di una misura su molti campioni.
    #[test]
    fn runs_that_spent_nothing_are_not_samples() {
        let seen = observed_from(&[vec![Some(0)], vec![], vec![None, None], vec![Some(900)]]);

        assert_eq!(seen.runs, 4, "le corse ci sono tutte");
        assert_eq!(seen.costed_runs, 1, "ma una sola ha speso");
        assert_eq!(seen.worst_run_micros, 900);
        assert_eq!(seen.calls_without_cost, 2, "e due chiamate restano fuori");
    }

    /// **UN FLUSSO DI SISTEMA NON SI RISCRIVE, E NON NE COMPARE UNO NUOVO.**
    ///
    /// Sta dentro il binario: non c'è nessun file da modificare. La strada è un
    /// omonimo in casa propria — che quel file lo crei chi lo vuole, sapendo di
    /// averlo creato. Un flusso comparso da sé cambierebbe cosa gira senza che
    /// nessuno l'abbia deciso.
    #[test]
    fn a_system_flow_refuses_the_cap_instead_of_growing_a_twin() {
        let home = TestDirectory::new();
        let sources = flow::system::sources(&home.0, None, None);
        let shipped = flow::system::FLOWS[0].0;

        let error = set_cap(&sources, shipped, "1000000").expect_err("un flusso di sistema");

        assert!(error.contains("ships inside the binary"), "{error}");
        assert!(
            entries_of(&home.0).is_empty(),
            "non deve essere comparso nessun file in casa: {:?}",
            entries_of(&home.0)
        );
    }

    /// Mettere il tetto scrive **nella cartella da cui il flusso viene**, e non
    /// tocca nient'altro del file.
    #[test]
    fn setting_the_cap_writes_where_the_flow_lives() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        let said = set_cap(&sources, "prova", "750000").expect("il tetto si scrive");

        assert!(said.contains("750000 micro"), "{said}");
        let after = written_flow(&home.0, "prova");
        assert_eq!(after.spend_cap_micros, Some(750_000));
        assert_eq!(after.description, "flusso di prova", "il resto è intatto");
        assert_eq!(after.graph.steps().len(), 1);
        assert_eq!(
            entries_of(&home.0).len(),
            1,
            "e non è comparso nessun gemello: {:?}",
            entries_of(&home.0)
        );
    }

    /// **UN FILE CHE NON SI CHIAMA COME IL PROPRIO `id` NON SI RISCRIVE.**
    ///
    /// Il registro indicizza per nome di file, la scrittura per `id`: dove i due
    /// divergono, riscrivere creerebbe un secondo flusso invece di sostituire
    /// questo. Senza questo rifiuto il comando direbbe «fatto» lasciando in
    /// cartella due flussi con lo stesso `id`.
    #[test]
    fn a_file_named_differently_from_its_id_is_refused_instead_of_duplicated() {
        let home = TestDirectory::new();
        home.write(
            "altro-nome.flow.json",
            &flow_json("shell_check", "[]", "{}"),
        );
        let sources = flow::system::sources(&home.0, None, None);

        let error = set_cap(&sources, "altro-nome", "500").expect_err("nome e id divergono");

        assert!(error.contains("a second flow"), "{error}");
        assert_eq!(entries_of(&home.0).len(), 1, "nessun gemello sul disco");
    }

    /// Un valore che non è un numero né «nessuno» si rifiuta dicendo cos'è un
    /// micro: chi sbaglia unità mette un tetto mille volte più basso di quello
    /// che credeva, e la corsa si ferma senza che lui capisca perché.
    #[test]
    fn a_cap_that_is_not_a_number_is_refused_with_the_unit_spelled_out() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        let error = set_cap(&sources, "prova", "1,50").expect_err("non è un numero di micro");
        assert!(error.contains("micro"), "{error}");

        let negative = set_cap(&sources, "prova", "-1").expect_err("un tetto negativo");
        assert!(negative.contains("a negative cap"), "{negative}");
    }

    /// **`nessuno` TOGLIE IL TETTO, E NON LO METTE A ZERO.** Senza questa parola
    /// il comando saprebbe entrare in uno stato e non uscirne: `0` è «non deve
    /// spendere niente», che ferma la corsa prima del primo passo.
    #[test]
    fn the_word_for_no_cap_clears_it_instead_of_setting_zero() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);
        set_cap(&sources, "prova", "500").expect("prima si mette");

        set_cap(&sources, "prova", NO_CAP).expect("poi si toglie");

        assert_eq!(written_flow(&home.0, "prova").spend_cap_micros, None);
    }

    /// **A WORD ALREADY TYPED INTO A SCRIPT KEEPS WORKING.** The vocabulary is
    /// English; the words it replaced stay accepted and stay out of the help.
    /// Without this test the promise is a comment, and the first person to tidy
    /// up `as_written_today` breaks somebody's run with a message that cannot
    /// explain itself.
    #[test]
    fn the_words_that_used_to_be_the_only_ones_still_work() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        set_cap(&sources, "prova", "500").expect("prima si mette");
        set_cap(&sources, "prova", "nessuno").expect("la parola di ieri toglie il tetto");
        assert_eq!(written_flow(&home.0, "prova").spend_cap_micros, None);

        set_schedule(&sources, "prova", "3600s", Some("leggero")).expect("e sceglie un peso");
        assert_eq!(
            weight_from("leggero").expect("è ancora un peso"),
            weight_from(LIGHT).expect("come la parola di oggi"),
            "«leggero» e «{LIGHT}» devono dire la stessa cosa"
        );
        assert_eq!(
            weight_from("pesante").expect("è ancora un peso"),
            weight_from(HEAVY).expect("come la parola di oggi")
        );

        set_schedule(&sources, "prova", "nessuno", None).expect("e toglie l'innesco");
        assert!(written_flow(&home.0, "prova").schedule.is_none());
    }

    /// L'aiuto non le nomina: una parola ritirata che compare dove si impara il
    /// comando non è ritirata, è la seconda forma ufficiale.
    #[test]
    fn the_retired_words_are_accepted_and_never_taught() {
        let shown = usage();
        for (retired, _) in RETIRED_WORDS {
            assert!(
                !shown.contains(retired),
                "«{retired}» è ritirata e l'aiuto la insegna ancora:\n{shown}"
            );
        }
        for current in [NO_CAP, LIGHT, HEAVY] {
            assert!(
                shown.contains(current),
                "«{current}» è la parola di oggi e l'aiuto non la nomina:\n{shown}"
            );
        }
    }

    // ── l'innesco si cambia da dentro Sailor ─────────────────────────
    //
    // **IL GUASTO 15 ALLA LETTERA.** Il 29/08/2026 per cambiare l'innesco di un
    // flusso è stato usato uno script Python che riscriveva il JSON a mano.
    // `sailor flow` sapeva elencare, controllare, eseguire — e per il gesto che
    // serviva davvero si usciva dal sistema. Uno strumento aggirato non
    // registra niente di ciò che gli succede intorno, e nessun suo controllo
    // vede l'aggiro: dal punto di vista di Sailor quel giorno non è successo
    // niente.

    /// **CAMBIARE L'INNESCO È UN COMANDO, E IL FILE SUL DISCO LO DICE.**
    ///
    /// Il mutante che la fa cadere è togliere `flow::system::save_in` da
    /// `set_schedule`: il comando continuerebbe a rispondere «fatto», e il file
    /// resterebbe com'era — che è il difetto peggiore di tutti, perché somiglia
    /// in tutto al lavoro fatto.
    #[test]
    fn the_trigger_of_a_flow_changes_from_inside_sailor() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);
        assert_eq!(
            written_flow(&home.0, "prova").schedule,
            None,
            "si parte da un flusso senza innesco"
        );

        let said =
            set_schedule(&sources, "prova", "3600s", Some(LIGHT)).expect("l'innesco si scrive");

        assert!(said.contains("every 3600s"), "{said}");
        let after = written_flow(&home.0, "prova");
        assert_eq!(
            after.schedule,
            Some(flow::Schedule {
                recurrence: flow::Recurrence::EverySeconds { seconds: 3600 },
                weight: flow::Weight::Light,
                perimeter: vec![],
            })
        );
        assert_eq!(after.description, "flusso di prova", "il resto è intatto");
        assert_eq!(after.graph.steps().len(), 1);
        assert_eq!(
            entries_of(&home.0).len(),
            1,
            "nessun gemello sul disco: {:?}",
            entries_of(&home.0)
        );
    }

    /// L'ora del giorno è l'altra forma che il motore sa eseguire, e va provata
    /// insieme: una sola delle due lascerebbe metà del comando senza misura.
    #[test]
    fn an_hour_of_the_day_is_the_other_form_the_engine_can_run() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        set_schedule(&sources, "prova", "07:30", Some(HEAVY)).expect("l'ora si scrive");

        assert_eq!(
            written_flow(&home.0, "prova").schedule,
            Some(flow::Schedule {
                recurrence: flow::Recurrence::DailyAt {
                    hour: 7,
                    minute: 30
                },
                weight: flow::Weight::Heavy,
                perimeter: vec![],
            })
        );
    }

    /// **IL PESO NON SI INVENTA SU UN FLUSSO CHE NON NE HA UNO.**
    ///
    /// Il ripiego ovvio sarebbe «leggero», e sarebbe un dato inventato con la
    /// faccia di una dichiarazione: chi legge `docs/da-fare.md` vedrebbe un peso
    /// che nessuno ha misurato. Il rifiuto scrive la riga da digitare, così
    /// costa una battuta e non una lettura del codice.
    #[test]
    fn a_weight_nobody_declared_is_refused_instead_of_guessed() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        let error =
            set_schedule(&sources, "prova", "3600s", None).expect_err("nessun peso da tenere");

        assert!(error.contains(LIGHT) && error.contains(HEAVY), "{error}");
        assert_eq!(
            written_flow(&home.0, "prova").schedule,
            None,
            "un rifiuto non scrive niente"
        );
    }

    /// Su un flusso che un innesco ce l'ha già, tacere il peso vuol dire
    /// «lascialo com'è» — e il perimetro dichiarato non si perde cambiando
    /// l'orario: sarebbe un permesso allargato da un comando che parlava
    /// d'altro.
    #[test]
    fn changing_only_the_hour_keeps_the_weight_and_the_perimeter() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);
        let mut with_perimeter = written_flow(&home.0, "prova");
        with_perimeter.schedule = Some(flow::Schedule {
            recurrence: flow::Recurrence::DailyAt { hour: 3, minute: 0 },
            weight: flow::Weight::Heavy,
            perimeter: vec!["~/progetti/sailor".to_owned()],
        });
        flow::system::save_in(&home.0, &with_perimeter).expect("il flusso di partenza");

        set_schedule(&sources, "prova", "05:15", None).expect("solo l'ora cambia");

        let after = written_flow(&home.0, "prova")
            .schedule
            .expect("l'innesco c'è");
        assert_eq!(
            after.recurrence,
            flow::Recurrence::DailyAt {
                hour: 5,
                minute: 15
            }
        );
        assert_eq!(after.weight, flow::Weight::Heavy, "il peso resta quello");
        assert_eq!(
            after.perimeter,
            vec!["~/progetti/sailor".to_owned()],
            "il perimetro non si perde cambiando l'orario"
        );
    }

    /// **`nessuno` TOGLIE L'INNESCO**, come `nessuno` toglie il tetto: senza la
    /// parola il comando saprebbe entrare in uno stato e non uscirne, e un
    /// flusso che parte solo a mano è un fatto, non un vuoto da riempire.
    #[test]
    fn the_word_for_no_trigger_clears_it() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);
        set_schedule(&sources, "prova", "3600s", Some(LIGHT)).expect("prima si mette");

        set_schedule(&sources, "prova", NO_SCHEDULE, None).expect("poi si toglie");

        assert_eq!(written_flow(&home.0, "prova").schedule, None);
        // E l'assenza si scrive assente, non `null`: chi rilegge il proprio
        // flusso dopo il comando non deve trovarci righe che nessuno ha scritto.
        let text = fs::read_to_string(home.0.join("prova.flow.json")).expect("rileggere");
        assert!(!text.contains("schedule"), "{text}");
    }

    /// Le forme non riconosciute si rifiutano **elencando quelle giuste**: un
    /// rifiuto che non dice cosa scrivere manda a leggere il codice, cioè fuori
    /// dal sistema — che è il guasto 15 daccapo.
    #[test]
    fn a_trigger_that_is_not_one_of_the_three_forms_says_what_the_three_are() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        for wrong in ["ogni-tanto", "0s", "25:00", "07:70", "3600"] {
            let error =
                set_schedule(&sources, "prova", wrong, Some(LIGHT)).unwrap_or_else(|error| error);
            assert!(
                error.contains(NO_SCHEDULE) || error.contains("hours run from"),
                "«{wrong}» è stato accettato o rifiutato senza dire come si scrive: {error}"
            );
        }
        assert_eq!(
            written_flow(&home.0, "prova").schedule,
            None,
            "nessuna delle forme sbagliate ha scritto qualcosa"
        );
    }

    /// **UN FLUSSO DI SISTEMA NON SI RISCRIVE**, e il rifiuto vale per ogni
    /// gesto che scrive, non solo per il tetto: è lo stesso controllo, chiamato
    /// da tutti e due.
    #[test]
    fn a_system_flow_refuses_the_trigger_too() {
        let home = TestDirectory::new();
        let sources = flow::system::sources(&home.0, None, None);
        let shipped = flow::system::FLOWS[0].0;

        let error = set_schedule(&sources, shipped, "3600s", Some(LIGHT))
            .expect_err("un flusso di sistema");

        assert!(error.contains("ships inside the binary"), "{error}");
        assert!(
            entries_of(&home.0).is_empty(),
            "non deve essere comparso nessun file in casa: {:?}",
            entries_of(&home.0)
        );
    }

    /// **GUASTO 41: IL COMANDO SCRIVEVA NEL FILE DI UN ALTRO FLUSSO E DICEVA
    /// «FATTO».**
    ///
    /// Il controllo ereditato da `set_cap` chiedeva se `<id>.flow.json`
    /// **esistesse**, non se fosse *quel* file. Con in cartella un file che si
    /// chiama come l'`id` di un altro flusso, la risposta era sì e la scrittura
    /// finiva là dentro, con uscita zero. Vale identico per `cap`, cioè esisteva
    /// già in produzione: qui si provano tutti e due i gesti, o la riparazione
    /// coprirebbe metà della superficie.
    ///
    /// Il mutante che la fa cadere è rimettere `target.exists()` al posto del
    /// confronto fra i nomi: entrambe le scritture tornano a riuscire, e il
    /// flusso estraneo si ritrova un innesco che nessuno gli ha messo.
    #[test]
    fn a_flow_whose_file_is_named_after_another_one_is_never_written_through() {
        let home = TestDirectory::new();
        // Due file, lo stesso `id` dentro: il registro li indicizza per nome di
        // file, la scrittura per `id`.
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        home.write(
            "nome-diverso.flow.json",
            &flow_json("shell_check", "[]", "{}"),
        );
        let sources = flow::system::sources(&home.0, None, None);

        let refused = set_schedule(&sources, "nome-diverso", "3600s", Some(LIGHT))
            .expect_err("il nome del file non è l'id");
        assert!(refused.contains("a second flow"), "{refused}");

        let refused_cap = set_cap(&sources, "nome-diverso", "500000")
            .expect_err("lo stesso rifiuto vale per il tetto");
        assert!(refused_cap.contains("a second flow"), "{refused_cap}");

        // **E LA PARTE CHE CONTA: IL FLUSSO ESTRANEO NON È STATO TOCCATO.** Un
        // rifiuto che avesse comunque scritto sarebbe peggio del difetto.
        let bystander = written_flow(&home.0, "prova");
        assert_eq!(
            bystander.schedule, None,
            "l'innesco di «prova» non si tocca"
        );
        assert_eq!(bystander.spend_cap_micros, None, "e nemmeno il suo tetto");
        assert_eq!(written_flow(&home.0, "nome-diverso").schedule, None);
        assert_eq!(
            entries_of(&home.0).len(),
            2,
            "e non è comparso nessun terzo file: {:?}",
            entries_of(&home.0)
        );
    }

    /// Un flusso che sta in un `.json` senza `.flow` non si riscrive: la
    /// scrittura andrebbe in un file diverso da quello letto, cioè nascerebbe un
    /// gemello. È lo stesso difetto del guasto 50 dall'altro lato — il file che
    /// si legge e il file che si scrive devono essere lo stesso.
    #[test]
    fn a_flow_read_from_a_plain_json_is_refused_instead_of_duplicated() {
        let home = TestDirectory::new();
        home.write("prova.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        let refused = set_schedule(&sources, "prova", "3600s", Some(LIGHT))
            .expect_err("il file letto non è quello che si scriverebbe");

        assert!(refused.contains("a second flow"), "{refused}");
        assert_eq!(entries_of(&home.0).len(), 1, "nessun gemello sul disco");
    }

    /// Leggere l'innesco è un gesto suo: chi non sa cosa c'è non sa cosa sta
    /// cambiando, e `flow list` non lo mostra.
    #[test]
    fn asking_for_the_trigger_says_what_is_there_and_what_is_not() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        let before = schedule_of(&sources, "prova").expect("si legge");
        assert!(before.contains(NO_SCHEDULE), "{before}");

        set_schedule(&sources, "prova", "300s", Some(HEAVY)).expect("si mette");

        let after = schedule_of(&sources, "prova").expect("si rilegge");
        assert!(after.contains("every 300s"), "{after}");
        assert!(after.contains(HEAVY), "{after}");
        assert!(
            after.contains("not declared"),
            "an empty scope says so: {after}"
        );
    }

    fn written_flow(dir: &std::path::Path, name: &str) -> FlowFile {
        let text = fs::read_to_string(dir.join(format!("{name}.flow.json")))
            .expect("il flusso scritto si rilegge");
        serde_json::from_str(&text).expect("e si deserializza")
    }

    fn entries_of(dir: &std::path::Path) -> Vec<String> {
        fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}
