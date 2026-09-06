//! What the next call can be held to, and what that makes of a spend cap.
//!
//! A cap is a **guaranteed cap** only when every call it covers can be bounded
//! before it starts: an enforceable per-call ceiling, and a tariff to price it
//! with. Otherwise the cost is known only afterwards, and its honest name is a
//! **stop threshold**. Decided here from those facts, never from a word.

use models::pricing::{PriceMicros, TokenCounts};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// The capability a descriptor declares a per-call ceiling under.
pub const NATIVE_SPEND_CAP: &str = "native_spend_cap";

/// The unit a ceiling in currency is written in: whole units, as the engines
/// measured take it (`--max-budget-usd`), converted to micros here.
pub const UNIT_CURRENCY: &str = "usd";

/// The unit a ceiling in tokens is written in. It is a reserve only once the
/// price list carries a tariff for every category the ceiling counts.
pub const UNIT_TOKENS: &str = "tokens";

/// A million micro-units make one unit of currency.
const MICROS_PER_UNIT: i64 = 1_000_000;

/// How the engine takes a ceiling: the options its name is written after, and
/// the unit it is written in. Both come from the descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CeilingOption {
    pub args: Vec<String>,
    pub unit: String,
}

/// The ceiling one call is held to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ceiling {
    /// Currency the engine imposes on itself, in micro-units.
    Currency(i64),
    /// Tokens, which must be priced before they are a reserve.
    Tokens(TokenCounts),
}

impl Ceiling {
    /// The value written on the command line, in the unit the engine takes.
    pub fn as_written(&self) -> Option<String> {
        match self {
            Ceiling::Currency(micros) => {
                Some(format!("{:.6}", *micros as f64 / MICROS_PER_UNIT as f64))
            }
            Ceiling::Tokens(counts) => counts.output.map(|tokens| tokens.to_string()),
        }
    }
}

/// The most the next call can cost.
///
/// **`Unknown` IS NEVER ZERO.** A missing tariff read as zero would let a cap
/// call itself guaranteed over a call nobody can bound, which is the defect this
/// tree names «a figure that looks like a measurement and is not».
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reserve {
    Known(i64),
    Unknown(String),
}

impl Reserve {
    pub fn micros(&self) -> Option<i64> {
        match self {
            Reserve::Known(micros) => Some(*micros),
            Reserve::Unknown(_) => None,
        }
    }

    pub fn why(&self) -> Option<&str> {
        match self {
            Reserve::Known(_) => None,
            Reserve::Unknown(why) => Some(why),
        }
    }
}

/// The name each usage category is priced under, for a reserve that must say
/// which tariff it went looking for.
const CATEGORIES: [&str; 5] = [
    "input",
    "output",
    "cached",
    "cache_write",
    "cache_write_long",
];

/// The reserve a ceiling makes, given the tariffs of the model it will run on.
pub fn reserve_of(ceiling: &Ceiling, prices: &PriceMicros) -> Reserve {
    let counts = match ceiling {
        Ceiling::Currency(micros) => return Reserve::Known(*micros),
        Ceiling::Tokens(counts) => counts,
    };
    let per_category = [
        (counts.input, prices.input),
        (counts.output, prices.output),
        (counts.cached, prices.cached),
        (counts.cache_write, prices.cache_write),
        (counts.cache_write_long, prices.cache_write_long),
    ];
    let mut total: i128 = 0;
    let mut counted = 0;
    for (name, (tokens, price)) in CATEGORIES.iter().zip(per_category) {
        let Some(tokens) = tokens else { continue };
        // The ceiling counts this category and the list does not price it:
        // leaving it out would lower the reserve by an amount nobody sees.
        let Some(price) = price else {
            return Reserve::Unknown(format!("the price list carries no tariff for «{name}»"));
        };
        total += i128::from(tokens) * i128::from(price);
        counted += 1;
    }
    if counted == 0 {
        return Reserve::Unknown("the ceiling counts no tokens".to_owned());
    }
    match i64::try_from((total + 500_000) / MICROS_PER_UNIT as i128) {
        Ok(micros) => Reserve::Known(micros),
        Err(_) => Reserve::Unknown("the ceiling priced out beyond what a sum holds".to_owned()),
    }
}

/// The ceiling a step declares, in each unit an engine may take one in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Declared {
    pub max_spend_micros: Option<i64>,
    pub max_tokens: Option<TokenCounts>,
}

