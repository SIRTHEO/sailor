//! What a run of a flow costs: the seed committed in the tree, and the gesture
//! that reads the ledger to say what the seed should be.
//!
//! **THE SPLIT IS THE WHOLE POINT.** A judge runs on a clean archive and reads
//! no state of this machine, so it can never ask the ledger. The number lives
//! in a file instead, and this is what tells a person which one to write.

use flow::FlowFile;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::default_ledger_dir;

/// Where the seeds are written, at the root of the tree: above both homes of
/// flows, because it holds rows for flows from each of them.
pub const SEED_FILE: &str = "token-seeds.json";

/// The two homes of a flow file, the same pair the battery counts.
pub const WHERE_FLOWS_LIVE: &[&str] = &["flows", "crates/flow/system"];

pub const FLOW_SUFFIX: &str = ".flow.json";

/// How many costed runs before a seed is worth writing.
///
/// Three, taken from the spending cap and for its reason: with two samples the
/// worst observed is one of the only two values there are, and calling it a
/// measure invents a number with the face of one.
pub const RUNS_BEFORE_A_SEED_IS_WORTH_WRITING: usize = 3;

/// What one run of a flow costs, as the tree declares it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seed {
    /// The tokens one run spends, read off the ledger by a person and written
    /// here. Zero means nobody has measured it, and `runs_measured` says so.
    pub tokens_a_run: u64,
    /// How many costed runs that number came from. **Zero is a declared
    /// unknown, never a hole**: a flow with no row at all is the hole, and the
    /// judge names it.
    pub runs_measured: usize,
    /// The characters of prose one run hands to engines, at most.
    ///
    /// **THIS IS WHAT BINDS THE TOKEN NUMBER TO A VERSION OF THE FLOW.** The
    /// judge recomputes it from the flow's own steps: change what the flow
    /// sends and this stops matching, so the tokens have to be measured again
    /// in the commit that changed them.
    pub words_it_sends: usize,
}

/// The seeds of every flow the repository carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seeds {
    /// How far above its seed a run may go before this command calls it a
    /// rise. Runs vary; a margin of zero would report every flow every day.
    pub margin_percent: u64,
    pub flows: BTreeMap<String, Seed>,
}

pub fn read_seeds(root: &Path) -> Result<Seeds, String> {
    let path = root.join(SEED_FILE);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// Every flow file the repository carries, by name, sorted.
pub fn flows_in(root: &Path) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = WHERE_FLOWS_LIVE
        .iter()
        .flat_map(|place| std::fs::read_dir(root.join(place)).into_iter().flatten())
        .flatten()
        .map(|entry| entry.path())
        .filter_map(|path| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            let stem = name.strip_suffix(FLOW_SUFFIX)?;
            path.is_file().then(|| (stem.to_owned(), path.clone()))
        })
        .collect();
    found.sort();
    found
}

/// The prose one run of this flow hands to engines, at most.
///
/// Counted only on steps that name an engine, because a step that names none
/// spends nothing, and multiplied by the attempts the step is allowed: a retry
/// really does send the words again.
pub fn words_it_sends(text: &str) -> Result<usize, String> {
    let flow: FlowFile = serde_json::from_str(text).map_err(|error| error.to_string())?;
    Ok(flow
        .graph
        .steps()
        .iter()
        .filter_map(|step| step.with.as_ref().map(|with| (step, with)))
        .filter(|(_, with)| !actions::engines_named_in(with).is_empty())
        .map(|(step, with)| prose_in(with) * step.max_attempts as usize)
        .sum())
}

/// The characters of text a value carries, keys excluded.
///
/// A reference brings its words from another step and is worth nothing here:
/// what it will hold is not in the file. The literal halves of a joined value
/// are in the file, and count.
fn prose_in(value: &Value) -> usize {
    match value {
        Value::String(text) => text.chars().count(),
        Value::Array(items) => items.iter().map(prose_in).sum(),
        Value::Object(fields) if fields.contains_key(flow::reference::FROM_KEY) => 0,
        Value::Object(fields) => fields.values().map(prose_in).sum(),
        _ => 0,
    }
}

/// What the ledger saw one flow's runs cost.
struct Observed {
    runs: usize,
    /// Those that declared tokens: the only ones a seed can be read off.
    costed_runs: usize,
    worst_run: u64,
    total: u64,
}

impl Observed {
    fn mean(&self) -> u64 {
        match self.costed_runs {
            0 => 0,
            costed => self.total / costed as u64,
        }
    }
}

/// Every token an engine declared for these calls.
///
/// **THE SUM GOES THROUGH THE ONE THAT ALREADY EXISTS.** `TokenTotals` keeps
/// the sides apart so a total is never counted twice, and a second reading
/// here would agree with it until the day it did not.
fn tokens_of(totals: &ui::dashboard::TokenTotals) -> u64 {
    totals.input_tokens
        + totals.output_tokens
        + totals.cached_tokens
        + totals.cache_write_tokens
        + totals.total_tokens_only
}

