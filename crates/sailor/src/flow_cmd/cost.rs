//! `sailor flow cost`: what a run spent, which checks refused what, and which
//! models the price list cannot price.

use flow::StepRecord;
use models::pricing::{Known, PriceList};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use super::{default_ledger_dir, now_secs};

/// What a flow's last run consumed: more or less than a single prompt?
/// Reading it meant opening SQLite by hand, which is fault 15.
///
/// **TOKENS COME BEFORE COST.** A local command line pays a subscription, not a
/// call: what is spent is quota, in tokens, and the currency figure is only what
/// the API would have charged. Cost alone hides engines that declare no cost.
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
    // The last by start, not the last written: an open run and a closed one
    // can arrive in reverse order in the projection.
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

/// What a run consumed, for a person.
///
/// **THE PRICE LIST COMES FROM OUTSIDE, AND IT IS NOT FUSSINESS:** it lets this
/// function be asked with a price list written in the test, rather than
/// depending on which file exists on the machine running the suite.
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
    // **TURNS BESIDE CALLS, AND IT IS NOT A DETAIL.** One call to an agentic
    // engine is dozens of turns, and those make the bill: a chain of four steps
    // reads 8% more per turn than a single session and consumes twice as much,
    // taking twice the turns. **AND THE RAW COUNT ONLY: no `cache read ÷ turns`
    // here.** It looks like «the context of a request» and is not — over four
    // steps of a real run it gives 21.165 / 13.566 / 48.984 / 50.885 against the
    // 46.702 / 31.651 / 63.266 / 71.173 the requests really read, off by 1,29 to
    // 2,33 times, with the factor changing **between steps of one run**: the
    // mean of a ramp, and a printed number gets used to decide.
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
    // **THE FIGURE IS NOT COMPOSED HERE.** Written by this function it would
    // redo the three-case rule in a `format!` — and the first time somebody
    // touches one of the two places the two versions diverge in silence.
    // Whoever decides how many steps to open and whoever reads the consumption
    // must read the same sentence.
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
    // **WHAT IS MISSING IS SAID, OR THE TOTAL READS AS COMPLETE.** Same rule as
    // the window's: a sum silent about what it did not count is a reassurance,
    // not a measurement. It stays even where the cost declares itself: missing
    // tokens are another gap, and a run can have one and not the other.
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
    // **AND WHICH MODEL IS SAID, NOT MERELY HOW MANY CALLS.** «Three with no
    // known cost» is a number nobody can act on; the name of the uncovered
    // model is a line to write in the price list. Second half of the cure for
    // fault 35: whoever has no price for a model must know it, not infer it
    // from a zero. The names come from `tokens_by_model`, from whoever really
    // answered in this run.
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
    // **WHICH IDENTITY THE PROCESSES OF THIS RUN STARTED WITH.** «If an AI
    // process starts there must be a profile behind it»: the data was written
    // in the ledger, reread inside a struct, and reached **no** screen and no
    // command, and data collected but never looked at is one step from becoming
    // wrong data nobody notices. **NO TOKEN APPEARS HERE**, and that is not an
    // oversight: which home and how it was picked is what one goes to look at.
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

/// What a unit of currency costs in micros. A million: `1_000_000` is a
/// dollar.
const MICROS_IN_A_UNIT: f64 = 1_000_000.0;

/// The models past runs of this flow really used.
///
/// **FROM THE LEDGER AND NOT THE FLOW, BECAUSE THE FLOW DOES NOT KNOW.** A step
/// names the tool — `claude-code`, `codex` — not the model: the command line
/// picks who answers, and it is found out afterwards from what was declared.
/// **`None` IS NOT AN EMPTY SET**: a ledger that will not open says nobody
/// could look, and confusing the two would print «never run here» for a flow
/// run a hundred times.
pub(super) fn models_seen_by(flow_id: &str) -> Option<BTreeSet<String>> {
    let dir = default_ledger_dir().ok()?;
    let data = ui::gather::gather(&dir).ok()??;
    Some(
        data.runs
            .iter()
            .filter(|run| run.entity == flow_id)
            .filter_map(|run| data.calls_by_run.get(&run.run_id))
            .flatten()
            // A call with no declared model is not a model with no price: it
            // is an engine that will not say who answered, and naming the
            // empty string among the uncovered models would send somebody
            // hunting a price-list entry for a name that does not exist.
            .filter(|call| !call.actual_model.trim().is_empty())
            .map(|call| call.actual_model.clone())
            .collect(),
    )
}