impl Declared {
    pub fn is_empty(&self) -> bool {
        self.max_spend_micros.is_none() && self.max_tokens.is_none()
    }
}

/// The ceiling a step's declared maximum makes for an engine that takes one.
///
/// `None` when the engine declares no way to be told a ceiling, or when the step
/// declares nothing in the unit that engine takes: both mean no reserve, and
/// both are said rather than guessed at.
pub fn ceiling_for(option: &CeilingOption, declared: &Declared) -> Option<Ceiling> {
    match option.unit.as_str() {
        UNIT_CURRENCY => declared.max_spend_micros.map(Ceiling::Currency),
        UNIT_TOKENS => declared.max_tokens.map(Ceiling::Tokens),
        _ => None,
    }
}

/// Why a step cannot be reserved, when it cannot.
pub fn why_no_ceiling(option: Option<&CeilingOption>, declared: &Declared) -> String {
    match option {
        None => format!(
            "its engine declares no `capabilities.{NATIVE_SPEND_CAP}` a ceiling is written after"
        ),
        Some(option) if declared.is_empty() => format!(
            "the step declares no ceiling for an engine that takes one in «{}»",
            option.unit
        ),
        Some(option) => format!(
            "the step declares no ceiling in «{}», the unit its engine takes one in",
            option.unit
        ),
    }
}

// ── what a cap is, over a whole run ──────────────────────────────────────

/// What a declared cap is, as a fact about the run it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapKind {
    /// Every call is bounded and priced before it starts: the cap holds.
    Guaranteed,
    /// The cap stops the next call, not the overshoot of the current one.
    StopThreshold,
}

/// A step, as far as a cap is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepFact {
    pub step: String,
    /// This step hands the work to an agent, which can start calls outside
    /// this control.
    pub handed_to_agent: bool,
    /// The reserve its call can be held to; `None` when it starts no call.
    pub reserve: Option<Reserve>,
}

/// The kind of a cap, and every reason it is not guaranteed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub kind: CapKind,
    pub because: Vec<String>,
}

/// What the cap of a run made of these steps is.
///
/// One handed step is enough on its own: Astra's rule, and the arithmetic
/// behind it — a call started outside this control enters no sum here.
pub fn verdict_on(facts: &[StepFact]) -> Verdict {
    let mut because = Vec::new();
    for fact in facts {
        if fact.handed_to_agent {
            because.push(format!(
                "«{}» hands the work to an agent, which can start calls outside this control",
                fact.step
            ));
        }
        if let Some(why) = fact.reserve.as_ref().and_then(Reserve::why) {
            because.push(format!("«{}» cannot be reserved: {why}", fact.step));
        }
    }
    Verdict {
        kind: if because.is_empty() {
            CapKind::Guaranteed
        } else {
            CapKind::StopThreshold
        },
        because,
    }
}

// ── the control before the call ──────────────────────────────────────────

/// The two numbers a suspended run says, and how far they can be trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suspension {
    /// What is left of the cap: the cap less what is spent and less the
    /// reserves of the calls already under way.
    pub remaining_micros: i64,
    /// What the next call would need, or why that cannot be known.
    pub reserve: Reserve,
    /// How the spend behind that remainder reads. `AtLeast` makes the remainder
    /// a ceiling on what is left, not a measure of it.
    pub spent: flow::CostReading,
}

/// Whether the next call may be authorised.
///
/// The condition is Astra's: `spend + the reserves in flight + the maximum of
/// the next call ≤ cap`. Either of two terms can be missing — a reserve nobody
/// can bound, a spend that reads `AtLeast` — and then what is left is the older
/// rule, stop when the remainder is gone, which stops the call *after*.
pub fn admits(
    cap_micros: i64,
    spent: &flow::Spend,
    in_flight_micros: i64,
    next: &Reserve,
) -> Result<(), Suspension> {
    let reading = spent.reading();
    let remaining = cap_micros - spent.micros - in_flight_micros;
    let fits = match (next, reading) {
        (Reserve::Known(reserve), flow::CostReading::Exact(_) | flow::CostReading::Nothing) => {
            *reserve <= remaining
        }
        _ => remaining > 0,
    };
    if fits {
        return Ok(());
    }
    Err(Suspension {
        remaining_micros: remaining,
        reserve: next.clone(),
        spent: reading,
    })
}