fn observed(data: &ui::gather::GatheredData, flow: &str, now: i64) -> Observed {
    let mut seen = Observed {
        runs: 0,
        costed_runs: 0,
        worst_run: 0,
        total: 0,
    };
    for run in data.runs.iter().filter(|run| run.entity == flow) {
        seen.runs += 1;
        let view = ui::dashboard::summarize_run(
            run,
            data.steps_by_run.get(&run.run_id).map_or(&[], Vec::as_slice),
            data.calls_by_run.get(&run.run_id).map_or(&[], Vec::as_slice),
            now,
        );
        let spent = tokens_of(&view.tokens);
        if spent == 0 {
            continue;
        }
        seen.costed_runs += 1;
        seen.total += spent;
        seen.worst_run = seen.worst_run.max(spent);
    }
    seen
}

fn over_margin(worst: u64, seed: u64, margin_percent: u64) -> bool {
    worst > seed.saturating_mul(100 + margin_percent) / 100
}

/// The row to write, whole, so nobody assembles it by hand from three numbers.
fn row_to_write(flow: &str, seed: &Seed, tokens: u64, runs: usize) -> String {
    format!(
        "      \"{flow}\": {{ \"tokens_a_run\": {tokens}, \"runs_measured\": {runs}, \
         \"words_it_sends\": {} }}",
        seed.words_it_sends
    )
}

pub(super) fn seeds_report() -> Result<String, String> {
    let root = crate::ratchet_cmd::root_to_measure()?;
    seeds_report_in(&default_ledger_dir()?, &root, super::now_secs()?)
}

