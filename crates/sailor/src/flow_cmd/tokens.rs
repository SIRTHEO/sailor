//! `sailor flow tokens`: how a flow's recent runs spent tokens, step by step —
//! what was read fresh, read from the cache, written to it and produced — in
//! tokens and ratios, with no currency in it.

use ledger::ModelCallRecord;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

const RUNS_BY_DEFAULT: usize = 10;

pub(super) fn tokens_of(flow: &str, runs: Option<&str>) -> Result<String, String> {
    let runs = match runs {
        None => RUNS_BY_DEFAULT,
        Some(written) => written
            .parse::<usize>()
            .ok()
            .filter(|count| *count > 0)
            .ok_or_else(|| catalogue::say("cli.flow.tokens.runs_not_a_count", &[("runs", written)]))?,
    };
    let list = actions::current_price_list();
    tokens_of_in(&super::default_ledger_dir()?, flow, runs, |cli| {
        list.engine(cli).is_some_and(|rules| rules.input_holds_cached)
    })
}

fn tokens_of_in(
    dir: &Path,
    flow: &str,
    runs: usize,
    input_holds_cached: impl Fn(&str) -> bool,
) -> Result<String, String> {
    let Some(data) = ui::gather::gather(dir).map_err(|error| error.to_string())? else {
        return Err(catalogue::say("cli.flow.no_store_here", &[("path", &dir.display().to_string())]));
    };
    let mut chosen: Vec<_> = data.runs.iter().filter(|run| run.entity == flow).collect();
    if chosen.is_empty() {
        return Err(catalogue::say("cli.flow.never_run_here", &[("flow", flow)]));
    }
    chosen.sort_by_key(|run| std::cmp::Reverse(run.started_at));
    chosen.truncate(runs);
    let calls: Vec<&ModelCallRecord> = chosen
        .iter()
        .flat_map(|run| data.calls_by_run.get(&run.run_id).into_iter().flatten())
        .collect();
    Ok(report(flow, chosen.len(), &calls, input_holds_cached))
}

/// What a set of calls read and wrote. A call that reported no counts is
/// counted apart, and every sum is then a floor.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Spent {
    calls: u64,
    without_counts: u64,
    uncached: u64,
    cache_read: u64,
    cache_written: u64,
    output: u64,
}

impl Spent {
    fn add(&mut self, call: &ModelCallRecord, input_holds_cached: bool) {
        self.calls += 1;
        let (Some(input), Some(output)) = (call.input_tokens, call.output_tokens) else {
            self.without_counts += 1;
            return;
        };
        let read = call.cached_tokens.unwrap_or(0);
        self.uncached += if input_holds_cached { input.saturating_sub(read) } else { input };
        self.cache_read += read;
        self.cache_written += call.cache_write_tokens.unwrap_or(0) + call.cache_write_long_tokens.unwrap_or(0);
        self.output += output;
    }

    fn input(&self) -> u64 {
        self.uncached + self.cache_read + self.cache_written
    }

    fn row(&self, label: &str) -> String {
        let share = |part: u64, whole: u64| {
            if whole == 0 { "-".to_owned() } else { format!("{:.1}%", 100.0 * part as f64 / whole as f64) }
        };
        let rereads = if self.cache_written == 0 {
            "-".to_owned()
        } else {
            format!("{:.1}x", self.cache_read as f64 / self.cache_written as f64)
        };
        let per_call = if self.calls > self.without_counts {
            (self.input() / (self.calls - self.without_counts)).to_string()
        } else {
            "-".to_owned()
        };
        format!(
            "{label}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{rereads}",
            self.calls,
            self.input(),
            per_call,
            self.uncached,
            self.cache_read,
            self.cache_written,
            self.output,
            share(self.cache_read, self.input()),
        )
    }
}

fn report(flow: &str, runs: usize, calls: &[&ModelCallRecord], input_holds_cached: impl Fn(&str) -> bool) -> String {
    let mut by_step: BTreeMap<String, Spent> = BTreeMap::new();
    let mut total = Spent::default();
    for call in calls {
        let holds = input_holds_cached(&call.cli);
        by_step.entry(call.step_id.clone().unwrap_or_default()).or_default().add(call, holds);
        total.add(call, holds);
    }
    let mut said = catalogue::say("cli.flow.tokens.heading", &[("flow", flow), ("runs", &runs.to_string())]);
    let _ = write!(said, "\n{}", catalogue::say("cli.flow.tokens.columns", &[]));
    for (step, spent) in &by_step {
        let _ = write!(said, "\n{}", spent.row(step));
    }
    let _ = write!(said, "\n{}", total.row(&catalogue::say("cli.flow.tokens.total", &[])));
    if runs > 0 {
        let _ = write!(
            said,
            "\n{}",
            catalogue::say("cli.flow.tokens.per_run", &[("input", &(total.input() / runs as u64).to_string()), ("output", &(total.output / runs as u64).to_string())])
        );
    }
    if total.without_counts > 0 {
        let _ = write!(
            said,
            "\n{}",
            catalogue::say("cli.flow.tokens.floor", &[("count", &total.without_counts.to_string())])
        );
    }
    said
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(step: &str, cli: &str, input: u64, cached: u64, written: u64, output: u64) -> ModelCallRecord {
        ModelCallRecord {
            call_id: format!("{step}-{cli}-{input}"),
            run_id: "run".to_owned(),
            step_id: Some(step.to_owned()),
            purpose: "external_engine".to_owned(),
            cli: cli.to_owned(),
            requested_model: String::new(),
            actual_model: String::new(),
            input_tokens: Some(input),
            output_tokens: Some(output),
            cached_tokens: Some(cached),
            cache_write_tokens: Some(written),
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
            engine_identity: ledger::EngineIdentity::default(),
            retry_chain: Vec::new(),
            error_type: None,
            started_at: 0,
            ended_at: None,
            session_id: None,
            work_kind: None,
            fell_back_from: Vec::new(),
            session_mode: None,
            role: None,
            role_resolved_to: Vec::new(),
        }
    }

    /// One engine counts its cache inside the input and the other apart; the
    /// same work must read the same in both, and the cache ratios are taken
    /// over everything read.
    #[test]
    fn tokens_are_counted_the_same_whether_the_engine_holds_its_cache_inside_the_input_or_not() {
        let apart = call("plan", "engine-apart", 100, 900, 300, 50);
        let inside = call("plan", "engine-inside", 1000, 900, 300, 50);
        let holds = |cli: &str| cli == "engine-inside";

        let said = report("a-flow", 2, &[&apart, &inside], holds);

        let row = said.lines().find(|line| line.starts_with("plan\t")).expect("a row for the step");
        assert_eq!(row, "plan\t2\t2600\t1300\t200\t1800\t600\t100\t69.2%\t3.0x", "{said}");
    }

    #[test]
    fn a_call_without_counts_makes_every_sum_a_floor() {
        let mut silent = call("plan", "engine-apart", 0, 0, 0, 0);
        silent.input_tokens = None;

        let said = report("a-flow", 1, &[&silent], |_| false);

        assert!(said.contains(&catalogue::say("cli.flow.tokens.floor", &[("count", "1")])), "{said}");
    }
}