/// The micro-units as a person reads them.
pub(super) fn in_units(micros: i64) -> String {
    format!("{:.2}", micros as f64 / MICROS_IN_A_UNIT)
}

// ── what the price list cannot price ─────────────────────────────────────

/// Why this list cannot price a name, or nothing at all when it can.
///
/// **THE WHY SITS BESIDE THE NAME BECAUSE THEY ARE TWO DIFFERENT REPAIRS.** A
/// name the list does not know wants an entry — or an alias, if it is the same
/// model under another name; an entry with no prices wants the prices. A list
/// of bare names would send somebody to rewrite an entry that is already there.
fn why_it_cannot_be_priced(prices: &PriceList, name: &str) -> Option<String> {
    match prices.knows(name) {
        Known::Priced => None,
        Known::Absent => Some(catalogue::say(
            "cli.flow.model_absent_from_the_list",
            &[("model", name)],
        )),
        Known::ListedWithoutPrice => Some(catalogue::say(
            "cli.flow.model_listed_without_price",
            &[("model", name)],
        )),
    }
}

/// The names this list cannot price, each with its why.
fn cannot_be_priced(prices: &PriceList, seen: &BTreeSet<String>) -> Vec<String> {
    seen.iter()
        .filter_map(|name| why_it_cannot_be_priced(prices, name))
        .collect()
}

/// What a flow says about the models of the run that is about to start.
///
/// **A STEP THAT NAMES NO MODEL IS NOT A STEP WITH A DEFAULT ONE.** Whoever
/// answers picks it, and it changes underneath: `unnamed` is therefore the list
/// of steps to repair by writing a `model`, and not an elegant absence.
#[derive(Debug, Default)]
pub(super) struct ModelsAskedByTheFlow {
    /// Each model the flow names, and the steps naming it.
    named: BTreeMap<String, BTreeSet<String>>,
    /// Each step asking an engine without naming its model, and those engines.
    unnamed: BTreeMap<String, BTreeSet<String>>,
}

/// Read off the steps, engine by engine of every chain.
///
/// **THE MISSING MODELS ARE ASKED OF THE DESCRIPTOR, NOT GUESSED FROM A NAME.**
/// The same action runs a build command, which names a tool and can be told no
/// model: sending somebody to write a `model` for it would be a repair that
/// gets refused. What is named is kept whoever it was asked of.
pub(super) fn models_asked_by(
    flow: &flow::FlowFile,
    tools: &dyn actions::ToolResolver,
) -> ModelsAskedByTheFlow {
    let mut asked = ModelsAskedByTheFlow::default();
    for step in flow.graph.steps() {
        if step.action != actions::EXTERNAL_ENGINE_ACTION {
            continue;
        }
        let Some(with) = step.with.as_ref() else {
            continue;
        };
        let named = actions::models_named_in(with);
        for engine in actions::engines_named_in(with) {
            match named.get(&engine) {
                Some(model) => {
                    asked
                        .named
                        .entry(model.clone())
                        .or_default()
                        .insert(step.id.clone());
                }
                None if tools.model_option(&engine).is_some() => {
                    asked
                        .unnamed
                        .entry(step.id.clone())
                        .or_default()
                        .insert(engine);
                }
                None => {}
            }
        }
    }
    asked
}

/// The names, joined as a person reads them.
fn joined(names: &BTreeSet<String>) -> String {
    names.iter().cloned().collect::<Vec<_>>().join(", ")
}

/// Every model this flow names that the list cannot price, with its steps.
fn named_and_unpriced(prices: &PriceList, asked: &ModelsAskedByTheFlow) -> Vec<String> {
    asked
        .named
        .iter()
        .filter_map(|(model, steps)| {
            let why = why_it_cannot_be_priced(prices, model)?;
            Some(catalogue::say(
                "cli.flow.model_named_by_steps",
                &[("named", &why), ("steps", &joined(steps))],
            ))
        })
        .collect()
}