/// The same report over a declared ledger and tree, so a test hands in a
/// scratch pair instead of asking this machine what it happens to hold.
fn seeds_report_in(ledger_dir: &Path, root: &Path, now: i64) -> Result<String, String> {
    let seeds = read_seeds(root)?;
    let Some(data) = ui::gather::gather(ledger_dir).map_err(|error| error.to_string())? else {
        return Err(catalogue::say(
            "cli.flow.no_store_here",
            &[("path", &ledger_dir.display().to_string())],
        ));
    };
    let mut report = catalogue::say(
        "cli.flow.seeds_heading",
        &[
            ("file", SEED_FILE),
            ("margin", &seeds.margin_percent.to_string()),
        ],
    );
    let mut risen = Vec::new();
    for (flow, seed) in &seeds.flows {
        let seen = observed(&data, flow, now);
        let _ = write!(
            report,
            "\n{}",
            catalogue::say(
                "cli.flow.seed_row",
                &[
                    ("flow", flow),
                    ("seed", &seed.tokens_a_run.to_string()),
                    ("runs", &seen.runs.to_string()),
                    ("costed", &seen.costed_runs.to_string()),
                    ("worst", &seen.worst_run.to_string()),
                    ("mean", &seen.mean().to_string()),
                ],
            )
        );
        if seen.costed_runs < RUNS_BEFORE_A_SEED_IS_WORTH_WRITING {
            let _ = write!(
                report,
                "\n{}",
                catalogue::say(
                    "cli.flow.seed_too_few_runs",
                    &[
                        ("costed", &seen.costed_runs.to_string()),
                        ("wanted", &RUNS_BEFORE_A_SEED_IS_WORTH_WRITING.to_string()),
                    ],
                )
            );
            continue;
        }
        if over_margin(seen.worst_run, seed.tokens_a_run, seeds.margin_percent) {
            risen.push(flow.clone());
            let _ = write!(
                report,
                "\n{}\n{}",
                catalogue::say("cli.flow.seed_over_margin", &[("flow", flow)]),
                row_to_write(flow, seed, seen.worst_run, seen.costed_runs)
            );
            continue;
        }
        let _ = write!(
            report,
            "\n{}\n{}",
            catalogue::say("cli.flow.seed_may_fall", &[]),
            row_to_write(flow, seed, seen.worst_run, seen.costed_runs)
        );
    }
    if risen.is_empty() {
        return Ok(report);
    }
    Err(format!(
        "{report}\n{}",
        catalogue::say(
            "cli.flow.seeds_have_risen",
            &[("flows", &risen.join(", "))],
        )
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A step that names an engine sends its prose as many times as it may be
    /// attempted; a step that names none sends nothing, whatever it carries.
    #[test]
    fn only_the_steps_that_name_an_engine_are_counted_and_a_retry_counts_twice() {
        let text = r#"{
            "id": "due-passi",
            "description": "one paid, one not",
            "graph": {"steps": [
                {"id": "libero", "deps": [], "input_schema": {"type": "any"},
                 "output_schema": {"type": "any"}, "when": null, "action": "shell_check",
                 "max_attempts": 1, "with": {"command": "0123456789"}},
                {"id": "pagato", "deps": [], "input_schema": {"type": "any"},
                 "output_schema": {"type": "any"}, "when": null, "action": "external_engine",
                 "max_attempts": 2, "with": {"tool": "ab", "stdin": "012345"}}
            ]},
            "inputs": {}
        }"#;

        assert_eq!(words_it_sends(text).expect("a flow that loads"), 16);
    }

    /// What a reference will hold is not in the file, so it weighs nothing;
    /// the literal half of a joined value is in the file, and weighs.
    #[test]
    fn a_reference_weighs_nothing_and_the_words_beside_it_weigh() {
        let joined = serde_json::json!({
            "tool": "",
            "stdin": {"$join": ["01234", {"$from": "/a-very-long-pointer"}]},
            "data": {"$from": "/another-long-one"}
        });

        assert_eq!(prose_in(&joined), 5);
    }

    /// The margin is a band above the seed, not a second seed: a run inside it
    /// is the same run, and one above it is a rise.
    #[test]
    fn a_run_inside_the_margin_is_not_a_rise_and_one_above_it_is() {
        assert!(!over_margin(125, 100, 25));
        assert!(over_margin(126, 100, 25));
        assert!(over_margin(1, 0, 25), "a flow seeded at zero rises on its first token");
    }

    fn a_costed_run(flow: &str, nth: usize, tokens: u64) -> (ledger::RunRecord, ledger::ModelCallRecord) {
        let run_id = format!("{flow}-{nth}");
        let call = ledger::ModelCallRecord {
            call_id: format!("{run_id}:call"),
            run_id: run_id.clone(),
            step_id: Some("ask".to_owned()),
            purpose: "external_engine".to_owned(),
            cli: "una-riga".to_owned(),
            requested_model: "m".to_owned(),
            actual_model: "m".to_owned(),
            input_tokens: Some(tokens),
            output_tokens: Some(0),
            cached_tokens: Some(0),
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: Some(1),
            cost_micros: Some(1),
            declared_cost_micros: None,
            price_currency: None,
            input_price_micros_per_million: None,
            output_price_micros_per_million: None,
            cached_price_micros_per_million: None,
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            engine_identity: ledger::EngineIdentity::default(),
            retry_chain: vec![],
            fell_back_from: vec![],
            error_type: None,
            started_at: 0,
            ended_at: Some(1),
            session_id: None,
            work_kind: None,
        };
        let run = ledger::RunRecord {
            run_id,
            kind: "flow".to_owned(),
            entity: flow.to_owned(),
            parent_run_id: None,
            started_by: "a test".to_owned(),
            status: "succeeded".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: 0,
            ended_at: Some(1),
            worktree: None,
            stop_reason: None,
        };
        (run, call)
    }

    /// The whole road on a scratch pair: a tree holding one seed and a ledger
    /// holding runs that went past it. What this command does with a rise is
    /// say so and exit red — it never rewrites the file, because the number
    /// carries a reason and only a person has it.
    #[test]
    fn a_ledger_that_saw_more_than_the_seed_declares_says_which_row_to_write() {
        let tree = super::super::test_support::TestDirectory::new();
        let store = super::super::test_support::TestDirectory::new();
        tree.write(
            SEED_FILE,
            r#"{"margin_percent": 25, "flows": {
                 "un-flusso": {"tokens_a_run": 100, "runs_measured": 3, "words_it_sends": 7}}}"#,
        );
        let ledger = ledger::Ledger::open(&store.0).expect("a scratch store");
        for (nth, tokens) in [(1, 100u64), (2, 120), (3, 400)] {
            let (run, call) = a_costed_run("un-flusso", nth, tokens);
            ledger.record_model_call(&call).expect("the call");
            ledger.record_run(&run).expect("the run");
        }

        let said = seeds_report_in(&store.0, &tree.0, 10).expect_err("a rise is red");

        assert!(said.contains("un-flusso"), "{said}");
        assert!(
            said.contains("\"tokens_a_run\": 400, \"runs_measured\": 3, \"words_it_sends\": 7"),
            "the whole row to write, not three numbers to assemble.\n{said}"
        );
    }

    /// Under the threshold the command refuses to suggest, and the refusal is
    /// not a rise: two runs is not a measurement, whatever the two say.
    #[test]
    fn too_few_costed_runs_suggests_nothing_and_is_not_a_rise() {
        let tree = super::super::test_support::TestDirectory::new();
        let store = super::super::test_support::TestDirectory::new();
        tree.write(
            SEED_FILE,
            r#"{"margin_percent": 25, "flows": {
                 "un-flusso": {"tokens_a_run": 0, "runs_measured": 0, "words_it_sends": 7}}}"#,
        );
        let ledger = ledger::Ledger::open(&store.0).expect("a scratch store");
        for (nth, tokens) in [(1, 900u64), (2, 900)] {
            let (run, call) = a_costed_run("un-flusso", nth, tokens);
            ledger.record_model_call(&call).expect("the call");
            ledger.record_run(&run).expect("the run");
        }

        let said = seeds_report_in(&store.0, &tree.0, 10).expect("no rise to declare");

        assert!(said.contains("too few"), "{said}");
        assert!(!said.contains("\"tokens_a_run\": 900"), "nothing to write yet.\n{said}");
    }
}