/// What a suspended run says, in both languages, from the catalogue.
///
/// The sentence is composed here and not by whoever displays: this one is read
/// by a person deciding whether to raise a cap, and the two numbers must arrive
/// together with what is not known about them.
pub fn why_it_is_suspended(stopped: &Suspension) -> String {
    let remaining = in_units(stopped.remaining_micros);
    let mut said = match stopped.reserve.why() {
        None => catalogue::say(
            "run.suspended.with_a_reserve",
            &[
                ("remaining", &remaining),
                (
                    "reserve",
                    &in_units(stopped.reserve.micros().unwrap_or_default()),
                ),
            ],
        ),
        Some(why) => catalogue::say(
            "run.suspended.without_a_reserve",
            &[("remaining", &remaining), ("why", why)],
        ),
    };
    if let flow::CostReading::AtLeast {
        calls,
        calls_without_cost,
        ..
    } = stopped.spent
    {
        said.push_str(&catalogue::say(
            "run.suspended.the_spend_is_a_floor",
            &[
                ("calls", &calls.to_string()),
                ("without_cost", &calls_without_cost.to_string()),
            ],
        ));
    }
    said
}

/// A figure of micro-units as a person reads it. The same scale the run record
/// and the flow commands print: equivalent cost, never money charged.
pub fn in_units(micros: i64) -> String {
    format!("{:.2}", micros as f64 / MICROS_PER_UNIT as f64)
}

// ── the reserves of the calls already under way ──────────────────────────

/// What each run has reserved for the calls it has in flight.
static IN_FLIGHT: Mutex<BTreeMap<String, i64>> = Mutex::new(BTreeMap::new());

/// A reserve held for as long as its call runs, released when it is dropped —
/// including when the call breaks, where a hand-written release is forgotten.
#[derive(Debug)]
pub struct Held {
    run_id: String,
    micros: i64,
}

impl Drop for Held {
    fn drop(&mut self) {
        let mut held = IN_FLIGHT.lock().unwrap_or_else(|held| held.into_inner());
        if let Some(total) = held.get_mut(&self.run_id) {
            *total -= self.micros;
            if *total <= 0 {
                held.remove(&self.run_id);
            }
        }
    }
}

/// Holds `micros` against `run_id` until the returned guard is dropped.
pub fn hold(run_id: &str, micros: i64) -> Held {
    let mut held = IN_FLIGHT.lock().unwrap_or_else(|held| held.into_inner());
    *held.entry(run_id.to_owned()).or_insert(0) += micros;
    Held {
        run_id: run_id.to_owned(),
        micros,
    }
}