/// What the ledger says about the models past runs were answered by, and
/// whether one of them has no price.
fn past_models_into(said: &mut String, prices: &PriceList, seen: Option<&BTreeSet<String>>) -> bool {
    let Some(seen) = seen else {
        said.push_str(&catalogue::say("cli.flow.models_store_unreadable", &[]));
        return false;
    };
    if seen.is_empty() {
        said.push_str(&catalogue::say("cli.flow.models_never_run_here", &[]));
        return false;
    }
    let unpriced = cannot_be_priced(prices, seen);
    let key = if unpriced.is_empty() {
        "cli.flow.past_models_all_priced"
    } else {
        "cli.flow.past_models_unpriced"
    };
    let names = if unpriced.is_empty() {
        joined(seen)
    } else {
        unpriced.join(", ")
    };
    let _ = write!(said, "\n{}", catalogue::say(key, &[("models", &names)]));
    !unpriced.is_empty()
}

/// What the price list can say about this flow, **before** it is launched.
///
/// **THREE THINGS AND NOT TWO, AND THE PAST RUNS ARE THE LAST.** A step that
/// names its model is a fact about the run ahead; a step naming none leaves the
/// choice to whoever answers, and that default changes underneath; the models
/// of past runs are a reading of the ledger, never a promise. See fault 104.
pub(super) fn what_is_priced(
    prices: &PriceList,
    asked: &ModelsAskedByTheFlow,
    seen: Option<&BTreeSet<String>>,
    cap: Option<i64>,
) -> String {
    let mut said = format!(
        "\n{}",
        catalogue::say(
            "cli.flow.price_list_size",
            &[("count", &prices.entries.len().to_string())],
        )
    );
    // **THE WORST FIRST**: currency nobody can count, knowable before launching.
    let unpriced = named_and_unpriced(prices, asked);
    if !unpriced.is_empty() {
        let _ = write!(
            said,
            "\n{}",
            catalogue::say("cli.flow.flow_models_unpriced", &[("models", &unpriced.join("; "))])
        );
    }
    let priced: Vec<String> = asked
        .named
        .keys()
        .filter(|name| prices.knows(name) == Known::Priced)
        .cloned()
        .collect();
    if !priced.is_empty() {
        let _ = write!(
            said,
            "\n{}",
            catalogue::say("cli.flow.flow_models_priced", &[("models", &priced.join(", "))])
        );
    }
    if !asked.unnamed.is_empty() {
        let steps: Vec<String> = asked
            .unnamed
            .iter()
            .map(|(step, engines)| format!("{step} ({})", joined(engines)))
            .collect();
        let _ = write!(
            said,
            "\n{}",
            catalogue::say(
                "cli.flow.steps_that_name_no_model",
                &[("steps", &steps.join(", "))]
            )
        );
    }
    let past_unpriced = past_models_into(&mut said, prices, seen);
    // **THE CAP LINE ONLY WHERE THE TWO THINGS MEET.** A cap with no uncovered
    // models has nothing to declare, and uncovered models with no cap stop
    // nothing: the coincidence is what is dangerous, and it is the sentence
    // fault 35 was written for — a brake that does not brake must be seen
    // before launching, not once the bill has arrived.
    if cap.is_some() && (past_unpriced || !unpriced.is_empty()) {
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

    // ── what the price list cannot price ─────────────────────────────────

    /// A price list knowing one model with its prices, and one entry declared
    /// half-way: enough to tell the three answers apart.
    fn a_small_price_list() -> PriceList {
        PriceList::parse(
            r#"{"currency":"USD","models":[
                {"id":"prezzato","input_per_million":5.0,"output_per_million":25.0},
                {"id":"a-meta","input_per_million":5.0}
            ]}"#,
        )
        .expect("the scratch price list reads")
    }

    /// A flow naming no model of its own: what every report looked like before
    /// a step could name one.
    fn naming_nothing() -> ModelsAskedByTheFlow {
        ModelsAskedByTheFlow::default()
    }

    /// **WHOEVER HAS NO PRICE FOR A MODEL MUST KNOW IT, AND KNOW WHICH.**
    ///
    /// The second half of the cure for fault 35. The first model is priced and
    /// must not appear; the other two are not, and must appear **with the
    /// why**, since they are repaired in two different ways.
    ///
    /// *Mutant run*: make `cannot_be_priced` return an empty list. The report
    /// falls silent again and this test goes red — which is exactly the defect:
    /// a zero in place of an answer.
    #[test]
    fn a_model_without_a_price_is_named_and_the_reason_with_it() {
        let said = what_is_priced(
            &a_small_price_list(),
            &naming_nothing(),
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

    /// **WHEN THEY ARE ALL PRICED IT SAYS SO ANYWAY.** A report that falls
    /// silent leaves the reader wondering whether the check looked at all — the
    /// same rule that puts the cap line there even where there is no cap.
    #[test]
    fn when_everything_is_priced_the_report_says_so_instead_of_falling_silent() {
        let said = what_is_priced(
            &a_small_price_list(),
            &naming_nothing(),
            Some(&names(&["prezzato"])),
            None,
        );

        assert!(said.contains("all priced"), "{said}");
        assert!(!said.contains("no entry"), "{said}");
    }

    /// **«NEVER RAN HERE» AND «I COULD NOT LOOK» ARE TWO DIFFERENT SENTENCES.**
    ///
    /// A ledger that will not open — absent, or refused by permissions — does
    /// not say the flow never ran: it says nobody could look. Confusing them
    /// prints «never run here» for a flow run a hundred times, the same rule
    /// by which a missing detector silences the report instead of declaring
    /// healthy what it has not seen.
    ///
    /// *Mutant run*: collapse the `None` branch onto the empty-set one. The two
    /// sentences become one and this test goes red.
    #[test]
    fn a_ledger_that_could_not_be_read_is_not_a_flow_that_never_ran() {
        let unreadable = what_is_priced(&a_small_price_list(), &naming_nothing(), None, None);
        let never_ran = what_is_priced(
            &a_small_price_list(),
            &naming_nothing(),
            Some(&BTreeSet::new()),
            None,
        );

        assert_ne!(unreadable, never_ran);
        assert!(unreadable.contains("could not be read"), "{unreadable}");
        assert!(!unreadable.contains("mai girato"), "{unreadable}");
    }

    /// **A FLOW THAT NEVER RAN HERE GETS A SENTENCE, NOT AN EMPTY LIST.**
    ///
    /// Saying «all priced» having seen nothing would be a reassurance built on
    /// air — the same distinction the detector keeps between «it is absent» and
    /// «I could not look».
    #[test]
    fn a_flow_that_never_ran_here_is_told_that_nothing_is_known_yet() {
        let said = what_is_priced(
            &a_small_price_list(),
            &naming_nothing(),
            Some(&BTreeSet::new()),
            None,
        );

        assert!(!said.contains("all priced"), "{said}");
        assert!(said.contains("has never run here"), "{said}");
    }

    /// A world where one tool is told a model and one is not, which is what
    /// the descriptors declare between an engine and a build command.
    struct WhereOneToolTakesNoModel;

    impl actions::ToolResolver for WhereOneToolTakesNoModel {
        fn resolve(&self, id: &str) -> Result<String, String> {
            Ok(id.to_owned())
        }

        fn model_option(&self, id: &str) -> Option<Vec<String>> {
            (id != "un-attrezzo").then(|| vec!["--model".to_owned()])
        }
    }

    /// Four steps: one names a model for the first engine of its chain and not
    /// for the second, one names none at all, one runs a build command through
    /// the same action, and one runs a shell check.
    fn a_flow_naming_one_model(model: &str) -> FlowFile {
        let json = format!(
            r#"{{
                "id": "prova", "description": "flusso di prova",
                "graph": {{"steps": [
                    {{"id": "nomina", "deps": [], "action": "external_engine",
                     "max_attempts": 1, "when": null,
                     "input_schema": {{"type": "any"}}, "output_schema": {{"type": "any"}},
                     "with": {{"tool": ["un-motore", "un-altro"],
                              "model": {{"un-motore": "{model}"}}, "timeout_secs": 10}}}},
                    {{"id": "tace", "deps": [], "action": "external_engine",
                     "max_attempts": 1, "when": null,
                     "input_schema": {{"type": "any"}}, "output_schema": {{"type": "any"}},
                     "with": {{"tool": "un-motore", "timeout_secs": 10}}}},
                    {{"id": "costruisce", "deps": [], "action": "external_engine",
                     "max_attempts": 1, "when": null,
                     "input_schema": {{"type": "any"}}, "output_schema": {{"type": "any"}},
                     "with": {{"tool": "un-attrezzo", "args": ["prova"], "timeout_secs": 10}}}},
                    {{"id": "esegue", "deps": [], "action": "shell_check",
                     "max_attempts": 1, "when": null,
                     "input_schema": {{"type": "any"}}, "output_schema": {{"type": "any"}},
                     "with": {{"tool": "un-attrezzo", "command": "true", "timeout_secs": 10}}}}
                ], "skippable_dependencies": []}},
                "inputs": {{}}
            }}"#
        );
        serde_json::from_str(&json).expect("it loads")
    }

    /// **A MODEL THE FLOW NAMES AND THE LIST CANNOT PRICE IS SAID FIRST.** It
    /// is currency nobody can count, and unlike the past runs it is a fact
    /// about the run ahead; the step naming it is where it is repaired.
    ///
    /// *Mutant run*: make `what_is_priced` ignore `asked`, which is the check
    /// that looked at the ledger alone.
    #[test]
    fn a_model_this_flow_names_and_the_list_cannot_price_is_said_first() {
        let asked = models_asked_by(
            &a_flow_naming_one_model("mai-visto"),
            &WhereOneToolTakesNoModel,
        );

        let said = what_is_priced(
            &a_small_price_list(),
            &asked,
            Some(&names(&["prezzato"])),
            None,
        );

        assert!(
            said.contains("mai-visto (no entry in the price list), named by nomina"),
            "{said}"
        );
        assert!(
            said.find("THIS FLOW NAMES") < said.find("all priced"),
            "the flow's own run comes before a reading of the past: {said}"
        );
    }

    /// **A NAMED MODEL THAT IS PRICED IS SAID TOO**, for the reason «all
    /// priced» is said: a report that falls silent leaves the reader wondering
    /// whether the check looked at all.
    #[test]
    fn a_model_this_flow_names_that_is_priced_is_said_to_be_priced() {
        let asked = models_asked_by(
            &a_flow_naming_one_model("prezzato"),
            &WhereOneToolTakesNoModel,
        );

        let said = what_is_priced(&a_small_price_list(), &asked, Some(&BTreeSet::new()), None);

        assert!(said.contains("this flow names, all priced: prezzato"), "{said}");
        assert!(!said.contains("THIS FLOW NAMES"), "{said}");
    }

    /// **A STEP THAT NAMES NO MODEL IS SAID BY NAME.** The default is picked
    /// by whoever answers and changes underneath — fault 104. The list is what
    /// a `model` repairs, uncovered engines of a chain included.
    ///
    /// *Mutant run*: make `what_is_priced` ignore `asked`.
    #[test]
    fn a_step_that_names_no_model_is_named_because_nobody_knows_who_answers() {
        let asked = models_asked_by(
            &a_flow_naming_one_model("prezzato"),
            &WhereOneToolTakesNoModel,
        );

        let said = what_is_priced(&a_small_price_list(), &asked, Some(&BTreeSet::new()), None);

        assert!(
            said.contains("nomina (un-altro), tace (un-motore)"),
            "both the uncovered engine of a chain and the silent step: {said}"
        );
        assert!(
            said.contains("no telling which model will answer"),
            "{said}"
        );
        assert!(
            !said.contains("esegue") && !said.contains("costruisce"),
            "a step running a command cannot be told a model: {said}"
        );
    }

    /// **THE PAST RUNS ARE A READING, NEVER A PROMISE.** «All priced» read as
    /// a guarantee about the run ahead is fault 104 word for word: the check
    /// was green and the model that answered had no entry in the list.
    ///
    /// *Mutant run*: put the old sentence back, «models used by past runs: all
    /// priced ({models})».
    #[test]
    fn the_models_of_past_runs_are_offered_as_a_reading_and_not_as_a_promise() {
        let said = what_is_priced(
            &a_small_price_list(),
            &naming_nothing(),
            Some(&names(&["prezzato"])),
            None,
        );

        assert!(
            said.contains("never a promise about which model will answer next"),
            "{said}"
        );
    }

    /// **A CAP THAT CANNOT FIRE MUST BE SEEN BEFORE LAUNCHING.**
    ///
    /// The sentence fault 35 was written for: the cap is measured on known
    /// costs, so a model with no price makes it wider than it says — and
    /// whoever launches finds out once the bill arrives. The line appears where
    /// both conditions hold, because their coincidence is the danger.
    ///
    /// *Mutant run*: drop the branch that looks at `cap` and print the sentence
    /// always. The third arm — a flow with no cap — goes red.
    #[test]
    fn a_cap_that_cannot_fire_is_declared_before_the_run_not_after() {
        let unpriced = names(&["mai-visto"]);
        let priced = names(&["prezzato"]);

        let with_cap = what_is_priced(
            &a_small_price_list(),
            &naming_nothing(),
            Some(&unpriced),
            Some(5_000_000),
        );
        assert!(with_cap.contains("the spend cap"), "{with_cap}");

        let all_priced = what_is_priced(
            &a_small_price_list(),
            &naming_nothing(),
            Some(&priced),
            Some(5_000_000),
        );
        assert!(
            !all_priced.contains("the spend cap"),
            "with no uncovered models the cap has nothing to declare: {all_priced}"
        );

        let no_cap = what_is_priced(&a_small_price_list(), &naming_nothing(), Some(&unpriced), None);
        assert!(
            !no_cap.contains("the spend cap"),
            "a flow with no cap has no cap to warn about: {no_cap}"
        );
    }

    /// A recorded call: the model that answered, its tokens, and whether a
    /// cost was computed.
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
            session_mode: None,
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

    /// **`flow cost` NAMES THE UNCOVERED MODEL, NOT MERELY HOW MANY CALLS.**
    ///
    /// «One call with no known cost» is a number nobody can act on; the model's
    /// name is a line to write in the price list. The run has two calls — one
    /// priced, one not — and the second alone must appear: naming both would
    /// send somebody to correct an entry that is already there.
    ///
    /// *Mutant run*: make `cannot_be_priced` return an empty list. The report
    /// goes back to saying «1 with no known cost» and this test goes red.
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
            "a priced model is not reported: {said}"
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

    /// **THE REPORT SAYS WHICH IDENTITY EACH PROCESS STARTED WITH.**
    ///
    /// The data was written in the ledger, reread inside `CallView`, and
    /// reached **no** screen and no command — searched in `crates/ui`,
    /// `desktop/src` and `crates/sailor`. Data collected but never looked at is
    /// one step from wrong data nobody notices, and this is where a person
    /// looks when something went wrong. **AND THE HOME PATH BELONGS HERE**: a
    /// profile name says under which label it ran, a path says where to look.
    ///
    /// *Mutant run*: take the block writing «identity:» out of
    /// `spending_report`. This one goes red and no other.
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
            "the case where the identity was changed on purpose is the one that must show: {said}"
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

    /// The twin: where everything is priced the line does not appear. Without
    /// it a mutant printing it always would pass the test above.
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

    /// **A RUN'S TOTAL IS WHAT ITS CALLS COST, NOT A ZERO.** The defect this
    /// test exists to catch lived in silence until the cost of calls became
    /// real: `record_run` wrote `total_cost_micros: 0` by hand, and the window
    /// showed that zero beside the right sum computed elsewhere.
    #[test]
    fn a_runs_total_is_what_its_calls_cost() {
        let directory = TestDirectory::new();
        let ledger = Ledger::open(&directory.0).expect("the ledger opens");
        let flow: FlowFile = serde_json::from_str(&flow_json("shell_check", "[]", "{}"))
            .expect("it loads");
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
        .expect("recording the run");

        let dump = ledger.projection_dump().expect("reading the projection");
        let run = dump["runs"]
            .as_array()
            .expect("the list of runs is there")
            .iter()
            .find(|row| row[0] == "corsa-costosa")
            .expect("the recorded run is found again");
        assert_eq!(
            run[6],
            serde_json::json!(100_000),
            "the total is the sum of the two calls, not a zero written by hand"
        );
    }

    /// A call that already cost something, to measure a run's total.
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
            session_mode: None,
        }
    }

    // ── the total that holds an unknown ──────────────────────────────────

    /// A call as the ledger keeps it, with the fields this count looks at.
    /// `cost` at `None` is an **unmeasured** call: the shape
    /// `sailor step close --turns` writes for a handed step.
    ///
    /// **THE NAME SAYS WHAT TELLS IT APART.** This one and its sister above
    /// were both called `a_call`, born on two branches; git merged them without
    /// a word — no line in common — and `cargo` refused the tree. Fault 36 of
    /// `docs/faults-encountered.md` repeating: a boundary drawn on files does
    /// not see names living in the same module.
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
            session_mode: None,
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

    /// **THE PRICE LIST KNOWS THE MODEL OF THESE TESTS, DELIBERATELY.** One
    /// thing is looked at here: the shape of the total when a call is not
    /// measured. With a list that did not know `m`, every report would also
    /// carry the uncovered-models line — a second reason for a gap, written
    /// over the first — and a red test would no longer say which of the two
    /// broke.
    fn report_for(calls: &[ledger::ModelCallRecord]) -> String {
        let prices = PriceList::parse(
            r#"{"currency":"USD","models":[
                {"id":"m","input_per_million":5.0,"output_per_million":25.0}
            ]}"#,
        )
        .expect("the scratch price list reads");
        let view = ui::dashboard::summarize_run(&a_run(), &[], calls, 100);
        spending_report(&view, &prices)
    }

    /// The bare figure, as a complete total would write it. Appearing in a
    /// partial report, it hands the reader a number that is not the total.
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

    /// **A TOTAL THAT HOLDS AN UNKNOWN IS NOT A TOTAL.**
    ///
    /// Fault 37 measured: the handed run of the A/B printed `1,6674` while it
    /// had cost `7,2080` — 4,3 times — because three calls out of four had no
    /// cost and the note saying so sat **below** the number. Whoever reads a
    /// total reads the number, not the note. Here the note takes the number's
    /// place: the bare figure must exist nowhere in the report.
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
            "the bare figure must not appear: it reads as the real total.\n{report}"
        );
        assert!(
            report.contains(&floored_total(1_667_400, 4, 3)),
            "the number must read as a floor, with what is missing beside the figure.\n{report}"
        );
    }

    /// **AND WHERE EVERYTHING IS KNOWN THE NUMBER STAYS BARE.** Without this
    /// half the warning is worth nothing: a report always declaring itself
    /// incomplete no longer tells the two cases apart — the same defect flipped.
    #[test]
    fn a_total_where_every_call_is_measured_stays_a_plain_figure() {
        let report = report_for(&[
            a_call_named("una", Some(1_000_000)),
            a_call_named("due", Some(667_400)),
        ]);

        assert!(
            report.contains(&bare_total(1_667_400)),
            "everything measured: the sum is the sum.\n{report}"
        );
        assert!(
            !report.contains("at least"),
            "no floors where nothing is missing.\n{report}"
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
        let ledger = Ledger::open(&directory.0).expect("the ledger opens");
        let flow: FlowFile = serde_json::from_str(&flow_json("shell_check", "[]", "{}"))
            .expect("it loads");
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
        .expect("recording the run");

        let report = cost_of_in(&directory.0, "prova").expect("the report is written");

        assert!(report.contains(&floored_total(1_667_400, 2, 1)), "{report}");
        assert!(!report.contains(&bare_total(1_667_400)), "{report}");
        assert!(
            the_line_naming(&report, "build").contains("self-declared"),
            "{report}"
        );
    }

    /// **NO MEASURED CALL IS NOT «AT LEAST ZERO».** It is the third case of
    /// `Spend`, the one an `Option` would collapse: «at least 0,0000» is true
    /// and says nothing, and its reader believes they saw a small spend.
    #[test]
    fn a_run_where_nothing_is_measured_says_unknown_instead_of_at_least_zero() {
        let report = report_for(&[a_call_named("consegnata", None)]);

        assert!(
            report.contains(&catalogue::say("ui.cost.unknown", &[("calls", "1")])),
            "with not one measure there is no floor to declare.\n{report}"
        );
        assert!(
            !report.contains(&bare_total(0)),
            "and above all there is no zero.\n{report}"
        );
    }
}