/// What this run holds for the calls already under way.
pub fn in_flight(run_id: &str) -> i64 {
    IN_FLIGHT
        .lock()
        .unwrap_or_else(|held| held.into_inner())
        .get(run_id)
        .copied()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prices(output: Option<i64>) -> PriceMicros {
        PriceMicros {
            input: Some(3_000_000),
            output,
            ..PriceMicros::default()
        }
    }

    fn tokens(output: Option<u64>) -> TokenCounts {
        TokenCounts {
            input: Some(1_000),
            output,
            ..TokenCounts::default()
        }
    }

    #[test]
    fn a_currency_ceiling_is_a_reserve_and_needs_no_tariff() {
        let reserve = reserve_of(&Ceiling::Currency(250_000), &PriceMicros::default());
        assert_eq!(reserve, Reserve::Known(250_000));
        assert_eq!(
            Ceiling::Currency(250_000).as_written().as_deref(),
            Some("0.250000")
        );
    }

    /// The control first: with every tariff there, the tokens price out.
    #[test]
    fn a_missing_tariff_leaves_the_reserve_unknown_and_never_zero() {
        let priced = reserve_of(
            &Ceiling::Tokens(tokens(Some(500))),
            &prices(Some(15_000_000)),
        );
        assert_eq!(priced, Reserve::Known(3_000 + 7_500));

        let missing = reserve_of(&Ceiling::Tokens(tokens(Some(500))), &prices(None));
        assert_eq!(
            missing.micros(),
            None,
            "a missing tariff is not a zero cost"
        );
        assert!(
            missing.why().is_some_and(|why| why.contains("«output»")),
            "{missing:?}"
        );
    }

    #[test]
    fn a_ceiling_that_counts_nothing_reserves_nothing() {
        let empty = reserve_of(&Ceiling::Tokens(TokenCounts::default()), &prices(Some(1)));
        assert_eq!(empty.micros(), None);
    }

    #[test]
    fn a_handed_step_takes_the_guarantee_away_from_the_whole_run() {
        let bounded = StepFact {
            step: "ask".to_owned(),
            handed_to_agent: false,
            reserve: Some(Reserve::Known(10)),
        };
        assert_eq!(
            verdict_on(std::slice::from_ref(&bounded)).kind,
            CapKind::Guaranteed
        );

        let handed = StepFact {
            step: "delegate".to_owned(),
            handed_to_agent: true,
            reserve: None,
        };
        let verdict = verdict_on(&[bounded, handed]);
        assert_eq!(verdict.kind, CapKind::StopThreshold);
        assert!(verdict.because[0].contains("«delegate»"), "{verdict:?}");
    }

    #[test]
    fn an_unreserved_step_takes_the_guarantee_away_too() {
        let verdict = verdict_on(&[StepFact {
            step: "ask".to_owned(),
            handed_to_agent: false,
            reserve: Some(Reserve::Unknown("no tariff".to_owned())),
        }]);
        assert_eq!(verdict.kind, CapKind::StopThreshold);
        assert!(verdict.because[0].contains("no tariff"), "{verdict:?}");
    }

    fn spent(micros: i64, without_cost: i64) -> flow::Spend {
        flow::Spend {
            micros,
            calls: 2,
            calls_without_cost: without_cost,
            dearest_micros: Some(micros),
        }
    }

    #[test]
    fn a_call_that_does_not_fit_the_remainder_is_not_authorised() {
        assert_eq!(
            admits(1_000, &spent(400, 0), 0, &Reserve::Known(600)),
            Ok(())
        );
        let refused =
            admits(1_000, &spent(400, 0), 0, &Reserve::Known(601)).expect_err("601 does not fit");
        assert_eq!(refused.remaining_micros, 600);
        assert_eq!(refused.reserve, Reserve::Known(601));
        // The reserves in flight take from the same remainder.
        assert!(admits(1_000, &spent(400, 0), 100, &Reserve::Known(600)).is_err());
    }

    #[test]
    fn with_no_knowable_reserve_only_a_spent_remainder_stops_the_call() {
        let unknown = Reserve::Unknown("no ceiling".to_owned());
        assert_eq!(admits(1_000, &spent(999, 0), 0, &unknown), Ok(()));
        let refused = admits(1_000, &spent(1_000, 0), 0, &unknown).expect_err("nothing is left");
        assert_eq!(refused.remaining_micros, 0);
        assert!(refused.reserve.why().is_some());
    }

    /// A spend that reads `AtLeast` is a floor: the guaranteed arithmetic
    /// cannot be done over it, however well the next call is bounded.
    #[test]
    fn a_partial_spend_takes_the_guarantee_away_from_a_bounded_call() {
        let bounded = Reserve::Known(601);
        assert!(
            admits(1_000, &spent(400, 1), 0, &bounded).is_ok(),
            "with the spend a floor, only an exhausted remainder stops the call"
        );
        let refused =
            admits(1_000, &spent(1_000, 1), 0, &bounded).expect_err("the remainder is gone");
        assert!(
            matches!(refused.spent, flow::CostReading::AtLeast { .. }),
            "{refused:?}"
        );
    }

    #[test]
    fn a_reserve_is_released_when_its_call_ends() {
        let run = "a-run-of-its-own";
        assert_eq!(in_flight(run), 0);
        {
            let _first = hold(run, 30);
            let _second = hold(run, 12);
            assert_eq!(in_flight(run), 42);
        }
        assert_eq!(in_flight(run), 0, "both were released");
    }

    #[test]
    fn a_step_says_why_it_has_no_ceiling() {
        let usd = CeilingOption {
            args: vec!["--max-budget-usd".to_owned()],
            unit: UNIT_CURRENCY.to_owned(),
        };
        let in_money = Declared {
            max_spend_micros: Some(500_000),
            max_tokens: None,
        };
        assert_eq!(
            ceiling_for(&usd, &in_money),
            Some(Ceiling::Currency(500_000))
        );
        assert_eq!(ceiling_for(&usd, &Declared::default()), None);
        assert!(why_no_ceiling(None, &in_money).contains(NATIVE_SPEND_CAP));
        assert!(why_no_ceiling(Some(&usd), &Declared::default()).contains("usd"));

        let in_tokens = CeilingOption {
            args: vec!["--max-tokens".to_owned()],
            unit: UNIT_TOKENS.to_owned(),
        };
        assert_eq!(ceiling_for(&in_tokens, &in_money), None);
        assert!(why_no_ceiling(Some(&in_tokens), &in_money).contains("tokens"));
    }
}
